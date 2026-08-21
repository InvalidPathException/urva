use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders", versioned)]
pub struct Order {
    pub version: i64,
    pub total: i64,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "notes", versioned = "rev")]
pub struct Note {
    #[serde(rename = "rev")]
    pub revision: u32,
    pub text: String,
}

fn main() {}
