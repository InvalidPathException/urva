use futures_util::TryStreamExt;
use mongodb::IndexModel;
use mongodb::bson::{Bson, Document};

use crate::entity::Entity;
use crate::store::Store;
use crate::{Error, Result};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct IndexDiff {
    pub to_create: Vec<String>,

    pub to_drop: Vec<String>,

    pub mismatched: Vec<String>,

    pub name_drift: Vec<NameDrift>,

    pub missing_external: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameDrift {
    pub expected: String,

    pub actual: String,
}

impl IndexDiff {
    pub fn is_empty(&self) -> bool {
        self.to_create.is_empty()
            && self.to_drop.is_empty()
            && self.mismatched.is_empty()
            && self.name_drift.is_empty()
            && self.missing_external.is_empty()
    }

    pub fn fails_verify(&self) -> bool {
        !self.to_create.is_empty()
            || !self.mismatched.is_empty()
            || !self.name_drift.is_empty()
            || !self.missing_external.is_empty()
    }
}

impl<E: Entity> Store<E> {
    pub async fn create_indexes(&self) -> Result<()> {
        let diff = self.diff_indexes().await?;
        self.create_named(diff.to_create.iter()).await
    }

    async fn create_named(&self, names: impl Iterator<Item = &String>) -> Result<()> {
        let names: Vec<&String> = names.collect();
        let models: Vec<IndexModel> = E::INDEX_SPECS
            .iter()
            .filter(|spec| names.iter().any(|n| *n == spec.name))
            .map(|spec| spec.to_model())
            .collect();
        if !models.is_empty() {
            self.raw().create_indexes(models).await?;
        }
        Ok(())
    }

    pub async fn diff_indexes(&self) -> Result<IndexDiff> {
        Ok(compute_diff::<E>(&self.list_indexes().await?))
    }

    pub async fn sync_indexes(&self) -> Result<IndexDiff> {
        let diff = self.diff_indexes().await?;
        let drifted_drops = diff
            .name_drift
            .iter()
            .map(|drift| &drift.actual)
            .filter(|actual| !E::EXTERNAL_INDEX_NAMES.contains(&actual.as_str()));
        for name in diff
            .to_drop
            .iter()
            .chain(&diff.mismatched)
            .chain(drifted_drops)
        {
            self.raw().drop_index(name).await?;
        }
        self.create_named(
            diff.to_create
                .iter()
                .chain(&diff.mismatched)
                .chain(diff.name_drift.iter().map(|drift| &drift.expected)),
        )
        .await?;
        Ok(diff)
    }

    pub async fn verify_indexes(&self) -> Result<()> {
        let diff = self.diff_indexes().await?;
        if diff.fails_verify() {
            return Err(Error::IndexDrift(Box::new(diff)));
        }
        Ok(())
    }

    async fn list_indexes(&self) -> Result<Vec<Document>> {
        let command = mongodb::bson::doc! { "listIndexes": E::COLLECTION };
        let collection = self.raw();
        let database = collection.client().database(&collection.namespace().db);
        match database.run_cursor_command(command).await {
            Ok(cursor) => Ok(cursor.try_collect().await?),
            Err(e) if is_namespace_missing(&e) => Ok(Vec::new()),
            Err(e) => Err(e.into()),
        }
    }
}

fn declared_document(model: &IndexModel) -> Document {
    mongodb::bson::to_document(model).expect("an index model serializes")
}

fn is_namespace_missing(error: &mongodb::error::Error) -> bool {
    matches!(
        &*error.kind,
        mongodb::error::ErrorKind::Command(c) if c.code == 26
    )
}

#[doc(hidden)]
pub fn compute_diff<E: Entity>(existing: &[Document]) -> IndexDiff {
    let mut diff = IndexDiff::default();

    let existing: Vec<(String, NormalIndex)> = existing
        .iter()
        .filter_map(|listing| {
            Some((
                listing.get_str("name").ok()?.to_string(),
                normalize(listing),
            ))
        })
        .collect();
    let mut used = vec![false; existing.len()];

    let declared: Vec<(String, NormalIndex)> = E::INDEX_SPECS
        .iter()
        .map(|spec| {
            (
                spec.name.to_string(),
                normalize(&declared_document(&spec.to_model())),
            )
        })
        .collect();

    let mut unbound = Vec::new();
    for (name, normal) in &declared {
        if let Some(i) = existing
            .iter()
            .enumerate()
            .position(|(i, (n, _))| !used[i] && n == name)
        {
            used[i] = true;
            if !structures_match(normal, &existing[i].1, false) {
                diff.mismatched.push(name.clone());
            }
        } else {
            unbound.push((name, normal));
        }
    }

    for (name, normal) in unbound {
        if let Some(i) = existing
            .iter()
            .enumerate()
            .position(|(i, (n, e))| !used[i] && n != "_id_" && structures_match(normal, e, true))
        {
            used[i] = true;
            diff.name_drift.push(NameDrift {
                expected: name.clone(),
                actual: existing[i].0.clone(),
            });
        } else {
            diff.to_create.push(name.clone());
        }
    }

    for (i, (name, _)) in existing.iter().enumerate() {
        if used[i] || name == "_id_" {
            continue;
        }
        if E::EXTERNAL_INDEX_NAMES.contains(&name.as_str()) {
            continue;
        }
        diff.to_drop.push(name.clone());
    }

    for external in E::EXTERNAL_INDEX_NAMES {
        if !existing.iter().any(|(name, _)| name == external) {
            diff.missing_external.push((*external).to_string());
        }
    }

    diff
}

#[derive(Debug, Clone, PartialEq)]
struct NormalIndex {
    prefix_keys: Vec<(String, NormalKey)>,

    text_fields: Vec<String>,

    suffix_keys: Vec<(String, NormalKey)>,
    unique: bool,
    sparse: bool,
    hidden: bool,
    ttl_seconds: Option<i64>,

    partial: Option<String>,

    weights: Vec<(String, i64)>,
    collation: NormalCollation,
    wildcard_projection: Option<Document>,
    bits: Option<i64>,
    min: Option<f64>,
    max: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
enum NormalKey {
    Direction(i64),
    Kind(String),
}

#[derive(Debug, Clone, PartialEq)]
enum NormalCollation {
    Unset,
    Simple,
    Locale { locale: String, strength: i64 },
}

fn collation_matches(declared: &NormalCollation, existing: &NormalCollation, strict: bool) -> bool {
    match declared {
        NormalCollation::Unset if !strict => true,
        NormalCollation::Locale { .. } => declared == existing,
        _ => matches!(existing, NormalCollation::Unset | NormalCollation::Simple),
    }
}

fn structures_match(
    declared: &NormalIndex,
    existing: &NormalIndex,
    strict_collation: bool,
) -> bool {
    collation_matches(&declared.collation, &existing.collation, strict_collation)
        && declared.prefix_keys == existing.prefix_keys
        && declared.text_fields == existing.text_fields
        && declared.suffix_keys == existing.suffix_keys
        && declared.unique == existing.unique
        && declared.sparse == existing.sparse
        && declared.hidden == existing.hidden
        && declared.ttl_seconds == existing.ttl_seconds
        && declared.partial == existing.partial
        && declared.weights == existing.weights
        && declared.wildcard_projection == existing.wildcard_projection
        && (declared.bits.is_none() || declared.bits == existing.bits)
        && (declared.min.is_none() || declared.min == existing.min)
        && (declared.max.is_none() || declared.max == existing.max)
}

fn normalize(listing: &Document) -> NormalIndex {
    let mut prefix_keys = Vec::new();
    let mut suffix_keys = Vec::new();
    let mut text_fields: Vec<String> = Vec::new();
    let mut seen_text = false;
    let keys = listing.get_document("key").cloned().unwrap_or_default();
    for (key, value) in &keys {
        if key == "_fts" || key == "_ftsx" {
            seen_text = true;
            continue;
        }
        match value {
            Bson::String(s) if s == "text" => {
                seen_text = true;
                text_fields.push(key.clone());
                continue;
            }
            _ => {}
        }
        let normal = match bson_to_i64(value) {
            Some(direction) => NormalKey::Direction(direction),
            None => NormalKey::Kind(match value {
                Bson::String(s) => s.clone(),
                other => other.to_string(),
            }),
        };
        if seen_text {
            suffix_keys.push((key.clone(), normal));
        } else {
            prefix_keys.push((key.clone(), normal));
        }
    }

    let mut weights: Vec<(String, i64)> = listing
        .get_document("weights")
        .map(|w| {
            w.iter()
                .filter_map(|(k, v)| Some((k.clone(), bson_to_i64(v)?)))
                .collect()
        })
        .unwrap_or_default();
    for (field, _) in &weights {
        if !text_fields.contains(field)
            && !prefix_keys.iter().any(|(k, _)| k == field)
            && !suffix_keys.iter().any(|(k, _)| k == field)
        {
            text_fields.push(field.clone());
        }
    }
    if !text_fields.is_empty() {
        for field in &text_fields {
            if !weights.iter().any(|(f, _)| f == field) {
                weights.push((field.clone(), 1));
            }
        }
    }
    text_fields.sort();
    weights.sort();

    let flag = |name: &str| listing.get_bool(name).unwrap_or(false);
    NormalIndex {
        prefix_keys,
        text_fields,
        suffix_keys,
        unique: flag("unique"),
        sparse: flag("sparse"),
        hidden: flag("hidden"),
        ttl_seconds: listing.get("expireAfterSeconds").and_then(bson_to_i64),
        partial: listing
            .get_document("partialFilterExpression")
            .ok()
            .map(|d| normalize_partial(d).to_string()),
        weights,
        collation: match listing.get_document("collation") {
            Err(_) => NormalCollation::Unset,
            Ok(c) => match c.get_str("locale") {
                Ok("simple") => NormalCollation::Simple,
                Ok(locale) => NormalCollation::Locale {
                    locale: locale.to_string(),
                    strength: c.get("strength").and_then(bson_to_i64).unwrap_or(3),
                },
                Err(_) => NormalCollation::Unset,
            },
        },
        wildcard_projection: listing
            .get_document("wildcardProjection")
            .ok()
            .map(normalize_document),
        bits: listing.get("bits").and_then(bson_to_i64),
        min: listing.get("min").and_then(bson_to_f64),
        max: listing.get("max").and_then(bson_to_f64),
    }
}

fn bson_to_f64(value: &Bson) -> Option<f64> {
    match value {
        Bson::Double(d) => Some(*d),
        Bson::Int32(i) => Some(f64::from(*i)),
        Bson::Int64(i) => Some(*i as f64),
        _ => None,
    }
}

fn bson_to_i64(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(i) => Some(i64::from(*i)),
        Bson::Int64(i) => Some(*i),
        Bson::Double(d) => Some(*d as i64),
        _ => None,
    }
}

fn normalize_partial(doc: &Document) -> Document {
    let mut entries: Vec<(String, Bson)> = doc
        .iter()
        .map(|(key, value)| {
            let normal = if key.starts_with('$') {
                match value {
                    Bson::Array(items) if matches!(key.as_str(), "$and" | "$or" | "$nor") => {
                        Bson::Array(
                            items
                                .iter()
                                .map(|item| match item {
                                    Bson::Document(d) => Bson::Document(normalize_partial(d)),
                                    other => normalize_bson(other),
                                })
                                .collect(),
                        )
                    }
                    other => normalize_bson(other),
                }
            } else {
                match value {
                    Bson::Document(d) if d.keys().any(|k| k.starts_with('$')) => {
                        Bson::Document(normalize_partial(d))
                    }
                    other => {
                        let mut eq = Document::new();
                        eq.insert("$eq", normalize_bson(other));
                        Bson::Document(eq)
                    }
                }
            };
            (key.clone(), normal)
        })
        .collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut out = Document::new();
    for (key, value) in entries {
        out.insert(key, value);
    }
    out
}

fn normalize_document(doc: &Document) -> Document {
    let mut out = Document::new();
    for (key, value) in doc {
        out.insert(key.clone(), normalize_bson(value));
    }
    out
}

fn normalize_bson(value: &Bson) -> Bson {
    match value {
        Bson::Int32(i) => Bson::Int64(i64::from(*i)),
        Bson::Double(d) if d.fract() == 0.0 => Bson::Int64(*d as i64),
        Bson::Document(d) => Bson::Document(normalize_document(d)),
        Bson::Array(items) => Bson::Array(items.iter().map(normalize_bson).collect()),
        other => other.clone(),
    }
}
