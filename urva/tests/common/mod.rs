use mongodb::{Client, Database};

pub struct TestDb {
    pub db: Database,
    uri: String,
    dropped: bool,
}

impl TestDb {
    pub async fn connect(test_name: &str) -> Option<TestDb> {
        let uri = match std::env::var("URVA_TEST_URI") {
            Ok(uri) => uri,
            Err(_) => {
                assert!(
                    std::env::var("URVA_REQUIRE_INTEGRATION").is_err(),
                    "URVA_REQUIRE_INTEGRATION is set but URVA_TEST_URI is not"
                );
                eprintln!("URVA_TEST_URI unset, skipping integration test {test_name}");
                return None;
            }
        };
        let client = Client::with_uri_str(&uri).await.expect("connect");
        let name = format!(
            "urva_{test_name}_{}",
            mongodb::bson::oid::ObjectId::new().to_hex()
        );
        Some(TestDb {
            db: client.database(&name),
            uri,
            dropped: false,
        })
    }

    pub async fn drop(mut self) {
        self.db.drop().await.expect("drop test database");
        self.dropped = true;
    }
}

impl Drop for TestDb {
    fn drop(&mut self) {
        if self.dropped {
            return;
        }
        let uri = self.uri.clone();
        let name = self.db.name().to_string();
        let cleanup = std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            let _ = rt.block_on(async {
                let client = Client::with_uri_str(&uri).await?;
                client.database(&name).drop().await
            });
        });
        let _ = cleanup.join();
    }
}
