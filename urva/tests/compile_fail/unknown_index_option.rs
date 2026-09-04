use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_x, keys(x), uniqe)]
pub struct Order {
    pub x: i64,
}

fn main() {}
