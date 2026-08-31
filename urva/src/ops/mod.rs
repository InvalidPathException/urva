pub mod count;
pub mod delete;
pub mod find;
pub mod find_and_modify;
pub mod partial;
pub mod replace;
pub mod save;
pub mod update;

pub struct ByFilter;

pub struct ById;

pub(crate) fn merge_array_filters(
    options: &mut Option<Vec<mongodb::bson::Document>>,
    element_filters: Vec<mongodb::bson::Document>,
) {
    if !element_filters.is_empty() {
        options.get_or_insert_with(Vec::new).extend(element_filters);
    }
}

macro_rules! builder {
    (
        $name:ident, $options:ty, $output:ty, [$($setter:ident $(: $setter_ty:ty)?),* $(,)?],
        { $($extra:ident : $extra_ty:ty = ($arg:ident : $arg_ty:ty) => $init:expr),* $(,)? },
        |$collection:ident, $filter:ident, $opts:ident| $body:block
    ) => {
        #[must_use = "builders do nothing until awaited"]
        pub struct $name<E: crate::entity::Entity, P = crate::ops::ByFilter> {
            _marker: ::std::marker::PhantomData<fn() -> P>,
            collection: mongodb::Collection<crate::Doc<E>>,
            filter: crate::Result<mongodb::bson::Document>,
            $($extra: $extra_ty,)*
            options: $options,
        }

        impl<E: crate::entity::Entity, P> $name<E, P> {
            pub(crate) fn new(
                collection: mongodb::Collection<crate::Doc<E>>,
                filter: crate::filter::Filter<E>,
                $($arg: $arg_ty),*
            ) -> Self {
                $name {
                    _marker: ::std::marker::PhantomData,
                    collection,
                    filter: filter.into_document(),
                    $($extra: $init,)*
                    options: <$options>::default(),
                }
            }

            pub fn with_options(mut self, options: $options) -> Self {
                self.options = options;
                self
            }

            $(crate::ops::setter!($setter $(: $setter_ty)?);)*
        }

        impl<E: crate::entity::Entity, P> ::std::future::IntoFuture for $name<E, P> {
            type Output = crate::Result<$output>;
            type IntoFuture = ::futures_util::future::BoxFuture<'static, Self::Output>;

            fn into_future(self) -> Self::IntoFuture {
                let $name { collection: $collection, filter: $filter, options: $opts, $($extra,)* .. } = self;
                Box::pin(async move { $body })
            }
        }
    };
}

macro_rules! upsert_setter {
    ($builder:ident) => {
        impl<E: crate::entity::Entity> $builder<E, crate::ops::ByFilter> {
            pub fn upsert(mut self) -> Self
            where
                E::Id: crate::doc::NewId,
            {
                self.options.upsert = Some(true);
                self
            }
        }

        impl<E: crate::entity::Entity> $builder<E, crate::ops::ById> {
            pub fn upsert(mut self) -> Self {
                self.options.upsert = Some(true);
                self
            }
        }
    };
}

macro_rules! setter {
    (sort) => {
        pub fn sort(mut self, sort: crate::sort::Sort<E>) -> Self {
            self.options.sort = Some(sort.into_document());
            self
        }
    };
    (skip) => {
        pub fn skip(mut self, skip: u64) -> Self {
            self.options.skip = Some(skip);
            self
        }
    };
    (limit: $ty:ty) => {
        pub fn limit(mut self, limit: $ty) -> Self {
            self.options.limit = Some(limit);
            self
        }
    };
    (return_document) => {
        pub fn return_document(mut self, which: mongodb::options::ReturnDocument) -> Self {
            self.options.return_document = Some(which);
            self
        }
    };
}

pub(crate) use {builder, setter, upsert_setter};

#[cfg(test)]
mod tests {
    use mongodb::bson::doc;

    #[test]
    fn array_filters_append_to_the_options() {
        let mut options = Some(vec![doc! { "hot.qty": { "$gt": 5 } }]);
        super::merge_array_filters(&mut options, vec![doc! { "cold.qty": 1 }]);
        assert_eq!(
            options.unwrap(),
            vec![doc! { "hot.qty": { "$gt": 5 } }, doc! { "cold.qty": 1 }]
        );

        let mut options = None;
        super::merge_array_filters(&mut options, Vec::new());
        assert!(options.is_none());
        super::merge_array_filters(&mut options, vec![doc! { "hot.qty": 1 }]);
        assert_eq!(options.unwrap(), vec![doc! { "hot.qty": 1 }]);
    }
}
