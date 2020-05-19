export RUST_LOG=info
NODE_ID=0xe9b968c7a9b4be74b322c92f718b13889fb7d4e9
APP_KEY=2478cba4f7a04c93bd37b329d42c9654
curl -X POST "http://localhost:5001/admin/import-key" -H "accept: application/json" -H "Content-Type: application/json-patch+json" -d "[ { \"key\": \"${APP_KEY}\", \"nodeId\": \"${NODE_ID}\" }]"
cargo run -p ya-provider -- --activity-url http://127.0.0.1:6001/activity-api/v1/ --app-key ${APP_KEY} --market-url http://127.0.0.1:5001/market-api/v1/ --exe-unit-path ../../../exe-unit
