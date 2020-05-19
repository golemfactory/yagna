export RUST_LOG=info
cargo run -p ya-requestor -- --payment-url http://127.0.0.1:6000/payment-api/v1/ --activity-url http://127.0.0.1:6000/activity-api/v1/ --exe-script exe_script.json --app-key 48e094f2e35e42c5a18565846fe18802 --market-url http://localhost:5001/market-api/v1/
