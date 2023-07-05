FROM ubuntu:22.04

RUN apt update \
    && apt install -y libssl-dev ca-certificates \
    && update-ca-certificates \
    && apt install -y iperf3

COPY outbound-bench/target/release/outbound-bench /usr/bin/outbound-bench
COPY entrypoint.sh /golem/entrypoints/entrypoint.sh

RUN chmod +x /usr/bin/outbound-bench
RUN chmod +x /golem/entrypoints/entrypoint.sh

VOLUME /golem/work /golem/output /golem/resource