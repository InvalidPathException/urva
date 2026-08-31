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

use item_fields as it;
use order_fields as o;

#[test]
fn field_operators() {
    assert_eq!(
        o::status
            .set("shipped")
            .into_parts()
            .map(|(d, _)| d)
            .unwrap(),
        doc! { "$set": { "status": "shipped" } }
    );
    assert_eq!(
        o::status.unset().into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$unset": { "status": "" } }
    );
    assert_eq!(
        o::status
            .set_on_insert("new")
            .into_parts()
            .map(|(d, _)| d)
            .unwrap(),
        doc! { "$setOnInsert": { "status": "new" } }
    );
    assert_eq!(
        o::total.inc(5).into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$inc": { "total": 5_i64 } }
    );
    assert_eq!(
        o::total.mul(2).into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$mul": { "total": 2_i64 } }
    );
    assert_eq!(
        o::total.min(1).into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$min": { "total": 1_i64 } }
    );
    assert_eq!(
        o::created_at
            .max(DateTime::from_millis(1_000))
            .into_parts()
            .map(|(d, _)| d)
            .unwrap(),
        doc! { "$max": { "createdAt": DateTime::from_millis(1_000) } }
    );
}

#[test]
fn array_operators() {
    assert_eq!(
        o::tags.push("rush").into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$push": { "tags": "rush" } }
    );
    assert_eq!(
        o::tags
            .push_each(["a", "b"])
            .into_parts()
            .map(|(d, _)| d)
            .unwrap(),
        doc! { "$push": { "tags": { "$each": ["a", "b"] } } }
    );
    assert_eq!(
        o::tags
            .push_each(Vec::<String>::new())
            .into_parts()
            .map(|(d, _)| d)
            .unwrap(),
        doc! { "$push": { "tags": { "$each": [] } } }
    );
    assert_eq!(
        o::tags.pull("rush").into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$pull": { "tags": "rush" } }
    );
    assert_eq!(
        o::tags
            .add_to_set("gift")
            .into_parts()
            .map(|(d, _)| d)
            .unwrap(),
        doc! { "$addToSet": { "tags": "gift" } }
    );
    assert_eq!(
        o::tags.pop_last().into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$pop": { "tags": 1 } }
    );
    assert_eq!(
        o::labels.pop_first().into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$pop": { "labels": -1 } },
        "an Option<Vec> token has the array operators"
    );
}

#[test]
fn composition() {
    let u = o::status.set("shipped").and(o::total.inc(5));
    assert_eq!(
        u.into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$set": { "status": "shipped" }, "$inc": { "total": 5_i64 } }
    );
    let u = o::status
        .set("shipped")
        .and(o::created_at.set(DateTime::from_millis(0)));
    assert_eq!(
        u.into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$set": { "status": "shipped", "createdAt": DateTime::from_millis(0) } },
        "the same operator merges its paths"
    );

    let u = o::total
        .inc(1)
        .and(Update::raw(doc! { "$rename": { "a": "b" } }));
    assert_eq!(
        u.into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$inc": { "total": 1_i64 }, "$rename": { "a": "b" } }
    );
    let u = Update::<Order>::raw(doc! { "shipping": { "city": "a" } })
        .and(Update::raw(doc! { "shipping": { "zip": "b" } }));
    assert_eq!(
        u.into_parts().map(|(d, _)| d).unwrap(),
        doc! { "shipping": { "city": "a", "zip": "b" } }
    );

    let u = apply([o::status.set("x"), o::total.inc(5), o::tags.push("t")]);
    assert_eq!(
        u.into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$set": { "status": "x" }, "$inc": { "total": 5_i64 }, "$push": { "tags": "t" } }
    );
    let collected: Update<Order> = [o::total.inc(1), o::status.set("y")].into_iter().collect();
    assert_eq!(
        collected.into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$inc": { "total": 1_i64 }, "$set": { "status": "y" } }
    );
}

#[test]
fn repeated_paths_are_refused() {
    let u = o::status.set("a").and(o::status.set("b"));
    assert!(matches!(
        u.into_parts().map(|(d, _)| d),
        Err(urva::Error::InvalidUpdate { .. })
    ));

    let u = o::tags.push("a").and(o::tags.push("b"));
    let err = u.into_parts().map(|(d, _)| d).unwrap_err().to_string();
    assert!(err.contains("push_each"), "{err}");

    let u = Update::<Order>::raw(doc! { "shipping": { "city": "a" } })
        .and(Update::raw(doc! { "shipping": { "city": "b" } }));
    assert!(matches!(
        u.into_parts().map(|(d, _)| d),
        Err(urva::Error::InvalidUpdate { .. })
    ));

    let err = apply([o::status.set("x"), o::status.set("y")])
        .into_parts()
        .map(|(d, _)| d)
        .unwrap_err();
    assert!(matches!(err, Error::InvalidUpdate { .. }), "{err:?}");
}

#[test]
fn version_bump_renders_inc_by_one() {
    assert_eq!(
        o::version.bump().into_parts().map(|(d, _)| d).unwrap(),
        doc! { "$inc": { "version": 1_i64 } }
    );
}

#[test]
fn positional_paths() {
    let (doc, filters) = o::items.each().dot(it::qty).inc(1).into_parts().unwrap();
    assert_eq!(doc, doc! { "$inc": { "items.$[].qty": 1_i64 } });
    assert!(filters.is_empty());

    let (doc, _) = o::items.matched().dot(it::qty).set(5).into_parts().unwrap();
    assert_eq!(doc, doc! { "$set": { "items.$.qty": 5_i64 } });

    let (doc, _) = o::tags.each().set("x").into_parts().unwrap();
    assert_eq!(doc, doc! { "$set": { "tags.$[]": "x" } });
    let (doc, _) = o::labels.each().set("x").into_parts().unwrap();
    assert_eq!(doc, doc! { "$set": { "labels.$[]": "x" } });
}

#[test]
fn element_filters() {
    let hot = element_filter("hot", it::qty.gt(5));
    let (doc, filters) = o::items
        .filtered(&hot)
        .dot(it::qty)
        .inc(1)
        .into_parts()
        .unwrap();
    assert_eq!(doc, doc! { "$inc": { "items.$[hot].qty": 1_i64 } });
    assert_eq!(filters, vec![doc! { "hot.qty": { "$gt": 5_i64 } }]);

    let small = element_filter::<String>("small", Filter::raw(doc! { "$lt": "m" }));
    let (doc, filters) = o::tags.filtered(&small).set("x").into_parts().unwrap();
    assert_eq!(doc, doc! { "$set": { "tags.$[small]": "x" } });
    assert_eq!(filters, vec![doc! { "small": { "$lt": "m" } }]);

    let both = element_filter("both", all([it::qty.gt(5), it::price.lt(9)]));
    let (_, filters) = o::items
        .filtered(&both)
        .dot(it::qty)
        .set(0)
        .into_parts()
        .unwrap();
    assert_eq!(
        filters,
        vec![doc! { "$and": [
            { "both.qty": { "$gt": 5_i64 } },
            { "both.price": { "$lt": 9_i64 } },
        ] }],
        "combinators are prefixed inside their branches"
    );

    let hot = element_filter("hot", it::qty.gt(5));
    let u = o::items
        .filtered(&hot)
        .dot(it::qty)
        .inc(1)
        .and(o::items.filtered(&hot).dot(it::price).inc(2));
    let (_, filters) = u.into_parts().unwrap();
    assert_eq!(filters.len(), 1, "the same filter used twice is sent once");

    let hot_a = element_filter("hot", it::qty.gt(5));
    let hot_b = element_filter("hot", it::qty.lt(2));
    let u = o::items
        .filtered(&hot_a)
        .dot(it::qty)
        .inc(1)
        .and(o::items.filtered(&hot_b).dot(it::price).inc(2));
    let (_, filters) = u.into_parts().unwrap();
    assert_eq!(
        filters.len(),
        2,
        "two filters under one name reach the server as written"
    );
}
