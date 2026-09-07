use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(by_a, keys(a))]
#[external_index(by_a)]
pub struct T {
    pub a: i64,
}

fn main() {}
