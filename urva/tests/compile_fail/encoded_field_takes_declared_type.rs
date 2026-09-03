use urva::prelude::*;

mod as_string {
    pub fn serialize<S: serde::Serializer>(total: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&total.to_string())
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
        let s = <String as serde::Deserialize>::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "orders")]
pub struct Order {
    #[serde(with = "as_string")]
    pub total: i64,
}

fn main() {
    let _ = order_fields::total.eq("5");
}
