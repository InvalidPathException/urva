use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(deep, keys(shipping.carrier.wrong))]
pub struct Order {
    pub shipping: Shipping,
}

#[derive(Embedded, Serialize, Deserialize)]
pub struct Shipping {
    pub carrier: Carrier,
}

#[derive(Embedded, Serialize, Deserialize)]
pub struct Carrier {
    pub code: String,
}

fn main() {}
