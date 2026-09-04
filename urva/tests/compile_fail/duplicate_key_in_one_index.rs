use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_total, keys(total, total = -1))]
pub struct Order {
    pub total: i64,
}

fn main() {}
