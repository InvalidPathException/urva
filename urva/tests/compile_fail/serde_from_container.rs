use urva::prelude::*;

#[derive(Deserialize)]
pub struct RawOrder {
    pub total: i64,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
#[serde(from = "RawOrder")]
pub struct Order {
    pub total: i64,
}

impl From<RawOrder> for Order {
    fn from(raw: RawOrder) -> Order {
        Order {
            total: raw.total,
        }
    }
}

fn main() {}
