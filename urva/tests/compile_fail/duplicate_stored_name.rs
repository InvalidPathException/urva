use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
pub struct T {
    pub status: String,
    #[serde(rename = "status")]
    pub state: String,
}

fn main() {}
