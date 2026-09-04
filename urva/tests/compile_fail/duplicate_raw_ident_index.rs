use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_total, keys(total))]
#[index(r#by_total, keys(status))]
pub struct Order {
    pub status: String,
    pub total: i64,
}

fn main() {}
