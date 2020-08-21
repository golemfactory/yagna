
export CENTRAL_NET_HOST=127.0.0.1:7477
export GSB_URL=tcp://127.0.0.1:6010
export YAGNA_MARKET_URL=http://127.0.0.1:5001/market-api/v1/
export YAGNA_API_URL=http://127.0.0.1:6000
export YAGNA_ACTIVITY_URL=http://127.0.0.1:6000/activity-api/v1/
cargo run --release service run -d requestor_data
