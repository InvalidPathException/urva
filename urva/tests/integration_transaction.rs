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
async fn forced_abort_rolls_back_and_refetch_recovers() {
    let Some(t) = TestDb::connect("transaction_abort").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let e = store.insert(order("abort_me", 1)).await.unwrap();
    let committed_version = e.version();

    let (store, e) = (&store, &e);
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            let mut e = e.clone();
            e.total = 100;
            store.save(&mut e).session(&mut tx).await?;

            Err(forced_abort())
        })
        .await;
    assert!(matches!(result, Err(Error::InvalidUpdate { .. })));

    assert_eq!(
        (e.total, e.version()),
        (1, committed_version),
        "the caller's value is untouched"
    );

    let mut fresh = store.find_one(o::_id.eq(e.id())).await.unwrap().unwrap();
    assert_eq!(fresh.total, 1, "server rolled back");
    assert_eq!(fresh.version(), committed_version);
    fresh.total = 5;
    store.save(&mut fresh).await.unwrap();

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
async fn save_after_in_transaction_delete_is_not_persisted() {
    let Some(t) = TestDb::connect("transaction_absent").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let e = store.insert(order("victim", 1)).await.unwrap();
    let id = *e.id();

    let (store, e) = (&store, &e);
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            let mut e = e.clone();
            store.delete(&e).session(&mut tx).await?;
            store.save(&mut e).session(&mut tx).await?;
            Ok((tx, ()))
        })
        .await;
    assert!(
        matches!(result, Err(urva::Error::VersionConflict { .. })),
        "got {result:?}"
    );

    let stored = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(stored.total, 1);

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

#[tokio::test]
async fn transactional_batch_failures_are_plain_errors() {
    let Some(t) = TestDb::connect("txn_batch_failure").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let taken = *store.insert(order("taken", 0)).await.unwrap().id();

    let store = &store;
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            let error = store
                .insert_many_with_ids([
                    (ObjectId::new(), order("a", 1)),
                    (taken, order("dup", 2)),
                    (ObjectId::new(), order("c", 3)),
                ])
                .session(&mut tx)
                .await
                .expect_err("duplicate id");
            assert!(error.is_duplicate_key(), "{error:?}");
            Err(error)
        })
        .await;
    assert!(result.unwrap_err().is_duplicate_key());

    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            let error = store
                .bulk()
                .insert(order("a", 1))
                .insert_with_id(taken, order("dup", 2))
                .insert(order("c", 3))
                .ordered(false)
                .session(&mut tx)
                .await
                .expect_err("duplicate id");
            assert!(error.is_duplicate_key(), "{error:?}");
            Err(error)
        })
        .await;
    assert!(result.unwrap_err().is_duplicate_key());
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 1);

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "notes")]
pub struct Note {
    pub text: String,
}

use note_fields as n;

fn note(text: &str) -> Note {
    Note {
        text: text.to_string(),
    }
}

#[tokio::test]
async fn save_if_inside_a_transaction() {
    let Some(t) = TestDb::connect("transaction_save_if").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let e = store.insert(order("open", 1)).await.unwrap();
    let id = *e.id();

    let store = &store;
    let e = &e;
    let e = client
        .transaction(|mut tx| async move {
            let mut e = e.clone();
            e.total = 2;
            store
                .save_if(&mut e, o::status.eq("open"))
                .session(&mut tx)
                .await?;
            Ok::<_, Error>((tx, e))
        })
        .await
        .unwrap();
    let read = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!((read.total, read.version()), (2, e.version()));

    let fresh = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    let fresh = &fresh;
    let miss: urva::Result<()> = client
        .transaction(|mut tx| async move {
            let mut e = fresh.clone();
            e.total = 99;
            store
                .save_if(&mut e, o::status.eq("closed"))
                .session(&mut tx)
                .await?;
            Ok((tx, ()))
        })
        .await;
    assert!(
        matches!(miss, Err(Error::ConditionFailed { .. })),
        "{miss:?}"
    );
    let read = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(read.total, 2, "aborted save_if left the document untouched");

    t.drop().await;
}

#[tokio::test]
async fn unversioned_entity_inside_a_transaction() {
    let Some(t) = TestDb::connect("transaction_unversioned").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let notes: Store<Note> = t.db.store();
    let client = t.db.client().clone();

    let notes_ref = &notes;
    client
        .transaction(|mut tx| async move {
            let keep = notes_ref.insert(note("draft")).session(&mut tx).await?;
            let remove = notes_ref.insert(note("gone")).session(&mut tx).await?;
            notes_ref
                .replace_one(n::_id.eq(keep.id()), &note("final"))
                .session(&mut tx)
                .await?;
            notes_ref
                .delete_one(n::_id.eq(remove.id()))
                .session(&mut tx)
                .await?;
            Ok::<_, Error>((tx, ()))
        })
        .await
        .unwrap();

    let stored = notes.find(Filter::empty()).await.unwrap();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].text, "final");

    t.drop().await;
}

#[tokio::test]
async fn transactional_insert_of_taken_id_is_the_servers_duplicate_key() {
    let Some(t) = TestDb::connect("transaction_taken_id").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let store = &store;
    let result: urva::Result<Doc<Order>> = client
        .transaction(|mut tx| async move {
            let first = store.insert(order("first", 1)).session(&mut tx).await?;
            store
                .insert_with_id(*first.id(), order("second", 2))
                .session(&mut tx)
                .await?;
            Ok((tx, first))
        })
        .await;
    let err = result.unwrap_err();
    assert!(err.is_duplicate_key(), "{err:?}");
    assert_eq!(
        store.count_documents(Filter::empty()).await.unwrap(),
        0,
        "the duplicate key aborted the whole transaction"
    );

    t.drop().await;
}

#[tokio::test]
async fn failed_commit_is_retried_without_rerunning_the_body() {
    let Some(t) = TestDb::connect("transaction_commit_retry").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }

    let uri = std::env::var("URVA_TEST_URI").unwrap();
    let mut options = mongodb::options::ClientOptions::parse(&uri).await.unwrap();
    options.app_name = Some("urva_commit_retry".to_string());
    let client = mongodb::Client::with_options(options).unwrap();
    let store: Store<Order> = client.database(t.db.name()).store();

    let e = store.insert(order("open", 1)).await.unwrap();
    let id = *e.id();

    let admin = t.db.client().database("admin");
    let armed = admin
        .run_command(mongodb::bson::doc! {
            "configureFailPoint": "failCommand",
            "mode": { "times": 2 },
            "data": {
                "failCommands": ["commitTransaction"],
                "errorCode": 91,
                "appName": "urva_commit_retry",
            },
        })
        .await;
    if armed.is_err() {
        eprintln!(
            "failCommand unavailable (server without enableTestCommands); \
             skipping failed_commit_is_retried_without_rerunning_the_body"
        );
        t.drop().await;
        return;
    }

    let bodies = AtomicU32::new(0);
    let (store, e_ref, bodies_ref) = (&store, &e, &bodies);
    let result = client
        .transaction(|mut tx| async move {
            bodies_ref.fetch_add(1, Ordering::SeqCst);
            let mut e = e_ref.clone();
            e.total = 2;
            store.save(&mut e).session(&mut tx).await?;
            Ok::<_, Error>((tx, e))
        })
        .await;

    let _ = admin
        .run_command(mongodb::bson::doc! { "configureFailPoint": "failCommand", "mode": "off" })
        .await;

    let e = result.unwrap();
    assert_eq!(
        bodies.load(Ordering::SeqCst),
        1,
        "the commit was retried; the body was not re-run"
    );

    let stored = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!(stored.total, 2, "the retried commit landed the change");
    assert_eq!(
        stored.version(),
        e.version(),
        "the returned version equals the committed one"
    );

    t.drop().await;
}

#[tokio::test]
async fn query_level_writes_leave_the_version_check_to_the_server() {
    let Some(t) = TestDb::connect("transaction_query_writes").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let store = &store;

    for tracked in [false, true] {
        let e = store.insert(order("open", 1)).await.unwrap();
        let id = *e.id();
        let e = &e;
        let result: urva::Result<()> = client
            .transaction(|mut tx| async move {
                let mut e = e.clone();
                if tracked {
                    e.total = 2;
                    store.save(&mut e).session(&mut tx).await?;
                }
                store
                    .update_one(o::_id.eq(id), o::version.bump())
                    .session(&mut tx)
                    .await?;
                e.total = 3;
                store.save(&mut e).session(&mut tx).await?;
                Ok((tx, ()))
            })
            .await;
        assert!(
            matches!(result, Err(Error::VersionConflict { .. })),
            "tracked={tracked}: expected VersionConflict, got {result:?}"
        );
    }

    let e = store.insert(order("open", 1)).await.unwrap();
    let id = *e.id();
    let e = &e;
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            let mut e = e.clone();
            e.total = 2;
            store.save(&mut e).session(&mut tx).await?;
            store.delete_one(o::_id.eq(id)).session(&mut tx).await?;
            e.total = 3;
            store.save(&mut e).session(&mut tx).await?;
            Ok((tx, ()))
        })
        .await;
    assert!(
        matches!(result, Err(Error::VersionConflict { .. })),
        "expected VersionConflict after the query delete, got {result:?}"
    );

    let a = store.insert(order("keep", 1)).await.unwrap();
    let b = store.insert(order("drop", 2)).await.unwrap();
    let a = &a;
    let a = client
        .transaction(|mut tx| async move {
            let mut a = a.clone();
            a.total = 2;
            store.save(&mut a).session(&mut tx).await?;
            let deleted = store
                .delete_many(o::status.eq("drop"))
                .session(&mut tx)
                .await?;
            assert_eq!(deleted.deleted_count, 1);
            a.total = 3;
            store.save(&mut a).session(&mut tx).await?;
            Ok::<_, Error>((tx, a))
        })
        .await
        .unwrap();
    let read = store.find_one(o::_id.eq(a.id())).await.unwrap().unwrap();
    assert_eq!((read.total, read.version()), (3, a.version()));
    assert!(store.find_one(o::_id.eq(b.id())).await.unwrap().is_none());

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

async fn retried_save_after_outside_writes(
    name: &str,
    outside_writes: usize,
    reread: bool,
) -> (urva::Result<Doc<Order>>, Doc<Order>) {
    let t = TestDb::connect(name).await.expect("URVA_TEST_URI");
    if !supports_transactions(&t).await {
        panic!("replica set required");
    }
    let store: Store<Order> = t.db.store();
    let doc = store.insert(order("doc", 0)).await.unwrap();
    let doc_id = *doc.id();
    let contended = store.insert(order("contended", 0)).await.unwrap();
    let contended_id = *contended.id();

    let (rival, release) = hold_rival(&t, contended_id).await;

    let client = t.db.client().clone();
    let attempts = AtomicU32::new(0);
    let (store, attempts_ref, doc, release_ref) = (&store, &attempts, &doc, &release);

    let result = client
        .transaction(|mut tx| async move {
            let attempt = attempts_ref.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 2 {
                for _ in 0..outside_writes {
                    let mut copy = store.find_one(o::_id.eq(doc_id)).await?.unwrap();
                    copy.total += 100;
                    store.save(&mut copy).await?;
                }
            }
            let mut current = if reread {
                store
                    .find_one(o::_id.eq(doc_id))
                    .session(&mut tx)
                    .await?
                    .unwrap()
            } else {
                doc.clone()
            };
            current.total += 1;
            store.save(&mut current).session(&mut tx).await?;
            if attempt == 1 {
                store
                    .update_one(o::_id.eq(contended_id), o::total.inc(1))
                    .session(&mut tx)
                    .await
                    .inspect_err(|_| release_ref.notify_one())?;
            }
            Ok::<_, Error>((tx, current))
        })
        .await;
    release.notify_one();
    rival.await.unwrap().unwrap();
    assert_eq!(attempts.load(Ordering::SeqCst), 2, "exactly one retry");
    let stored = store.find_one(o::_id.eq(doc_id)).await.unwrap().unwrap();
    t.drop().await;
    (result, stored)
}

#[tokio::test]
async fn retried_attempt_saves_a_fresh_in_transaction_read() {
    if TestDb::connect("probe").await.is_none() {
        return;
    }
    let (result, stored) = retried_save_after_outside_writes("retry_reread_one", 1, true).await;
    let saved = result.expect("a fresh read inside the retried attempt saves");
    assert_eq!((saved.total, saved.version().value()), (101, 3));
    assert_eq!(stored.version(), saved.version());
    assert_eq!(stored.total, saved.total);
    let (result, stored) = retried_save_after_outside_writes("retry_reread_zero", 0, true).await;
    let saved = result.unwrap();
    assert_eq!((saved.total, saved.version().value()), (1, 2));
    assert_eq!(stored.version(), saved.version());
}

#[tokio::test]
async fn retried_attempt_reusing_the_callers_copy_starts_from_it() {
    if TestDb::connect("probe").await.is_none() {
        return;
    }
    let (result, stored) = retried_save_after_outside_writes("retry_stale_state", 1, false).await;
    assert!(
        matches!(result, Err(urva::Error::VersionConflict { .. })),
        "stale copy must conflict, got {result:?}"
    );
    assert_eq!(stored.total, 100);

    let (result, stored) = retried_save_after_outside_writes("retry_same_state", 0, false).await;
    let saved = result.expect("without an outside write the retry succeeds");
    assert_eq!(
        (saved.total, saved.version().value()),
        (1, 2),
        "the retry restarted from the caller's copy, not the first attempt's mutation"
    );
    assert_eq!(stored.version(), saved.version());
}

#[tokio::test]
async fn dropped_attempt_leaves_the_callers_value_and_the_server_untouched() {
    let Some(t) = TestDb::connect("transaction_dropped").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let mut e = store.insert(order("open", 1)).await.unwrap();
    let id = *e.id();
    let committed = e.version();

    let client = t.db.client().clone();
    {
        let (store, e) = (&store, &e);
        let attempt = client.transaction(|mut tx| async move {
            let mut e = e.clone();
            e.total = 2;
            store.save(&mut e).session(&mut tx).await?;
            std::future::pending::<()>().await;
            Ok::<_, Error>((tx, ()))
        });
        let timed_out = tokio::time::timeout(std::time::Duration::from_millis(500), attempt).await;
        assert!(timed_out.is_err());
    }

    assert_eq!(
        (e.total, e.version()),
        (1, committed),
        "the dropped attempt never touched the caller's value"
    );
    let mut fresh = None;
    for _ in 0..50 {
        fresh = store.find_one(o::_id.eq(id)).await.unwrap();
        if fresh.as_ref().is_some_and(|f| f.total == 1) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    }
    assert_eq!(
        fresh.unwrap().total,
        1,
        "the server rolled the attempt back"
    );
    e.total = 3;
    store.save(&mut e).await.unwrap();
    assert_eq!(e.version().value(), 2);

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

#[tokio::test]
async fn transactional_insert_many_and_bulk_roll_back_with_the_transaction() {
    let Some(t) = TestDb::connect("txn_batches").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();

    let many = vec![order("a", 0), order("b", 0)];
    let one = order("c", 0);
    let (store, many, one) = (&store, &many, &one);
    let outcome: urva::Result<Vec<Doc<Order>>> = client
        .transaction(|mut tx| async move {
            let inserted = store.insert_many(many.clone()).session(&mut tx).await?;
            let bulk = store
                .bulk()
                .insert(one.clone())
                .update_one(o::status.eq("a"), o::total.inc(5))
                .session(&mut tx)
                .await?;
            assert_eq!(
                bulk.inserted[0].version().value(),
                1,
                "the attempt hands back the inserted doc"
            );
            assert!(inserted.iter().all(|d| d.version().value() == 1));
            Err(forced_abort())
        })
        .await;
    assert!(
        matches!(outcome, Err(Error::InvalidUpdate { .. })),
        "{outcome:?}"
    );
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 0);

    let docs = client
        .transaction(|mut tx| async move {
            let mut docs = store.insert_many(many.clone()).session(&mut tx).await?;
            let bulk = store
                .bulk()
                .insert(one.clone())
                .update_one(o::status.eq("a"), o::total.inc(5))
                .session(&mut tx)
                .await?;
            assert_eq!(
                (bulk.result.inserted_count, bulk.result.modified_count),
                (1, 1)
            );
            docs.extend(bulk.inserted);
            Ok::<_, Error>((tx, docs))
        })
        .await
        .unwrap();
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 3);
    assert_eq!(docs.len(), 3);
    let a = store.find_one(o::status.eq("a")).await.unwrap().unwrap();
    assert_eq!(
        (a.id(), a.total, a.version()),
        (docs[0].id(), 5, docs[0].version())
    );

    t.drop().await;
}

#[derive(Debug)]
enum AppError {
    Rule(&'static str),
    Urva(Error),
}

impl From<Error> for AppError {
    fn from(error: Error) -> Self {
        AppError::Urva(error)
    }
}

impl TransactionError for AppError {}

fn check_rule(total: i64) -> std::result::Result<(), AppError> {
    if total > 1 {
        return Err(AppError::Rule("insufficient funds"));
    }
    Ok(())
}

#[tokio::test]
async fn application_errors_abort_and_propagate_verbatim() {
    let Some(t) = TestDb::connect("transaction_app_error").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let e = store.insert(order("open", 1)).await.unwrap();
    let committed = e.version();

    let (store, e_ref) = (&store, &e);
    let result = client
        .try_transaction(|mut tx| async move {
            let mut e = e_ref.clone();
            e.total = 2;
            store.save(&mut e).session(&mut tx).await?;
            check_rule(e.total)?;
            Ok((tx, e))
        })
        .await;
    assert!(
        matches!(result, Err(AppError::Rule("insufficient funds"))),
        "{result:?}"
    );
    assert_eq!(
        (e.total, e.version()),
        (1, committed),
        "caller's value untouched"
    );
    let stored = store.find_one(o::_id.eq(e.id())).await.unwrap().unwrap();
    assert_eq!(stored.total, 1, "the attempt was aborted");

    let saved = client
        .try_transaction(|mut tx| async move {
            let mut e = e_ref.clone();
            e.total = 0;
            store.save(&mut e).session(&mut tx).await?;
            check_rule(e.total)?;
            Ok::<_, AppError>((tx, e))
        })
        .await
        .unwrap();
    assert_eq!(saved.version().value(), committed.value() + 1);

    t.drop().await;
}

#[tokio::test]
async fn app_errors_without_as_driver_still_retry_transient_conflicts() {
    let Some(t) = TestDb::connect("transaction_app_error_retry").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let counter = store.insert(order("counter", 0)).await.unwrap();
    let id = *counter.id();
    let client = t.db.client().clone();
    let attempts = Arc::new(AtomicU32::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));

    let mut tasks = Vec::new();
    for _ in 0..2 {
        let client = client.clone();
        let store = store.clone();
        let attempts = attempts.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            let (store, attempts) = (&store, &attempts);
            let mut first_attempt = true;
            client
                .try_transaction(|mut tx| {
                    let barrier = first_attempt.then(|| barrier.clone());
                    first_attempt = false;
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        let mut current = store
                            .find_one(o::_id.eq(id))
                            .session(&mut tx)
                            .await?
                            .expect("counter exists");
                        if let Some(barrier) = barrier {
                            barrier.wait().await;
                        }
                        current.total += 1;
                        store.save(&mut current).session(&mut tx).await?;
                        check_rule(0)?;
                        Ok::<_, AppError>((tx, ()))
                    }
                })
                .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap_or_else(|e| match e {
            AppError::Urva(e) => panic!("urva error escaped the retry loop: {e}"),
            AppError::Rule(r) => panic!("rule error: {r}"),
        });
    }

    let after = store.find_one(o::_id.eq(id)).await.unwrap().unwrap();
    assert_eq!((after.total, after.version().value()), (2, 3));
    assert!(
        attempts.load(Ordering::SeqCst) >= 3,
        "the transient path ran"
    );

    t.drop().await;
}

#[tokio::test]
async fn aborted_save_resolves_back_to_the_committed_version() {
    let Some(t) = TestDb::connect("transaction_abort_resolves").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let mut holder = store.insert(order("open", 1)).await.unwrap();

    let mut tx = client.begin().await.unwrap();
    holder.status = "packed".to_string();
    store.save(&mut holder).session(&mut tx).await.unwrap();
    assert_eq!(
        holder.version().value(),
        2,
        "reads as bumped while the tx is open"
    );
    holder.total = 2;
    store.save(&mut holder).session(&mut tx).await.unwrap();
    assert_eq!(
        holder.version().value(),
        3,
        "a second save chains on the pending bump"
    );
    tx.abort().await.unwrap();
    assert_eq!(
        holder.version().value(),
        1,
        "abort discards the pending bumps"
    );

    let mut rival = store.find_by_id(holder.id()).await.unwrap().unwrap();
    rival.status = "cancelled".to_string();
    store.save(&mut rival).await.unwrap();

    let stale = store.save(&mut holder).await;
    assert!(
        matches!(stale, Err(Error::VersionConflict { .. })),
        "{stale:?}"
    );
    let stored = store.find_by_id(holder.id()).await.unwrap().unwrap();
    assert_eq!(
        (stored.status.as_str(), stored.version().value()),
        ("cancelled", 2)
    );

    t.drop().await;
}

#[tokio::test]
async fn dropped_transaction_resolves_the_save_back_too() {
    let Some(t) = TestDb::connect("transaction_drop_resolves").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let mut holder = store.insert(order("open", 1)).await.unwrap();
    {
        let mut tx = client.begin().await.unwrap();
        holder.total = 9;
        store.save(&mut holder).session(&mut tx).await.unwrap();
        assert_eq!(holder.version().value(), 2);
    }
    assert_eq!(holder.version().value(), 1);
    holder.total = 3;
    store.save(&mut holder).await.unwrap();
    assert_eq!(holder.version().value(), 2);
    let stored = store.find_by_id(holder.id()).await.unwrap().unwrap();
    assert_eq!((stored.total, stored.version().value()), (3, 2));
    t.drop().await;
}

#[tokio::test]
async fn committed_save_keeps_the_bump_and_later_saves_build_on_it() {
    let Some(t) = TestDb::connect("transaction_commit_resolves").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let mut holder = store.insert(order("open", 1)).await.unwrap();

    let mut tx = client.begin().await.unwrap();
    holder.total = 2;
    store.save(&mut holder).session(&mut tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(holder.version().value(), 2);

    let clone = holder.clone();
    assert_eq!(clone.version().value(), 2);
    assert_eq!(clone, holder);

    let mut tx = client.begin().await.unwrap();
    holder.total = 3;
    store.save(&mut holder).session(&mut tx).await.unwrap();
    assert_eq!(holder.version().value(), 3);
    tx.commit().await.unwrap();

    holder.total = 4;
    store.save(&mut holder).await.unwrap();
    assert_eq!(holder.version().value(), 4);
    let stored = store.find_by_id(holder.id()).await.unwrap().unwrap();
    assert_eq!((stored.total, stored.version().value()), (4, 4));
    assert_eq!(stored, holder);

    let mut tx = client.begin().await.unwrap();
    holder.total = 5;
    store.save(&mut holder).session(&mut tx).await.unwrap();
    tx.abort().await.unwrap();
    assert_eq!(
        holder.version().value(),
        4,
        "a committed bump folds before a new pending one"
    );
    let stale_delete = store.delete(&holder).await;
    assert!(stale_delete.is_ok(), "{stale_delete:?}");
    t.drop().await;
}

#[tokio::test]
async fn shared_doc_mutated_inside_a_failed_body_resolves_back() {
    let Some(t) = TestDb::connect("transaction_shared_doc").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let seed = store.insert(order("open", 1)).await.unwrap();
    let shared = Arc::new(tokio::sync::Mutex::new(seed.clone()));

    let (store_ref, shared_ref) = (&store, &shared);
    let result: Result<()> = client
        .transaction(|mut tx| async move {
            let mut guard = shared_ref.lock().await;
            guard.total = 2;
            store_ref.save(&mut guard).session(&mut tx).await?;
            assert_eq!(guard.version().value(), 2);
            drop(guard);
            Err(Error::InvalidUpdate {
                message: "domain failure after the save".into(),
            })
        })
        .await;
    assert!(result.is_err());
    let mut held = shared.lock().await;
    assert_eq!(held.version().value(), 1);

    let mut rival = store.find_by_id(seed.id()).await.unwrap().unwrap();
    rival.status = "cancelled".to_string();
    store.save(&mut rival).await.unwrap();
    assert!(matches!(
        store.save(&mut held).await,
        Err(Error::VersionConflict { .. })
    ));
    t.drop().await;
}

#[tokio::test]
async fn unknown_commit_outcome_resolves_to_the_prior_version() {
    let Some(t) = TestDb::connect("transaction_unknown_commit").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let mut holder = store.insert(order("open", 1)).await.unwrap();

    let mut tx = client.begin().await.unwrap();
    holder.total = 2;
    store.save(&mut holder).session(&mut tx).await.unwrap();
    let admin = client.database("admin");
    admin
        .run_command(mongodb::bson::doc! {
            "configureFailPoint": "failCommand",
            "mode": { "times": 1 },
            "data": {
                "failCommands": ["commitTransaction"],
                "errorCode": 91,
                "errorLabels": ["UnknownTransactionCommitResult"],
                "closeConnection": false,
            }
        })
        .await
        .unwrap();
    let committed = tx.commit().await;
    admin
        .run_command(mongodb::bson::doc! { "configureFailPoint": "failCommand", "mode": "off" })
        .await
        .unwrap();
    assert!(committed.is_ok(), "the retry commits: {committed:?}");
    assert_eq!(holder.version().value(), 2);
    let stored = store.find_by_id(holder.id()).await.unwrap().unwrap();
    assert_eq!(stored.version().value(), 2);
    t.drop().await;
}

#[tokio::test]
async fn body_that_swallows_a_failed_op_gets_that_error_instead_of_a_rerun() {
    let Some(t) = TestDb::connect("transaction_swallowed_failure").await else {
        return;
    };
    if !supports_transactions(&t).await {
        return;
    }
    let store: Store<Order> = t.db.store();
    let client = t.db.client().clone();
    let taken_id = *store.insert(order("taken", 1)).await.unwrap().id();

    let bodies = AtomicU32::new(0);
    let (store, bodies_ref) = (&store, &bodies);
    let result: urva::Result<()> = client
        .transaction(|mut tx| async move {
            bodies_ref.fetch_add(1, Ordering::SeqCst);
            let inserted = store
                .insert_with_id(taken_id, order("again", 2))
                .session(&mut tx)
                .await;
            assert!(inserted.is_err());
            Ok((tx, ()))
        })
        .await;
    let err = result.unwrap_err();
    assert!(err.is_duplicate_key(), "{err:?}");
    assert_eq!(bodies.load(Ordering::SeqCst), 1, "the body was not rerun");

    let mut tx = client.begin().await.unwrap();
    let inserted = store
        .insert_with_id(taken_id, order("again", 2))
        .session(&mut tx)
        .await;
    assert!(inserted.is_err());
    let committed = tx.commit().await.unwrap_err();
    assert!(committed.is_duplicate_key(), "{committed:?}");
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 1);

    t.drop().await;
}
