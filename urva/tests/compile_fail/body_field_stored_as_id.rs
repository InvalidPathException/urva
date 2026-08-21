use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub _id: ObjectId,
    pub total: i64,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "tags", id = String)]
pub struct Tag {
    #[serde(rename = "_id")]
    pub slug: String,
    pub label: String,
}

fn main() {}
