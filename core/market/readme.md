# Decentralized market Mk1

## Running decentralized market test suite

To invoke market test suite use:
```
cargo test --workspace --features ya-market/test-suite
```
or for market crate only
```
cargo test -p ya-market --features ya-market/test-suite
```

Note that market test suite uses single thread.

### Running with logs enabled

It is very useful to see logs, if we want to debug test. We can do this as
always by adding RUST_LOG environment variable, but in test case we need to
add `env_logger::init();` on the beginning. 

```
RUST_LOG=debug cargo test -p ya-market --features ya-market/test-suite 
```