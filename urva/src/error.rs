use mongodb::bson::Bson;
use mongodb::error::{ErrorKind, WriteFailure};

use crate::lifecycle::IndexDiff;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Driver(#[from] mongodb::error::Error),

    #[error("{message}")]
    InvalidUpdate { message: String },

    #[error("version conflict in `{collection}` for id {id}")]
    VersionConflict {
        collection: &'static str,
        id: Box<Bson>,
    },

    #[error("condition failed in `{collection}` for id {id}")]
    ConditionFailed {
        collection: &'static str,
        id: Box<Bson>,
    },

    #[error("no document in `{collection}` with id {id}")]
    NotFound {
        collection: &'static str,
        id: Box<Bson>,
    },

    #[error("transaction gave up after {attempts} attempts: {last}")]
    TransactionTimeout {
        attempts: u32,
        last: mongodb::error::Error,
    },

    #[error("transaction commit failed: {0}")]
    CommitFailed(mongodb::error::Error),

    #[error("index drift detected: {0:?}")]
    IndexDrift(Box<IndexDiff>),
}

impl From<mongodb::bson::ser::Error> for Error {
    fn from(error: mongodb::bson::ser::Error) -> Self {
        Error::Driver(error.into())
    }
}

impl From<mongodb::bson::de::Error> for Error {
    fn from(error: mongodb::bson::de::Error) -> Self {
        Error::Driver(error.into())
    }
}

impl Error {
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Driver(e) | Error::CommitFailed(e) => {
                e.contains_label(mongodb::error::TRANSIENT_TRANSACTION_ERROR)
            }
            _ => false,
        }
    }

    pub fn is_duplicate_key(&self) -> bool {
        match self {
            Error::Driver(e) => driver_error_is_duplicate_key(e),
            _ => false,
        }
    }
}

const DUPLICATE_KEY_CODE: i32 = 11000;

fn driver_error_is_duplicate_key(error: &mongodb::error::Error) -> bool {
    match &*error.kind {
        ErrorKind::Write(WriteFailure::WriteError(write_error)) => {
            write_error.code == DUPLICATE_KEY_CODE
        }
        ErrorKind::Command(command_error) => command_error.code == DUPLICATE_KEY_CODE,
        ErrorKind::InsertMany(insert_many_error) => insert_many_error
            .write_errors
            .iter()
            .flatten()
            .any(|e| e.code == DUPLICATE_KEY_CODE),
        ErrorKind::BulkWrite(bulk_error) => bulk_error
            .write_errors
            .values()
            .any(|e| e.code == DUPLICATE_KEY_CODE),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn error_stays_small_enough_for_result() {
        assert!(std::mem::size_of::<super::Error>() <= 128);
    }
}
