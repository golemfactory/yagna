
set CENTRAL_NET_HOST=127.0.0.1:7477
set GSB_URL=tcp://127.0.0.1:6011
set YAGNA_MARKET_URL=http://127.0.0.1:5001/market-api/v1/
set YAGNA_API_URL=http://127.0.0.1:6001
set YAGNA_ACTIVITY_URL=http://127.0.0.1:6001/activity-api/v1/
cargo run --release service run -d provider_data
