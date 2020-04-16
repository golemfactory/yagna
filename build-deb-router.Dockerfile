FROM build-deb

RUN cargo deb -p ya-sb-router -- --example ya_sb_router
