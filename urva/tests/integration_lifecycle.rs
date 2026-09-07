mod common;

use common::TestDb;
use futures_util::TryStreamExt;
use mongodb::IndexModel;
use mongodb::bson::doc;
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

    let diff = store.diff_indexes().await.unwrap();
    assert_eq!(diff.missing_external, ["legacy_geo"]);
    assert!(diff.to_create.is_empty());
    assert!(diff.mismatched.is_empty() && diff.name_drift.is_empty());

    create_external(&store).await;
    let diff = store.diff_indexes().await.unwrap();
    assert!(
        diff.is_empty(),
        "faithful server state is a no-op: {diff:?}"
    );

    let before = list_snapshot(&store).await;
    store.create_indexes().await.unwrap();
    assert_eq!(
        before,
        list_snapshot(&store).await,
        "a second create_indexes changes nothing"
    );

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
