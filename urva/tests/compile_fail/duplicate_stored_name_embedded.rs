use urva::prelude::*;

#[derive(Embedded, Serialize, Deserialize)]
pub struct Address {
    pub city: String,
    #[serde(rename = "city")]
    pub town: String,
}

fn main() {}
