use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(by_a, keys(a), collation(locale = "simple", strength = 2))]
pub struct T {
    pub a: String,
}

fn main() {}
