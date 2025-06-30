FROM ghcr.io/golemfactory/goth/yagna-goth-base:1.0.0

RUN update-ca-certificates

COPY deb/* ./
COPY bin/* /usr/bin/

RUN chmod +x /usr/bin/* \
    && apt install -y ./*.deb \
    && ln -s /usr/bin/exe-unit /usr/lib/yagna/plugins/exe-unit

ENTRYPOINT /usr/bin/yagna

ENV GOLEM_BASE_NETWORK=Custom
ENV GOLEM_BASE_CUSTOM_RPC_URL=http://golem-base:8545
ENV GOLEM_BASE_CUSTOM_WS_URL=ws://golem-base:8545
ENV GOLEM_BASE_CUSTOM_FAUCET_URL=http://golem-base:8545
ENV GOLEM_BASE_CUSTOM_L2_RPC_URL=http://golem-base:8555
ENV GOLEM_BASE_CUSTOM_FUND_PREALLOCATED=true
ENV GOLEM_BASE_OFFER_PUBLISH_TIMEOUT=10s

