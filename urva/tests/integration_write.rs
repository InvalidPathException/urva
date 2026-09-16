mod common;

use common::TestDb;
use mongodb::bson::{Document, doc};
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "orders", versioned)]
#[index(by_status, keys(status))]
pub struct Order {
    pub status: String,
    pub total: i64,
}

use order_fields as o;
use order_index as ix;

fn order(status: &str, total: i64) -> Order {
    Order {
        status: status.to_string(),
        total,
    }
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "notes", id = String)]
#[index(by_text, keys(text))]
pub struct Note {
    pub text: String,
}
use note_fields as n;
use note_index as nix;

#[tokio::test]
async fn insert_returns_the_stored_document() {
    let Some(t) = TestDb::connect("insert").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let inserted = store.insert(order("open", 10)).await.unwrap();
    assert_eq!(inserted.version().value(), 1);
    assert_eq!(inserted.body, order("open", 10));

    let found = store
        .find_by_id(*inserted.id())
        .await
        .unwrap()
        .expect("inserted");
    assert_eq!(found, inserted);

    t.drop().await;
}

#[tokio::test]
async fn insert_with_id_uses_the_chosen_id() {
    let Some(t) = TestDb::connect("insert_with_id").await else {
        return;
    };
    let store: Store<Note> = t.db.store();

    let note = store
        .insert_with_id("n1".to_string(), Note { text: "a".into() })
        .await
        .unwrap();
    assert_eq!(note.id(), "n1");
    assert_eq!(
        store.find_by_id("n1").await.unwrap().expect("inserted"),
        note
    );

    let dup = store
        .insert_with_id("n1".to_string(), Note { text: "b".into() })
        .await
        .unwrap_err();
    assert!(dup.is_duplicate_key(), "{dup}");
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 1);

    t.drop().await;
}

#[tokio::test]
async fn query_level_verbs() {
    let Some(t) = TestDb::connect("query_writes").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let a = store.insert(order("open", 10)).await.unwrap();
    let b = store.insert(order("open", 20)).await.unwrap();
    store.insert(order("packed", 30)).await.unwrap();

    let res = store
        .update_one(
            o::_id.eq(a.id()).and(o::version.eq(a.version())),
            o::status.set("claimed").and(o::version.bump()),
        )
        .await
        .unwrap();
    assert_eq!(res.modified_count, 1);
    let claimed = store.find_by_id(*a.id()).await.unwrap().unwrap();
    assert_eq!(claimed.status, "claimed");
    assert_eq!(claimed.version().value(), 2, "the explicit bump counts");

    let res = store.update_by_id(b.id(), o::total.inc(5)).await.unwrap();
    assert_eq!(res.modified_count, 1);
    assert_eq!(store.find_by_id(*b.id()).await.unwrap().unwrap().total, 25);

    let res = store
        .update_many(o::status.eq("open"), o::total.inc(1))
        .await
        .unwrap();
    assert_eq!(res.modified_count, 1, "only b is still open");

    let res = store
        .update_one(o::status.eq("packed"), o::status.set("shipped"))
        .sort(o::total.desc())
        .await
        .unwrap();
    assert_eq!(res.matched_count, 1);

    let res = store.delete_one(o::status.eq("claimed")).await.unwrap();
    assert_eq!(res.deleted_count, 1);
    let res = store.delete_by_id(b.id()).await.unwrap();
    assert_eq!(res.deleted_count, 1);
    let res = store.delete_many(Filter::empty()).await.unwrap();
    assert_eq!(res.deleted_count, 1);
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 0);

    t.drop().await;
}

async fn stored(store: &Store<Order>, status: &str) -> Document {
    store
        .raw()
        .clone_with_type::<Document>()
        .find_one(doc! { "status": status })
        .await
        .unwrap()
        .expect("upserted")
}

#[tokio::test]
async fn upserts_seed_the_version_field() {
    let Some(t) = TestDb::connect("upsert_seed").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    store
        .update_one(o::status.eq("ghost"), o::total.set(1))
        .upsert()
        .await
        .unwrap();
    let read = store
        .find_one(o::status.eq("ghost"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read.version().value(), 1, "the upsert seeded the lock");
    assert_eq!(read.total, 1);

    store
        .update_one(
            o::status.eq("phantom"),
            o::status
                .set("phantom")
                .and(o::total.set_on_insert(0))
                .and(o::version.bump()),
        )
        .upsert()
        .await
        .unwrap();
    assert_eq!(
        stored(&store, "phantom").await.get_i64("version").unwrap(),
        1,
        "the bump seeded the lock"
    );

    store
        .update_one(
            o::status.eq("manual"),
            o::status
                .set("manual")
                .and(Update::raw(doc! { "$setOnInsert": { "version": 5_i64 } })),
        )
        .upsert()
        .await
        .unwrap();
    assert_eq!(
        stored(&store, "manual").await.get_i64("version").unwrap(),
        5,
        "an explicit version is left alone"
    );

    let notes: Store<Note> = t.db.store();
    notes
        .update_by_id("n1", n::text.set("x"))
        .upsert()
        .await
        .unwrap();
    let raw = notes
        .raw()
        .clone_with_type::<Document>()
        .find_one(doc! { "_id": "n1" })
        .await
        .unwrap()
        .expect("upserted by id");
    assert!(
        !raw.contains_key("version"),
        "an unversioned entity seeds nothing"
    );

    t.drop().await;
}

#[tokio::test]
async fn find_and_modify_verbs() {
    let Some(t) = TestDb::connect("write_find_and_modify").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    let a = store.insert(order("open", 1)).await.unwrap();
    store.insert(order("open", 2)).await.unwrap();

    let before = store
        .find_one_and_update(o::status.eq("open"), o::status.set("claimed"))
        .sort(o::total.desc())
        .await
        .unwrap()
        .expect("matched");
    assert_eq!((before.status.as_str(), before.total), ("open", 2));

    let after = store
        .find_one_and_update_by_id(a.id(), o::total.inc(10))
        .return_document(ReturnDocument::After)
        .await
        .unwrap()
        .expect("matched");
    assert_eq!((after.id(), after.total), (a.id(), 11));

    let created = store
        .find_one_and_update(
            o::status.eq("missing"),
            o::status.set("created").and(o::total.set_on_insert(0)),
        )
        .upsert()
        .return_document(ReturnDocument::After)
        .await
        .unwrap()
        .expect("upserted");
    assert_eq!((created.status.as_str(), created.total), ("created", 0));
    assert_eq!(created.version().value(), 1, "the upsert seeded the lock");

    let gone = store
        .find_one_and_delete(o::status.eq("claimed"))
        .await
        .unwrap()
        .expect("matched");
    assert_eq!(gone.total, 2);
    assert!(store.find_by_id(*gone.id()).await.unwrap().is_none());
    assert!(
        store
            .find_one_and_delete(o::status.eq("claimed"))
            .await
            .unwrap()
            .is_none()
    );

    t.drop().await;
}

#[tokio::test]
async fn replacement_on_unversioned_entities() {
    let Some(t) = TestDb::connect("write_replace").await else {
        return;
    };
    let notes: Store<Note> = t.db.store();

    let before = notes
        .insert_with_id("a".to_string(), Note { text: "one".into() })
        .await
        .unwrap();
    let res = notes
        .replace_one(n::_id.eq("a"), &Note { text: "two".into() })
        .await
        .unwrap();
    assert_eq!(res.modified_count, 1);
    assert_eq!(notes.find_by_id("a").await.unwrap().unwrap().text, "two");

    let replaced = notes
        .find_one_and_replace(
            n::text.eq("two"),
            &Note {
                text: "three".into(),
            },
        )
        .await
        .unwrap()
        .expect("matched");
    assert_eq!(
        (replaced.id(), replaced.text.as_str()),
        (before.id(), "two")
    );

    let after = notes
        .find_one_and_replace_by_id(
            "b",
            &Note {
                text: "four".into(),
            },
        )
        .upsert()
        .return_document(ReturnDocument::After)
        .await
        .unwrap()
        .expect("upserted");
    assert_eq!((after.id().as_str(), after.text.as_str()), ("b", "four"));

    let res = notes
        .replace_by_id(
            "c",
            &Note {
                text: "five".into(),
            },
        )
        .upsert()
        .await
        .unwrap();
    assert!(res.upserted_id.is_some());
    assert_eq!(notes.count_documents(Filter::empty()).await.unwrap(), 3);

    t.drop().await;
}

#[tokio::test]
async fn interleaved_saves_yield_version_conflict() {
    let Some(t) = TestDb::connect("race").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let doc = store.insert(order("open", 10)).await.unwrap();

    let mut first = store.find_by_id(doc.id()).await.unwrap().unwrap();
    let mut second = store.find_by_id(doc.id()).await.unwrap().unwrap();

    first.status = "packed".into();
    store.save(&mut first).await.unwrap();
    assert_eq!(first.version().value(), 2);

    second.status = "cancelled".into();
    let conflict = store.save(&mut second).await;
    assert!(
        matches!(conflict, Err(Error::VersionConflict { collection, .. }) if collection == "orders"),
        "the loser of the race gets VersionConflict: {conflict:?}"
    );
    assert_eq!(
        second.version().value(),
        1,
        "a refused save does not advance"
    );

    let mut fresh = store.find_by_id(second.id()).await.unwrap().unwrap();
    fresh.status = "cancelled".into();
    store.save(&mut fresh).await.unwrap();
    assert_eq!(fresh.version().value(), 3);

    t.drop().await;
}

#[tokio::test]
async fn deletes_honor_the_lock() {
    let Some(t) = TestDb::connect("delete").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let mut doc = store.insert(order("open", 10)).await.unwrap();
    let stale = doc.clone();

    doc.total = 11;
    store.save(&mut doc).await.unwrap();

    assert!(matches!(
        store.delete(&stale).await,
        Err(Error::VersionConflict { .. })
    ));
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 1);

    let before = doc.clone();
    store.delete(&doc).await.unwrap();
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 0);
    assert_eq!(doc, before, "delete leaves the wrapper unchanged");

    let again = store
        .insert_with_id(*doc.id(), doc.body.clone())
        .await
        .unwrap();
    assert_eq!(again.id(), doc.id());
    assert_eq!(
        again.version().value(),
        1,
        "re-insertion starts the lock over"
    );

    t.drop().await;
}

#[tokio::test]
async fn save_if_adds_a_condition_to_the_lock() {
    let Some(t) = TestDb::connect("save_if").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let mut doc = store.insert(order("open", 5)).await.unwrap();
    doc.total = 6;
    store.save_if(&mut doc, o::status.eq("open")).await.unwrap();
    assert_eq!(doc.version().value(), 2);

    doc.total = 7;
    assert!(matches!(
        store.save_if(&mut doc, o::status.eq("packed")).await,
        Err(Error::ConditionFailed { .. })
    ));
    assert_eq!(doc.version().value(), 2, "a refused save does not advance");

    let mut stale = store.find_by_id(doc.id()).await.unwrap().unwrap();
    doc.total = 8;
    store.save(&mut doc).await.unwrap();
    stale.total = 99;
    let miss = store.save_if(&mut stale, o::status.eq("open")).await;
    assert!(
        matches!(miss, Err(Error::ConditionFailed { collection, .. }) if collection == "orders"),
        "a stale version under save_if is ConditionFailed, not VersionConflict: {miss:?}"
    );
    let read = store.find_by_id(doc.id()).await.unwrap().unwrap();
    assert_eq!((read.total, read.version()), (8, doc.version()));

    t.drop().await;
}

#[tokio::test]
async fn save_options_pass_through_but_cannot_upsert_past_the_lock() {
    let Some(t) = TestDb::connect("write_protocol_options").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    let mut doc = store.insert(order("open", 1)).await.unwrap();
    let mut stale = doc.clone();
    store.save(&mut doc).await.unwrap();

    let mut options = mongodb::options::ReplaceOptions::default();
    options.upsert = Some(true);
    let err = store
        .save(&mut stale)
        .with_options(options)
        .await
        .unwrap_err();
    assert!(matches!(err, Error::VersionConflict { .. }), "{err:?}");
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 1);

    let mut options = mongodb::options::InsertOneOptions::default();
    options.comment = Some("probe".into());
    store
        .insert(order("second", 2))
        .with_options(options)
        .await
        .unwrap();
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 2);

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "flat", versioned)]
pub struct Flat {
    pub name: String,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, mongodb::bson::Bson>,
}

#[tokio::test]
async fn store_owned_fields_win_over_a_flattened_body() {
    let Some(t) = TestDb::connect("flatten_lock").await else {
        return;
    };
    let store: Store<Flat> = t.db.store();
    let foreign = ObjectId::new();
    let extra = std::collections::HashMap::from([
        ("_id".to_string(), mongodb::bson::Bson::ObjectId(foreign)),
        ("version".to_string(), mongodb::bson::Bson::Int64(7)),
    ]);
    let mut doc = store
        .insert(Flat {
            name: "a".into(),
            extra: extra.clone(),
        })
        .await
        .unwrap();
    assert_ne!(*doc.id(), foreign);
    let raw = store.raw().clone_with_type::<Document>();
    let stored = raw
        .find_one(doc! { "_id": doc.id() })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.get_i64("version").unwrap(), 1);
    assert!(
        raw.find_one(doc! { "_id": foreign })
            .await
            .unwrap()
            .is_none()
    );

    doc.name = "b".into();
    doc.extra = extra;
    store.save(&mut doc).await.unwrap();
    assert_eq!(doc.version().value(), 2);
    let stored = raw
        .find_one(doc! { "_id": doc.id() })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.get_i64("version").unwrap(), 2);
    assert_eq!(stored.get_str("name").unwrap(), "b");
    let read = store.find_by_id(doc.id()).await.unwrap().unwrap();
    assert!(
        read.extra.is_empty(),
        "the store's fields never reach the body"
    );

    t.drop().await;
}

#[tokio::test]
async fn unversioned_save_and_delete_are_unchecked() {
    let Some(t) = TestDb::connect("write_unversioned_save").await else {
        return;
    };
    let store: Store<Note> = t.db.store();
    let mut a = store
        .insert_with_id("a".to_string(), Note { text: "one".into() })
        .await
        .unwrap();
    let mut stale = store.find_by_id("a").await.unwrap().unwrap();

    a.text = "two".into();
    store.save(&mut a).await.unwrap();
    stale.text = "three".into();
    store.save(&mut stale).await.unwrap();
    let stored = store.find_by_id("a").await.unwrap().unwrap();
    assert_eq!(stored.text, "three", "last writer wins without a lock");
    let raw = store
        .raw()
        .clone_with_type::<Document>()
        .find_one(doc! { "_id": "a" })
        .await
        .unwrap()
        .unwrap();
    assert!(!raw.contains_key("version"));

    let result = store.save_if(&mut a, n::text.eq("nope")).await;
    assert!(matches!(result, Err(Error::ConditionFailed { .. })));

    store.delete(&a).await.unwrap();
    assert!(matches!(
        store.delete(&stale).await,
        Err(Error::NotFound {
            collection: "notes",
            ..
        })
    ));
    assert!(matches!(
        store.save(&mut stale).await,
        Err(Error::NotFound {
            collection: "notes",
            ..
        })
    ));

    t.drop().await;
}

#[tokio::test]
async fn insert_many_returns_docs_and_partial_failure() {
    let Some(t) = TestDb::connect("insert_many").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let docs = store
        .insert_many([order("a", 1), order("b", 2)])
        .await
        .unwrap();
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0].status, "a");
    assert_eq!(docs[1].status, "b");
    assert_ne!(docs[0].id(), docs[1].id());
    assert!(docs.iter().all(|d| d.version().value() == 1));

    let failure = store
        .insert_many_with_ids([
            (ObjectId::new(), order("c", 3)),
            (*docs[0].id(), order("dup", 4)),
            (ObjectId::new(), order("e", 5)),
        ])
        .partial()
        .await
        .expect_err("duplicate key");
    assert!(failure.error.is_duplicate_key(), "{:?}", failure.error);

    let inserted: Vec<usize> = failure.inserted.iter().map(|(i, _)| *i).collect();
    assert_eq!(inserted, [0], "ordered: only the doc before the failure");
    assert_eq!(failure.inserted[0].1.status, "c");
    assert_eq!(failure.inserted[0].1.version().value(), 1);

    let rejected: Vec<usize> = failure.rejected.iter().map(|(i, _)| *i).collect();
    assert_eq!(rejected, [1, 2], "the duplicate and everything after it");
    assert_eq!(failure.rejected[0].1.status, "dup");
    assert_eq!(failure.rejected[1].1.status, "e");
    assert!(failure.unknown.is_empty());

    let plain = store
        .insert_many_with_ids([(*docs[1].id(), order("dup", 6))])
        .await
        .expect_err("duplicate key");
    assert!(plain.is_duplicate_key(), "{plain:?}");

    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 3);

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "carts")]
pub struct Cart {
    pub items: Vec<CartItem>,
}

#[derive(Embedded, Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CartItem {
    pub sku: String,
    pub qty: i64,
}

use cart_fields as c;
use cart_item_fields as ci;

fn cart_item(sku: &str, qty: i64) -> CartItem {
    CartItem {
        sku: sku.to_string(),
        qty,
    }
}

#[tokio::test]
async fn positional_updates_apply_on_the_server() {
    let Some(t) = TestDb::connect("positional").await else {
        return;
    };
    let carts: Store<Cart> = t.db.store();
    let cart = carts
        .insert(Cart {
            items: vec![cart_item("a", 1), cart_item("b", 5), cart_item("c", 9)],
        })
        .await
        .unwrap();
    let id = *cart.id();

    carts
        .update_one(c::_id.eq(id), c::items.each().dot(ci::qty).inc(100))
        .await
        .unwrap();
    let read = carts.find_one(c::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(
        read.items.iter().map(|i| i.qty).collect::<Vec<_>>(),
        vec![101, 105, 109]
    );

    carts
        .update_one(
            all([c::_id.eq(id), c::items.dot(ci::sku).eq("b")]),
            c::items.matched().dot(ci::qty).set(0),
        )
        .await
        .unwrap();
    let read = carts.find_one(c::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(read.items[1].qty, 0);

    let err = carts
        .update_one(c::_id.eq(id), c::items.matched().dot(ci::qty).set(1))
        .await
        .unwrap_err();
    assert!(matches!(err, Error::Driver(_)), "{err:?}");

    let big = element_filter("big", ci::qty.gte(100));
    carts
        .update_one(c::_id.eq(id), c::items.filtered(&big).dot(ci::qty).set(-1))
        .await
        .unwrap();
    let read = carts.find_one(c::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(
        read.items.iter().map(|i| i.qty).collect::<Vec<_>>(),
        vec![-1, 0, -1]
    );

    t.drop().await;
}

#[tokio::test]
async fn positional_updates_through_other_verbs() {
    let Some(t) = TestDb::connect("positional_verbs").await else {
        return;
    };
    let carts: Store<Cart> = t.db.store();
    let cart = carts
        .insert(Cart {
            items: vec![cart_item("a", 1), cart_item("b", 5)],
        })
        .await
        .unwrap();
    let id = *cart.id();

    let low = element_filter("low", ci::qty.lt(3));
    let after = carts
        .find_one_and_update(c::_id.eq(id), c::items.filtered(&low).dot(ci::qty).set(3))
        .return_document(ReturnDocument::After)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        after.items.iter().map(|i| i.qty).collect::<Vec<_>>(),
        vec![3, 5]
    );

    let high = element_filter("high", ci::qty.gte(5));
    carts
        .bulk()
        .update_one(c::_id.eq(id), c::items.filtered(&high).dot(ci::qty).inc(1))
        .update_one(c::_id.eq(id), c::items.each().dot(ci::sku).set("z"))
        .ordered(true)
        .await
        .unwrap();
    let read = carts.find_one(c::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(
        read.items.iter().map(|i| i.qty).collect::<Vec<_>>(),
        vec![3, 6]
    );
    assert!(read.items.iter().all(|i| i.sku == "z"));

    t.drop().await;
}

#[tokio::test]
async fn hints_reach_the_server_on_write_builders() {
    let Some(t) = TestDb::connect("write_hints").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    store.create_indexes().await.unwrap();

    store
        .insert_many([order("open", 1), order("open", 2), order("open", 3)])
        .await
        .unwrap();

    let res = store
        .update_one(o::status.eq("open"), o::total.inc(10))
        .hint(ix::by_status)
        .await
        .unwrap();
    assert_eq!(res.modified_count, 1);

    let res = store
        .update_many(o::status.eq("open"), o::total.inc(100))
        .hint(ix::by_status)
        .await
        .unwrap();
    assert_eq!(res.modified_count, 3);

    let before = store
        .find_one_and_update(o::status.eq("open"), o::status.set("claimed"))
        .hint(ix::by_status)
        .await
        .unwrap();
    assert!(before.is_some());

    let res = store
        .delete_one(o::status.eq("claimed"))
        .hint(ix::by_status)
        .await
        .unwrap();
    assert_eq!(res.deleted_count, 1);

    let res = store
        .delete_many(o::status.eq("open"))
        .hint(ix::by_status)
        .await
        .unwrap();
    assert_eq!(res.deleted_count, 2);

    store.insert(order("open", 4)).await.unwrap();
    let bogus = store
        .update_one(o::status.eq("open"), o::total.inc(1))
        .hint(mongodb::options::Hint::Name("no_such_index".into()))
        .await;
    assert!(matches!(bogus, Err(Error::Driver(_))), "{bogus:?}");

    let notes: Store<Note> = t.db.store();
    notes.create_indexes().await.unwrap();
    notes
        .insert_with_id("h1".into(), Note { text: "old".into() })
        .await
        .unwrap();
    let replacement = Note { text: "new".into() };
    let res = notes
        .replace_one(n::text.eq("old"), &replacement)
        .hint(nix::by_text)
        .await
        .unwrap();
    assert_eq!(res.modified_count, 1);
    let bogus = notes
        .replace_one(n::text.eq("new"), &replacement)
        .hint(mongodb::options::Hint::Name("no_such_index".into()))
        .await;
    assert!(matches!(bogus, Err(Error::Driver(_))), "{bogus:?}");

    t.drop().await;
}
