pub mod count;
pub mod delete;
pub mod find;
pub mod find_and_modify;
pub mod partial;
pub mod replace;
pub mod save;
pub mod update;

use futures_util::future::BoxFuture;

use crate::transaction::Transaction;

mod sealed {
    pub trait Sealed {}
}

#[doc(hidden)]
pub trait Exec: sealed::Sealed + Send + Sized {
    type Fut<T: Send + 'static>: Future<Output = T> + Send;

    fn run<T, F>(self, f: F) -> Self::Fut<T>
    where
        T: Send + 'static,
        F: for<'s> FnOnce(Option<&'s mut Transaction>) -> BoxFuture<'s, T> + Send + 'static;
}

pub struct Detached;

pub struct Attached<'s>(pub(crate) &'s mut Transaction);

pub struct ByFilter;

pub struct ById;

impl sealed::Sealed for Detached {}

impl Exec for Detached {
    type Fut<T: Send + 'static> = BoxFuture<'static, T>;

    fn run<T, F>(self, f: F) -> BoxFuture<'static, T>
    where
        T: Send + 'static,
        F: for<'s> FnOnce(Option<&'s mut Transaction>) -> BoxFuture<'s, T> + Send + 'static,
    {
        f(None)
    }
}

impl sealed::Sealed for Attached<'_> {}

impl<'a> Exec for Attached<'a> {
    type Fut<T: Send + 'static> = BoxFuture<'a, T>;

    fn run<T, F>(self, f: F) -> BoxFuture<'a, T>
    where
        T: Send + 'static,
        F: for<'s> FnOnce(Option<&'s mut Transaction>) -> BoxFuture<'s, T> + Send + 'static,
    {
        f(Some(self.0))
    }
}

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
        |$collection:ident, $filter:ident, $opts:ident, $session:ident| $body:block
    ) => {
        #[must_use = "builders do nothing until awaited"]
        pub struct $name<E: crate::entity::Entity, X = crate::ops::Detached, P = crate::ops::ByFilter> {
            _marker: ::std::marker::PhantomData<fn() -> (E, P)>,
            collection: mongodb::Collection<crate::Doc<E>>,
            exec: X,
            filter: crate::Result<mongodb::bson::Document>,
            $($extra: $extra_ty,)*
            options: $options,
        }

        impl<E: crate::entity::Entity, P> $name<E, crate::ops::Detached, P> {
            pub(crate) fn new(
                collection: mongodb::Collection<crate::Doc<E>>,
                filter: crate::filter::Filter<E>,
                $($arg: $arg_ty),*
            ) -> Self {
                $name {
                    _marker: ::std::marker::PhantomData,
                    collection,
                    exec: crate::ops::Detached,
                    filter: filter.into_document(),
                    $($extra: $init,)*
                    options: <$options>::default(),
                }
            }

            pub fn session<'t>(self, tx: &'t mut crate::Transaction) -> $name<E, crate::ops::Attached<'t>, P> {
                $name {
                    _marker: ::std::marker::PhantomData,
                    collection: self.collection,
                    exec: crate::ops::Attached(tx),
                    filter: self.filter,
                    $($extra: self.$extra,)*
                    options: self.options,
                }
            }
        }

        crate::ops::option_setters!($name, $options, [$($setter $(: $setter_ty)?),*]);

        impl<E: crate::entity::Entity, X: crate::ops::Exec, P> ::std::future::IntoFuture for $name<E, X, P> {
            type Output = crate::Result<$output>;
            type IntoFuture = X::Fut<Self::Output>;

            fn into_future(self) -> Self::IntoFuture {
                let $name { exec, collection: $collection, filter: $filter, options: $opts, $($extra,)* .. } = self;
                exec.run(move |$session| Box::pin(async move { $body }))
            }
        }
    };
}

macro_rules! option_setters {
    ($builder:ident, $options:ty, [$($method:ident $(: $ty:ty)?),* $(,)?]) => {
        impl<E: crate::entity::Entity, X, P> $builder<E, X, P> {
            pub fn with_options(mut self, options: $options) -> Self {
                self.options = options;
                self
            }

            $(crate::ops::setter!($method $(: $ty)?);)*
        }
    };
}

macro_rules! upsert_setter {
    ($builder:ident) => {
        impl<E: crate::entity::Entity, X> $builder<E, X, crate::ops::ByFilter> {
            pub fn upsert(mut self) -> Self
            where
                E::Id: crate::doc::NewId,
            {
                self.options.upsert = Some(true);
                self
            }
        }

        impl<E: crate::entity::Entity, X> $builder<E, X, crate::ops::ById> {
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
    (hint) => {
        pub fn hint(mut self, hint: impl crate::index::HintFor<E>) -> Self {
            self.options.hint = Some(hint.to_hint());
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

macro_rules! run {
    ($action:expr, $tx:expr) => {
        match $tx {
            Some(tx) => {
                let result = $action.session(tx.raw()).await;
                tx.note(result)
            }
            None => $action.await,
        }
    };
}

pub(crate) use {builder, option_setters, run, setter, upsert_setter};

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
