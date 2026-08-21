use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    #[serde(rename = "a.b")]
    pub status: String,
    #[serde(rename = "c.d")]
    pub total: i64,
}

fn main() {}
