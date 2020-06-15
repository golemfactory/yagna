export RUST_LOG=info
export GSB_URL=tcp://127.0.0.1:6011
cargo run -p ya-provider -- --credit-address 0x213123123 --payment-url http://127.0.0.1:6001/payment-api/v1/ --activity-url http://127.0.0.1:6001/activity-api/v1/ --app-key f5d4942f47c04674bea9ab2da1549995 --market-url http://127.0.0.1:5001/market-api/v1/ --exe-unit-path local-exeunits-descriptor.json
