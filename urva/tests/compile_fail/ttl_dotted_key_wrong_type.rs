use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "a")]
#[index(expiry, keys(inner.at), ttl = 60)]
pub struct A {
    pub inner: Inner,
}

#[derive(Embedded, Serialize, Deserialize)]
pub struct Inner {
    pub at: String,
}

fn main() {}
