FROM rust:1.41

WORKDIR /usr/src/yagna-net
COPY . .

RUN cargo build --workspace --examples --bins
EXPOSE 9000
CMD [ "cargo", "run", "-p", "ya-sb-router", "--example", "ya_sb_router", "--", "-l", "tcp://0.0.0.0:9000" ]
