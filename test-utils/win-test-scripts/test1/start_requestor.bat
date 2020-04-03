set RUST_LOG=debug
curl -X POST "http://localhost:5001/admin/import-key" -H "accept: application/json" -H "Content-Type: application/json-patch+json" -d "[ { \"key\": \"5c34e0bddade4ad3af6d6a91e6aaafde\", \"nodeId\": \"0x6d241dd737f221ecfed38d614dae59994f59365a\" }]"
cargo run --bin ya-requestor -- --payment-url http://127.0.0.1:6000/payment-api/v1/ --activity-url http://127.0.0.1:6000/activity-api/v1/ --exe-script exe_script.json --app-key 5c34e0bddade4ad3af6d6a91e6aaafde --market-url http://localhost:5001/market-api/v1/
