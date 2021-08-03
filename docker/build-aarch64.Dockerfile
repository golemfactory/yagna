# golemfactory/build-aarch64:0.1.1
#  Pre-configured environment for building aarch64 yagna binaries

# Example usage:
#  docker run -w /git-repo -it -v ~/Projects/yagna:/git-repo golemfactory/build-aarch64:0.1.1
#  $ cargo build --release --features static-openssl

FROM alpine:3.14

ENV PATH="/aarch64/bin:/aarch64/aarch64-linux-musl/bin:/root/.cargo/bin:$PATH"
ENV PROTOC="/usr/bin/protoc"

ENV RUSTFLAGS="-C linker=/aarch64/bin/aarch64-linux-musl-gcc -C link-arg=-lc -C link-arg=-lgcc -C link-arg=-latomic"
ENV CARGO_BUILD_TARGET="aarch64-unknown-linux-musl"

VOLUME /git-repo

RUN apk add --no-cache \
    gcc make pkgconfig \
    openssl-dev musl-dev \
    perl protoc curl bash

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && \
    rustup target add aarch64-unknown-linux-musl

RUN curl -O https://musl.cc/aarch64-linux-musl-cross.tgz && \
    tar xf aarch64-linux-musl-cross.tgz && \
    rm aarch64-linux-musl-cross.tgz && \
    mv /aarch64-linux-musl-cross /aarch64

CMD ["/bin/bash"]
