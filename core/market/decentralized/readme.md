# Decentralized market Mk1

## Running yagna with decentralized market

You can enable decentralized market using cargo features.
Run yagna daemon with flags:
```
cargo run --no-default-features --features market-decentralized --features gnt-driver service run
```

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

### Building .deb
Prerequisites: 
- You need cargo-deb installed (`cargo install cargo-deb`).
- Build .deb on the oldest operating system version, you want to support.
Otherwise linking with GLIBC will fail.

Build yagna with all binaries needed in .deb:
```
cargo build --release --no-default-features --features market-decentralized --features gnt-driver --workspace
```

Run cargo-deb using binaries compiled in the previous step:
```
cargo deb --deb-version $(git rev-parse --short HEAD) --no-build
```
