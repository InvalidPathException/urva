use urva::prelude::*;

#[derive(Embedded, Serialize, Deserialize)]
pub struct Item {
    pub sku: String,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[index(by_sku, keys(items.skuu))]
pub struct T {
    pub items: Vec<Item>,
}

fn main() {}
