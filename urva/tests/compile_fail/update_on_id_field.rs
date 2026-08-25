use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub total: i64,
}

fn set_the_id() -> Update<Order> {
    order_fields::_id.set(ObjectId::new())
}

fn unset_the_id() -> Update<Order> {
    order_fields::_id.unset()
}

fn main() {}
