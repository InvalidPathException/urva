use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub status: String,
}

impl urva::Unversioned for Order {}

fn main() {}
