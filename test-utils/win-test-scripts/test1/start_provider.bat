set RUST_LOG=info
set GSB_URL=tcp://127.0.0.1:6011
cargo run --bin ya-provider -- --credit-address 0x213123123 --payment-url http://127.0.0.1:6001/payment-api/v1/ --activity-url http://127.0.0.1:6001/activity-api/v1/ --app-key 8fec2b8fce0f471ea7182fe76b6781f2 --market-url http://127.0.0.1:5001/market-api/v1/ --exe-unit-path local-exeunits-descriptor.json
