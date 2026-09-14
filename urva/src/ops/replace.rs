use mongodb::bson::Document;
use mongodb::options::ReplaceOptions;
use mongodb::results::UpdateResult;

use crate::Result;

crate::ops::builder!(
    ReplaceOneBuilder, ReplaceOptions, UpdateResult, [hint, sort],
    { replacement: Result<Document> = (entity: &E) => mongodb::bson::to_document(entity).map_err(Into::into) },
    |collection, filter, options, session| {
        let collection = collection.clone_with_type::<Document>();
        let action = collection
            .replace_one(filter?, replacement?)
            .with_options(options);
        Ok(crate::ops::run!(action, session)?)
    }
);

crate::ops::upsert_setter!(ReplaceOneBuilder);
