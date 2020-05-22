FROM rust:1.41

WORKDIR /usr/src/yagna-net
COPY . .

RUN cargo build --workspace --example ya_sb_router --example test_net_mk1
CMD target/debug/examples/test_net_mk1 --hub-addr 'hub:9000' listener
