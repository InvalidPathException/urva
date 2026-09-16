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

#[tokio::test]
async fn heterogeneous_bulk_applies_in_order() {
    let Some(t) = TestDb::connect("bulk_ok").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    store.insert(order("open", 1)).await.unwrap();

    let outcome = store
        .bulk()
        .insert(order("new", 10))
        .update_one(o::status.eq("open"), o::total.inc(5))
        .delete_one(o::status.eq("nothing_matches"))
        .ordered(true)
        .await
        .unwrap();

    assert_eq!(
        (
            outcome.result.inserted_count,
            outcome.result.modified_count,
            outcome.result.deleted_count
        ),
        (1, 1, 0)
    );
    assert_eq!(outcome.inserted.len(), 1, "the bulk insert is handed back");
    let fresh = &outcome.inserted[0];
    assert_eq!(fresh.status, "new");
    assert_eq!(fresh.version().value(), 1, "bulk insert starts the lock");
    let stored = store
        .find_one(o::_id.eq(fresh.id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&stored, fresh);

    let updated = store.find_one(o::status.eq("open")).await.unwrap().unwrap();
    assert_eq!(updated.total, 6);

    t.drop().await;
}

#[tokio::test]
async fn empty_batches_are_the_drivers_error() {
    let Some(t) = TestDb::connect("bulk_empty").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let invalid_argument = |err: &Error| {
        matches!(err, Error::Driver(e)
            if matches!(&*e.kind, mongodb::error::ErrorKind::InvalidArgument { .. }))
    };
    let error = store.bulk().await.unwrap_err();
    assert!(invalid_argument(&error), "{error:?}");
    let failure = store
        .insert_many(Vec::<Order>::new())
        .partial()
        .await
        .unwrap_err();
    assert!(invalid_argument(&failure.error), "{:?}", failure.error);
    assert!(failure.inserted.is_empty() && failure.rejected.is_empty());

    t.drop().await;
}

#[tokio::test]
async fn ordered_after_with_options_wins() {
    let Some(t) = TestDb::connect("bulk_precedence").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let seeded = store.insert(order("open", 1)).await.unwrap();

    let error = store
        .bulk()
        .insert_with_id(*seeded.id(), order("dup", 2))
        .update_one(o::status.eq("open"), o::total.inc(5))
        .with_options({
            let mut o = mongodb::options::BulkWriteOptions::default();
            o.ordered = Some(true);
            o
        })
        .ordered(false)
        .await
        .expect_err("duplicate key at queue position 0");
    match &error {
        Error::Bulk(report) => {
            assert_eq!(
                report.applied,
                [1],
                "unordered complement: the later call won"
            );
            assert_eq!(report.error.write_errors.len(), 1);
            assert!(report.error.write_errors.contains_key(&0));
        }
        other => panic!("expected Error::Bulk, got {other:?}"),
    }
    let read = store.find_one(o::status.eq("open")).await.unwrap().unwrap();
    assert_eq!(read.total, 6);

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "racks")]
pub struct Rack {
    pub slots: Vec<Slot>,
}

#[derive(Embedded, Serialize, Deserialize, Debug)]
pub struct Slot {
    pub tag: String,
    pub bins: Vec<Bin>,
}

#[derive(Embedded, Serialize, Deserialize, Debug)]
pub struct Bin {
    pub n: i64,
}

use bin_fields as bf;
use rack_fields as r;
use slot_fields as sl;

#[tokio::test]
async fn bulk_element_filters_merge_and_conflicts_are_the_servers_error() {
    let Some(t) = TestDb::connect("bulk_element_filters").await else {
        return;
    };
    let racks: Store<Rack> = t.db.store();
    let rack = racks
        .insert(Rack {
            slots: vec![
                Slot {
                    tag: "a".into(),
                    bins: vec![Bin { n: 1 }, Bin { n: 10 }],
                },
                Slot {
                    tag: "b".into(),
                    bins: vec![Bin { n: 20 }],
                },
            ],
        })
        .await
        .unwrap();
    let id = *rack.id();

    let outer = urva::element_filter("hot", sl::tag.eq("a"));
    let inner = urva::element_filter("hot", bf::n.gt(5));
    let refused = racks
        .bulk()
        .update_one(
            r::_id.eq(id),
            r::slots
                .filtered(&outer)
                .dot(sl::bins)
                .filtered(&inner)
                .dot(bf::n)
                .inc(1),
        )
        .await;
    let refused = refused.expect_err("two element filters under one name");
    assert!(
        refused.to_string().contains("array filters"),
        "the server refuses the duplicate identifier: {:?}",
        refused
    );

    let outer = urva::element_filter("hot", sl::tag.eq("a"));
    let inner = urva::element_filter("big", bf::n.gt(5));
    let outcome = racks
        .bulk()
        .update_one(
            r::_id.eq(id),
            r::slots
                .filtered(&outer)
                .dot(sl::bins)
                .filtered(&inner)
                .dot(bf::n)
                .inc(100),
        )
        .await
        .unwrap();
    assert_eq!(outcome.result.modified_count, 1);
    assert!(outcome.inserted.is_empty());
    let read = racks.find_one(r::_id.eq(id)).await.unwrap().unwrap();
    let ns: Vec<i64> = read.slots[0].bins.iter().map(|b| b.n).collect();
    assert_eq!(ns, [1, 110], "only slot `a`'s bins over 5 moved");
    assert_eq!(read.slots[1].bins[0].n, 20);

    t.drop().await;
}

#[tokio::test]
async fn bulk_upserts_seed_the_version_field() {
    let Some(t) = TestDb::connect("bulk_upsert").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    let existing = store.insert(order("open", 1)).await.unwrap();

    let outcome = store
        .bulk()
        .update_one_with(o::status.eq("ghost"), o::total.set(7), |m| {
            m.upsert = Some(true)
        })
        .update_many_with(o::status.eq("phantom"), o::total.set(9), |m| {
            m.upsert = Some(true)
        })
        .update_one_with(o::_id.eq(existing.id()), o::total.set(2), |m| {
            m.upsert = Some(true)
        })
        .await
        .unwrap();
    assert_eq!(outcome.result.upserted_count, 2);
    assert_eq!(outcome.result.matched_count, 1);

    let ghost = store
        .find_one(o::status.eq("ghost"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!((ghost.version().value(), ghost.total), (1, 7));
    let phantom = store
        .find_one(o::status.eq("phantom"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!((phantom.version().value(), phantom.total), (1, 9));
    let matched = store
        .find_one(o::_id.eq(existing.id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (matched.version(), matched.total),
        (existing.version(), 2),
        "a matched upsert does not touch the lock"
    );

    t.drop().await;
}

#[tokio::test]
async fn bulk_inherits_the_stores_write_concern() {
    let Some(t) = TestDb::connect("bulk_write_concern").await else {
        return;
    };
    let db = t.db.client().database_with_options(
        t.db.name(),
        mongodb::options::DatabaseOptions::builder()
            .write_concern(
                mongodb::options::WriteConcern::builder()
                    .w(mongodb::options::Acknowledgment::Nodes(0))
                    .build(),
            )
            .build(),
    );
    let store: Store<Order> = db.store();

    let error = store.bulk().insert(order("a", 1)).await.unwrap_err();
    assert!(
        matches!(&error, Error::Driver(e)
            if matches!(&*e.kind, mongodb::error::ErrorKind::InvalidArgument { .. })),
        "{error:?}"
    );

    let mut majority = mongodb::options::BulkWriteOptions::default();
    majority.write_concern = Some(mongodb::options::WriteConcern::majority());
    let outcome = store
        .bulk()
        .insert(order("b", 2))
        .with_options(majority)
        .await
        .unwrap();
    assert_eq!(outcome.result.inserted_count, 1);

    t.drop().await;
}
