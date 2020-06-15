set YAGNA_API_URL=http://127.0.0.1:6000
set CENTRAL_NET_HOST=127.0.0.1:7477
set GSB_URL=tcp://127.0.0.1:6011
cargo run -- %1 %2 %3 %4 %5 %6 -d provider_data
