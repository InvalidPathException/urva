use std::borrow::Cow;
use std::marker::PhantomData;

use mongodb::bson::{Bson, Document, ser};

pub struct Full;

pub struct MatchOnly;

pub struct Positional;

pub struct Plain;

pub struct Encoded<X: ?Sized>(PhantomData<fn() -> Box<X>>);

pub trait Encode<T: ?Sized> {
    fn encode(value: &T) -> Result<Bson, ser::Error>;
}

pub type MatchField<E, T> = Field<E, T, MatchOnly>;

pub type UpdateField<E, T> = Field<E, T, Positional>;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Full {}
    impl Sealed for super::MatchOnly {}
    impl Sealed for super::Positional {}
    impl<T> Sealed for Vec<T> {}
    impl<T> Sealed for Option<Vec<T>> {}
}

pub trait Capability: sealed::Sealed {}
impl Capability for Full {}
impl Capability for MatchOnly {}
impl Capability for Positional {}

pub trait Filterable: Capability {}
impl Filterable for Full {}
impl Filterable for MatchOnly {}

pub trait Updatable: Capability {}
impl Updatable for Full {}
impl Updatable for Positional {}

pub trait ArrayLike: sealed::Sealed {
    type Elem;
}
impl<T> ArrayLike for Vec<T> {
    type Elem = T;
}
impl<T> ArrayLike for Option<Vec<T>> {
    type Elem = T;
}

pub trait Ordered {}

impl Ordered for i8 {}
impl Ordered for i16 {}
impl Ordered for i32 {}
impl Ordered for i64 {}
impl Ordered for u8 {}
impl Ordered for u16 {}
impl Ordered for u32 {}
impl Ordered for u64 {}
impl Ordered for f32 {}
impl Ordered for f64 {}
impl Ordered for String {}
impl Ordered for mongodb::bson::DateTime {}
impl Ordered for mongodb::bson::Decimal128 {}
impl Ordered for mongodb::bson::oid::ObjectId {}
impl<T: Ordered> Ordered for Option<T> {}

#[diagnostic::on_unimplemented(
    message = "`{T}` has no range operators",
    label = "the field type is not ordered; implement `urva::Ordered` for `{T}` if its BSON form is"
)]
pub trait OrderedIn<T: ?Sized> {}
impl<T: Ordered + ?Sized> OrderedIn<T> for Plain {}
impl<T: ?Sized, X: ?Sized> OrderedIn<T> for Encoded<X> {}

pub struct Field<E: ?Sized, T: ?Sized, Cap = Full, Enc = Plain> {
    path: Cow<'static, str>,
    array_filters: Vec<(String, Document)>,
    deferred_error: Option<crate::Error>,
    _marker: PhantomData<Marker<E, T, Cap, Enc>>,
}

type Marker<E, T, Cap, Enc> = fn() -> (Box<E>, Box<T>, Cap, Enc);

impl<E: ?Sized, T: ?Sized, Cap, Enc> Field<E, T, Cap, Enc> {
    pub(crate) const fn new(path: &'static str) -> Self {
        Field {
            path: Cow::Borrowed(path),
            array_filters: Vec::new(),
            deferred_error: None,
            _marker: PhantomData,
        }
    }

    #[doc(hidden)]
    pub fn path(&self) -> &str {
        &self.path
    }

    pub(crate) fn retype<T2: ?Sized, Cap2, Enc2>(self) -> Field<E, T2, Cap2, Enc2> {
        Field {
            path: self.path,
            array_filters: self.array_filters,
            deferred_error: self.deferred_error,
            _marker: PhantomData,
        }
    }

    pub(crate) fn push_segment(&mut self, segment: &str) {
        let path = self.path.to_mut();
        path.push('.');
        path.push_str(segment);
    }

    pub(crate) fn into_positional_parts(
        self,
    ) -> (
        Cow<'static, str>,
        Vec<(String, Document)>,
        Option<crate::Error>,
    ) {
        (self.path, self.array_filters, self.deferred_error)
    }

    pub(crate) fn add_element_filter(
        &mut self,
        name: String,
        doc: Document,
        error: Option<crate::Error>,
    ) {
        self.array_filters.push((name, doc));
        if self.deferred_error.is_none() {
            self.deferred_error = error;
        }
    }
}

impl<E: ?Sized, T: ?Sized, Cap, Enc> Clone for Field<E, T, Cap, Enc> {
    fn clone(&self) -> Self {
        Field {
            path: self.path.clone(),
            array_filters: self.array_filters.clone(),
            deferred_error: self.deferred_error.clone(),
            _marker: PhantomData,
        }
    }
}

impl<E: ?Sized, T: ?Sized, Cap, Enc> std::fmt::Debug for Field<E, T, Cap, Enc> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Field").field("path", &self.path).finish()
    }
}

pub struct VersionField<E: ?Sized> {
    path: &'static str,
    _marker: PhantomData<fn() -> Box<E>>,
}

impl<E: ?Sized> VersionField<E> {
    pub(crate) const fn new(path: &'static str) -> Self {
        VersionField {
            path,
            _marker: PhantomData,
        }
    }

    #[doc(hidden)]
    pub fn path(&self) -> &'static str {
        self.path
    }
}

impl<E: ?Sized> Clone for VersionField<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E: ?Sized> Copy for VersionField<E> {}

impl<E: ?Sized> std::fmt::Debug for VersionField<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VersionField")
            .field("path", &self.path)
            .finish()
    }
}
