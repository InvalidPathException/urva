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

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not accepted for this `{Rule}`",
    label = "a `ttl` key must serialize as a BSON date: `bson::DateTime`, `Option<bson::DateTime>`, or a type that implements `urva::Accepts<urva::TtlKey>`"
)]
pub trait Accepts<Rule> {}

pub struct TtlKey;

impl Accepts<TtlKey> for bson::DateTime {}
impl Accepts<TtlKey> for Option<bson::DateTime> {}
impl Accepts<TtlKey> for __private::CustomWritten {}

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
    use std::marker::PhantomData;

    pub trait Sealed {}

    pub struct CustomWritten;

    pub struct Seg<const WORD: u128, Rest>(PhantomData<Rest>);

    pub struct End;

    #[diagnostic::on_unimplemented(
        message = "`{Self}` has no stored field with this name",
        label = "this path segment does not resolve: the field must exist, serde must store it, and every earlier segment must be an embedded type"
    )]
    pub trait NestedField<N> {
        const STORED: &'static str;
        type Ty;
        type Declared;
    }

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
