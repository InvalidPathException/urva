use std::future::IntoFuture;

use futures_util::future::BoxFuture;

use mongodb::Collection;
use mongodb::Namespace;
use mongodb::bson::{Bson, Document};
use mongodb::error::{BulkWriteError, ErrorKind};
use mongodb::options::{
    BulkWriteOptions, DeleteManyModel, DeleteOneModel, InsertOneModel, UpdateManyModel,
    UpdateOneModel, WriteModel,
};
use mongodb::results::SummaryBulkWriteResult;

use crate::doc::{Doc, NewId};
use crate::entity::Entity;
use crate::filter::{FieldValue, Filter};
use crate::ops::partial::{BulkClass, Summary, classify_bulk_failure, nothing_sent};
use crate::ops::save::prepare;
use crate::ops::{Attached, Detached, Exec};
use crate::store::Store;
use crate::store::id_filter;
use crate::transaction::Transaction;
use crate::update::Update;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct BulkReport {
    pub error: BulkWriteError,

    pub applied: Vec<usize>,
}

pub struct BulkOutcome<E: Entity> {
    pub result: SummaryBulkWriteResult,

    pub inserted: Vec<Doc<E>>,
}

impl<E: Entity + std::fmt::Debug> std::fmt::Debug for BulkOutcome<E>
where
    E::Id: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BulkOutcome")
            .field("result", &self.result)
            .field("inserted", &self.inserted)
            .finish()
    }
}

impl<E: Entity> Store<E> {
    pub fn bulk(&self) -> BulkBuilder<E> {
        BulkBuilder::new(self.raw().clone())
    }
}

#[must_use = "builders do nothing until awaited"]
pub struct BulkBuilder<E: Entity, X = Detached, R = Summary> {
    collection: Collection<Doc<E>>,
    exec: X,
    namespace: Namespace,
    ops: Vec<WriteModel>,
    docs: Vec<(usize, Doc<E>)>,
    options: BulkWriteOptions,
    deferred_error: Option<Error>,
    _report: std::marker::PhantomData<fn() -> R>,
}

trait UpdateModel: Into<WriteModel> {
    fn new(namespace: Namespace, filter: Document) -> Self;
    fn array_filters(&mut self) -> &mut Option<Vec<Bson>>;
    fn upsert(&self) -> Option<bool>;
    fn set_update(&mut self, update: Document);
}

macro_rules! update_model {
    ($model:ident) => {
        impl UpdateModel for $model {
            fn new(namespace: Namespace, filter: Document) -> Self {
                $model::builder()
                    .namespace(namespace)
                    .filter(filter)
                    .update(Document::new())
                    .build()
            }

            fn array_filters(&mut self) -> &mut Option<Vec<Bson>> {
                &mut self.array_filters
            }

            fn upsert(&self) -> Option<bool> {
                self.upsert
            }

            fn set_update(&mut self, update: Document) {
                self.update = update.into();
            }
        }
    };
}

update_model!(UpdateOneModel);
update_model!(UpdateManyModel);

impl<E: Entity> BulkBuilder<E> {
    fn new(collection: Collection<Doc<E>>) -> Self {
        BulkBuilder {
            namespace: collection.namespace(),
            collection,
            exec: Detached,
            ops: Vec::new(),
            docs: Vec::new(),
            options: BulkWriteOptions::default(),
            deferred_error: None,
            _report: std::marker::PhantomData,
        }
    }
}

impl<E: Entity> BulkBuilder<E, Detached, Summary> {
    pub fn session(self, tx: &mut Transaction) -> BulkBuilder<E, Attached<'_>, Summary> {
        self.retype(Attached(tx))
    }
}

impl<E: Entity, X, R> BulkBuilder<E, X, R> {
    fn retype<X2, R2>(self, exec: X2) -> BulkBuilder<E, X2, R2> {
        BulkBuilder {
            collection: self.collection,
            exec,
            namespace: self.namespace,
            ops: self.ops,
            docs: self.docs,
            options: self.options,
            deferred_error: self.deferred_error,
            _report: std::marker::PhantomData,
        }
    }

    pub fn insert(self, body: E) -> Self
    where
        E::Id: NewId,
    {
        self.insert_with_id(E::Id::new_id(), body)
    }

    pub fn insert_with_id(mut self, id: E::Id, body: E) -> Self {
        let doc = prepare(id, body);
        let document = match doc.to_stored() {
            Ok(document) => document,
            Err(error) => {
                self.defer_rejection(error);
                Document::new()
            }
        };
        self.docs.push((self.ops.len(), doc));
        self.ops.push(
            InsertOneModel::builder()
                .namespace(self.namespace.clone())
                .document(document)
                .build()
                .into(),
        );
        self
    }

    pub fn update_one(self, filter: Filter<E>, update: Update<E>) -> Self {
        self.push_update(filter, update, |_: &mut UpdateOneModel| {})
    }

    pub fn update_by_id(self, id: impl FieldValue<E::Id>, update: Update<E>) -> Self {
        self.push_update(id_filter(id), update, |_: &mut UpdateOneModel| {})
    }

    pub fn update_one_with(
        self,
        filter: Filter<E>,
        update: Update<E>,
        configure: impl FnOnce(&mut UpdateOneModel),
    ) -> Self
    where
        E::Id: NewId,
    {
        self.push_update(filter, update, configure)
    }

    pub fn update_by_id_with(
        self,
        id: impl FieldValue<E::Id>,
        update: Update<E>,
        configure: impl FnOnce(&mut UpdateOneModel),
    ) -> Self {
        self.push_update(id_filter(id), update, configure)
    }

    pub fn update_many(self, filter: Filter<E>, update: Update<E>) -> Self {
        self.push_update(filter, update, |_: &mut UpdateManyModel| {})
    }

    pub fn update_many_with(
        self,
        filter: Filter<E>,
        update: Update<E>,
        configure: impl FnOnce(&mut UpdateManyModel),
    ) -> Self
    where
        E::Id: NewId,
    {
        self.push_update(filter, update, configure)
    }

    fn push_update<M: UpdateModel>(
        mut self,
        filter: Filter<E>,
        update: Update<E>,
        configure: impl FnOnce(&mut M),
    ) -> Self {
        match (filter.into_document(), update.into_parts()) {
            (Ok(filter), Ok((mut update, element_filters))) => {
                let mut model = M::new(self.namespace.clone(), filter);
                *model.array_filters() = array_filter_option(element_filters);
                configure(&mut model);
                crate::update::seed_on_upsert::<E>(&mut update, model.upsert());
                model.set_update(update);
                self.ops.push(model.into());
            }
            (Err(error), _) | (_, Err(error)) => self.defer_rejection(error),
        }
        self
    }

    pub fn delete_one(mut self, filter: Filter<E>) -> Self {
        match filter.into_document() {
            Ok(filter) => self.ops.push(
                DeleteOneModel::builder()
                    .namespace(self.namespace.clone())
                    .filter(filter)
                    .build()
                    .into(),
            ),
            Err(error) => self.defer_rejection(error),
        }
        self
    }

    pub fn delete_many(mut self, filter: Filter<E>) -> Self {
        match filter.into_document() {
            Ok(filter) => self.ops.push(
                DeleteManyModel::builder()
                    .namespace(self.namespace.clone())
                    .filter(filter)
                    .build()
                    .into(),
            ),
            Err(error) => self.defer_rejection(error),
        }
        self
    }

    fn defer_rejection(&mut self, error: Error) {
        if self.deferred_error.is_none() {
            self.deferred_error = Some(error);
        }
    }

    pub fn ordered(mut self, ordered: bool) -> Self {
        self.options.ordered = Some(ordered);
        self
    }

    pub fn with_options(mut self, options: BulkWriteOptions) -> Self {
        self.options = options;
        self
    }
}

fn write_all<'x, E: Entity, X: Exec + 'x>(
    collection: Collection<Doc<E>>,
    exec: X,
    ops: Vec<WriteModel>,
    options: BulkWriteOptions,
) -> BoxFuture<'x, mongodb::error::Result<SummaryBulkWriteResult>> {
    Box::pin(exec.run(move |session| {
        Box::pin(async move {
            let mut options = options;
            if session.is_none() && options.write_concern.is_none() {
                options.write_concern = collection.write_concern().cloned();
            }
            let action = collection.client().bulk_write(ops).with_options(options);
            crate::ops::run!(action, session)
        })
    }))
}

fn outcome<E: Entity>(
    result: SummaryBulkWriteResult,
    docs: Vec<(usize, Doc<E>)>,
) -> BulkOutcome<E> {
    BulkOutcome {
        result,
        inserted: docs.into_iter().map(|(_, doc)| doc).collect(),
    }
}

fn classify(
    error: mongodb::error::Error,
    ordered: bool,
    queued: usize,
) -> (Error, Option<Vec<usize>>) {
    match &*error.kind {
        ErrorKind::BulkWrite(bulk_error) => {
            let indices: Vec<usize> = bulk_error.write_errors.keys().copied().collect();
            match classify_bulk_failure(
                ordered,
                queued,
                &indices,
                std::error::Error::source(&error).is_some(),
            ) {
                BulkClass::PerOp { applied } => (
                    Error::Bulk(Box::new(BulkReport {
                        error: bulk_error.clone(),
                        applied: applied.clone(),
                    })),
                    Some(applied),
                ),
                BulkClass::Opaque => (error.into(), None),
            }
        }
        _ if nothing_sent(&error) => (error.into(), Some(Vec::new())),
        _ => (error.into(), None),
    }
}

async fn run_detached<E: Entity>(
    collection: Collection<Doc<E>>,
    ops: Vec<WriteModel>,
    options: BulkWriteOptions,
) -> std::result::Result<SummaryBulkWriteResult, (Error, Option<Vec<usize>>)> {
    let queued = ops.len();
    let ordered = options.ordered.unwrap_or(true);
    write_all(collection, Detached, ops, options)
        .await
        .map_err(|error| classify(error, ordered, queued))
}

impl<E: Entity> IntoFuture for BulkBuilder<E, Detached, Summary> {
    type Output = Result<BulkOutcome<E>>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let BulkBuilder {
            collection,
            ops,
            docs,
            options,
            deferred_error,
            ..
        } = self;
        Box::pin(async move {
            if let Some(error) = deferred_error {
                return Err(error);
            }
            match run_detached::<E>(collection, ops, options).await {
                Ok(result) => Ok(outcome(result, docs)),
                Err((error, _)) => Err(error),
            }
        })
    }
}

impl<'t, E: Entity> IntoFuture for BulkBuilder<E, Attached<'t>, Summary> {
    type Output = Result<BulkOutcome<E>>;
    type IntoFuture = BoxFuture<'t, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        let BulkBuilder {
            collection,
            exec,
            ops,
            docs,
            options,
            deferred_error,
            ..
        } = self;
        Box::pin(async move {
            if let Some(error) = deferred_error {
                return Err(error);
            }
            let result = write_all(collection, exec, ops, options).await?;
            Ok(outcome(result, docs))
        })
    }
}

fn array_filter_option(element_filters: Vec<Document>) -> Option<Vec<Bson>> {
    (!element_filters.is_empty()).then(|| element_filters.into_iter().map(Bson::from).collect())
}

#[cfg(test)]
mod tests {
    use super::BulkReport;
    use mongodb::error::{BulkWriteError, WriteError};

    #[test]
    fn bulk_report_answers_is_duplicate_key() {
        let report = |code| {
            let mut error = BulkWriteError::default();
            let write_error: WriteError =
                mongodb::bson::from_document(mongodb::bson::doc! { "code": code, "errmsg": "" })
                    .expect("a write error deserializes");
            error.write_errors.insert(1, write_error);
            crate::Error::Bulk(Box::new(BulkReport {
                error,
                applied: vec![0],
            }))
        };
        assert!(report(11000).is_duplicate_key());
        assert!(!report(14).is_duplicate_key());
    }
}
