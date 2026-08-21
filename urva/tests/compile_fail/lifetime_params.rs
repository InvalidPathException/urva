use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order<'a> {
    pub title: &'a str,
}

fn main() {}
