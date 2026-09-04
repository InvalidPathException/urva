use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(search, keys(title = text), weights(missing = 5))]
pub struct Order {
    pub title: String,
}

fn main() {}
