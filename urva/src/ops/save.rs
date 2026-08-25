use std::future::IntoFuture;

use futures_util::future::BoxFuture;
use mongodb::Collection;
use mongodb::options::InsertOneOptions;

use crate::Result;
use crate::doc::{Doc, Lock, NewId};
use crate::entity::Entity;
use crate::store::Store;

pub(crate) fn prepare<E: Entity>(id: E::Id, body: E) -> Doc<E> {
    Doc::new(id, E::Lock::first(), body)
}

#[must_use = "builders do nothing until awaited"]
pub struct InsertBuilder<E: Entity> {
    collection: Collection<Doc<E>>,
    doc: Doc<E>,
    options: InsertOneOptions,
}

impl<E: Entity> InsertBuilder<E> {
    pub(crate) fn new(collection: Collection<Doc<E>>, doc: Doc<E>) -> Self {
        InsertBuilder {
            collection,
            doc,
            options: InsertOneOptions::default(),
        }
    }

    pub fn with_options(mut self, options: InsertOneOptions) -> Self {
        self.options = options;
        self
    }
}

impl<E: Entity> IntoFuture for InsertBuilder<E> {
    type Output = Result<Doc<E>>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let InsertBuilder {
            collection,
            doc,
            options,
        } = self;
        Box::pin(async move {
            collection.insert_one(&doc).with_options(options).await?;
            Ok(doc)
        })
    }
}

impl<E: Entity> Store<E> {
    pub fn insert(&self, body: E) -> InsertBuilder<E>
    where
        E::Id: NewId,
    {
        self.insert_with_id(E::Id::new_id(), body)
    }

    pub fn insert_with_id(&self, id: E::Id, body: E) -> InsertBuilder<E> {
        InsertBuilder::new(self.raw().clone(), prepare(id, body))
    }
}
