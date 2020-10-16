# Decentralized market Mk1

## Running decentralized market test suite

To test market-test-suite run:
```
cargo test --workspace --features ya-market-decentralized/market-test-suite
```
or for market crate only
```
cargo test -p ya-market-decentralized --features ya-market-decentralized/market-test-suite
```

Note that market tests should be run in single thread.
We should investigate in future, why they sometimes fail with multiple threads
and tests running simultaneously.

### Running with logs enabled

It is very useful to see logs, if we want to debug test. We can do this as
always by adding RUST_LOG environment variable, but in test case we need to
add `env_logger::init();` on the beginning. 

```
RUST_LOG=debug cargo test -p ya-market-decentralized --features ya-market-decentralized/market-test-suite 
```