mod common;

use common::TestDb;
use mongodb::bson::{Document, doc};
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub status: String,
    pub total: i64,
}

use order_fields as o;

fn order(status: &str, total: i64) -> Order {
    Order {
        status: status.to_string(),
        total,
    }
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "notes", id = String)]
pub struct Note {
    pub text: String,
}

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
    use note_fields as n;
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
