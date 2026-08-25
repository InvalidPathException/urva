use mongodb::bson::doc;
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub status: String,
    pub total: i64,
    pub tags: Vec<String>,
    pub labels: Option<Vec<String>>,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub items: Vec<Item>,
}

#[derive(Embedded, Serialize, Deserialize, Debug)]
pub struct Item {
    pub qty: i64,
    pub price: i64,
}

use order_fields as o;

#[test]
fn field_operators() {
    assert_eq!(
        o::status.set("shipped").into_document().unwrap(),
        doc! { "$set": { "status": "shipped" } }
    );
    assert_eq!(
        o::status.unset().into_document().unwrap(),
        doc! { "$unset": { "status": "" } }
    );
    assert_eq!(
        o::status.set_on_insert("new").into_document().unwrap(),
        doc! { "$setOnInsert": { "status": "new" } }
    );
    assert_eq!(
        o::total.inc(5).into_document().unwrap(),
        doc! { "$inc": { "total": 5_i64 } }
    );
    assert_eq!(
        o::total.mul(2).into_document().unwrap(),
        doc! { "$mul": { "total": 2_i64 } }
    );
    assert_eq!(
        o::total.min(1).into_document().unwrap(),
        doc! { "$min": { "total": 1_i64 } }
    );
    assert_eq!(
        o::created_at
            .max(DateTime::from_millis(1_000))
            .into_document()
            .unwrap(),
        doc! { "$max": { "createdAt": DateTime::from_millis(1_000) } }
    );
}

#[test]
fn array_operators() {
    assert_eq!(
        o::tags.push("rush").into_document().unwrap(),
        doc! { "$push": { "tags": "rush" } }
    );
    assert_eq!(
        o::tags.push_each(["a", "b"]).into_document().unwrap(),
        doc! { "$push": { "tags": { "$each": ["a", "b"] } } }
    );
    assert_eq!(
        o::tags
            .push_each(Vec::<String>::new())
            .into_document()
            .unwrap(),
        doc! { "$push": { "tags": { "$each": [] } } }
    );
    assert_eq!(
        o::tags.pull("rush").into_document().unwrap(),
        doc! { "$pull": { "tags": "rush" } }
    );
    assert_eq!(
        o::tags.add_to_set("gift").into_document().unwrap(),
        doc! { "$addToSet": { "tags": "gift" } }
    );
    assert_eq!(
        o::tags.pop_last().into_document().unwrap(),
        doc! { "$pop": { "tags": 1 } }
    );
    assert_eq!(
        o::labels.pop_first().into_document().unwrap(),
        doc! { "$pop": { "labels": -1 } },
        "an Option<Vec> token has the array operators"
    );
}

#[test]
fn composition() {
    let u = o::status.set("shipped").and(o::total.inc(5));
    assert_eq!(
        u.into_document().unwrap(),
        doc! { "$set": { "status": "shipped" }, "$inc": { "total": 5_i64 } }
    );
    let u = o::status
        .set("shipped")
        .and(o::created_at.set(DateTime::from_millis(0)));
    assert_eq!(
        u.into_document().unwrap(),
        doc! { "$set": { "status": "shipped", "createdAt": DateTime::from_millis(0) } },
        "the same operator merges its paths"
    );

    let u = o::total
        .inc(1)
        .and(Update::raw(doc! { "$rename": { "a": "b" } }));
    assert_eq!(
        u.into_document().unwrap(),
        doc! { "$inc": { "total": 1_i64 }, "$rename": { "a": "b" } }
    );
    let u = Update::<Order>::raw(doc! { "shipping": { "city": "a" } })
        .and(Update::raw(doc! { "shipping": { "zip": "b" } }));
    assert_eq!(
        u.into_document().unwrap(),
        doc! { "shipping": { "city": "a", "zip": "b" } }
    );

    let u = apply([o::status.set("x"), o::total.inc(5), o::tags.push("t")]);
    assert_eq!(
        u.into_document().unwrap(),
        doc! { "$set": { "status": "x" }, "$inc": { "total": 5_i64 }, "$push": { "tags": "t" } }
    );
    let collected: Update<Order> = [o::total.inc(1), o::status.set("y")].into_iter().collect();
    assert_eq!(
        collected.into_document().unwrap(),
        doc! { "$inc": { "total": 1_i64 }, "$set": { "status": "y" } }
    );
}

#[test]
fn repeated_paths_are_refused() {
    let u = o::status.set("a").and(o::status.set("b"));
    assert!(matches!(
        u.into_document(),
        Err(urva::Error::InvalidUpdate { .. })
    ));

    let u = o::tags.push("a").and(o::tags.push("b"));
    let err = u.into_document().unwrap_err().to_string();
    assert!(err.contains("push_each"), "{err}");

    let u = Update::<Order>::raw(doc! { "shipping": { "city": "a" } })
        .and(Update::raw(doc! { "shipping": { "city": "b" } }));
    assert!(matches!(
        u.into_document(),
        Err(urva::Error::InvalidUpdate { .. })
    ));

    let err = apply([o::status.set("x"), o::status.set("y")])
        .into_document()
        .unwrap_err();
    assert!(matches!(err, Error::InvalidUpdate { .. }), "{err:?}");
}

#[test]
fn version_bump_renders_inc_by_one() {
    assert_eq!(
        o::version.bump().into_document().unwrap(),
        doc! { "$inc": { "version": 1_i64 } }
    );
}
