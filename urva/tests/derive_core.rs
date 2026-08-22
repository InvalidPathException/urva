use urva::prelude::*;
use urva::{Field, MatchField, VersionField};

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub tenant_id: ObjectId,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
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

#[test]
fn entities_declared_inside_a_function_body_compile() {
    #[derive(Embedded, Serialize, Deserialize, Debug)]
    struct Local {
        n: i64,
    }
    #[derive(Entity, Serialize, Deserialize, Debug)]
    #[entity(collection = "scoped")]
    struct Scoped {
        local: Local,
    }
    assert_eq!(Scoped::COLLECTION, "scoped");
    assert_eq!(scoped_fields::local.path(), "local");
    assert_eq!(local_fields::n.path(), "n");
    assert_unversioned::<Scoped>();
    let _ = Scoped {
        local: Local { n: 1 },
    };
}
