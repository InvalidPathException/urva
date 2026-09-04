use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_tenant, keys(tenant_id))]
pub struct Order {
    pub tenant_id: ObjectId,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "users")]
#[index(by_email, keys(email))]
pub struct User {
    pub email: String,
}

fn wrong_entity_hint(orders: &Store<Order>) {
    let _ = orders.find(Filter::empty()).hint(user_index::by_email);
}

fn main() {}
