use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_cache, keys(cached_total))]
pub struct Order {
    #[serde(skip)]
    pub cached_total: i64,
}

fn main() {}
