use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub total: i64,
}

fn main() {
    let _ = urva::Field::<Order, String>::new("total");
    let _ = urva::Version::committed(1);
    let _ = urva::Doc::<Order>::new(ObjectId::new(), (), Order { total: 1 });
}
