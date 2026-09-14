use mongodb::bson::Document;
use mongodb::options::{
    FindOneAndDeleteOptions, FindOneAndReplaceOptions, FindOneAndUpdateOptions,
};

use crate::Result;
use crate::update::Update;

crate::ops::builder!(
    FindOneAndUpdateBuilder, FindOneAndUpdateOptions, Option<crate::Doc<E>>, [sort, return_document, hint],
    { update: Result<(Document, Vec<Document>)> = (update: Update<E>) => update.into_parts() },
    |collection, filter, options, session| {
        let mut options = options;
        let (mut update, element_filters) = update?;
        crate::update::seed_on_upsert::<E>(&mut update, options.upsert);
        crate::ops::merge_array_filters(&mut options.array_filters, element_filters);
        let action = collection
            .find_one_and_update(filter?, update)
            .with_options(options);
        Ok(crate::ops::run!(action, session)?)
    }
);

crate::ops::builder!(
    FindOneAndDeleteBuilder,
    FindOneAndDeleteOptions,
    Option<crate::Doc<E>>,
    [sort, hint],
    {},
    |collection, filter, options, session| {
        let action = collection
            .find_one_and_delete(filter?)
            .with_options(options);
        Ok(crate::ops::run!(action, session)?)
    }
);

crate::ops::builder!(
    FindOneAndReplaceBuilder, FindOneAndReplaceOptions, Option<crate::Doc<E>>, [sort, return_document, hint],
    { replacement: Result<Document> = (entity: &E) => mongodb::bson::to_document(entity).map_err(Into::into) },
    |collection, filter, options, session| {
        let collection = collection.clone_with_type::<Document>();
        let action = collection
            .find_one_and_replace(filter?, replacement?)
            .with_options(options);
        let found = crate::ops::run!(action, session)?;
        Ok(found.map(mongodb::bson::from_document).transpose()?)
    }
);

crate::ops::upsert_setter!(FindOneAndUpdateBuilder);
crate::ops::upsert_setter!(FindOneAndReplaceBuilder);
