FROM build-deb

RUN cargo build --release --example ya_sb_router
