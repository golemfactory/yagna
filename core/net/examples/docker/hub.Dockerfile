FROM rust:1.40

WORKDIR /usr/src/yagna-net
COPY . .

RUN cargo build --examples --bins
EXPOSE 9000
CMD [ "cargo", "run", "--bin", "ya-sb-router", "--", "-l", "0.0.0.0:9000" ]
