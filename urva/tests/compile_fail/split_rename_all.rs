use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct Order {
    pub total_amount: i64,
}

fn main() {}
