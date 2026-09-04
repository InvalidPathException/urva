use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(keys(a))]
pub struct T {
    pub a: i64,
}

fn main() {}
