use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "a")]
#[index(expiry, keys(at = hashed), ttl = 60)]
pub struct A {
    pub at: DateTime,
}

fn main() {}
