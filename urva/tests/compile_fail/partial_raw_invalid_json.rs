use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(open, keys(x), partial_raw = "{\"status\": }")]
pub struct Order {
    pub x: i64,
    pub status: String,
}

fn main() {}
