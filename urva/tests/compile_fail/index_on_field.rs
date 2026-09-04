use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    #[index(by_total, keys(total))]
    pub total: i64,
}

fn main() {}
