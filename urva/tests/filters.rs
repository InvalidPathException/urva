use mongodb::bson::{doc, to_vec};
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub tenant_id: ObjectId,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub total: i64,
    pub count: u64,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub labels: Option<Vec<String>>,
    pub shipping: Shipping,
    pub home: Option<Shipping>,
    pub stops: Vec<Shipping>,
    pub extra_stops: Option<Vec<Shipping>>,
}

#[derive(Embedded, Serialize, Deserialize, Debug)]
pub struct Shipping {
    #[serde(rename = "cityName")]
    pub city: String,
    pub zip: String,
}

use order_fields as o;
use shipping_fields as sf;

#[test]
fn comparison_operators() {
    assert_eq!(
        o::status.eq("open").into_document().unwrap(),
        doc! { "status": { "$eq": "open" } }
    );
    assert_eq!(
        o::total.ne(5).into_document().unwrap(),
        doc! { "total": { "$ne": 5_i64 } }
    );
    assert_eq!(
        o::status.is_in(["open", "packed"]).into_document().unwrap(),
        doc! { "status": { "$in": ["open", "packed"] } }
    );
    assert_eq!(
        o::status.not_in(["void"]).into_document().unwrap(),
        doc! { "status": { "$nin": ["void"] } }
    );
    assert_eq!(
        o::status
            .is_in(Vec::<String>::new())
            .into_document()
            .unwrap(),
        doc! { "status": { "$in": [] } },
        "an empty list is sent as is, the server rejects it"
    );
    assert_eq!(
        o::note.exists(true).into_document().unwrap(),
        doc! { "note": { "$exists": true } }
    );
    assert_eq!(
        o::note.is_null().into_document().unwrap(),
        doc! { "note": { "$eq": null } }
    );
    assert_eq!(
        o::labels.is_null().into_document().unwrap(),
        o::labels.eq(None::<Vec<String>>).into_document().unwrap(),
        "is_null is eq(None)"
    );
}

#[test]
fn range_operators() {
    assert_eq!(
        o::total.gt(5).into_document().unwrap(),
        doc! { "total": { "$gt": 5_i64 } }
    );
    assert_eq!(
        o::total.gte(10).into_document().unwrap(),
        doc! { "total": { "$gte": 10_i64 } }
    );
    assert_eq!(
        o::total.lte(5).into_document().unwrap(),
        doc! { "total": { "$lte": 5_i64 } }
    );
    let since = DateTime::from_millis(0);
    assert_eq!(
        o::created_at.lt(since).into_document().unwrap(),
        doc! { "createdAt": { "$lt": since } }
    );
    let id = ObjectId::new();
    assert_eq!(
        o::_id.gt(id).into_document().unwrap(),
        doc! { "_id": { "$gt": id } }
    );
    assert_eq!(
        o::status.gte("a").into_document().unwrap(),
        doc! { "status": { "$gte": "a" } },
        "strings are ordered"
    );
    assert_eq!(
        o::note.lt("z").into_document().unwrap(),
        doc! { "note": { "$lt": "z" } },
        "an Option token has its inner type's operators"
    );
}

#[test]
fn string_operators() {
    assert_eq!(
        o::note.regex("^x$").into_document().unwrap(),
        doc! { "note": { "$regex": "^x$" } }
    );
    assert_eq!(
        o::note.starts_with("op.en").into_document().unwrap(),
        doc! { "note": { "$regex": "^op\\.en" } },
        "the prefix is escaped"
    );
}

#[test]
fn array_operators() {
    assert_eq!(
        o::tags.contains("rush").into_document().unwrap(),
        doc! { "tags": { "$eq": "rush" } }
    );
    assert_eq!(
        o::tags.contains_any(["a", "b"]).into_document().unwrap(),
        doc! { "tags": { "$in": ["a", "b"] } }
    );
    assert_eq!(
        o::labels.contains_none(["x"]).into_document().unwrap(),
        doc! { "labels": { "$nin": ["x"] } }
    );
    assert_eq!(
        o::labels.size(2).into_document().unwrap(),
        doc! { "labels": { "$size": 2_i64 } }
    );
}

#[test]
fn combinators() {
    let since = DateTime::from_millis(0);
    let tenant = ObjectId::new();
    let f = all([
        o::tenant_id.eq(tenant),
        any([o::created_at.gte(since), o::status.eq("open")]),
    ]);
    assert_eq!(
        f.into_document().unwrap(),
        doc! { "$and": [
            { "tenant_id": { "$eq": tenant } },
            { "$or": [
                { "createdAt": { "$gte": since } },
                { "status": { "$eq": "open" } },
            ] },
        ] }
    );

    let chained = o::status
        .eq("a")
        .and(o::status.eq("b"))
        .and(o::status.eq("c"));
    assert_eq!(
        chained.into_document().unwrap(),
        doc! { "$and": [
            { "status": { "$eq": "a" } },
            { "status": { "$eq": "b" } },
            { "status": { "$eq": "c" } },
        ] },
        "a chain of one combinator stays flat"
    );
    let mixed = o::status
        .eq("a")
        .and(o::status.eq("b"))
        .or(o::status.eq("c"));
    assert_eq!(
        mixed.into_document().unwrap(),
        doc! { "$or": [
            { "$and": [
                { "status": { "$eq": "a" } },
                { "status": { "$eq": "b" } },
            ] },
            { "status": { "$eq": "c" } },
        ] },
        "a different combinator nests"
    );

    assert_eq!(
        all::<Order>([])
            .and(o::status.eq("a"))
            .into_document()
            .unwrap(),
        doc! { "$and": [{ "$and": [] }, { "status": { "$eq": "a" } }] },
        "an empty all() stays visible, the server rejects it"
    );

    assert_eq!(
        o::status
            .eq("open")
            .and(Filter::raw(doc! { "custom": 1 }))
            .into_document()
            .unwrap(),
        doc! { "$and": [
            { "status": { "$eq": "open" } },
            { "custom": 1 },
        ] }
    );
    assert_eq!(
        text::<Order>("rush")
            .and(o::total.eq(1))
            .into_document()
            .unwrap(),
        doc! { "$and": [
            { "$text": { "$search": "rush" } },
            { "total": { "$eq": 1_i64 } },
        ] }
    );
}

#[test]
fn accepted_value_forms() {
    let expected = doc! { "note": { "$eq": "hello" } };
    assert_eq!(o::note.eq("hello").into_document().unwrap(), expected);
    assert_eq!(
        o::note.eq("hello".to_string()).into_document().unwrap(),
        expected
    );
    let hello = String::from("hello");
    assert_eq!(o::note.eq(&hello).into_document().unwrap(), expected);
    assert_eq!(
        o::note
            .eq(Some("hello".to_string()))
            .into_document()
            .unwrap(),
        expected
    );
    assert_eq!(
        o::note.eq(None).into_document().unwrap(),
        doc! { "note": { "$eq": null } }
    );
    let tags = vec![String::from("a"), String::from("b")];
    assert_eq!(
        o::tags.eq(&tags).into_document().unwrap(),
        doc! { "tags": { "$eq": ["a", "b"] } }
    );
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "tickets")]
pub struct Ticket {
    pub state: State,
    pub count: u32,
    pub meta: Shipping,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Open,
    OnHold,
}

use ticket_fields as tf;

#[test]
fn values_encode_as_serde_stores_them() {
    assert_eq!(
        tf::state.eq(State::OnHold).into_document().unwrap(),
        doc! { "state": { "$eq": "on-hold" } }
    );

    let stored = mongodb::bson::to_document(&Ticket {
        state: State::Open,
        count: 3,
        meta: Shipping {
            city: "Vienna".into(),
            zip: "1010".into(),
        },
    })
    .unwrap();
    assert_eq!(
        tf::count.eq(3u32).into_document().unwrap(),
        doc! { "count": { "$eq": stored.get("count").unwrap() } }
    );
    assert_eq!(
        o::count.gt(5u64).into_document().unwrap(),
        doc! { "count": { "$gt": 5_i64 } }
    );
    assert!(
        o::count.eq(u64::MAX).into_document().is_err(),
        "a value BSON cannot hold fails instead of wrapping"
    );

    assert_eq!(
        tf::meta
            .eq(Shipping {
                city: "Vienna".into(),
                zip: "1010".into(),
            })
            .into_document()
            .unwrap(),
        doc! { "meta": { "$eq": { "cityName": "Vienna", "zip": "1010" } } }
    );
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "bad")]
pub struct Holder {
    pub map: std::collections::HashMap<i64, i64>,
}

#[test]
fn unencodable_values_defer_the_error() {
    let bad = std::collections::HashMap::from([(1_i64, 2_i64)]);
    let filter = holder_fields::map.eq(bad.clone());
    let is_bson_error = |e: &urva::Error| {
        matches!(e, urva::Error::Driver(d)
            if matches!(*d.kind, mongodb::error::ErrorKind::BsonSerialization(_)))
    };
    let copy = filter.clone();
    assert!(filter.into_document().is_err_and(|e| is_bson_error(&e)));
    assert!(copy.into_document().is_err_and(|e| is_bson_error(&e)));

    let combined = holder_fields::map.eq(bad.clone()).and(Filter::empty());
    assert!(combined.into_document().is_err());
    let combined = all([Filter::empty(), holder_fields::map.eq(bad)]);
    assert!(combined.into_document().is_err());
}

#[test]
fn nested_paths() {
    assert_eq!(
        o::shipping
            .dot(sf::city)
            .eq("Vienna")
            .into_document()
            .unwrap(),
        doc! { "shipping.cityName": { "$eq": "Vienna" } }
    );
    assert_eq!(
        o::home.dot(sf::city).eq("Vienna").into_document().unwrap(),
        doc! { "home.cityName": { "$eq": "Vienna" } }
    );
    assert_eq!(
        o::stops.dot(sf::zip).eq("1010").into_document().unwrap(),
        doc! { "stops.zip": { "$eq": "1010" } }
    );
    assert_eq!(
        o::extra_stops
            .dot(sf::city)
            .eq("Vienna")
            .into_document()
            .unwrap(),
        doc! { "extra_stops.cityName": { "$eq": "Vienna" } }
    );
    assert_eq!(
        o::stops
            .elem_match(sf::city.eq("Vienna"))
            .into_document()
            .unwrap(),
        doc! { "stops": { "$elemMatch": { "cityName": { "$eq": "Vienna" } } } }
    );
    assert_eq!(
        o::extra_stops
            .elem_match(all([sf::city.eq("Vienna"), sf::zip.eq("1010")]))
            .into_document()
            .unwrap(),
        doc! { "extra_stops": { "$elemMatch": { "$and": [
            { "cityName": { "$eq": "Vienna" } },
            { "zip": { "$eq": "1010" } },
        ] } } }
    );
}

#[test]
fn version_token_filters_by_exact_value() {
    let v: Version = mongodb::bson::from_bson(mongodb::bson::Bson::Int64(3)).unwrap();
    assert_eq!(
        o::version.eq(v).into_document().unwrap(),
        doc! { "version": { "$eq": 3_i64 } }
    );
}

#[test]
fn sorts() {
    let s = o::created_at.desc().then(o::total.asc());
    assert_eq!(
        to_vec(&s.into_document()).unwrap(),
        to_vec(&doc! { "createdAt": -1, "total": 1 }).unwrap(),
        "keys keep the order they were added in"
    );
    let s = o::created_at
        .asc()
        .then(o::total.asc())
        .then(o::created_at.desc());
    assert_eq!(
        to_vec(&s.into_document()).unwrap(),
        to_vec(&doc! { "total": 1, "createdAt": -1 }).unwrap(),
        "naming a field again moves it last with the new direction"
    );
    assert_eq!(o::_id.desc().into_document(), doc! { "_id": -1 });
}
