use mongodb::bson::Document;
use mongodb::options::{FindOneAndDeleteOptions, FindOneAndUpdateOptions};

use crate::Result;
use crate::update::Update;

crate::ops::builder!(
    FindOneAndUpdateBuilder, FindOneAndUpdateOptions, Option<crate::Doc<E>>, [sort, return_document],
    { update: Result<Document> = (update: Update<E>) => update.into_document() },
    |collection, filter, options| {
        let mut update = update?;
        crate::update::seed_on_upsert::<E>(&mut update, options.upsert);
        Ok(collection
            .find_one_and_update(filter?, update)
            .with_options(options)
            .await?)
    }
);

crate::ops::builder!(
    FindOneAndDeleteBuilder,
    FindOneAndDeleteOptions,
    Option<crate::Doc<E>>,
    [sort],
    {},
    |collection, filter, options| {
        Ok(collection
            .find_one_and_delete(filter?)
            .with_options(options)
            .await?)
    }
);

crate::ops::upsert_setter!(FindOneAndUpdateBuilder);
