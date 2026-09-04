use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(everything, keys(wildcard))]
pub struct T {
    pub wildcard: bool,
}

fn main() {}
