FROM ubuntu:18.04
WORKDIR /src/
COPY . .
RUN apt-get update && apt-get install -y curl gcc
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install cargo-deb
RUN cargo deb -p yagna
