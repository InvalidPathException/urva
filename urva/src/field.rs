use std::marker::PhantomData;

pub struct Full;

pub struct MatchOnly;

pub struct Plain;

pub type MatchField<E, T> = Field<E, T, MatchOnly>;

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Full {}
    impl Sealed for super::MatchOnly {}
    impl<T> Sealed for Vec<T> {}
    impl<T> Sealed for Option<Vec<T>> {}
}

pub trait Capability: sealed::Sealed {}
impl Capability for Full {}
impl Capability for MatchOnly {}

pub trait Filterable: Capability {}
impl Filterable for Full {}
impl Filterable for MatchOnly {}

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

pub struct Field<E: ?Sized, T: ?Sized, Cap = Full, Enc = Plain> {
    path: &'static str,
    _marker: PhantomData<Marker<E, T, Cap, Enc>>,
}

type Marker<E, T, Cap, Enc> = fn() -> (Box<E>, Box<T>, Cap, Enc);

impl<E: ?Sized, T: ?Sized, Cap, Enc> Field<E, T, Cap, Enc> {
    pub(crate) const fn new(path: &'static str) -> Self {
        Field {
            path,
            _marker: PhantomData,
        }
    }

    #[doc(hidden)]
    pub fn path(&self) -> &str {
        self.path
    }
}

impl<E: ?Sized, T: ?Sized, Cap, Enc> Clone for Field<E, T, Cap, Enc> {
    fn clone(&self) -> Self {
        Field {
            path: self.path,
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
