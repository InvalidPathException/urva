use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    pub total: i64,
}

async fn body(store: &Store<Order>, tx: &mut Transaction) {
    let _ = store.insert_many([Order { total: 1 }]).partial().session(tx);
}

fn main() {
    let _ = body;
}
