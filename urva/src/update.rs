use std::marker::PhantomData;

use mongodb::bson::{Bson, Document, doc, ser};
use serde::Serialize;

use crate::field::{ArrayLike, Field, Ordered, Updatable, VersionField};
use crate::filter::FieldValue;

pub struct Update<E: ?Sized> {
    doc: Document,
    deferred_error: Option<crate::Error>,
    _marker: PhantomData<fn() -> Box<E>>,
}

impl<E: ?Sized> Update<E> {
    pub fn raw(doc: Document) -> Self {
        Update {
            doc,
            deferred_error: None,
            _marker: PhantomData,
        }
    }

    pub fn and(mut self, other: Update<E>) -> Update<E> {
        let mut repeated: Option<(String, String)> = None;
        for (op, value) in other.doc {
            match (self.doc.get_mut(&op), value) {
                (Some(Bson::Document(existing)), Bson::Document(incoming)) => {
                    for (path, value) in incoming {
                        if repeated.is_none() && existing.contains_key(&path) {
                            repeated = Some((path.clone(), op.clone()));
                        }
                        existing.insert(path, value);
                    }
                }
                (_, value) => {
                    self.doc.insert(op, value);
                }
            }
        }
        if self.deferred_error.is_none() {
            self.deferred_error = other.deferred_error;
        }
        if self.deferred_error.is_none()
            && let Some((path, op)) = repeated
        {
            let message = if op == "$push" {
                format!(
                    "`{path}` is written twice through `$push`. Use `push_each` to push several values"
                )
            } else {
                format!("`{path}` is written twice through `{op}`")
            };
            self.deferred_error = Some(crate::Error::InvalidUpdate { message });
        }
        self
    }

    #[doc(hidden)]
    pub fn into_document(self) -> crate::Result<Document> {
        match self.deferred_error {
            Some(error) => Err(error),
            None => Ok(self.doc),
        }
    }

    fn field_op(op: &str, path: &str, value: Result<Bson, ser::Error>) -> Self {
        match value {
            Ok(value) => Update::raw(doc! { op: { path: value } }),
            Err(error) => Update {
                doc: Document::new(),
                deferred_error: Some(error.into()),
                _marker: PhantomData,
            },
        }
    }
}

pub fn apply<E: ?Sized>(updates: impl IntoIterator<Item = Update<E>>) -> Update<E> {
    updates
        .into_iter()
        .fold(Update::raw(Document::new()), Update::and)
}

impl<E: ?Sized> FromIterator<Update<E>> for Update<E> {
    fn from_iter<I: IntoIterator<Item = Update<E>>>(iter: I) -> Self {
        apply(iter)
    }
}

impl<E: ?Sized> Clone for Update<E> {
    fn clone(&self) -> Self {
        Update {
            doc: self.doc.clone(),
            deferred_error: self.deferred_error.clone(),
            _marker: PhantomData,
        }
    }
}

impl<E: ?Sized> std::fmt::Debug for Update<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.deferred_error {
            None => f.debug_tuple("Update").field(&self.doc).finish(),
            Some(error) => f.debug_tuple("Update").field(error).finish(),
        }
    }
}

pub trait Numeric {}

impl Numeric for i8 {}
impl Numeric for i16 {}
impl Numeric for i32 {}
impl Numeric for i64 {}
impl Numeric for u8 {}
impl Numeric for u16 {}
impl Numeric for u32 {}
impl Numeric for u64 {}
impl Numeric for f32 {}
impl Numeric for f64 {}
impl Numeric for mongodb::bson::Decimal128 {}
impl<T: Numeric> Numeric for Option<T> {}

impl<E: ?Sized, T: ?Sized, C: Updatable, X> Field<E, T, C, X> {
    fn update_op(self, op: &str, value: Result<Bson, ser::Error>) -> Update<E> {
        Update::field_op(op, self.path(), value)
    }

    pub fn unset(self) -> Update<E> {
        self.update_op("$unset", Ok(Bson::String(String::new())))
    }
}

impl<E: ?Sized, T, C: Updatable, X> Field<E, T, C, X> {
    pub fn set(self, value: impl FieldValue<T, X>) -> Update<E> {
        self.update_op("$set", value.encode_value())
    }

    pub fn set_on_insert(self, value: impl FieldValue<T, X>) -> Update<E> {
        self.update_op("$setOnInsert", value.encode_value())
    }
}

impl<E: ?Sized, T: Numeric, C: Updatable, X> Field<E, T, C, X> {
    pub fn inc(self, by: impl FieldValue<T, X>) -> Update<E> {
        self.update_op("$inc", by.encode_value())
    }

    #[allow(clippy::should_implement_trait)]
    pub fn mul(self, by: impl FieldValue<T, X>) -> Update<E> {
        self.update_op("$mul", by.encode_value())
    }
}

impl<E: ?Sized, T: Ordered, C: Updatable, X> Field<E, T, C, X> {
    pub fn min(self, value: impl FieldValue<T, X>) -> Update<E> {
        self.update_op("$min", value.encode_value())
    }

    pub fn max(self, value: impl FieldValue<T, X>) -> Update<E> {
        self.update_op("$max", value.encode_value())
    }
}

impl<E: ?Sized, A: ArrayLike, C: Updatable> Field<E, A, C> {
    pub fn push(self, value: impl FieldValue<A::Elem>) -> Update<E>
    where
        A::Elem: Serialize,
    {
        self.update_op("$push", value.encode_value())
    }

    pub fn push_each<V: FieldValue<A::Elem>>(self, values: impl IntoIterator<Item = V>) -> Update<E>
    where
        A::Elem: Serialize,
    {
        let items: Result<Vec<Bson>, _> = values.into_iter().map(|v| v.encode_value()).collect();
        self.update_op(
            "$push",
            items.map(|items| Bson::from(doc! { "$each": items })),
        )
    }

    pub fn pull(self, value: impl FieldValue<A::Elem>) -> Update<E>
    where
        A::Elem: Serialize,
    {
        self.update_op("$pull", value.encode_value())
    }

    pub fn add_to_set(self, value: impl FieldValue<A::Elem>) -> Update<E>
    where
        A::Elem: Serialize,
    {
        self.update_op("$addToSet", value.encode_value())
    }

    pub fn pop_last(self) -> Update<E> {
        self.update_op("$pop", Ok(Bson::Int32(1)))
    }

    pub fn pop_first(self) -> Update<E> {
        self.update_op("$pop", Ok(Bson::Int32(-1)))
    }
}

impl<E: ?Sized> VersionField<E> {
    pub fn bump(self) -> Update<E> {
        Update::field_op("$inc", self.path(), Ok(Bson::Int64(1)))
    }
}
