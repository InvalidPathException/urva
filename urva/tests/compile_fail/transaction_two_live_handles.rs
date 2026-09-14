use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub total: i64,
}

use order_fields as o;

async fn two_handles(store: &Store<Order>, tx: &mut Transaction) -> urva::Result<()> {
    let a = store.find_one(o::total.eq(1i64)).session(tx);
    let b = store.find_one(o::total.eq(2i64)).session(tx);
    let _ = a.await?;
    let _ = b.await?;
    Ok(())
}

fn main() {}
