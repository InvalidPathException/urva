use std::sync::Arc;
use urva::prelude::*;

#[derive(Embedded, Serialize, Deserialize)]
pub struct Shipping {
    pub city: String,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_city, keys(home.city))]
pub struct Order {
    pub home: Arc<Shipping>,
}

fn main() {}
