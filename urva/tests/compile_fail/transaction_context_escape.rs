use urva::prelude::*;

async fn escape(client: &urva::mongodb::Client) {
    let mut stash: Option<Transaction> = None;
    let _ = client
        .transaction(|tx| async {
            stash = Some(tx);
            Err::<(Transaction, ()), urva::Error>(Error::InvalidUpdate {
                message: String::new(),
            })
        })
        .await;
}

fn main() {}
