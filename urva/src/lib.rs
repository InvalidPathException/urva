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

#[doc(hidden)]
pub mod __private {
    pub trait Sealed {}
}
