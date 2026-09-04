use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(big, keys(total), partial_raw = "{\"total\": 1e999}")]
pub struct Order {
    pub total: i64,
}

fn main() {}
