use std::collections::HashSet;

use mongodb::error::ErrorKind;

use crate::Error;
use crate::doc::Doc;
use crate::entity::Entity;

pub struct Summary;

pub struct Partial;

pub struct PartialFailure<E: Entity> {
    pub error: Error,
    pub inserted: Vec<(usize, Doc<E>)>,
    pub rejected: Vec<(usize, E)>,
    pub unknown: Vec<(usize, E)>,
}

impl<E: Entity> PartialFailure<E> {
    pub(crate) fn split(
        error: Error,
        docs: Vec<(usize, Doc<E>)>,
        applied: Option<&[usize]>,
    ) -> Self {
        let mut failure = PartialFailure {
            error,
            inserted: Vec::new(),
            rejected: Vec::new(),
            unknown: Vec::new(),
        };
        match applied {
            None => {
                failure.unknown = docs
                    .into_iter()
                    .map(|(i, doc)| (i, doc.into_body()))
                    .collect()
            }
            Some(applied) => {
                let applied: HashSet<usize> = applied.iter().copied().collect();
                for (i, doc) in docs {
                    if applied.contains(&i) {
                        failure.inserted.push((i, doc));
                    } else {
                        failure.rejected.push((i, doc.into_body()));
                    }
                }
            }
        }
        failure
    }
}

impl<E: Entity> From<PartialFailure<E>> for Error {
    fn from(failure: PartialFailure<E>) -> Error {
        failure.error
    }
}

impl<E: Entity + std::fmt::Debug> std::fmt::Debug for PartialFailure<E>
where
    E::Id: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PartialFailure")
            .field("error", &self.error)
            .field("inserted", &self.inserted)
            .field("rejected", &self.rejected)
            .field("unknown", &self.unknown)
            .finish()
    }
}

pub(crate) fn indexed<E: Entity>(docs: Vec<Doc<E>>) -> Vec<(usize, Doc<E>)> {
    docs.into_iter().enumerate().collect()
}

pub(crate) fn nothing_sent(error: &mongodb::error::Error) -> bool {
    matches!(
        &*error.kind,
        ErrorKind::InvalidArgument { .. } | ErrorKind::BsonSerialization(_)
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BulkClass {
    PerOp { applied: Vec<usize> },
    Opaque,
}

pub(crate) fn classify_bulk_failure(
    ordered: bool,
    ops_len: usize,
    write_error_indices: &[usize],
    has_source: bool,
) -> BulkClass {
    if has_source || write_error_indices.is_empty() {
        return BulkClass::Opaque;
    }
    let applied = if ordered {
        let first = *write_error_indices.iter().min().expect("non-empty");
        (0..first).collect()
    } else {
        let failed: HashSet<usize> = write_error_indices.iter().copied().collect();
        (0..ops_len).filter(|i| !failed.contains(i)).collect()
    };
    BulkClass::PerOp { applied }
}

#[cfg(test)]
mod tests {
    use super::{BulkClass, PartialFailure, classify_bulk_failure};
    use crate::{Entity, Error};

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Body;

    impl crate::__private::Sealed for Body {}

    impl Entity for Body {
        const COLLECTION: &'static str = "bodies";
        type Id = i64;
        type Lock = ();
        const VERSION_FIELD: &'static str = "version";
    }

    fn docs() -> Vec<(usize, crate::Doc<Body>)> {
        (0..3)
            .map(|i| (i, crate::ops::save::prepare(i as i64, Body)))
            .collect()
    }

    fn error() -> Error {
        Error::InvalidUpdate {
            message: String::new(),
        }
    }

    fn indices<T>(entries: &[(usize, T)]) -> Vec<usize> {
        entries.iter().map(|(i, _)| *i).collect()
    }

    #[test]
    fn known_outcomes_split_into_inserted_and_rejected() {
        let failure = PartialFailure::split(error(), docs(), Some(&[0, 2]));
        assert_eq!(indices(&failure.inserted), [0, 2]);
        assert_eq!(indices(&failure.rejected), [1]);
        assert!(failure.unknown.is_empty());
    }

    #[test]
    fn opaque_outcomes_claim_nothing() {
        let failure = PartialFailure::split(error(), docs(), None);
        assert!(failure.inserted.is_empty());
        assert!(failure.rejected.is_empty());
        assert_eq!(indices(&failure.unknown), [0, 1, 2]);
    }

    #[test]
    fn ordered_applies_everything_before_the_first_failure() {
        assert_eq!(
            classify_bulk_failure(true, 5, &[3, 1], false),
            BulkClass::PerOp { applied: vec![0] }
        );
    }

    #[test]
    fn unordered_applies_the_complement_of_the_failures() {
        assert_eq!(
            classify_bulk_failure(false, 5, &[3, 1], false),
            BulkClass::PerOp {
                applied: vec![0, 2, 4]
            }
        );
    }

    #[test]
    fn wrapped_or_empty_reports_are_opaque() {
        assert_eq!(
            classify_bulk_failure(true, 5, &[1], true),
            BulkClass::Opaque
        );
        assert_eq!(
            classify_bulk_failure(true, 3, &[], false),
            BulkClass::Opaque
        );
    }
}
