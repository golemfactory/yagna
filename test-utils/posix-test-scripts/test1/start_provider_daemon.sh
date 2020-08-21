
export CENTRAL_NET_HOST=127.0.0.1:7477
export GSB_URL=tcp://127.0.0.1:6011
export YAGNA_MARKET_URL=http://127.0.0.1:5001/market-api/v1/
export YAGNA_API_URL=http://127.0.0.1:6001
export YAGNA_ACTIVITY_URL=http://127.0.0.1:6001/activity-api/v1/
cargo run service run -d provider_data
