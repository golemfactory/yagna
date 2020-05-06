export RUST_LOG=info
NODE_ID=0xff64fcc901d9331b21d7ec89ee71b22e13d5e771
APP_KEY=0ef04f1d09954f129ba1c5b43109ba1a
curl -X POST "http://localhost:5001/admin/import-key" -H "accept: application/json" -H "Content-Type: application/json-patch+json" -d "[ { \"key\": \"${APP_KEY}\", \"nodeId\": \"${NODE_ID}\" }]"
cargo run --bin ya-requestor -- --activity-url http://127.0.0.1:6000/activity-api/v1/ --app-key ${APP_KEY} --market-url http://localhost:5001/market-api/v1/
