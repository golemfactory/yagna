#!/bin/bash

export RUST_TEST_THREADS=1
cargo test -p ya-market --features ya-market/test-suite "$@"
