use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(everything, keys(wildcard), wildcard_projection = "[\"title\"]")]
pub struct ArrayProjection {
    pub title: String,
}

fn main() {}
