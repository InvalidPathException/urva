# urva

MongoDB ODM for Rust.

## Testing

Unit and compile-fail tests only require Rust toolchain:

```sh
cargo test --workspace
```

Integration tests run against a live single-node replica set. They skip
themselves when `URVA_TEST_URI` is unset, `URVA_REQUIRE_INTEGRATION=1`
makes tests fail instead of skipped. Each test creates its own database
and drops it afterwards.

```sh
docker compose up -d --wait
URVA_TEST_URI='mongodb://localhost:27018/?directConnection=true' URVA_REQUIRE_INTEGRATION=1 cargo test --workspace
docker compose down
```

On top of the above, CI also runs these, which you should run before commiting:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings -A clippy::duplicated_attributes
```
