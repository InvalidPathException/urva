use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(open_orders, keys(total), partial_raw = "[{\"total\": {\"$gt\": 1}}]")]
pub struct ArrayPartial {
    pub total: i64,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[index(open_orders, keys(total), partial_raw = "5")]
pub struct ScalarPartial {
    pub total: i64,
}

fn main() {}
