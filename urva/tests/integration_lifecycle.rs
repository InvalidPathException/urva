mod common;

use common::TestDb;
use futures_util::TryStreamExt;
use mongodb::IndexModel;
use mongodb::bson::doc;
use urva::lifecycle::NameDrift;
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders")]
#[index(tenant_recent, keys(tenant_id, created_at = -1), unique)]
#[index(order_search,  keys(title = text, body = text), weights(title = 10))]
#[index(by_city, keys(city), collation(locale = "en", strength = 2))]
#[index(by_city_simple, keys(city, title), collation(locale = "simple"))]
#[index(everything, keys(wildcard), wildcard_projection = "{\"title\": 1}")]
#[index(geo,           keys(location = 2d), bits = 26)]
#[index(ghost, keys(body), hidden)]
#[index(sparse_title, keys(title), sparse)]
#[external_index(legacy_geo)]
pub struct Order {
    pub tenant_id: ObjectId,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub title: String,
    pub body: String,
    pub city: String,
    pub location: Vec<f64>,
}

async fn list_snapshot(store: &Store<Order>) -> Vec<mongodb::bson::Document> {
    let models: Vec<IndexModel> = store
        .raw()
        .list_indexes()
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    let mut docs: Vec<_> = models
        .into_iter()
        .map(|m| mongodb::bson::to_document(&m).unwrap())
        .collect();
    docs.sort_by_key(|d| d.get_str("name").unwrap_or_default().to_string());
    docs
}

async fn create_external(store: &Store<Order>) {
    store
        .raw()
        .create_index(
            IndexModel::builder()
                .keys(doc! { "location": "2dsphere" })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .name("legacy_geo".to_string())
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn create_verify_and_no_op_sync() {
    let Some(t) = TestDb::connect("lifecycle_noop").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    store.create_indexes().await.unwrap();

    let err = store.verify_indexes().await.unwrap_err();
    match err {
        Error::IndexDrift(diff) => {
            assert_eq!(diff.missing_external, ["legacy_geo"]);
            assert!(diff.to_create.is_empty());
        }
        other => panic!("expected IndexDrift, got {other:?}"),
    }

    create_external(&store).await;
    store.verify_indexes().await.unwrap();

    let before = list_snapshot(&store).await;
    let diff = store.sync_indexes().await.unwrap();
    assert!(
        diff.is_empty(),
        "no-op sync reports an empty diff: {diff:?}"
    );
    let after = list_snapshot(&store).await;
    assert_eq!(before, after, "listIndexes snapshot unchanged by sync");

    store.create_indexes().await.unwrap();
    assert_eq!(before, list_snapshot(&store).await);

    t.drop().await;
}

#[tokio::test]
async fn sync_drops_undeclared_but_never_external() {
    let Some(t) = TestDb::connect("lifecycle_drop").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    store.create_indexes().await.unwrap();
    create_external(&store).await;

    store
        .raw()
        .create_index(IndexModel::builder().keys(doc! { "city": -1 }).build())
        .await
        .unwrap();

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(diff.to_drop, ["city_-1"]);
    assert!(
        !diff.fails_verify(),
        "undeclared indexes do not fail verify"
    );

    let acted = store.sync_indexes().await.unwrap();
    assert_eq!(acted.to_drop, ["city_-1"]);

    let names: Vec<String> = list_snapshot(&store)
        .await
        .iter()
        .map(|d| d.get_str("name").unwrap().to_string())
        .collect();
    assert!(
        !names.contains(&"city_-1".to_string()),
        "undeclared dropped"
    );
    assert!(
        names.contains(&"legacy_geo".to_string()),
        "external index survives sync"
    );

    t.drop().await;
}

#[tokio::test]
async fn name_drift_fails_verify_and_sync_renames() {
    let Some(t) = TestDb::connect("lifecycle_drift").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    store.create_indexes().await.unwrap();
    create_external(&store).await;

    store.raw().drop_index("tenant_recent").await.unwrap();
    store
        .raw()
        .create_index(
            IndexModel::builder()
                .keys(doc! { "tenant_id": 1, "createdAt": -1 })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .name("tenant_recent_v1".to_string())
                        .unique(true)
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(
        diff.name_drift,
        [NameDrift {
            expected: "tenant_recent".into(),
            actual: "tenant_recent_v1".into(),
        }]
    );
    assert!(
        diff.to_drop.is_empty(),
        "drift is reported on its own, not as a drop"
    );
    assert!(store.verify_indexes().await.is_err(), "drift fails verify");

    let synced = store.sync_indexes().await.unwrap();
    assert_eq!(synced, diff);
    let names: Vec<String> = list_snapshot(&store)
        .await
        .iter()
        .map(|d| d.get_str("name").unwrap().to_string())
        .collect();
    assert!(names.contains(&"tenant_recent".to_string()));
    assert!(!names.contains(&"tenant_recent_v1".to_string()));
    store.verify_indexes().await.unwrap();
    assert!(store.sync_indexes().await.unwrap().is_empty());

    t.drop().await;
}

#[tokio::test]
async fn mismatched_structure_is_recreated_by_sync() {
    let Some(t) = TestDb::connect("lifecycle_mismatch").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    store.create_indexes().await.unwrap();
    create_external(&store).await;

    store.raw().drop_index("tenant_recent").await.unwrap();
    store
        .raw()
        .create_index(
            IndexModel::builder()
                .keys(doc! { "tenant_id": 1, "createdAt": -1 })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .name("tenant_recent".to_string())
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(diff.mismatched, ["tenant_recent"]);

    store.sync_indexes().await.unwrap();
    store.verify_indexes().await.unwrap();

    t.drop().await;
}

#[tokio::test]
async fn failed_creation_leaves_the_gap_in_the_diff() {
    let Some(t) = TestDb::connect("lifecycle_create_fails").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    let tenant = ObjectId::new();
    let when = mongodb::bson::DateTime::from_millis(1_000);
    t.db.collection::<mongodb::bson::Document>("orders")
        .insert_many([
            doc! { "tenant_id": tenant, "createdAt": when },
            doc! { "tenant_id": tenant, "createdAt": when },
        ])
        .await
        .unwrap();
    for (name, keys) in [
        ("undeclared_extra", doc! { "city": 1 }),
        ("tenant_recent", doc! { "tenant_id": 1, "createdAt": -1 }),
    ] {
        store
            .raw()
            .create_index(
                IndexModel::builder()
                    .keys(keys)
                    .options(
                        mongodb::options::IndexOptions::builder()
                            .name(name.to_string())
                            .build(),
                    )
                    .build(),
            )
            .await
            .unwrap();
    }
    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(diff.mismatched, ["tenant_recent"]);
    assert_eq!(diff.to_drop, ["undeclared_extra"]);

    let err = store.sync_indexes().await.unwrap_err();
    assert!(matches!(err, urva::Error::Driver(_)), "got {err}");

    let models = list_snapshot(&store).await;
    assert!(
        !models
            .iter()
            .any(|d| d.get_str("name") == Ok("undeclared_extra")),
        "drops run before the failing creation"
    );
    assert!(
        !models
            .iter()
            .any(|d| d.get_str("name") == Ok("tenant_recent")),
        "the mismatched index was dropped and its unique rebuild refused"
    );
    let diff = store.diff_indexes().await.unwrap();
    assert!(diff.to_create.contains(&"tenant_recent".to_string()));
    assert!(diff.to_drop.is_empty());
    assert!(matches!(
        store.verify_indexes().await,
        Err(urva::Error::IndexDrift(_))
    ));

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "expiring")]
#[index(expiry, keys(expires_at), ttl = 3600)]
pub struct Expiring {
    pub expires_at: DateTime,
}

#[tokio::test]
async fn double_typed_ttl_on_the_server_is_a_no_op() {
    let Some(t) = TestDb::connect("ttl_double").await else {
        return;
    };
    let store: Store<Expiring> = t.db.store();
    t.db.run_command(mongodb::bson::doc! {
        "createIndexes": "expiring",
        "indexes": [{
            "key": { "expires_at": 1 },
            "name": "expiry",
            "expireAfterSeconds": mongodb::bson::Bson::Double(3600.0),
        }],
    })
    .await
    .unwrap();
    let diff = store.diff_indexes().await.unwrap();
    assert!(diff.is_empty(), "{diff:?}");
    store.verify_indexes().await.unwrap();
    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "swapped")]
#[index(alpha, keys(x))]
#[index(beta, keys(y))]
pub struct Swapped {
    pub x: i64,
    pub y: i64,
}

#[tokio::test]
async fn sync_rebuilds_pattern_swapped_mismatched_indexes() {
    let Some(t) = TestDb::connect("lifecycle_swap").await else {
        return;
    };
    let store: Store<Swapped> = t.db.store();

    for (name, key) in [("alpha", "y"), ("beta", "x")] {
        store
            .raw()
            .create_index(
                IndexModel::builder()
                    .keys(doc! { key: 1 })
                    .options(
                        mongodb::options::IndexOptions::builder()
                            .name(name.to_string())
                            .build(),
                    )
                    .build(),
            )
            .await
            .unwrap();
    }

    let diff = store.diff_indexes().await.unwrap();
    let mut mismatched = diff.mismatched.clone();
    mismatched.sort();
    assert_eq!(mismatched, vec!["alpha".to_string(), "beta".to_string()]);

    store.sync_indexes().await.unwrap();

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(
        diff,
        urva::lifecycle::IndexDiff::default(),
        "sync must converge in one run"
    );
    store.verify_indexes().await.unwrap();
    t.drop().await;
}

#[allow(clippy::duplicated_attributes)]
#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "twin_lifecycle")]
#[index(open_by_status, keys(status), partial(status = "open"))]
#[index(closed_by_status, keys(status), partial(status = "closed"))]
#[index(status_full, keys(status))]
#[index(status_de, keys(status), collation(locale = "de", strength = 2))]
pub struct TwinOrder {
    pub status: String,
}

#[tokio::test]
async fn same_pattern_twins_create_and_sync_cleanly() {
    let Some(t) = TestDb::connect("lifecycle_twins").await else {
        return;
    };
    let store: Store<TwinOrder> = t.db.store();

    store.create_indexes().await.unwrap();
    let diff = store.diff_indexes().await.unwrap();
    assert!(diff.is_empty(), "twins diff clean: {diff:?}");

    let diff = store.sync_indexes().await.unwrap();
    assert!(
        diff.to_create.is_empty() && diff.to_drop.is_empty() && diff.mismatched.is_empty(),
        "no-op sync: {diff:?}"
    );
    store.verify_indexes().await.unwrap();

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "geo_kinds")]
#[index(by_slug, keys(slug = hashed))]
#[index(geo_sphere, keys(location = 2dsphere))]
#[index(flat_geo, keys(flat = 2d), bits = 28, min = -500, max = 500)]
pub struct GeoKinds {
    pub slug: String,
    pub location: Vec<f64>,
    pub flat: Vec<f64>,
}

#[tokio::test]
async fn hashed_and_geo_index_kinds_create_and_diff_clean() {
    let Some(t) = TestDb::connect("lifecycle_geo_kinds").await else {
        return;
    };
    let store: Store<GeoKinds> = t.db.store();
    store.create_indexes().await.unwrap();

    let models: Vec<IndexModel> = store
        .raw()
        .list_indexes()
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    let by_name = |name: &str| {
        models
            .iter()
            .find(|m| m.options.as_ref().and_then(|o| o.name.as_deref()) == Some(name))
            .unwrap_or_else(|| panic!("index {name} listed"))
    };

    assert_eq!(by_name("by_slug").keys, doc! { "slug": "hashed" });
    assert_eq!(by_name("geo_sphere").keys, doc! { "location": "2dsphere" });

    let flat = by_name("flat_geo");
    assert_eq!(flat.keys, doc! { "flat": "2d" });
    let options = flat.options.as_ref().unwrap();
    assert_eq!(options.bits, Some(28));
    assert_eq!(options.min, Some(-500.0));
    assert_eq!(options.max, Some(500.0));

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(
        diff,
        urva::lifecycle::IndexDiff::default(),
        "no-op diff over the listed kinds"
    );

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "deferred")]
#[index(fresh, keys(a))]
#[index(fresh_text, keys(b = text))]
pub struct Deferred {
    pub a: i64,
    pub b: String,
    pub c: String,
}

#[tokio::test]
async fn sync_defers_creations_conflicting_with_removed_indexes() {
    let Some(t) = TestDb::connect("lifecycle_deferred").await else {
        return;
    };
    let store: Store<Deferred> = t.db.store();

    store
        .raw()
        .create_index(
            IndexModel::builder()
                .keys(doc! { "a": 1 })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .name("stale".to_string())
                        .unique(true)
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();
    store
        .raw()
        .create_index(
            IndexModel::builder()
                .keys(doc! { "c": "text" })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .name("old_text".to_string())
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(diff.to_create, ["fresh", "fresh_text"]);
    assert_eq!(
        {
            let mut drops = diff.to_drop.clone();
            drops.sort();
            drops
        },
        ["old_text", "stale"]
    );

    store.sync_indexes().await.unwrap();
    let names: Vec<String> = store
        .raw()
        .list_indexes()
        .await
        .unwrap()
        .try_collect::<Vec<IndexModel>>()
        .await
        .unwrap()
        .into_iter()
        .filter_map(|m| m.options.and_then(|o| o.name))
        .collect();
    let mut names = names;
    names.sort();
    assert_eq!(names, ["_id_", "fresh", "fresh_text"]);
    assert!(store.diff_indexes().await.unwrap().is_empty());

    t.drop().await;
}

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "pinned")]
#[index(pin_simple, keys(s), collation(locale = "simple"))]
pub struct SimplePin {
    pub s: String,
}

#[tokio::test]
async fn simple_collation_drift_is_mismatched_and_synced() {
    let Some(t) = TestDb::connect("lifecycle_simple_drift").await else {
        return;
    };
    t.db.create_collection("pinned")
        .collation(
            mongodb::options::Collation::builder()
                .locale("de".to_string())
                .build(),
        )
        .await
        .unwrap();
    t.db.collection::<mongodb::bson::Document>("pinned")
        .create_index(
            IndexModel::builder()
                .keys(doc! { "s": 1 })
                .options(
                    mongodb::options::IndexOptions::builder()
                        .name("pin_simple".to_string())
                        .build(),
                )
                .build(),
        )
        .await
        .unwrap();
    let store: Store<SimplePin> = t.db.store();
    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(diff.mismatched, vec!["pin_simple".to_string()], "{diff:?}");

    store.sync_indexes().await.unwrap();
    assert!(store.diff_indexes().await.unwrap().is_empty());
    let listed: Vec<IndexModel> = store
        .raw()
        .list_indexes()
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    let pinned = listed
        .iter()
        .find(|m| {
            m.options
                .as_ref()
                .and_then(|o| o.name.as_deref())
                .is_some_and(|n| n == "pin_simple")
        })
        .expect("recreated");
    assert!(
        pinned
            .options
            .as_ref()
            .is_none_or(|o| o.collation.is_none()),
        "{pinned:?}"
    );

    t.drop().await;
}
