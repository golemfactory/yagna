FROM rust:1.42
RUN cargo install cargo-deb
COPY . .
RUN cargo deb -p yagna
