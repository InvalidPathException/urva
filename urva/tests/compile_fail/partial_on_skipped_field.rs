use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(open, keys(x), partial(status = "open"))]
pub struct Order {
    pub x: i64,
    #[serde(skip)]
    pub status: String,
}

fn main() {}
