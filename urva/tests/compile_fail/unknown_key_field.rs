use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(by_missing, keys(missing))]
pub struct T {
    pub a: i64,
}

fn main() {}
