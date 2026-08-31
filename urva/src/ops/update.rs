use mongodb::bson::Document;
use mongodb::options::UpdateOptions;
use mongodb::results::UpdateResult;

use crate::Result;
use crate::update::Update;

macro_rules! update_builder {
    ($name:ident, $verb:ident, [$($setter:ident),*]) => {
        crate::ops::builder!(
            $name, UpdateOptions, UpdateResult, [$($setter),*],
            { update: Result<(Document, Vec<Document>)> = (update: Update<E>) => update.into_parts() },
            |collection, filter, options| {
                let (mut update, element_filters) = update?;
                let mut options = options;
                crate::ops::merge_array_filters(&mut options.array_filters, element_filters);
                crate::update::seed_on_upsert::<E>(&mut update, options.upsert);
                Ok(collection
                    .$verb(filter?, update)
                    .with_options(options)
                    .await?)
            }
        );
    };
}

update_builder!(UpdateOneBuilder, update_one, [sort]);
update_builder!(UpdateManyBuilder, update_many, []);
crate::ops::upsert_setter!(UpdateOneBuilder);
crate::ops::upsert_setter!(UpdateManyBuilder);
