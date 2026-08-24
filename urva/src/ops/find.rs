use futures_util::TryStreamExt;
use mongodb::Cursor;
use mongodb::options::{FindOneOptions, FindOptions};

use crate::Result;
use crate::doc::Doc;
use crate::entity::Entity;

crate::ops::builder!(
    FindBuilder,
    FindOptions,
    Vec<Doc<E>>,
    [sort, skip, limit: i64],
    |collection, filter, options| {
        Ok(collection
            .find(filter?)
            .with_options(options)
            .await?
            .try_collect()
            .await?)
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

crate::ops::builder!(
    FindOneBuilder,
    FindOneOptions,
    Option<Doc<E>>,
    [sort, skip],
    |collection, filter, options| { Ok(collection.find_one(filter?).with_options(options).await?) }
);
