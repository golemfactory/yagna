
FROM rust as build
RUN apt-get update && apt-get install -y musl-tools
WORKDIR /usr/src/myapp
RUN rustup target add x86_64-unknown-linux-musl
COPY . .
WORKDIR /usr/src/myapp/core/serv
RUN cargo build --release --features log/release_max_level_info --target x86_64-unknown-linux-musl
WORKDIR /usr/src/myapp
RUN cd agent/requestor && cargo build --release --features log/release_max_level_info --target x86_64-unknown-linux-musl
RUN cd agent/provider && cargo build --release --features log/release_max_level_info --target x86_64-unknown-linux-musl
RUN cd target/x86_64-unknown-linux-musl/release && strip yagna ya-requestor ya-provider

FROM alpine
COPY --from=build /usr/src/myapp/target/x86_64-unknown-linux-musl/release/yagna /usr/bin/
COPY --from=build /usr/src/myapp/target/x86_64-unknown-linux-musl/release/ya-requestor /usr/bin/
COPY --from=build /usr/src/myapp/target/x86_64-unknown-linux-musl/release/ya-provider /usr/bin/
VOLUME /var/lib/yagna
ENV YAGNA_DATADIR=/var/lib/yagna



