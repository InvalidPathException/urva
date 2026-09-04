use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_x, keys(x = 3))]
pub struct Order {
    pub x: i64,
}

fn main() {}
