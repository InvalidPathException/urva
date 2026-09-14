use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub status: String,
}

use order_fields as o;

async fn replace_it(orders: &Store<Order>, tx: &mut Transaction, order: &Order) -> urva::Result<()> {
    orders
        .replace_one(o::status.eq("open"), order)
        .session(tx)
        .await?;
    Ok(())
}

fn main() {}
