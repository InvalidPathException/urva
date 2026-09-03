use mongodb::options::DeleteOptions;
use mongodb::results::DeleteResult;

crate::ops::builder!(
    DeleteOneBuilder,
    DeleteOptions,
    DeleteResult,
    [hint],
    {},
    |collection, filter, options| {
        Ok(collection.delete_one(filter?).with_options(options).await?)
    }
);

crate::ops::builder!(
    DeleteManyBuilder,
    DeleteOptions,
    DeleteResult,
    [hint],
    {},
    |collection, filter, options| {
        Ok(collection
            .delete_many(filter?)
            .with_options(options)
            .await?)
    }
);
