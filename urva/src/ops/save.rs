use std::future::IntoFuture;

use futures_util::future::BoxFuture;
use mongodb::Collection;
use mongodb::bson::{Bson, Document, doc};
use mongodb::options::{DeleteOptions, InsertOneOptions, ReplaceOptions};

use crate::doc::{Doc, Lock, NewId};
use crate::entity::Entity;
use crate::filter::Filter;
use crate::store::Store;
use crate::{Error, Result};

pub(crate) fn prepare<E: Entity>(id: E::Id, body: E) -> Doc<E> {
    Doc::new(id, E::Lock::first(), body)
}

fn version_filter<E: Entity>(doc: &Doc<E>, extra: Option<Filter<E>>) -> Result<(Bson, Document)> {
    let id = doc.encode_id()?;
    let mut base = doc! { "_id": id.clone() };
    doc.version().write(&mut base, E::VERSION_FIELD);
    let filter = match extra {
        None => base,
        Some(extra) => doc! { "$and": [base, extra.into_document()?] },
    };
    Ok((id, filter))
}

fn prepare_save<E: Entity>(
    doc: &Doc<E>,
    extra: Option<Filter<E>>,
) -> Result<(Bson, Document, Document, E::Lock)> {
    let next = doc.version().next();
    let (id, filter) = version_filter(doc, extra)?;
    let mut replacement = doc.to_stored()?;
    next.write(&mut replacement, E::VERSION_FIELD);
    Ok((id, filter, replacement, next))
}

macro_rules! entity_builder {
    ($name:ident, $options:ty, $target:ty, { $($extra:ident : $extra_ty:ty),* }) => {
        #[must_use = "builders do nothing until awaited"]
        pub struct $name<'e, E: Entity> {
            collection: Collection<Doc<E>>,
            target: $target,
            $($extra: $extra_ty,)*
            options: $options,
            _marker: ::std::marker::PhantomData<&'e ()>,
        }

        impl<'e, E: Entity> $name<'e, E> {
            pub(crate) fn new(collection: Collection<Doc<E>>, target: $target, $($extra: $extra_ty),*) -> Self {
                $name {
                    collection,
                    target,
                    $($extra,)*
                    options: <$options>::default(),
                    _marker: ::std::marker::PhantomData,
                }
            }

            pub fn with_options(mut self, options: $options) -> Self {
                self.options = options;
                self
            }
        }
    };
}

entity_builder!(InsertBuilder, InsertOneOptions, Doc<E>, {});
entity_builder!(SaveBuilder, ReplaceOptions, &'e mut Doc<E>, { extra: Option<Filter<E>> });
entity_builder!(DeleteBuilder, DeleteOptions, &'e Doc<E>, {});

impl<E: Entity> IntoFuture for InsertBuilder<'_, E> {
    type Output = Result<Doc<E>>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let InsertBuilder {
            collection,
            target: doc,
            options,
            ..
        } = self;
        Box::pin(async move {
            collection.insert_one(&doc).with_options(options).await?;
            Ok(doc)
        })
    }
}

impl<'e, E: Entity> IntoFuture for SaveBuilder<'e, E> {
    type Output = Result<()>;
    type IntoFuture = BoxFuture<'e, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let SaveBuilder {
            collection,
            target: doc,
            extra,
            mut options,
            ..
        } = self;
        Box::pin(async move {
            let conditional = extra.is_some();
            let (id, filter, replacement, next) = prepare_save(doc, extra)?;
            options.upsert = None;
            let result = collection
                .clone_with_type::<Document>()
                .replace_one(filter, replacement)
                .with_options(options)
                .await?;
            if result.matched_count == 0 {
                return Err(if conditional {
                    Error::ConditionFailed {
                        collection: E::COLLECTION,
                        id: Box::new(id),
                    }
                } else {
                    E::Lock::miss(E::COLLECTION, id)
                });
            }
            doc.version = next;
            Ok(())
        })
    }
}

impl<'e, E: Entity> IntoFuture for DeleteBuilder<'e, E> {
    type Output = Result<()>;
    type IntoFuture = BoxFuture<'e, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let DeleteBuilder {
            collection,
            target: doc,
            options,
            ..
        } = self;
        Box::pin(async move {
            let (id, filter) = version_filter(doc, None)?;
            let result = collection.delete_one(filter).with_options(options).await?;
            if result.deleted_count == 0 {
                return Err(E::Lock::miss(E::COLLECTION, id));
            }
            Ok(())
        })
    }
}

impl<E: Entity> Store<E> {
    pub fn insert(&self, body: E) -> InsertBuilder<'static, E>
    where
        E::Id: NewId,
    {
        self.insert_with_id(E::Id::new_id(), body)
    }

    pub fn insert_with_id(&self, id: E::Id, body: E) -> InsertBuilder<'static, E> {
        InsertBuilder::new(self.raw().clone(), prepare(id, body))
    }

    pub fn save<'e>(&self, doc: &'e mut Doc<E>) -> SaveBuilder<'e, E> {
        SaveBuilder::new(self.raw().clone(), doc, None)
    }

    pub fn save_if<'e>(&self, doc: &'e mut Doc<E>, extra: Filter<E>) -> SaveBuilder<'e, E> {
        SaveBuilder::new(self.raw().clone(), doc, Some(extra))
    }

    pub fn delete<'e>(&self, doc: &'e Doc<E>) -> DeleteBuilder<'e, E> {
        DeleteBuilder::new(self.raw().clone(), doc)
    }
}
