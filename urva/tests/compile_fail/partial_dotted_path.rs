use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(bad, keys(city), partial(shipping.city = "Vienna"))]
pub struct Order {
    pub city: String,
    pub shipping: Shipping,
}

#[derive(Embedded, Serialize, Deserialize)]
pub struct Shipping {
    pub city: String,
}

fn main() {}
