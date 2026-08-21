use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    #[entity(collection = "totals")]
    pub total: i64,
}

fn main() {}
