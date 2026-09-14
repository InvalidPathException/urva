use futures_util::TryStreamExt;
use mongodb::options::{FindOneOptions, FindOptions};
use mongodb::{Cursor, SessionCursor};

use crate::Result;
use crate::doc::Doc;
use crate::entity::Entity;
use crate::ops::Attached;
use crate::transaction::Transaction;

crate::ops::builder!(
    FindBuilder,
    FindOptions,
    Vec<Doc<E>>,
    [sort, hint, skip, limit: i64],
    {},
    |collection, filter, options, session| {
        let action = collection.find(filter?).with_options(options);
        Ok(match session {
            Some(tx) => {
                let cursor = action.session(tx.raw()).await;
                let mut cursor = tx.note(cursor)?;
                let docs = cursor.stream(tx.raw()).try_collect().await;
                tx.note(docs)?
            }
            None => action.await?.try_collect().await?,
        })
    }
);

impl<E: Entity> FindBuilder<E> {
    pub async fn stream(self) -> Result<Cursor<Doc<E>>> {
        Ok(self
            .collection
            .find(self.filter?)
            .with_options(self.options)
            .await?)
    }
}

impl<E: Entity> FindBuilder<E, Attached<'_>> {
    pub async fn stream(self) -> Result<TransactionCursor<E>> {
        let tx = self.exec.0;
        let cursor = self
            .collection
            .find(self.filter?)
            .with_options(self.options)
            .session(tx.raw())
            .await;
        Ok(TransactionCursor {
            cursor: tx.note(cursor)?,
        })
    }
}

pub struct TransactionCursor<E: Entity> {
    cursor: SessionCursor<Doc<E>>,
}

impl<E: Entity> TransactionCursor<E> {
    #[allow(clippy::should_implement_trait)]
    pub async fn next(&mut self, tx: &mut Transaction) -> Option<Result<Doc<E>>> {
        let item = self.cursor.next(tx.raw()).await?;
        Some(tx.note(item).map_err(Into::into))
    }
}

crate::ops::builder!(
    FindOneBuilder,
    FindOneOptions,
    Option<Doc<E>>,
    [sort, hint, skip],
    {},
    |collection, filter, options, session| {
        let action = collection.find_one(filter?).with_options(options);
        Ok(crate::ops::run!(action, session)?)
    }
);
