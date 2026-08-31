use mongodb::bson::Document;
use mongodb::options::{
    FindOneAndDeleteOptions, FindOneAndReplaceOptions, FindOneAndUpdateOptions,
};

use crate::Result;
use crate::update::Update;

crate::ops::builder!(
    FindOneAndUpdateBuilder, FindOneAndUpdateOptions, Option<crate::Doc<E>>, [sort, return_document],
    { update: Result<(Document, Vec<Document>)> = (update: Update<E>) => update.into_parts() },
    |collection, filter, options| {
        let (mut update, element_filters) = update?;
        let mut options = options;
        crate::ops::merge_array_filters(&mut options.array_filters, element_filters);
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

crate::ops::builder!(
    FindOneAndReplaceBuilder, FindOneAndReplaceOptions, Option<crate::Doc<E>>, [sort, return_document],
    { replacement: Result<Document> = (entity: &E) => mongodb::bson::to_document(entity).map_err(Into::into) },
    |collection, filter, options| {
        let found = collection
            .clone_with_type::<Document>()
            .find_one_and_replace(filter?, replacement?)
            .with_options(options)
            .await?;
        Ok(found.map(mongodb::bson::from_document).transpose()?)
    }
);

crate::ops::upsert_setter!(FindOneAndUpdateBuilder);
crate::ops::upsert_setter!(FindOneAndReplaceBuilder);
