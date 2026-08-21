pub use mongodb;
pub use mongodb::bson;

mod doc;
mod entity;
mod error;
mod version;

pub use doc::{Doc, Lock, NewId};
pub use entity::{Embedded, Entity, Unversioned, Versioned};
pub use error::{Error, Result};
pub use version::Version;

pub use urva_derive::{Embedded, Entity};

pub mod prelude {
    pub use crate::bson::DateTime;
    pub use crate::bson::oid::ObjectId;
    pub use crate::{Doc, Embedded, Entity, Error, Result, Version};
    pub use serde::{Deserialize, Serialize};
}

#[doc(hidden)]
pub mod __private {
    pub trait Sealed {}
}
