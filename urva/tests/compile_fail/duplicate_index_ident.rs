use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(by_a, keys(a))]
#[index(by_a, keys(b))]
pub struct T {
    pub a: i64,
    pub b: i64,
}

fn main() {}
