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
            |collection, filter, options, session| {
                let mut options = options;
                let (mut update, element_filters) = update?;
                crate::update::seed_on_upsert::<E>(&mut update, options.upsert);
                crate::ops::merge_array_filters(&mut options.array_filters, element_filters);
                let action = collection.$verb(filter?, update).with_options(options);
                Ok(crate::ops::run!(action, session)?)
            }
        );
    };
}

update_builder!(UpdateOneBuilder, update_one, [hint, sort]);
update_builder!(UpdateManyBuilder, update_many, [hint]);
crate::ops::upsert_setter!(UpdateOneBuilder);
crate::ops::upsert_setter!(UpdateManyBuilder);
