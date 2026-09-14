use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug, Clone)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub total: i64,
}

async fn bypass(client: &urva::mongodb::Client, orders: &Store<Order>, order: &mut Doc<Order>) -> Result<()> {
    client
        .transaction(|mut tx| async move {
            order.total += 1;
            orders.save(order).session(&mut tx).await?;
            Ok::<_, Error>((tx, ()))
        })
        .await
}

fn main() {}
