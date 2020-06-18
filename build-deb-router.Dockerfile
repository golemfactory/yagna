FROM build-deb
ARG DEB_VERSION=unknown

RUN cargo deb -p ya-sb-router --deb-version $DEB_VERSION -- --example ya_sb_router
