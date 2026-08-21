use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "a")]
pub struct Generic<T> {
    pub value: T,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "b")]
pub enum Shape {
    Circle,
    Square,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "c")]
pub struct Tuple(pub i64);

fn main() {}
