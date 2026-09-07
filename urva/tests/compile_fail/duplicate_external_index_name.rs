use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t")]
#[external_index(geo_a, name = "legacy-geo.1")]
#[external_index(geo_b, name = "legacy-geo.1")]
pub struct T {
    pub a: i64,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "t2")]
#[index(by_a, keys(a))]
#[external_index(other, name = "by_a")]
pub struct U {
    pub a: i64,
}

fn main() {}
