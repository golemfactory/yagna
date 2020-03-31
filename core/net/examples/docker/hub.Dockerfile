FROM rust:1.41

WORKDIR /usr/src/yagna-net
COPY . .

RUN cargo build --examples --bins
EXPOSE 9000
CMD [ "cargo", "run", "--example", "ya_sb_router", "--", "-l", "0.0.0.0:9000" ]
