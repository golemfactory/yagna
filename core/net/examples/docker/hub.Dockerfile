FROM rust:1.41

WORKDIR /usr/src/yagna-net
COPY . .

RUN cargo build --workspace --example ya_sb_router --example test_net_mk1
EXPOSE 9000
CMD target/debug/examples/ya_sb_router -l 'tcp://0.0.0.0:9000'
