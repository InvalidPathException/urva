use std::fmt;
use std::ops::{Deref, DerefMut};

use mongodb::bson::{Bson, Document, de, from_bson, from_document, to_bson, to_document};
use serde::de::Error as _;
use serde::ser::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::entity::Entity;
use crate::version::Version;

pub struct Doc<E: Entity> {
    pub(crate) id: E::Id,
    pub(crate) version: E::Lock,
    pub body: E,
}

impl<E: Entity> Doc<E> {
    pub(crate) fn new(id: E::Id, version: E::Lock, body: E) -> Self {
        Doc { id, version, body }
    }

    pub fn id(&self) -> &E::Id {
        &self.id
    }

    pub fn version(&self) -> E::Lock {
        self.version
    }

    pub fn into_body(self) -> E {
        self.body
    }

    pub fn into_parts(self) -> (E::Id, E) {
        (self.id, self.body)
    }

    pub(crate) fn to_stored(&self) -> crate::Result<Document> {
        let mut stored = to_document(&self.body)?;
        stored.insert("_id", to_bson(&self.id)?);
        self.version().write(&mut stored, E::VERSION_FIELD);
        Ok(stored)
    }
}

impl<E: Entity> Deref for Doc<E> {
    type Target = E;

    fn deref(&self) -> &E {
        &self.body
    }
}

impl<E: Entity> DerefMut for Doc<E> {
    fn deref_mut(&mut self) -> &mut E {
        &mut self.body
    }
}

impl<E: Entity + Clone> Clone for Doc<E> {
    fn clone(&self) -> Self {
        Doc {
            id: self.id.clone(),
            version: self.version,
            body: self.body.clone(),
        }
    }
}

impl<E: Entity + fmt::Debug> fmt::Debug for Doc<E>
where
    E::Id: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Doc")
            .field("id", &self.id)
            .field("version", &self.version())
            .field("body", &self.body)
            .finish()
    }
}

impl<E: Entity + PartialEq> PartialEq for Doc<E>
where
    E::Id: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.version() == other.version() && self.body == other.body
    }
}

impl<E: Entity> Serialize for Doc<E> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_stored()
            .map_err(S::Error::custom)?
            .serialize(serializer)
    }
}

impl<'de, E: Entity> Deserialize<'de> for Doc<E> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut stored = Document::deserialize(deserializer)?;
        let id = stored
            .remove("_id")
            .ok_or_else(|| D::Error::custom("stored document has no `_id`"))?;
        let id: E::Id = from_bson(id).map_err(D::Error::custom)?;
        let version = E::Lock::read(&mut stored, E::VERSION_FIELD).map_err(D::Error::custom)?;
        let body: E = from_document(stored).map_err(D::Error::custom)?;
        Ok(Doc::new(id, version, body))
    }
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not an id type the store can generate",
    label = "only `ObjectId` is generated, try explicitly supplying the id"
)]
pub trait NewId {
    fn new_id() -> Self;
}

impl NewId for mongodb::bson::oid::ObjectId {
    fn new_id() -> Self {
        mongodb::bson::oid::ObjectId::new()
    }
}

pub trait Lock:
    crate::__private::Sealed + Copy + PartialEq + fmt::Debug + Send + Sync + Unpin + 'static
{
    #[doc(hidden)]
    fn first() -> Self;

    #[doc(hidden)]
    fn next(self) -> Self;

    #[doc(hidden)]
    fn read(stored: &mut Document, field: &str) -> Result<Self, de::Error>;

    #[doc(hidden)]
    fn write(self, stored: &mut Document, field: &str);

    #[doc(hidden)]
    fn miss(collection: &'static str, id: Bson) -> crate::Error;
}

impl crate::__private::Sealed for Version {}

impl Lock for Version {
    fn first() -> Self {
        Version::committed(1)
    }

    fn next(self) -> Self {
        Version::committed(
            self.value()
                .checked_add(1)
                .expect("version counter exhausted at i64::MAX"),
        )
    }

    fn read(stored: &mut Document, field: &str) -> Result<Self, de::Error> {
        match stored.remove(field) {
            Some(value) => from_bson(value),
            None => Err(de::Error::custom(format!(
                "stored document has no `{field}` lock, backfill the collection before deploying a versioned entity"
            ))),
        }
    }

    fn write(self, stored: &mut Document, field: &str) {
        stored.insert(field, Bson::Int64(self.value()));
    }

    fn miss(collection: &'static str, id: Bson) -> crate::Error {
        crate::Error::VersionConflict {
            collection,
            id: Box::new(id),
        }
    }
}

impl crate::__private::Sealed for () {}

impl Lock for () {
    fn first() -> Self {}

    fn next(self) -> Self {}

    fn read(_: &mut Document, _: &str) -> Result<Self, de::Error> {
        Ok(())
    }

    fn write(self, _: &mut Document, _: &str) {}

    fn miss(collection: &'static str, id: Bson) -> crate::Error {
        crate::Error::NotFound {
            collection,
            id: Box::new(id),
        }
    }
}

#[cfg(test)]
mod tests {
    use mongodb::bson::{doc, from_document, to_document};
    use serde::{Deserialize, Serialize};

    use super::Doc;
    use crate::{Entity, Version};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    struct Order {
        status: String,
        total: i64,
    }

    impl crate::__private::Sealed for Order {}

    impl Entity for Order {
        const COLLECTION: &'static str = "orders";
        type Id = i64;
        type Lock = Version;
        const VERSION_FIELD: &'static str = "version";
    }

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
    struct Note {
        text: String,
    }

    impl crate::__private::Sealed for Note {}

    impl Entity for Note {
        const COLLECTION: &'static str = "notes";
        type Id = String;
        type Lock = ();
        const VERSION_FIELD: &'static str = "version";
    }

    fn order() -> Order {
        Order {
            status: "open".to_string(),
            total: 100,
        }
    }

    #[test]
    fn versioned_doc_stores_id_and_lock() {
        let doc = Doc::new(7, Version::committed(3), order());
        let stored = to_document(&doc).unwrap();
        assert_eq!(
            stored,
            doc! { "status": "open", "total": 100_i64, "_id": 7_i64, "version": 3_i64 }
        );
        let read: Doc<Order> = from_document(stored).unwrap();
        assert_eq!(read, doc);
    }

    #[test]
    fn stored_document_without_id_is_rejected() {
        let error = from_document::<Doc<Order>>(doc! {
            "status": "open", "total": 100_i64, "version": 1_i64
        })
        .unwrap_err();
        assert!(error.to_string().contains("no `_id`"), "{error}");
    }

    #[test]
    fn versioned_document_without_lock_is_rejected() {
        let error = from_document::<Doc<Order>>(doc! {
            "_id": 7_i64, "status": "open", "total": 100_i64
        })
        .unwrap_err();
        assert!(error.to_string().contains("backfill"), "{error}");
    }

    #[test]
    fn unversioned_doc_has_no_lock_field() {
        let doc = Doc::new(
            "n1".to_string(),
            (),
            Note {
                text: "hello".to_string(),
            },
        );
        let stored = to_document(&doc).unwrap();
        assert_eq!(stored, doc! { "text": "hello", "_id": "n1" });
        let read: Doc<Note> = from_document(stored).unwrap();
        assert_eq!(read, doc);
    }
}
