use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize, Debug)]
#[entity(collection = "orders")]
#[index(search, keys(title = text), weights(title = 10, title = 2))]
pub struct Order {
    pub title: String,
}

fn main() {}
