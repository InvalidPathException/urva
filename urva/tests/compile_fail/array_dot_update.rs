use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub items: Vec<Item>,
}

#[derive(Embedded, Serialize, Deserialize)]
pub struct Item {
    pub qty: i64,
}

fn array_dot_update() -> Update<Order> {
    order_fields::items.dot(item_fields::qty).inc(1)
}

fn main() {}
