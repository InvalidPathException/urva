use mongodb::bson::Document;
use mongodb::options::ReplaceOptions;
use mongodb::results::UpdateResult;

use crate::Result;

crate::ops::builder!(
    ReplaceOneBuilder, ReplaceOptions, UpdateResult, [hint, sort],
    { replacement: Result<Document> = (entity: &E) => mongodb::bson::to_document(entity).map_err(Into::into) },
    |collection, filter, options| {
        Ok(collection
            .clone_with_type::<Document>()
            .replace_one(filter?, replacement?)
            .with_options(options)
            .await?)
    }
);

crate::ops::upsert_setter!(ReplaceOneBuilder);
