mod common;

use common::TestDb;
use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone, PartialEq)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub status: String,
    pub total: i64,
}

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
