use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(by_missing, keys(no_such_field))]
#[index(by_other_missing, keys(nor_this_one))]
pub struct Order {
    pub status: String,
    pub created: DateTime,
}

fn main() {}
