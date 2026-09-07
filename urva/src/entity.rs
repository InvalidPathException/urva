use mongodb::IndexModel;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::doc::Lock;
use crate::index::IndexSpec;
use crate::version::Version;

pub trait Entity:
    crate::__private::Sealed + Serialize + DeserializeOwned + Send + Sync + Unpin + Sized + 'static
{
    const COLLECTION: &'static str;

    type Id: Serialize + DeserializeOwned + Clone + Send + Sync + Unpin + 'static;

    type Lock: Lock;

    #[doc(hidden)]
    const VERSION_FIELD: &'static str;

    #[doc(hidden)]
    const INDEX_SPECS: &'static [&'static IndexSpec];

    #[doc(hidden)]
    const EXTERNAL_INDEX_NAMES: &'static [&'static str];

    #[doc(hidden)]
    fn index_models() -> Vec<IndexModel> {
        Self::INDEX_SPECS
            .iter()
            .map(|spec| spec.to_model())
            .collect()
    }
}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is not versioned",
    label = "requires `versioned` in its `#[entity(...)]` attribute"
)]
pub trait Versioned: Entity<Lock = Version> {}

#[diagnostic::on_unimplemented(
    message = "`{Self}` is versioned, so whole-document replacement does not exist for it",
    label = "whole-document replacement bypasses the version lock",
    note = "use `save`, or `store.raw()` for an unchecked replacement"
)]
pub trait Unversioned: Entity<Lock = ()> {}

pub trait Embedded: crate::__private::Sealed + Serialize + DeserializeOwned {}
