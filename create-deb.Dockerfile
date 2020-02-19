FROM rust
WORKDIR /src/
COPY . .
RUN cargo install cargo-deb
RUN cargo deb -p yagna
