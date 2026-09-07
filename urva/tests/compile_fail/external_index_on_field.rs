use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    #[external_index(legacy_total)]
    pub total: i64,
}

fn main() {}
