FROM ubuntu:latest

# Update & install networking tools
RUN apt-get update && apt-get install -y --no-install-recommends \
    iproute2 \
    iputils-ping \
    net-tools \
    curl \
    wget \
    dnsutils \
    telnet \
    traceroute \
    nmap \
    tcpdump \
    netcat-openbsd \
    mtr-tiny \
    whois \
    ethtool \
    iftop \
    iperf3 \
    socat \
    lsof \
    && apt-get clean && rm -rf /var/lib/apt/lists/*

COPY outbound-bench/target/x86_64-unknown-linux-musl/release/outbound-bench /usr/bin/outbound-bench

VOLUME /golem/work /golem/output /golem/resource