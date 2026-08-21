use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
pub struct T {
    #[serde(rename = "a.b")]
    pub weird: String,
}

fn main() {}
