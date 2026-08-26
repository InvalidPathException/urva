pub use mongodb;
pub use mongodb::bson;

mod doc;
mod entity;
mod error;
mod field;
mod filter;
mod ops;
mod sort;
mod store;
mod update;
mod version;

pub use doc::{Doc, Lock, NewId};
pub use entity::{Embedded, Entity, Unversioned, Versioned};
pub use error::{Error, Result};
pub use field::{
    ArrayLike, Capability, Field, Filterable, Full, MatchField, MatchOnly, Ordered, Plain,
    Updatable, VersionField,
};
pub use filter::{FieldValue, Filter, all, any, text};
pub use ops::count::{CountBuilder, EstimatedCountBuilder};
pub use ops::delete::{DeleteManyBuilder, DeleteOneBuilder};
pub use ops::find::{FindBuilder, FindOneBuilder};
pub use ops::save::InsertBuilder;
pub use ops::update::{UpdateManyBuilder, UpdateOneBuilder};
pub use ops::{ByFilter, ById};
pub use sort::Sort;
pub use store::{Store, StoreExt};
pub use update::{Numeric, Update, apply};
pub use version::Version;

pub use urva_derive::{Embedded, Entity};

pub mod prelude {
    pub use crate::bson::DateTime;
    pub use crate::bson::oid::ObjectId;
    pub use crate::{
        Doc, Embedded, Entity, Error, Filter, Result, Sort, Store, StoreExt, Update, Version, all,
        any, apply, text,
    };
    pub use serde::{Deserialize, Serialize};
}

#[doc(hidden)]
pub mod __private {
    pub trait Sealed {}

    pub const fn new_field<E: ?Sized, T: ?Sized, Cap, Enc>(
        path: &'static str,
    ) -> crate::Field<E, T, Cap, Enc> {
        crate::Field::new(path)
    }

    pub const fn new_version_field<E: ?Sized>(path: &'static str) -> crate::VersionField<E> {
        crate::VersionField::new(path)
    }
}
