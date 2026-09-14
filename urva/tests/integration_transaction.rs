mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use common::TestDb;
use tokio::sync::Notify;
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

fn forced_abort() -> Error {
    Error::InvalidUpdate {
        message: "forced abort".to_string(),
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
async fn version_conflict_propagates_unretried() {
    let Some(t) = TestDb::connect("transaction_version_conflict").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let mut e = store.insert(order("open", 1)).await.unwrap();

    let stale = e.clone();
    e.total = 2;
    store.save(&mut e).await.unwrap();

    let client = t.db.client().clone();
    let attempts = AtomicU32::new(0);
    let (store, stale, attempts) = (&store, &stale, &attempts);
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            let mut stale = stale.clone();
            stale.total = 99;
            store.save(&mut stale).session(&mut tx).await?;
            Ok((tx, ()))
        })
        .await;

    assert!(matches!(result, Err(Error::VersionConflict { .. })));
    assert_eq!(attempts.load(Ordering::SeqCst), 1, "no auto-retry");

    let after = store.find_one(o::status.eq("open")).await.unwrap().unwrap();
    assert_eq!(after.total, 2, "aborted transaction wrote nothing");

    t.drop().await;
}

#[tokio::test]
async fn transient_retry_reruns_the_body_from_the_callers_values() {
    let Some(t) = TestDb::connect("transaction_retry").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let contended = store.insert(order("contended", 0)).await.unwrap();
    let contended_id = *contended.id();

    let (rival, release) = hold_rival(&t, contended_id).await;

    let client = t.db.client().clone();
    let attempts = AtomicU32::new(0);
    let saved = store.insert(order("saved", 1)).await.unwrap();
    let body = order("inserted", 7);

    let (store, attempts, saved, body, release_ref) = (&store, &attempts, &saved, &body, &release);
    let result = client
        .transaction(|mut tx| async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            let mut saved = saved.clone();
            saved.total += 10;
            store.save(&mut saved).session(&mut tx).await?;
            let inserted = store.insert(body.clone()).session(&mut tx).await?;
            store
                .update_one(o::_id.eq(contended_id), o::total.inc(1))
                .session(&mut tx)
                .await
                .inspect_err(|_| release_ref.notify_one())?;
            Ok::<_, Error>((tx, (saved, inserted)))
        })
        .await;

    release.notify_one();
    let (committed, inserted) = result.unwrap();
    rival.await.unwrap().unwrap();

    assert!(
        attempts.load(Ordering::SeqCst) >= 2,
        "the body was retried transiently"
    );
    assert_eq!(saved.total, 1, "the caller's value is untouched");

    let stored = store
        .find_one(o::_id.eq(saved.id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (stored.total, stored.version()),
        (11, committed.version()),
        "every attempt started from the caller's value, so the increment applied once"
    );
    let n = store
        .count_documents(o::status.eq("inserted"))
        .await
        .unwrap();
    assert_eq!(n, 1);
    let stored = store
        .find_one(o::_id.eq(inserted.id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored, inserted);

    t.drop().await;
}

#[tokio::test]
async fn two_saves_within_one_attempt() {
    let Some(t) = TestDb::connect("transaction_two_saves").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let e = store.insert(order("twice", 1)).await.unwrap();

    let (store, e) = (&store, &e);
    let e = client
        .transaction(|mut tx| async move {
            let mut e = e.clone();
            e.total = 2;
            store.save(&mut e).session(&mut tx).await?;
            e.total = 3;
            store.save(&mut e).session(&mut tx).await?;
            Ok::<_, Error>((tx, e))
        })
        .await
        .unwrap();

    let stored = store.find_one(o::_id.eq(e.id())).await.unwrap().unwrap();
    assert_eq!(stored.total, 3);
    assert_eq!(stored.version(), e.version(), "committed version returned");

    t.drop().await;
}

#[tokio::test]
async fn delete_then_reinsert_commits() {
    let Some(t) = TestDb::connect("transaction_reinsert").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let mut original = store.insert(order("original", 1)).await.unwrap();
    original.total = 2;
    store.save(&mut original).await.unwrap();
    original.total = 3;
    store.save(&mut original).await.unwrap();
    let id = *original.id();
    let original_version = original.version();

    let (store, original) = (&store, &original);
    let replacement = client
        .transaction(|mut tx| async move {
            store.delete(original).session(&mut tx).await?;
            let mut reinserted = store
                .insert_with_id(*original.id(), order("replacement", 10))
                .session(&mut tx)
                .await?;
            reinserted.total = 11;
            store.save(&mut reinserted).session(&mut tx).await?;
            Ok::<_, Error>((tx, reinserted))
        })
        .await
        .unwrap();

    let stored = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(stored.status, "replacement");
    assert_eq!(stored.total, 11);
    assert_eq!(
        stored.version(),
        replacement.version(),
        "reinserted lock restarted"
    );
    assert_eq!(stored.version().value(), 2);
    assert_ne!(stored.version(), original_version);
    let n = store.count_documents(o::_id.eq(id)).await.unwrap();
    assert_eq!(n, 1);

    t.drop().await;
}

#[tokio::test]
async fn delete_then_reinsert_survives_transient_retry() {
    let Some(t) = TestDb::connect("transaction_reinsert_retry").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();

    let mut original = store.insert(order("original", 1)).await.unwrap();
    original.total = 2;
    store.save(&mut original).await.unwrap();
    let id = *original.id();

    let contended = store.insert(order("contended", 0)).await.unwrap();
    let contended_id = *contended.id();

    let (rival, release) = hold_rival(&t, contended_id).await;

    let client = t.db.client().clone();
    let attempts = AtomicU32::new(0);
    let (store, original, attempts, release_ref) = (&store, &original, &attempts, &release);
    let result = client
        .transaction(|mut tx| async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            store.delete(original).session(&mut tx).await?;
            let replacement = store
                .insert_with_id(*original.id(), order("replacement", 10))
                .session(&mut tx)
                .await?;

            store
                .update_one(o::_id.eq(contended_id), o::total.inc(1))
                .session(&mut tx)
                .await
                .inspect_err(|_| release_ref.notify_one())?;
            Ok::<_, Error>((tx, replacement))
        })
        .await;

    release.notify_one();
    let replacement = result.unwrap();
    rival.await.unwrap().unwrap();

    assert!(
        attempts.load(Ordering::SeqCst) >= 2,
        "the body was retried transiently"
    );
    let stored = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(stored.status, "replacement");
    assert_eq!(stored.version(), replacement.version());
    let n = store
        .count_documents(o::status.eq("replacement"))
        .await
        .unwrap();
    assert_eq!(n, 1);

    t.drop().await;
}

#[tokio::test]
async fn aborted_insert_recovers_by_reinserting() {
    let Some(t) = TestDb::connect("transaction_aborted_insert").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let pending = order("aborted", 1);
    let (store, pending_ref) = (&store, &pending);
    let result: urva::Result<Doc<Order>> = client
        .transaction(|mut tx| async move {
            store.insert(pending_ref.clone()).session(&mut tx).await?;

            Err(forced_abort())
        })
        .await;
    assert!(matches!(result, Err(Error::InvalidUpdate { .. })));
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 0);

    let doc = store.insert(pending).await.unwrap();
    let stored = store.find_one(o::_id.eq(doc.id())).await.unwrap().unwrap();
    assert_eq!(stored, doc);

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
    let other_store = &other_store;
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            other_store.find_one(o::_id.eq(id)).session(&mut tx).await?;
            Ok((tx, ()))
        })
        .await;
    assert!(
        matches!(result, Err(Error::Driver(_))),
        "a foreign client's store surfaces a driver error: {result:?}"
    );

    t.drop().await;
}

#[tokio::test]
async fn nested_transactions_are_independent() {
    let Some(t) = TestDb::connect("transaction_nested").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let outer_doc = store.insert(order("outer", 1)).await.unwrap();
    let inner_doc = store.insert(order("inner", 1)).await.unwrap();
    let (outer_id, inner_id) = (*outer_doc.id(), *inner_doc.id());

    let client = t.db.client().clone();
    let (client_ref, store) = (&client, &store);
    client
        .transaction(|mut tx| async move {
            store
                .update_one(o::_id.eq(outer_id), o::total.set(2))
                .session(&mut tx)
                .await?;

            client_ref
                .transaction(|mut inner| async move {
                    store
                        .update_one(o::_id.eq(inner_id), o::total.set(20))
                        .session(&mut inner)
                        .await?;
                    Ok::<_, Error>((inner, ()))
                })
                .await?;

            store
                .update_one(o::_id.eq(outer_id), o::status.set("outer_done"))
                .session(&mut tx)
                .await?;
            Ok::<_, Error>((tx, ()))
        })
        .await
        .unwrap();

    let outer_read = store.find_one(o::_id.eq(outer_id)).await.unwrap().unwrap();
    assert_eq!(outer_read.total, 2);
    assert_eq!(outer_read.status, "outer_done");
    let inner_read = store.find_one(o::_id.eq(inner_id)).await.unwrap().unwrap();
    assert_eq!(inner_read.total, 20);

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

    let store = &store;
    client
        .transaction(|mut tx| async move {
            store.insert(order("open", 1)).session(&mut tx).await?;
            store.insert(order("open", 2)).session(&mut tx).await?;
            let n = store
                .count_documents(o::status.eq("open"))
                .session(&mut tx)
                .await?;
            assert_eq!(n, 2, "count sees the attempt's inserts");
            let mut cursor = store
                .find(o::status.eq("open"))
                .sort(o::total.asc())
                .session(&mut tx)
                .stream()
                .await?;
            let mut totals = Vec::new();
            while let Some(entity) = cursor.next(&mut tx).await {
                let entity = entity?;
                store
                    .update_one(o::_id.eq(entity.id()), o::status.set("seen"))
                    .session(&mut tx)
                    .await?;
                totals.push(entity.total);
            }
            assert_eq!(totals, vec![1, 2], "stream sees the attempt's inserts");
            assert_eq!(
                store
                    .count_documents(o::status.eq("seen"))
                    .session(&mut tx)
                    .await?,
                2,
                "writes interleave with an open cursor"
            );
            Ok::<_, Error>((tx, ()))
        })
        .await
        .unwrap();

    assert_eq!(
        store.count_documents(o::status.eq("seen")).await.unwrap(),
        2,
        "committed"
    );
    t.drop().await;
}

async fn hold_rival(
    t: &TestDb,
    contended_id: ObjectId,
) -> (tokio::task::JoinHandle<urva::Result<()>>, Arc<Notify>) {
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let client = t.db.client().clone();
    let db_name = t.db.name().to_string();
    let (started_for_body, release_for_body) = (started.clone(), release.clone());
    let handle = tokio::spawn(async move {
        let store: Store<Order> = client.database(&db_name).store();
        let (store, started, release) = (&store, &started_for_body, &release_for_body);
        client
            .transaction(|mut tx| async move {
                store
                    .update_one(o::_id.eq(contended_id), o::status.set("held"))
                    .session(&mut tx)
                    .await?;
                started.notify_one();
                release.notified().await;
                Ok((tx, ()))
            })
            .await
    });
    started.notified().await;
    (handle, release)
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
    let e = order("open", 1);
    let (store, e_ref) = (&store, &e);
    let inserted = client
        .transaction_with(options.clone(), |mut tx| async move {
            let inserted = store.insert(e_ref.clone()).session(&mut tx).await?;
            Ok::<_, Error>((tx, inserted))
        })
        .await
        .unwrap();
    assert_eq!(inserted.version().value(), 1);
    assert_eq!(inserted.body, e);

    let mut tx = client.begin_with(options).await.unwrap();
    store
        .insert(order("manual", 2))
        .session(&mut tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 2);

    t.drop().await;
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
