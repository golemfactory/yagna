# Decentralized market Mk1

## Running yagna with decentralized market

You can enable decentralized market using cargo features.
Run yagna daemon with flags:
```
cargo run --no-default-features --features market-decentralized service run
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

