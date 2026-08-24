mod common;

use common::TestDb;
use futures_util::TryStreamExt;
use mongodb::bson::{Document, doc};
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "orders")]
pub struct Order {
    pub tenant_id: i64,
    pub status: String,
    #[serde(rename = "createdAt")]
    pub created_at: DateTime,
    pub total: i64,
}

use order_fields as o;

#[tokio::test]
async fn reads_on_an_empty_collection() {
    let Some(t) = TestDb::connect("read_empty").await else {
        return;
    };
    let store: Store<Order> = t.db.store();

    assert!(store.find(Filter::empty()).await.unwrap().is_empty());
    assert!(
        store
            .find_one(o::status.eq("open"))
            .await
            .unwrap()
            .is_none()
    );
    assert!(store.find_by_id(ObjectId::new()).await.unwrap().is_none());
    assert_eq!(store.count_documents(Filter::empty()).await.unwrap(), 0);
    assert_eq!(store.estimated_document_count().await.unwrap(), 0);

    let mut cursor = store
        .find(Filter::empty())
        .sort(o::created_at.desc())
        .skip(1)
        .limit(2)
        .stream()
        .await
        .unwrap();
    assert!(cursor.try_next().await.unwrap().is_none());

    t.drop().await;
}

#[tokio::test]
async fn stored_documents_read_back_as_docs() {
    let Some(t) = TestDb::connect("read_stored").await else {
        return;
    };
    let store: Store<Order> = t.db.store();
    let raw = store.raw().clone_with_type::<Document>();
    let id = ObjectId::new();
    let created_at = DateTime::from_millis(1_000);
    raw.insert_one(doc! {
        "_id": id,
        "tenant_id": 7_i64,
        "status": "open",
        "createdAt": created_at,
        "total": 100_i64,
    })
    .await
    .unwrap();

    let found = store.find_by_id(id).await.unwrap().expect("inserted");
    assert_eq!(*found.id(), id);
    assert_eq!(
        found.body,
        Order {
            tenant_id: 7,
            status: "open".to_string(),
            created_at,
            total: 100,
        }
    );
    assert_eq!(
        store.count_documents(o::status.eq("open")).await.unwrap(),
        1
    );
    assert!(
        store
            .find_one(o::status.eq("packed"))
            .await
            .unwrap()
            .is_none()
    );

    t.drop().await;
}
