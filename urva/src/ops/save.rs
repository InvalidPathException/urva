use std::future::IntoFuture;

use futures_util::future::BoxFuture;
use mongodb::Collection;
use mongodb::bson::{Bson, Document, doc};
use mongodb::error::ErrorKind;
use mongodb::options::{DeleteOptions, InsertManyOptions, InsertOneOptions, ReplaceOptions};
use mongodb::results::InsertManyResult;

use crate::doc::{Doc, Lock, NewId};
use crate::entity::Entity;
use crate::filter::Filter;
use crate::ops::partial::{
    BulkClass, Partial, PartialFailure, Summary, classify_bulk_failure, indexed, nothing_sent,
};
use crate::ops::{Attached, Detached, Exec};
use crate::store::Store;
use crate::transaction::Transaction;
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
        pub struct $name<'e, E: Entity, X = Detached> {
            collection: Collection<Doc<E>>,
            exec: X,
            target: $target,
            $($extra: $extra_ty,)*
            options: $options,
            _marker: ::std::marker::PhantomData<(&'e (), fn() -> E)>,
        }

        impl<'e, E: Entity> $name<'e, E> {
            pub(crate) fn new(collection: Collection<Doc<E>>, target: $target, $($extra: $extra_ty),*) -> Self {
                $name {
                    collection,
                    exec: Detached,
                    target,
                    $($extra,)*
                    options: <$options>::default(),
                    _marker: ::std::marker::PhantomData,
                }
            }

            pub fn session(self, tx: &'e mut Transaction) -> $name<'e, E, Attached<'e>> {
                $name {
                    collection: self.collection,
                    exec: Attached(tx),
                    target: self.target,
                    $($extra: self.$extra,)*
                    options: self.options,
                    _marker: ::std::marker::PhantomData,
                }
            }
        }

        impl<'e, E: Entity, X> $name<'e, E, X> {
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

#[must_use = "builders do nothing until awaited"]
pub struct InsertManyBuilder<'e, E: Entity, X = Detached, R = Summary> {
    collection: Collection<Doc<E>>,
    exec: X,
    docs: Vec<Doc<E>>,
    options: InsertManyOptions,
    _marker: ::std::marker::PhantomData<ManyMarker<'e, E, R>>,
}

type ManyMarker<'e, E, R> = (&'e (), fn() -> (E, R));

impl<E: Entity> InsertManyBuilder<'_, E> {
    pub(crate) fn new(collection: Collection<Doc<E>>, docs: Vec<Doc<E>>) -> Self {
        InsertManyBuilder {
            collection,
            exec: Detached,
            docs,
            options: InsertManyOptions::default(),
            _marker: ::std::marker::PhantomData,
        }
    }

    pub fn partial(self) -> InsertManyBuilder<'static, E, Detached, Partial> {
        self.retype(Detached)
    }
}

impl<'e, E: Entity> InsertManyBuilder<'e, E, Detached, Summary> {
    pub fn session(
        self,
        tx: &'e mut Transaction,
    ) -> InsertManyBuilder<'e, E, Attached<'e>, Summary> {
        self.retype(Attached(tx))
    }
}

impl<E: Entity, X, R> InsertManyBuilder<'_, E, X, R> {
    fn retype<'e2, X2, R2>(self, exec: X2) -> InsertManyBuilder<'e2, E, X2, R2> {
        InsertManyBuilder {
            collection: self.collection,
            exec,
            docs: self.docs,
            options: self.options,
            _marker: ::std::marker::PhantomData,
        }
    }

    pub fn with_options(mut self, options: InsertManyOptions) -> Self {
        self.options = options;
        self
    }

    pub fn ordered(mut self, ordered: bool) -> Self {
        self.options.ordered = Some(ordered);
        self
    }
}

fn encode_all<E: Entity>(docs: &[Doc<E>]) -> Result<Vec<Document>> {
    docs.iter().map(Doc::to_stored).collect()
}

fn insert_all<'e, E: Entity, X: Exec + 'e>(
    collection: Collection<Doc<E>>,
    exec: X,
    stored: Vec<Document>,
    options: InsertManyOptions,
) -> BoxFuture<'e, mongodb::error::Result<InsertManyResult>> {
    Box::pin(exec.run(move |session| {
        Box::pin(async move {
            let documents = collection.clone_with_type::<Document>();
            let action = documents.insert_many(stored).with_options(options);
            crate::ops::run!(action, session)
        })
    }))
}

impl<'e, E: Entity, X: Exec + 'e> IntoFuture for InsertManyBuilder<'e, E, X, Summary> {
    type Output = Result<Vec<Doc<E>>>;
    type IntoFuture = BoxFuture<'e, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let InsertManyBuilder {
            collection,
            exec,
            docs,
            options,
            ..
        } = self;
        Box::pin(async move {
            let stored = encode_all(&docs)?;
            insert_all(collection, exec, stored, options).await?;
            Ok(docs)
        })
    }
}

impl<E: Entity> IntoFuture for InsertManyBuilder<'_, E, Detached, Partial> {
    type Output = Result<Vec<Doc<E>>, PartialFailure<E>>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let InsertManyBuilder {
            collection,
            exec,
            docs,
            options,
            ..
        } = self;
        Box::pin(async move {
            let stored = match encode_all(&docs) {
                Ok(stored) => stored,
                Err(error) => return Err(PartialFailure::split(error, indexed(docs), Some(&[]))),
            };
            let ordered = options.ordered.unwrap_or(true);
            match insert_all(collection, exec, stored, options).await {
                Ok(_) => Ok(docs),
                Err(error) => {
                    let applied = match &*error.kind {
                        ErrorKind::InsertMany(e) => {
                            let indices: Vec<usize> =
                                e.write_errors.iter().flatten().map(|we| we.index).collect();
                            match classify_bulk_failure(
                                ordered,
                                docs.len(),
                                &indices,
                                std::error::Error::source(&error).is_some(),
                            ) {
                                BulkClass::PerOp { applied } => Some(applied),
                                BulkClass::Opaque => None,
                            }
                        }
                        _ if nothing_sent(&error) => Some(Vec::new()),
                        _ => None,
                    };
                    Err(PartialFailure::split(
                        error.into(),
                        indexed(docs),
                        applied.as_deref(),
                    ))
                }
            }
        })
    }
}

impl<'e, E: Entity, X: Exec + 'e> IntoFuture for InsertBuilder<'e, E, X> {
    type Output = Result<Doc<E>>;
    type IntoFuture = BoxFuture<'e, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let InsertBuilder {
            collection,
            exec,
            target: doc,
            options,
            ..
        } = self;
        Box::pin(async move {
            let stored = doc.to_stored()?;
            exec.run(move |session| {
                Box::pin(async move {
                    let documents = collection.clone_with_type::<Document>();
                    let action = documents.insert_one(stored).with_options(options);
                    crate::ops::run!(action, session)
                })
            })
            .await?;
            Ok(doc)
        })
    }
}

impl<'e, E: Entity, X: Exec + 'e> IntoFuture for SaveBuilder<'e, E, X> {
    type Output = Result<()>;
    type IntoFuture = BoxFuture<'e, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let SaveBuilder {
            collection,
            exec,
            target: doc,
            extra,
            mut options,
            ..
        } = self;
        Box::pin(async move {
            let conditional = extra.is_some();
            let (id, filter, replacement, next) = prepare_save(doc, extra)?;
            options.upsert = None;
            let (result, outcome) = exec
                .run(move |session| {
                    Box::pin(async move {
                        let outcome = session.as_ref().map(|tx| tx.outcome());
                        let documents = collection.clone_with_type::<Document>();
                        let action = documents
                            .replace_one(filter, replacement)
                            .with_options(options);
                        (crate::ops::run!(action, session), outcome)
                    })
                })
                .await;
            let result = result?;
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
            doc.settle(next, outcome);
            Ok(())
        })
    }
}

impl<'e, E: Entity, X: Exec + 'e> IntoFuture for DeleteBuilder<'e, E, X> {
    type Output = Result<()>;
    type IntoFuture = BoxFuture<'e, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let DeleteBuilder {
            collection,
            exec,
            target: doc,
            options,
            ..
        } = self;
        Box::pin(async move {
            let (id, filter) = version_filter(doc, None)?;
            let result = exec
                .run(move |session| {
                    Box::pin(async move {
                        let action = collection.delete_one(filter).with_options(options);
                        crate::ops::run!(action, session)
                    })
                })
                .await?;
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

    pub fn insert_many(&self, bodies: impl IntoIterator<Item = E>) -> InsertManyBuilder<'static, E>
    where
        E::Id: NewId,
    {
        self.insert_many_with_ids(bodies.into_iter().map(|body| (E::Id::new_id(), body)))
    }

    pub fn insert_many_with_ids(
        &self,
        entries: impl IntoIterator<Item = (E::Id, E)>,
    ) -> InsertManyBuilder<'static, E> {
        InsertManyBuilder::new(
            self.raw().clone(),
            entries
                .into_iter()
                .map(|(id, body)| prepare(id, body))
                .collect(),
        )
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
