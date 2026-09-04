use mongodb::bson::{Bson, doc, to_vec};
use urva::prelude::*;
use urva::{Field, MatchField, VersionField};

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders", versioned)]
#[index(tenant_recent, keys(tenant_id, created_at = -1), unique)]
#[index(open_status, keys(status), partial(status = "open"))]
#[index(order_search, keys(title = text, body = text), weights(title = 10))]
#[index(expiry, keys(expires_at), ttl = 3600)]
#[index(hidden_probe, keys(total), hidden)]
#[index(everything, keys(wildcard), wildcard_projection = "{\"title\": 1}")]
pub struct Order {
    pub tenant_id: ObjectId,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub title: String,
    pub body: String,
    pub expires_at: DateTime,
    pub total: i64,
    pub shipping: Shipping,
}

#[derive(Embedded, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Shipping {
    pub city_name: String,
    pub zip: String,
}

fn token<E, T>(_: Field<E, T>) {}
fn match_token<E, T>(_: MatchField<E, T>) {}
fn version_token<E>(_: VersionField<E>) {}

fn named(model: &mongodb::IndexModel, name: &str) -> bool {
    model
        .options
        .as_ref()
        .and_then(|o| o.name.as_deref())
        .is_some_and(|n| n == name)
}

#[test]
fn field_tokens_use_stored_names() {
    assert_eq!(order_fields::tenant_id.path(), "tenant_id");
    assert_eq!(order_fields::created_at.path(), "createdAt");
    assert_eq!(order_fields::_id.path(), "_id");
    assert_eq!(order_fields::version.path(), "version");
    assert_eq!(shipping_fields::city_name.path(), "cityName");
}

#[test]
fn field_tokens_carry_the_declared_types() {
    token::<Order, ObjectId>(order_fields::tenant_id);
    token::<Order, DateTime>(order_fields::created_at);
    token::<Order, Shipping>(order_fields::shipping);
    match_token::<Order, ObjectId>(order_fields::_id);
    version_token::<Order>(order_fields::version);
    token::<Shipping, String>(shipping_fields::zip);
}

#[test]
fn entity_contract_constants() {
    assert_eq!(Order::COLLECTION, "orders");
    assert_eq!(Order::VERSION_FIELD, "version");
    assert_versioned::<Order>();
    assert_id::<Order, ObjectId>();
    assert_generates_ids::<Order>();
}

#[test]
fn index_models_render_names_keys_and_options() {
    let models = Order::index_models();
    assert_eq!(models.len(), 6);

    let tenant_recent = models.iter().find(|m| named(m, "tenant_recent")).unwrap();
    assert_eq!(
        to_vec(&tenant_recent.keys).unwrap(),
        to_vec(&doc! { "tenant_id": 1, "createdAt": -1 }).unwrap()
    );
    assert_eq!(tenant_recent.options.as_ref().unwrap().unique, Some(true));

    let open_status = models.iter().find(|m| named(m, "open_status")).unwrap();
    assert_eq!(
        to_vec(&open_status.keys).unwrap(),
        to_vec(&doc! { "status": 1 }).unwrap()
    );
    assert_eq!(
        open_status
            .options
            .as_ref()
            .unwrap()
            .partial_filter_expression,
        Some(doc! { "status": { "$eq": "open" } })
    );

    let search = models.iter().find(|m| named(m, "order_search")).unwrap();
    assert_eq!(
        to_vec(&search.keys).unwrap(),
        to_vec(&doc! { "title": "text", "body": "text" }).unwrap()
    );
    assert_eq!(
        search.options.as_ref().unwrap().weights,
        Some(doc! { "title": 10 })
    );

    let expiry = models.iter().find(|m| named(m, "expiry")).unwrap();
    assert_eq!(
        to_vec(&expiry.keys).unwrap(),
        to_vec(&doc! { "expires_at": 1 }).unwrap()
    );
    assert_eq!(
        expiry.options.as_ref().unwrap().expire_after,
        Some(std::time::Duration::from_secs(3600))
    );

    let hidden = models.iter().find(|m| named(m, "hidden_probe")).unwrap();
    assert_eq!(
        to_vec(&hidden.keys).unwrap(),
        to_vec(&doc! { "total": 1 }).unwrap()
    );
    assert_eq!(hidden.options.as_ref().unwrap().hidden, Some(true));

    let wildcard = models.iter().find(|m| named(m, "everything")).unwrap();
    assert_eq!(
        to_vec(&wildcard.keys).unwrap(),
        to_vec(&doc! { "$**": 1 }).unwrap()
    );
    assert_eq!(
        wildcard.options.as_ref().unwrap().wildcard_projection,
        Some(doc! { "title": 1_i64 })
    );
}

#[test]
fn hint_symbols_typecheck() {
    fn assert_hint<E, H: urva::HintFor<E>>(h: H) -> mongodb::options::Hint {
        urva::HintFor::<E>::to_hint(&h)
    }
    let hint = assert_hint::<Order, _>(order_index::tenant_recent);
    assert_eq!(hint, mongodb::options::Hint::Name("tenant_recent".into()));
    assert_eq!(order_index::tenant_recent.name(), "tenant_recent");

    let raw = mongodb::options::Hint::Keys(doc! { "status": 1 });
    let _ = assert_hint::<Order, _>(raw.clone());
    let _ = assert_hint::<Note, _>(raw);
}

fn assert_versioned<E: urva::Versioned>() {}
fn assert_unversioned<E: urva::Unversioned>() {}
fn assert_id<E: Entity<Id = I>, I>() {}
fn assert_generates_ids<E: Entity>()
where
    E::Id: urva::NewId,
{
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "notes", id = String)]
pub struct Note {
    pub text: String,
}

#[test]
fn declared_id_type_and_unversioned_lock() {
    assert_eq!(Note::COLLECTION, "notes");
    assert_eq!(note_fields::_id.path(), "_id");
    match_token::<Note, String>(note_fields::_id);
    assert_unversioned::<Note>();
    assert_id::<Note, String>();
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "revisions", versioned = "rev")]
pub struct Revision {
    pub text: String,
}

#[test]
fn renamed_version_field_is_the_stored_lock_name() {
    assert_eq!(Revision::VERSION_FIELD, "rev");
    assert_eq!(revision_fields::rev.path(), "rev");
    assert_versioned::<Revision>();
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "events")]
#[index(by_type, keys(r#type))]
pub struct Event {
    pub r#type: String,
}

#[test]
fn raw_identifier_fields_resolve_to_serde_names() {
    let e = Event {
        r#type: "click".into(),
    };
    let stored = mongodb::bson::to_document(&e).unwrap();
    assert!(stored.contains_key("type"), "serde writes the unraw key");
    assert_eq!(event_fields::r#type.path(), "type");
    let models = Event::index_models();
    assert_eq!(models[0].keys, doc! { "type": 1 });
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "counters")]
#[index(by_cached, keys(cached_total))]
pub struct Counter {
    #[serde(skip_deserializing)]
    pub cached_total: i64,
}

#[test]
fn skip_deserializing_field_keeps_its_token_and_index() {
    assert_eq!(counter_fields::cached_total.path(), "cached_total");
    let models = Counter::index_models();
    assert!(models.iter().any(|m| named(m, "by_cached")));
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "cards")]
#[index(by_field, keys(r#wildcard))]
pub struct Card {
    pub wildcard: String,
}

#[test]
fn raw_field_spelling_indexes_the_field() {
    let models = Card::index_models();
    let by_field = models.iter().find(|m| named(m, "by_field")).unwrap();
    assert_eq!(
        to_vec(&by_field.keys).unwrap(),
        to_vec(&doc! { "wildcard": 1 }).unwrap()
    );
    assert_eq!(card_fields::wildcard.path(), "wildcard");
}

#[allow(clippy::duplicated_attributes)]
#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "hygiene")]
#[index(foo, keys(x))]
#[index(__SPEC_foo, keys(y))]
#[index(__PATH_foo_0, keys(x, y))]
pub struct Hygiene {
    pub x: i64,
    pub y: i64,
}

#[test]
fn generated_names_cannot_collide_with_index_idents() {
    let models = Hygiene::index_models();
    assert_eq!(models.len(), 3);
    assert!(models.iter().any(|m| named(m, "__SPEC_foo")));
    assert!(models.iter().any(|m| named(m, "__PATH_foo_0")));
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "hygiene")]
#[index(part, keys(a))]
#[index(i, keys(a = -1, b))]
#[index(s, keys(ship))]
pub struct HygieneProbe {
    pub a: i32,
    pub b: i32,
    pub ship: Shipping,
}

#[test]
fn index_idents_matching_emitted_bindings_expand() {
    let models = HygieneProbe::index_models();
    let s = models.iter().find(|m| named(m, "s")).expect("declared");
    assert_eq!(
        to_vec(&s.keys).unwrap(),
        to_vec(&doc! { "ship": 1 }).unwrap()
    );
}

#[allow(clippy::duplicated_attributes)]
#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "twin_orders")]
#[index(open_by_status, keys(status), partial(status = "open"))]
#[index(closed_by_status, keys(status), partial(status = "closed"))]
#[index(status_full, keys(status))]
#[index(status_sparse, keys(status), sparse)]
#[index(status_de, keys(status), collation(locale = "de", strength = 2))]
pub struct TwinOrder {
    pub status: String,
}

#[test]
fn same_key_pattern_twins_compile_and_emit() {
    let models = TwinOrder::index_models();
    assert_eq!(models.len(), 5);
    for name in [
        "open_by_status",
        "closed_by_status",
        "status_full",
        "status_sparse",
        "status_de",
    ] {
        let model = models.iter().find(|m| named(m, name)).unwrap();
        assert_eq!(
            to_vec(&model.keys).unwrap(),
            to_vec(&doc! { "status": 1 }).unwrap()
        );
    }
    let sparse = models.iter().find(|m| named(m, "status_sparse")).unwrap();
    assert_eq!(sparse.options.as_ref().unwrap().sparse, Some(true));
    let de = models.iter().find(|m| named(m, "status_de")).unwrap();
    let collation = de.options.as_ref().unwrap().collation.as_ref().unwrap();
    assert_eq!(collation.locale, "de");
    assert!(matches!(
        collation.strength,
        Some(mongodb::options::CollationStrength::Secondary)
    ));
}

#[allow(clippy::duplicated_attributes)]
#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "wild_twins")]
#[index(titles_only, keys(wildcard), wildcard_projection = "{\"title\": 1}")]
#[index(bodies_only, keys(wildcard), wildcard_projection = "{\"body\": 1}")]
pub struct WildcardTwins {
    pub title: String,
    pub body: String,
}

#[test]
fn wildcard_twins_differing_in_projection_compile_and_emit() {
    let models = WildcardTwins::index_models();
    assert_eq!(models.len(), 2);
    for name in ["titles_only", "bodies_only"] {
        let model = models.iter().find(|m| named(m, name)).unwrap();
        assert_eq!(
            to_vec(&model.keys).unwrap(),
            to_vec(&doc! { "$**": 1 }).unwrap()
        );
    }
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "skipped_wildcards")]
#[index(everything, keys(wildcard))]
pub struct SkippedWildcardCarrier {
    #[serde(skip)]
    pub wildcard: bool,
    pub a: i64,
}

#[test]
fn bare_wildcard_key_is_whole_document_when_the_wildcard_field_is_skipped() {
    let models = SkippedWildcardCarrier::index_models();
    let everything = models.iter().find(|m| named(m, "everything")).unwrap();
    assert_eq!(
        to_vec(&everything.keys).unwrap(),
        to_vec(&doc! { "$**": 1 }).unwrap()
    );
}

#[allow(clippy::duplicated_attributes)]
#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "credits")]
#[index(overdrawn, keys(balance), partial(balance = -5))]
#[index(cold, keys(score), partial(score = -0.5))]
pub struct Credit {
    pub balance: i64,
    pub score: f64,
}

#[test]
fn negative_partial_literals_render() {
    let models = Credit::index_models();
    let partial = |name: &str| {
        models
            .iter()
            .find(|m| named(m, name))
            .unwrap()
            .options
            .as_ref()
            .unwrap()
            .partial_filter_expression
            .clone()
    };
    assert_eq!(
        partial("overdrawn"),
        Some(doc! { "balance": { "$eq": -5i64 } })
    );
    assert_eq!(partial("cold"), Some(doc! { "score": { "$eq": -0.5 } }));
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "parcels")]
pub struct Parcel {
    pub home: Box<Shipping>,
    pub maybe_home: Option<Box<Shipping>>,
    pub label: Box<String>,
    pub boxed_opt: Box<Option<Shipping>>,
    pub boxed_vec: Box<Vec<Shipping>>,
}

#[test]
fn boxed_fields_erase_the_box() {
    use parcel_fields as p;
    token::<Parcel, Shipping>(p::home);
    token::<Parcel, Option<Shipping>>(p::maybe_home);
    token::<Parcel, String>(p::label);
    token::<Parcel, Option<Shipping>>(p::boxed_opt);
    token::<Parcel, Vec<Shipping>>(p::boxed_vec);
    assert_eq!(p::label.path(), "label");
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "lowercased")]
#[serde(rename_all = "lowercase")]
#[allow(non_snake_case)]
pub struct Lowercased {
    pub miXed: i64,
}

#[test]
fn rename_all_lowercase_is_identity_like_serde() {
    let stored = mongodb::bson::to_document(&Lowercased { miXed: 1 }).unwrap();
    assert!(
        stored.contains_key("miXed"),
        "serde stores the field name unchanged under `lowercase`"
    );
    assert_eq!(lowercased_fields::miXed.path(), "miXed");
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "skips")]
pub struct SkipCarrier {
    #[serde(skip, rename = "_id")]
    pub legacy_id: i64,
    #[serde(skip_serializing, skip_deserializing, rename = "_id")]
    pub older_id: i64,
    #[serde(skip, rename = "text")]
    pub cache: i64,
    pub text: String,
}

#[test]
fn fully_skipped_fields_occupy_no_stored_name() {
    let doc = mongodb::bson::to_document(&SkipCarrier {
        legacy_id: 7,
        older_id: 8,
        cache: 9,
        text: "x".into(),
    })
    .unwrap();
    assert!(!doc.contains_key("_id"), "skipped rename is never written");
    assert_eq!(doc.get_str("text").unwrap(), "x");
    assert_eq!(skip_carrier_fields::_id.path(), "_id");
    assert_eq!(skip_carrier_fields::text.path(), "text");
}

fn total_from_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    let s = String::deserialize(d)?;
    s.parse().map_err(serde::de::Error::custom)
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "receipts")]
pub struct Receipt {
    #[serde(deserialize_with = "total_from_str")]
    pub total: i64,
}

#[test]
fn a_custom_reader_alone_keeps_the_token() {
    assert_eq!(receipt_fields::total.path(), "total");
    token::<Receipt, i64>(receipt_fields::total);
}

mod cents_as_string {
    pub fn serialize<S: serde::Serializer>(cents: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&cents.to_string())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

fn upper<S: serde::Serializer>(label: &Option<String>, serializer: S) -> Result<S::Ok, S::Error> {
    match label {
        Some(l) => serializer.serialize_str(&l.to_uppercase()),
        None => serializer.serialize_none(),
    }
}

#[derive(Embedded, Serialize, Deserialize, Debug)]
pub struct Money {
    #[serde(with = "self::cents_as_string")]
    pub cents: i64,
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "invoices")]
pub struct Invoice {
    #[serde(with = "cents_as_string")]
    pub total: i64,
    #[serde(serialize_with = "upper", rename = "lbl")]
    pub label: Option<String>,
    pub price: Money,
}

use invoice_fields as inv;
use money_fields as mo;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "written_leases")]
#[index(expiry, keys(expires_at), ttl = 60)]
pub struct WrittenLease {
    #[serde(with = "cents_as_string")]
    pub expires_at: i64,
}

#[test]
fn encoded_fields_get_tokens_that_encode_through_the_writer() {
    let stored = mongodb::bson::to_document(&Invoice {
        total: 1250,
        label: Some("net".into()),
        price: Money { cents: 7 },
    })
    .unwrap();
    assert_eq!(
        inv::total.eq(1250).into_document().unwrap(),
        doc! { "total": { "$eq": stored.get("total").unwrap() } }
    );
    assert_eq!(
        inv::total.gte(1250).into_document().unwrap(),
        doc! { "total": { "$gte": "1250" } }
    );
    assert_eq!(
        inv::total.is_in([1, 2]).into_document().unwrap(),
        doc! { "total": { "$in": ["1", "2"] } }
    );
    assert_eq!(
        inv::total.set(3).into_parts().unwrap().0,
        doc! { "$set": { "total": "3" } }
    );
    assert_eq!(
        inv::label
            .eq(Some("net".to_string()))
            .into_document()
            .unwrap(),
        doc! { "lbl": { "$eq": stored.get("lbl").unwrap() } }
    );
    assert_eq!(
        inv::label.eq(None).into_document().unwrap(),
        doc! { "lbl": { "$eq": Bson::Null } }
    );
    assert_eq!(
        inv::price.dot(mo::cents).eq(7).into_document().unwrap(),
        doc! { "price.cents": { "$eq": "7" } }
    );
    assert_eq!(
        inv::total.desc().then(inv::label.asc()).into_document(),
        doc! { "total": -1, "lbl": 1 }
    );
}

#[test]
fn encoded_fields_expose_range_and_numeric_operators_and_ttl() {
    assert_eq!(
        inv::total.gt(5).into_document().unwrap(),
        doc! { "total": { "$gt": "5" } }
    );
    assert_eq!(
        inv::total.inc(5).into_parts().unwrap().0,
        doc! { "$inc": { "total": "5" } }
    );
    assert_eq!(
        inv::price.dot(mo::cents).max(9).into_parts().unwrap().0,
        doc! { "$max": { "price.cents": "9" } }
    );
    let models = <WrittenLease as urva::Entity>::index_models();
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].keys, doc! { "expires_at": 1 });
    assert_eq!(
        models[0].options.as_ref().unwrap().expire_after,
        Some(std::time::Duration::from_secs(60))
    );
}

#[test]
fn entities_declared_inside_a_function_body_compile() {
    #[derive(Embedded, Serialize, Deserialize, Debug)]
    struct Local {
        n: i64,
    }
    #[derive(Entity, Serialize, Deserialize, Debug)]
    #[entity(collection = "scoped")]
    #[index(by_local, keys(local), unique)]
    struct Scoped {
        local: Local,
    }
    use scoped_fields as s;
    assert_eq!(Scoped::COLLECTION, "scoped");
    assert_eq!(s::local.path(), "local");
    assert_eq!(local_fields::n.path(), "n");
    assert_eq!(
        s::local.dot(local_fields::n).eq(1).into_document().unwrap(),
        doc! { "local.n": { "$eq": 1_i64 } }
    );
    assert_eq!(scoped_index::by_local.name(), "by_local");
    assert_eq!(Scoped::index_models()[0].keys, doc! { "local": 1 });
    assert_unversioned::<Scoped>();
    let _ = Scoped {
        local: Local { n: 1 },
    };
}
