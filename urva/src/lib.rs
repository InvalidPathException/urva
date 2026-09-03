pub use mongodb;
pub use mongodb::bson;

mod doc;
mod entity;
mod error;
mod field;
mod filter;
mod index;
mod ops;
mod sort;
mod store;
mod update;
mod version;

pub use doc::{Doc, Lock, NewId};
pub use entity::{Embedded, Entity, Unversioned, Versioned};
pub use error::{Error, Result};
pub use field::{
    ArrayLike, Capability, Encode, Encoded, Field, Filterable, Full, MatchField, MatchOnly,
    Ordered, OrderedIn, Plain, Positional, Updatable, UpdateField, VersionField,
};
pub use filter::{FieldValue, Filter, Nested, all, any, text};
pub use index::{HintFor, IndexRef};
pub use ops::count::{CountBuilder, EstimatedCountBuilder};
pub use ops::delete::{DeleteManyBuilder, DeleteOneBuilder};
pub use ops::find::{FindBuilder, FindOneBuilder};
pub use ops::find_and_modify::{
    FindOneAndDeleteBuilder, FindOneAndReplaceBuilder, FindOneAndUpdateBuilder,
};
pub use ops::partial::{Partial, PartialFailure, Summary};
pub use ops::replace::ReplaceOneBuilder;
pub use ops::save::{DeleteBuilder, InsertBuilder, InsertManyBuilder, SaveBuilder};
pub use ops::update::{UpdateManyBuilder, UpdateOneBuilder};
pub use ops::{ByFilter, ById};
pub use sort::Sort;
pub use store::{Store, StoreExt};
pub use update::{ElementFilter, Numeric, NumericIn, Update, apply, element_filter};
pub use version::Version;

pub use urva_derive::{Embedded, Entity};

#[doc(hidden)]
pub use index::{CollationSpec, ConstBson, IndexKey, IndexSpec, KeyKind, Weight};

pub mod prelude {
    pub use crate::bson::DateTime;
    pub use crate::bson::oid::ObjectId;
    pub use crate::{
        Doc, Embedded, Entity, Error, Filter, HintFor, Result, Sort, Store, StoreExt, Update,
        Version, all, any, apply, element_filter, text,
    };
    pub use mongodb::options::ReturnDocument;
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

    pub const fn index_ref_from_spec<E: ?Sized>(
        spec: &'static crate::IndexSpec,
    ) -> crate::IndexRef<E> {
        crate::IndexRef::from_spec(spec)
    }
}
