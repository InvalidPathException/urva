use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub status: String,
}

use order_fields as o;

async fn replace_it(orders: &Store<Order>, order: &Order) -> urva::Result<()> {
    orders.find_one_and_replace(o::status.eq("open"), order).await?;
    Ok(())
}

fn main() {}
