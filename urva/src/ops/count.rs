use std::marker::PhantomData;

use futures_util::future::BoxFuture;
use mongodb::Collection;
use mongodb::options::{CountOptions, EstimatedDocumentCountOptions};

use crate::doc::Doc;
use crate::entity::Entity;

crate::ops::builder!(
    CountBuilder,
    CountOptions,
    u64,
    [skip, limit: u64],
    |collection, filter, options| {
        Ok(collection
            .count_documents(filter?)
            .with_options(options)
            .await?)
    }
);

#[must_use = "builders do nothing until awaited"]
pub struct EstimatedCountBuilder<E: Entity> {
    _marker: PhantomData<fn() -> E>,
    collection: Collection<Doc<E>>,
    options: EstimatedDocumentCountOptions,
}

impl<E: Entity> EstimatedCountBuilder<E> {
    pub(crate) fn new(collection: Collection<Doc<E>>) -> Self {
        EstimatedCountBuilder {
            _marker: PhantomData,
            collection,
            options: EstimatedDocumentCountOptions::default(),
        }
    }

    pub fn with_options(mut self, options: EstimatedDocumentCountOptions) -> Self {
        self.options = options;
        self
    }
}

impl<E: Entity> std::future::IntoFuture for EstimatedCountBuilder<E> {
    type Output = crate::Result<u64>;
    type IntoFuture = BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            Ok(self
                .collection
                .estimated_document_count()
                .with_options(self.options)
                .await?)
        })
    }
}
