pub mod count;
pub mod find;

macro_rules! builder {
    (
        $name:ident, $options:ty, $output:ty, [$($setter:ident $(: $setter_ty:ty)?),* $(,)?],
        |$collection:ident, $filter:ident, $opts:ident| $body:block
    ) => {
        #[must_use = "builders do nothing until awaited"]
        pub struct $name<E: crate::entity::Entity> {
            collection: mongodb::Collection<crate::Doc<E>>,
            filter: crate::Result<mongodb::bson::Document>,
            options: $options,
        }

        impl<E: crate::entity::Entity> $name<E> {
            pub(crate) fn new(
                collection: mongodb::Collection<crate::Doc<E>>,
                filter: crate::filter::Filter<E>,
            ) -> Self {
                $name {
                    collection,
                    filter: filter.into_document(),
                    options: <$options>::default(),
                }
            }

            pub fn with_options(mut self, options: $options) -> Self {
                self.options = options;
                self
            }

            $(crate::ops::setter!($setter $(: $setter_ty)?);)*
        }

        impl<E: crate::entity::Entity> ::std::future::IntoFuture for $name<E> {
            type Output = crate::Result<$output>;
            type IntoFuture = ::futures_util::future::BoxFuture<'static, Self::Output>;

            fn into_future(self) -> Self::IntoFuture {
                let $name { collection: $collection, filter: $filter, options: $opts } = self;
                Box::pin(async move { $body })
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
}

pub(crate) use {builder, setter};
