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

fn positional_in_filter() -> Filter<Order> {
    order_fields::items.each().dot(item_fields::qty).eq(5)
}

fn main() {}
