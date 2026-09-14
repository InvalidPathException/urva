use mongodb::options::DeleteOptions;
use mongodb::results::DeleteResult;

crate::ops::builder!(
    DeleteOneBuilder,
    DeleteOptions,
    DeleteResult,
    [hint],
    {},
    |collection, filter, options, session| {
        let action = collection.delete_one(filter?).with_options(options);
        Ok(crate::ops::run!(action, session)?)
    }
);

crate::ops::builder!(
    DeleteManyBuilder,
    DeleteOptions,
    DeleteResult,
    [hint],
    {},
    |collection, filter, options, session| {
        let action = collection.delete_many(filter?).with_options(options);
        Ok(crate::ops::run!(action, session)?)
    }
);
