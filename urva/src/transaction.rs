use std::time::{Duration, Instant};

use mongodb::error::{ErrorKind, UNKNOWN_TRANSACTION_COMMIT_RESULT, WriteFailure};
use mongodb::options::TransactionOptions;
use mongodb::{Client, ClientSession};

use crate::{Error, Result};

const MAX_TRANSACTION_TIME: Duration = Duration::from_secs(120);

pub struct Transaction {
    session: ClientSession,
    started: Instant,
}

impl Transaction {
    async fn begin(client: &Client, options: Option<TransactionOptions>) -> Result<Self> {
        let mut session = client.start_session().await?;
        session.start_transaction().with_options(options).await?;
        Ok(Transaction {
            session,
            started: Instant::now(),
        })
    }

    pub fn raw(&mut self) -> &mut ClientSession {
        &mut self.session
    }

    pub async fn commit(mut self) -> Result<()> {
        let started = self.started;
        commit_with_retry(&mut self.session, started)
            .await
            .map_err(Error::CommitFailed)
    }

    pub async fn abort(mut self) -> Result<()> {
        Ok(self.session.abort_transaction().await?)
    }
}

pub trait TransactionError: From<Error> {
    fn as_driver(&self) -> Option<&mongodb::error::Error> {
        None
    }
}

impl TransactionError for Error {
    fn as_driver(&self) -> Option<&mongodb::error::Error> {
        match self {
            Error::Driver(error) | Error::CommitFailed(error) => Some(error),
            _ => None,
        }
    }
}

pub trait TransactionExt {
    fn begin(&self) -> impl Future<Output = Result<Transaction>> + Send;

    fn begin_with(
        &self,
        options: TransactionOptions,
    ) -> impl Future<Output = Result<Transaction>> + Send;
}

impl TransactionExt for Client {
    fn begin(&self) -> impl Future<Output = Result<Transaction>> + Send {
        Transaction::begin(self, None)
    }

    fn begin_with(
        &self,
        options: TransactionOptions,
    ) -> impl Future<Output = Result<Transaction>> + Send {
        Transaction::begin(self, Some(options))
    }
}

async fn commit_with_retry(
    session: &mut ClientSession,
    started: Instant,
) -> mongodb::error::Result<()> {
    loop {
        match session.commit_transaction().await {
            Ok(()) => return Ok(()),
            Err(error)
                if error.contains_label(UNKNOWN_TRANSACTION_COMMIT_RESULT)
                    && !max_time_expired(&error)
                    && started.elapsed() < MAX_TRANSACTION_TIME => {}
            Err(error) => return Err(error),
        }
    }
}

fn max_time_expired(error: &mongodb::error::Error) -> bool {
    match &*error.kind {
        ErrorKind::Command(command) => command.code == 50,
        ErrorKind::Write(WriteFailure::WriteConcernError(concern)) => concern.code == 50,
        _ => false,
    }
}
