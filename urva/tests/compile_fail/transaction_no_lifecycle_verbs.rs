use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub total: i64,
}

async fn lifecycle_in_transaction(store: &Store<Order>, tx: &mut Transaction) -> urva::Result<()> {
    store.create_indexes().session(tx).await?;
    store.estimated_document_count().session(tx).await?;
    Ok(())
}

fn main() {}
