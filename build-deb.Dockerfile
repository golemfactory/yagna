FROM ubuntu:16.04
ARG DEB_VERSION=unknown
WORKDIR /src/
RUN apt-get update && apt-get install -y curl gcc pkg-config libssl-dev
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-deb
COPY . .
RUN cargo deb -p yagna --deb-version $DEB_VERSION
