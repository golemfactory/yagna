FROM ubuntu:latest

RUN apt update \
    && apt install -y iperf3

COPY outbound-bench/target/x86_64-unknown-linux-musl/release/outbound-bench /usr/bin/outbound-bench

VOLUME /golem/work /golem/output /golem/resource