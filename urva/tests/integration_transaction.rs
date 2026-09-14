mod common;

use common::TestDb;
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

async fn supports_transactions(t: &TestDb) -> bool {
    let hello =
        t.db.run_command(mongodb::bson::doc! { "hello": 1 })
            .await
            .expect("hello");
    let is_replica_set = hello.contains_key("setName");
    if !is_replica_set {
        assert!(
            std::env::var("URVA_REQUIRE_INTEGRATION").is_err(),
            "URVA_REQUIRE_INTEGRATION is set but the server is standalone: \
             the transaction tests would silently skip"
        );
        eprintln!("server is standalone; skipping transaction test");
    }
    is_replica_set
}

#[tokio::test]
async fn manual_transactions_commit_abort_and_drop() {
    let Some(t) = TestDb::connect("transaction_manual").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let mut e = store.insert(order("open", 1)).await.unwrap();

    let mut tx = client.begin().await.unwrap();
    e.total = 2;
    store.save(&mut e).session(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    let stored = store.find_one(o::_id.eq(e.id())).await.unwrap().unwrap();
    assert_eq!((stored.total, stored.version()), (2, e.version()));

    let mut tx = client.begin().await.unwrap();
    let mut copy = e.clone();
    copy.total = 3;
    store.save(&mut copy).session(&mut tx).await.unwrap();
    tx.abort().await.unwrap();
    let stored = store.find_one(o::_id.eq(e.id())).await.unwrap().unwrap();
    assert_eq!(stored.total, 2, "aborted");

    {
        let mut tx = client.begin().await.unwrap();
        let mut copy = e.clone();
        copy.total = 4;
        store.save(&mut copy).session(&mut tx).await.unwrap();
    }
    let mut stored = None;
    for _ in 0..50 {
        stored = store.find_one(o::_id.eq(e.id())).await.unwrap();
        if stored.as_ref().is_some_and(|s| s.total == 2) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    assert_eq!(stored.unwrap().total, 2, "dropping a transaction aborts it");

    e.total = 5;
    store.save(&mut e).await.unwrap();
    assert_eq!(e.version().value(), 3);

    t.drop().await;
}

#[tokio::test]
async fn transaction_options_reach_the_driver() {
    let Some(t) = TestDb::connect("transaction_options").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let options = mongodb::options::TransactionOptions::builder()
        .read_concern(mongodb::options::ReadConcern::snapshot())
        .write_concern(mongodb::options::WriteConcern::majority())
        .max_commit_time(std::time::Duration::from_secs(5))
        .build();
    let mut tx = client.begin_with(options).await.unwrap();
    let inserted = store
        .insert(order("manual", 2))
        .session(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(inserted.version().value(), 1);
    assert_eq!(inserted.body, order("manual", 2));
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 1);

    t.drop().await;
}

#[tokio::test]
async fn cross_client_store_is_a_runtime_error() {
    let Some(t) = TestDb::connect("transaction_cross_client").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let e = store.insert(order("here", 1)).await.unwrap();
    let id = *e.id();

    let uri = std::env::var("URVA_TEST_URI").unwrap();
    let other_client = mongodb::Client::with_uri_str(&uri).await.unwrap();
    let other_store: Store<Order> = other_client.database(t.db.name()).store();

    let client = t.db.client().clone();
    let mut tx = client.begin().await.unwrap();
    let result = other_store.find_one(o::_id.eq(id)).session(&mut tx).await;
    assert!(
        matches!(result, Err(Error::Driver(_))),
        "a foreign client's store surfaces a driver error: {result:?}"
    );
    tx.abort().await.unwrap();

    t.drop().await;
}

#[tokio::test]
async fn count_and_stream_read_the_transaction_snapshot() {
    let Some(t) = TestDb::connect("transaction_count_stream").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let mut tx = client.begin().await.unwrap();
    store
        .insert(order("open", 1))
        .session(&mut tx)
        .await
        .unwrap();
    store
        .insert(order("open", 2))
        .session(&mut tx)
        .await
        .unwrap();
    let n = store
        .count_documents(o::status.eq("open"))
        .session(&mut tx)
        .await
        .unwrap();
    assert_eq!(n, 2, "count sees the transaction's inserts");
    assert_eq!(
        store.count_documents(o::status.eq("open")).await.unwrap(),
        0,
        "nothing is visible outside the transaction"
    );
    let mut cursor = store
        .find(o::status.eq("open"))
        .sort(o::total.asc())
        .session(&mut tx)
        .stream()
        .await
        .unwrap();
    let mut totals = Vec::new();
    while let Some(entity) = cursor.next(&mut tx).await {
        let entity = entity.unwrap();
        store
            .update_one(o::_id.eq(entity.id()), o::status.set("seen"))
            .session(&mut tx)
            .await
            .unwrap();
        totals.push(entity.total);
    }
    assert_eq!(totals, vec![1, 2], "stream sees the transaction's inserts");
    assert_eq!(
        store
            .count_documents(o::status.eq("seen"))
            .session(&mut tx)
            .await
            .unwrap(),
        2,
        "writes interleave with an open cursor"
    );
    tx.commit().await.unwrap();

    assert_eq!(
        store.count_documents(o::status.eq("seen")).await.unwrap(),
        2,
        "committed"
    );
    t.drop().await;
}
