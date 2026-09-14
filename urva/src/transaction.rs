use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mongodb::error::{
    ErrorKind, TRANSIENT_TRANSACTION_ERROR, UNKNOWN_TRANSACTION_COMMIT_RESULT, WriteFailure,
};
use mongodb::options::TransactionOptions;
use mongodb::{Client, ClientSession};

use crate::{Error, Result};

const MAX_TRANSACTION_TIME: Duration = Duration::from_secs(120);

pub struct Transaction {
    session: Option<ClientSession>,
    started: Instant,
    transient: Arc<Mutex<Option<mongodb::error::Error>>>,
    first_failure: Arc<Mutex<Option<mongodb::error::Error>>>,
    park: Option<Park>,
}

type Park = Arc<Mutex<Option<ClientSession>>>;

impl Drop for Transaction {
    fn drop(&mut self) {
        if let Some(park) = self.park.take()
            && let Some(session) = self.session.take()
        {
            *park.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
        }
    }
}

impl Transaction {
    async fn begin(client: &Client, options: Option<TransactionOptions>) -> Result<Self> {
        let mut session = client.start_session().await?;
        session.start_transaction().with_options(options).await?;
        Ok(Transaction {
            session: Some(session),
            started: Instant::now(),
            transient: Arc::default(),
            first_failure: Arc::default(),
            park: None,
        })
    }

    fn session(&mut self) -> &mut ClientSession {
        self.session
            .as_mut()
            .expect("the session leaves only in Drop")
    }

    pub(crate) fn note<T>(&self, result: mongodb::error::Result<T>) -> mongodb::error::Result<T> {
        if let Err(error) = &result {
            if error.contains_label(TRANSIENT_TRANSACTION_ERROR) {
                *self.transient.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
            } else if !crate::ops::partial::nothing_sent(error) {
                self.first_failure
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get_or_insert_with(|| error.clone());
            }
        }
        result
    }

    fn take_first_failure(&self) -> Option<mongodb::error::Error> {
        self.first_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
    }

    pub fn raw(&mut self) -> &mut ClientSession {
        self.session()
    }

    pub async fn commit(mut self) -> Result<()> {
        if let Some(first) = self.take_first_failure() {
            let _ = self.session().abort_transaction().await;
            return Err(Error::Driver(first));
        }
        let started = self.started;
        commit_with_retry(self.session(), started)
            .await
            .map_err(Error::CommitFailed)
    }

    pub async fn abort(mut self) -> Result<()> {
        Ok(self.session().abort_transaction().await?)
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

    fn transaction<R, F, Fut>(&self, body: F) -> impl Future<Output = Result<R>>
    where
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R)>>;

    fn transaction_with<R, F, Fut>(
        &self,
        options: TransactionOptions,
        body: F,
    ) -> impl Future<Output = Result<R>>
    where
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R)>>;

    fn try_transaction<R, E, F, Fut>(&self, body: F) -> impl Future<Output = Result<R, E>>
    where
        E: TransactionError,
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R), E>>;

    fn try_transaction_with<R, E, F, Fut>(
        &self,
        options: TransactionOptions,
        body: F,
    ) -> impl Future<Output = Result<R, E>>
    where
        E: TransactionError,
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R), E>>;
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

    fn transaction<R, F, Fut>(&self, body: F) -> impl Future<Output = Result<R>>
    where
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R)>>,
    {
        run(self, None, body)
    }

    fn transaction_with<R, F, Fut>(
        &self,
        options: TransactionOptions,
        body: F,
    ) -> impl Future<Output = Result<R>>
    where
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R)>>,
    {
        run(self, Some(options), body)
    }

    fn try_transaction<R, E, F, Fut>(&self, body: F) -> impl Future<Output = Result<R, E>>
    where
        E: TransactionError,
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R), E>>,
    {
        run(self, None, body)
    }

    fn try_transaction_with<R, E, F, Fut>(
        &self,
        options: TransactionOptions,
        body: F,
    ) -> impl Future<Output = Result<R, E>>
    where
        E: TransactionError,
        F: FnMut(Transaction) -> Fut,
        Fut: Future<Output = Result<(Transaction, R), E>>,
    {
        run(self, Some(options), body)
    }
}

async fn run<R, E, F, Fut>(
    client: &Client,
    options: Option<TransactionOptions>,
    mut body: F,
) -> Result<R, E>
where
    E: TransactionError,
    F: FnMut(Transaction) -> Fut,
    Fut: Future<Output = Result<(Transaction, R), E>>,
{
    let started = Instant::now();
    let mut attempt = 0u32;
    let mut last_transient: Option<mongodb::error::Error> = None;
    loop {
        if let Some(last) = last_transient.take() {
            let backoff = backoff(attempt);
            if started.elapsed() + backoff >= MAX_TRANSACTION_TIME {
                return Err(Error::TransactionTimeout {
                    attempts: attempt,
                    last,
                }
                .into());
            }
            tokio::time::sleep(backoff).await;
        }
        attempt += 1;

        let mut tx = Transaction::begin(client, options.clone()).await?;
        tx.started = started;
        let transient = tx.transient.clone();
        let park: Park = Arc::default();
        tx.park = Some(park.clone());
        let (mut tx, value) = match body(tx).await {
            Ok(done) => done,
            Err(error) => {
                let parked = park.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(mut session) = parked {
                    let _ = session.abort_transaction().await;
                }
                let noted = transient.lock().unwrap_or_else(|e| e.into_inner()).take();
                let returned = error
                    .as_driver()
                    .filter(|d| d.contains_label(TRANSIENT_TRANSACTION_ERROR))
                    .cloned();
                match noted.or(returned) {
                    Some(driver) => {
                        last_transient = Some(driver);
                        continue;
                    }
                    None => return Err(error),
                }
            }
        };
        tx.park = None;
        if let Some(first) = tx.take_first_failure() {
            let _ = tx.session().abort_transaction().await;
            return Err(Error::Driver(first).into());
        }
        let noted = transient.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(noted) = noted {
            let _ = tx.session().abort_transaction().await;
            last_transient = Some(noted);
            continue;
        }
        match commit_with_retry(tx.session(), started).await {
            Ok(()) => return Ok(value),
            Err(error) if error.contains_label(TRANSIENT_TRANSACTION_ERROR) => {
                last_transient = Some(error);
            }
            Err(error) => return Err(Error::CommitFailed(error).into()),
        }
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

fn backoff(attempt: u32) -> Duration {
    let jitter = f64::from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0),
    ) / 1e9;
    let computed = jitter * 5.0 * 1.5f64.powi(attempt as i32);
    let max = jitter * 500.0;
    Duration::from_millis(computed.min(max).round() as u64)
}
