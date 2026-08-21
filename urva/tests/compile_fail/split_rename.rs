use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    #[serde(rename(serialize = "writeName", deserialize = "readName"))]
    pub total: i64,
}

fn main() {}
