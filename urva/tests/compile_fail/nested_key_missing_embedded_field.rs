use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(ship_city, keys(shipping.city))]
pub struct T {
    pub shipping: Shipping,
}

#[derive(Embedded, Serialize, Deserialize)]
pub struct Shipping {
    pub zip: String,
}

fn main() {}
