use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(expiry, keys(label), ttl = 3600)]
pub struct T {
    pub label: String,
}

fn main() {}
