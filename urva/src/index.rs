use std::marker::PhantomData;
use std::time::Duration;

use mongodb::IndexModel;
use mongodb::bson::{Bson, Document};
use mongodb::options::{Collation as DriverCollation, CollationStrength, IndexOptions};

pub struct IndexRef<E: ?Sized> {
    spec: &'static IndexSpec,
    _marker: PhantomData<fn() -> Box<E>>,
}

impl<E: ?Sized> IndexRef<E> {
    pub(crate) const fn from_spec(spec: &'static IndexSpec) -> Self {
        IndexRef {
            spec,
            _marker: PhantomData,
        }
    }

    pub fn name(&self) -> &'static str {
        self.spec.name
    }

    #[doc(hidden)]
    pub const fn spec(&self) -> &'static IndexSpec {
        self.spec
    }
}

impl<E: ?Sized> Clone for IndexRef<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: ?Sized> Copy for IndexRef<E> {}

pub trait HintFor<E: ?Sized> {
    #[doc(hidden)]
    fn to_hint(&self) -> mongodb::options::Hint;
}

impl<E: ?Sized> HintFor<E> for IndexRef<E> {
    fn to_hint(&self) -> mongodb::options::Hint {
        mongodb::options::Hint::Name(self.spec.name.to_string())
    }
}

impl<E: ?Sized> HintFor<E> for mongodb::options::Hint {
    fn to_hint(&self) -> mongodb::options::Hint {
        self.clone()
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstBson {
    Null,
    Bool(bool),
    I64(i64),
    F64(f64),
    Str(&'static str),
    Arr(&'static [ConstBson]),
    Doc(&'static [(&'static str, ConstBson)]),
}

impl ConstBson {
    pub fn to_bson(&self) -> Bson {
        match *self {
            ConstBson::Null => Bson::Null,
            ConstBson::Bool(b) => Bson::Boolean(b),
            ConstBson::I64(i) => Bson::Int64(i),
            ConstBson::F64(f) => Bson::Double(f),
            ConstBson::Str(s) => Bson::String(s.to_string()),
            ConstBson::Arr(items) => Bson::Array(items.iter().map(ConstBson::to_bson).collect()),
            ConstBson::Doc(entries) => Bson::Document(
                entries
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), v.to_bson()))
                    .collect(),
            ),
        }
    }

    pub fn to_document(&self) -> Option<Document> {
        match self.to_bson() {
            Bson::Document(d) => Some(d),
            _ => None,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndexKey {
    pub path: &'static str,
    pub kind: KeyKind,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyKind {
    Asc,
    Desc,
    Text,
    Hashed,
    TwoDSphere,
    TwoD,
    Wildcard,
}

impl KeyKind {
    pub fn to_bson(self) -> Bson {
        match self {
            KeyKind::Asc | KeyKind::Wildcard => Bson::Int32(1),
            KeyKind::Desc => Bson::Int32(-1),
            KeyKind::Text => Bson::String("text".to_string()),
            KeyKind::Hashed => Bson::String("hashed".to_string()),
            KeyKind::TwoDSphere => Bson::String("2dsphere".to_string()),
            KeyKind::TwoD => Bson::String("2d".to_string()),
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weight {
    pub field: &'static str,
    pub weight: i32,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct CollationSpec {
    pub locale: &'static str,
    pub strength: Option<CollationStrength>,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct IndexSpec {
    pub name: &'static str,
    pub keys: &'static [IndexKey],
    pub unique: bool,
    pub sparse: bool,
    pub hidden: bool,
    pub ttl_seconds: Option<u64>,
    pub partial: Option<ConstBson>,
    pub weights: &'static [Weight],
    pub collation: Option<CollationSpec>,
    pub wildcard_projection: Option<ConstBson>,
    pub bits: Option<u32>,
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl IndexSpec {
    pub const DEFAULT: IndexSpec = IndexSpec {
        name: "",
        keys: &[],
        unique: false,
        sparse: false,
        hidden: false,
        ttl_seconds: None,
        partial: None,
        weights: &[],
        collation: None,
        wildcard_projection: None,
        bits: None,
        min: None,
        max: None,
    };

    pub fn keys_document(&self) -> Document {
        let mut keys = Document::new();
        for key in self.keys {
            keys.insert(key.path, key.kind.to_bson());
        }
        keys
    }

    pub fn to_model(&self) -> IndexModel {
        let mut options = IndexOptions::default();
        options.name = Some(self.name.to_string());
        if self.unique {
            options.unique = Some(true);
        }
        if self.sparse {
            options.sparse = Some(true);
        }
        if self.hidden {
            options.hidden = Some(true);
        }
        if let Some(secs) = self.ttl_seconds {
            options.expire_after = Some(Duration::from_secs(secs));
        }
        if let Some(partial) = &self.partial {
            options.partial_filter_expression =
                Some(partial.to_document().expect("partial filter is an object"));
        }
        if !self.weights.is_empty() {
            let mut weights = Document::new();
            for w in self.weights {
                weights.insert(w.field, w.weight);
            }
            options.weights = Some(weights);
        }
        if let Some(collation) = &self.collation {
            let mut driver = DriverCollation::builder()
                .locale(collation.locale.to_string())
                .build();
            driver.strength = collation.strength;
            options.collation = Some(driver);
        }
        if let Some(projection) = &self.wildcard_projection {
            options.wildcard_projection =
                Some(projection.to_document().expect("projection is an object"));
        }
        if let Some(bits) = self.bits {
            options.bits = Some(bits);
        }
        if let Some(min) = self.min {
            options.min = Some(min);
        }
        if let Some(max) = self.max {
            options.max = Some(max);
        }
        IndexModel::builder()
            .keys(self.keys_document())
            .options(options)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use mongodb::bson::doc;
    use mongodb::options::CollationStrength;

    use super::*;

    #[test]
    fn every_option_renders_into_the_driver_model() {
        const PARTIAL: ConstBson = ConstBson::Doc(&[(
            "status",
            ConstBson::Doc(&[(
                "$in",
                ConstBson::Arr(&[ConstBson::Str("open"), ConstBson::Null]),
            )]),
        )]);
        const PROJECTION: ConstBson = ConstBson::Doc(&[("title", ConstBson::I64(1))]);
        const SPEC: IndexSpec = IndexSpec {
            name: "everything",
            keys: &[
                IndexKey {
                    path: "tenant",
                    kind: KeyKind::Asc,
                },
                IndexKey {
                    path: "created",
                    kind: KeyKind::Desc,
                },
            ],
            unique: true,
            sparse: true,
            hidden: true,
            ttl_seconds: Some(3600),
            partial: Some(PARTIAL),
            weights: &[Weight {
                field: "title",
                weight: 10,
            }],
            collation: Some(CollationSpec {
                locale: "en",
                strength: Some(CollationStrength::Secondary),
            }),
            wildcard_projection: Some(PROJECTION),
            bits: Some(26),
            min: Some(-180.0),
            max: Some(180.0),
        };

        let model = SPEC.to_model();
        assert_eq!(model.keys, doc! { "tenant": 1, "created": -1 });
        let options = model.options.unwrap();
        assert_eq!(options.name.as_deref(), Some("everything"));
        assert_eq!(options.unique, Some(true));
        assert_eq!(options.sparse, Some(true));
        assert_eq!(options.hidden, Some(true));
        assert_eq!(options.expire_after, Some(Duration::from_secs(3600)));
        assert_eq!(
            options.partial_filter_expression,
            Some(doc! { "status": { "$in": ["open", Bson::Null] } })
        );
        assert_eq!(options.weights, Some(doc! { "title": 10 }));
        let collation = options.collation.unwrap();
        assert_eq!(collation.locale, "en");
        assert!(matches!(
            collation.strength,
            Some(CollationStrength::Secondary)
        ));
        assert_eq!(options.wildcard_projection, Some(doc! { "title": 1_i64 }));
        assert_eq!(options.bits, Some(26));
        assert_eq!(options.min, Some(-180.0));
        assert_eq!(options.max, Some(180.0));
    }

    #[test]
    fn the_default_spec_renders_only_its_name_and_keys() {
        const SPEC: IndexSpec = IndexSpec {
            name: "plain",
            keys: &[IndexKey {
                path: "status",
                kind: KeyKind::Asc,
            }],
            ..IndexSpec::DEFAULT
        };
        let model = SPEC.to_model();
        assert_eq!(model.keys, doc! { "status": 1 });
        let options = model.options.unwrap();
        assert_eq!(options.name.as_deref(), Some("plain"));
        assert_eq!(options.unique, None);
        assert_eq!(options.sparse, None);
        assert_eq!(options.hidden, None);
        assert_eq!(options.expire_after, None);
        assert_eq!(options.partial_filter_expression, None);
        assert_eq!(options.weights, None);
        assert!(options.collation.is_none());
        assert_eq!(options.wildcard_projection, None);
        assert_eq!(options.bits, None);
        assert_eq!(options.min, None);
        assert_eq!(options.max, None);
    }

    #[test]
    fn every_key_kind_renders_its_server_spelling() {
        assert_eq!(KeyKind::Asc.to_bson(), Bson::Int32(1));
        assert_eq!(KeyKind::Desc.to_bson(), Bson::Int32(-1));
        assert_eq!(KeyKind::Text.to_bson(), Bson::String("text".into()));
        assert_eq!(KeyKind::Hashed.to_bson(), Bson::String("hashed".into()));
        assert_eq!(
            KeyKind::TwoDSphere.to_bson(),
            Bson::String("2dsphere".into())
        );
        assert_eq!(KeyKind::TwoD.to_bson(), Bson::String("2d".into()));
        assert_eq!(KeyKind::Wildcard.to_bson(), Bson::Int32(1));
    }

    #[test]
    fn index_refs_hint_by_name_and_driver_hints_pass_through() {
        const SPEC: IndexSpec = IndexSpec {
            name: "by_status",
            ..IndexSpec::DEFAULT
        };
        let index: IndexRef<()> = IndexRef::from_spec(&SPEC);
        assert_eq!(index.name(), "by_status");
        assert!(matches!(
            HintFor::<()>::to_hint(&index),
            mongodb::options::Hint::Name(name) if name == "by_status"
        ));
        let keys = mongodb::options::Hint::Keys(doc! { "status": 1 });
        assert!(matches!(
            HintFor::<()>::to_hint(&keys),
            mongodb::options::Hint::Keys(doc) if doc == doc! { "status": 1 }
        ));
    }
}
