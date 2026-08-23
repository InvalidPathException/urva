use std::marker::PhantomData;

use mongodb::bson::{Bson, Document, doc, ser};
use serde::Serialize;

use crate::field::{ArrayLike, Field, Filterable, Ordered, Plain, VersionField};
use crate::version::Version;

pub(crate) fn encode<T: Serialize>(value: &T) -> Result<Bson, ser::Error> {
    mongodb::bson::to_bson(value)
}

pub struct Filter<E: ?Sized> {
    doc: Document,
    deferred_error: Option<crate::Error>,
    _marker: PhantomData<fn() -> Box<E>>,
}

impl<E: ?Sized> Filter<E> {
    pub fn raw(doc: Document) -> Self {
        Filter {
            doc,
            deferred_error: None,
            _marker: PhantomData,
        }
    }

    pub fn empty() -> Self {
        Filter::raw(Document::new())
    }

    pub fn and(self, other: Filter<E>) -> Filter<E> {
        merge_operator(self, other, "$and")
    }

    pub fn or(self, other: Filter<E>) -> Filter<E> {
        merge_operator(self, other, "$or")
    }

    #[doc(hidden)]
    pub fn into_document(self) -> crate::Result<Document> {
        match self.deferred_error {
            Some(error) => Err(error),
            None => Ok(self.doc),
        }
    }

    pub(crate) fn field_op(path: &str, op: &str, value: Result<Bson, ser::Error>) -> Self {
        match value {
            Ok(value) => Filter::raw(doc! { path: { op: value } }),
            Err(error) => Filter {
                doc: Document::new(),
                deferred_error: Some(error.into()),
                _marker: PhantomData,
            },
        }
    }
}

impl<E: ?Sized> Clone for Filter<E> {
    fn clone(&self) -> Self {
        Filter {
            doc: self.doc.clone(),
            deferred_error: self.deferred_error.clone(),
            _marker: PhantomData,
        }
    }
}

impl<E: ?Sized> std::fmt::Debug for Filter<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.deferred_error {
            None => f.debug_tuple("Filter").field(&self.doc).finish(),
            Some(error) => f.debug_tuple("Filter").field(error).finish(),
        }
    }
}

fn merge_operator<E: ?Sized>(mut left: Filter<E>, right: Filter<E>, op: &str) -> Filter<E> {
    if left.doc.len() == 1
        && let Some(Bson::Array(items)) = left.doc.get_mut(op)
        && !items.is_empty()
    {
        items.push(Bson::Document(right.doc));
        if left.deferred_error.is_none() {
            left.deferred_error = right.deferred_error;
        }
        return left;
    }
    combine([left, right], op)
}

fn combine<E: ?Sized>(filters: impl IntoIterator<Item = Filter<E>>, op: &str) -> Filter<E> {
    let mut deferred_error = None;
    let list: Vec<Bson> = filters
        .into_iter()
        .map(|mut filter| {
            if deferred_error.is_none() {
                deferred_error = filter.deferred_error.take();
            }
            Bson::Document(filter.doc)
        })
        .collect();
    Filter {
        doc: doc! { op: list },
        deferred_error,
        _marker: PhantomData,
    }
}

pub fn all<E: ?Sized>(filters: impl IntoIterator<Item = Filter<E>>) -> Filter<E> {
    combine(filters, "$and")
}

pub fn any<E: ?Sized>(filters: impl IntoIterator<Item = Filter<E>>) -> Filter<E> {
    combine(filters, "$or")
}

pub fn text<E: ?Sized>(query: impl Into<String>) -> Filter<E> {
    Filter::raw(doc! { "$text": { "$search": query.into() } })
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an accepted value for a field of type `{T}`",
    label = "expected the field's type, or its inner type for `Option` fields"
)]
pub trait FieldValue<T: ?Sized, Enc = Plain> {
    #[doc(hidden)]
    fn encode_value(&self) -> Result<Bson, ser::Error>;
}

impl<T: Serialize> FieldValue<T> for T {
    fn encode_value(&self) -> Result<Bson, ser::Error> {
        encode(self)
    }
}

impl<T: Serialize> FieldValue<Option<T>> for T {
    fn encode_value(&self) -> Result<Bson, ser::Error> {
        encode(self)
    }
}

impl<T: Serialize> FieldValue<T> for &T {
    fn encode_value(&self) -> Result<Bson, ser::Error> {
        encode(*self)
    }
}

impl<T: Serialize> FieldValue<Option<T>> for &T {
    fn encode_value(&self) -> Result<Bson, ser::Error> {
        encode(*self)
    }
}

impl FieldValue<String> for &str {
    fn encode_value(&self) -> Result<Bson, ser::Error> {
        encode(self)
    }
}

impl FieldValue<Option<String>> for &str {
    fn encode_value(&self) -> Result<Bson, ser::Error> {
        encode(self)
    }
}

impl<E: ?Sized, T: ?Sized, C: Filterable, X> Field<E, T, C, X> {
    pub(crate) fn filter_op(self, op: &str, value: Result<Bson, ser::Error>) -> Filter<E> {
        Filter::field_op(self.path(), op, value)
    }

    pub fn exists(self, exists: bool) -> Filter<E> {
        self.filter_op("$exists", Ok(Bson::Boolean(exists)))
    }
}

impl<E: ?Sized, T, C: Filterable, X> Field<E, T, C, X> {
    pub fn eq(self, value: impl FieldValue<T, X>) -> Filter<E> {
        self.filter_op("$eq", value.encode_value())
    }

    pub fn ne(self, value: impl FieldValue<T, X>) -> Filter<E> {
        self.filter_op("$ne", value.encode_value())
    }

    pub fn is_in<V: FieldValue<T, X>>(self, values: impl IntoIterator<Item = V>) -> Filter<E> {
        let items: Result<Vec<Bson>, _> = values.into_iter().map(|v| v.encode_value()).collect();
        self.filter_op("$in", items.map(Bson::Array))
    }

    pub fn not_in<V: FieldValue<T, X>>(self, values: impl IntoIterator<Item = V>) -> Filter<E> {
        let items: Result<Vec<Bson>, _> = values.into_iter().map(|v| v.encode_value()).collect();
        self.filter_op("$nin", items.map(Bson::Array))
    }
}

impl<E: ?Sized, T, C: Filterable, X> Field<E, Option<T>, C, X> {
    pub fn is_null(self) -> Filter<E> {
        self.filter_op("$eq", Ok(Bson::Null))
    }
}

impl<E: ?Sized, T: Ordered, C: Filterable, X> Field<E, T, C, X> {
    pub fn gt(self, value: impl FieldValue<T, X>) -> Filter<E> {
        self.filter_op("$gt", value.encode_value())
    }

    pub fn gte(self, value: impl FieldValue<T, X>) -> Filter<E> {
        self.filter_op("$gte", value.encode_value())
    }

    pub fn lt(self, value: impl FieldValue<T, X>) -> Filter<E> {
        self.filter_op("$lt", value.encode_value())
    }

    pub fn lte(self, value: impl FieldValue<T, X>) -> Filter<E> {
        self.filter_op("$lte", value.encode_value())
    }
}

impl<E: ?Sized, T, C: Filterable> Field<E, T, C>
where
    String: FieldValue<T>,
{
    pub fn regex(self, pattern: impl Into<String>) -> Filter<E> {
        self.filter_op("$regex", Ok(Bson::String(pattern.into())))
    }

    pub fn starts_with(self, prefix: impl AsRef<str>) -> Filter<E> {
        self.regex(prefix_pattern(prefix.as_ref()))
    }
}

impl<E: ?Sized, A: ArrayLike, C: Filterable> Field<E, A, C> {
    pub fn contains(self, value: impl FieldValue<A::Elem>) -> Filter<E>
    where
        A::Elem: Serialize,
    {
        self.filter_op("$eq", value.encode_value())
    }

    pub fn contains_any<V: FieldValue<A::Elem>>(
        self,
        values: impl IntoIterator<Item = V>,
    ) -> Filter<E>
    where
        A::Elem: Serialize,
    {
        let items: Result<Vec<Bson>, _> = values.into_iter().map(|v| v.encode_value()).collect();
        self.filter_op("$in", items.map(Bson::Array))
    }

    pub fn contains_none<V: FieldValue<A::Elem>>(
        self,
        values: impl IntoIterator<Item = V>,
    ) -> Filter<E>
    where
        A::Elem: Serialize,
    {
        let items: Result<Vec<Bson>, _> = values.into_iter().map(|v| v.encode_value()).collect();
        self.filter_op("$nin", items.map(Bson::Array))
    }

    pub fn size(self, len: u32) -> Filter<E> {
        self.filter_op("$size", Ok(Bson::Int64(i64::from(len))))
    }
}

fn prefix_pattern(prefix: &str) -> String {
    let mut pattern = String::from("^");
    for ch in prefix.chars() {
        if "\\.^$|?*+()[]{}".contains(ch) {
            pattern.push('\\');
        }
        pattern.push(ch);
    }
    pattern
}

impl<E: ?Sized> VersionField<E> {
    pub fn eq(self, version: Version) -> Filter<E> {
        Filter::field_op(self.path(), "$eq", Ok(Bson::Int64(version.value())))
    }
}
