use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(open, keys(x), partial(missing = "open"))]
pub struct Order {
    pub x: i64,
}

fn main() {}
