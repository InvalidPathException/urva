use urva::prelude::*;

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "a")]
#[index(bad_opt, keys(x), collation(locale = "en", caseLevel = true))]
pub struct A {
    pub x: String,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "b")]
#[index(no_locale, keys(x), collation(strength = 2))]
pub struct B {
    pub x: String,
}

#[derive(Entity, Serialize, Deserialize)]
#[entity(collection = "c")]
#[index(bad_strength, keys(x), collation(locale = "en", strength = 9))]
pub struct C {
    pub x: String,
}

fn main() {}
