# debian:bullseye-slim, 2021-05-12
FROM debian@sha256:e827c9bc6913625ec75c8b466e6e79a6b936d0801956be607bce08a07078f57a

RUN apt update \
    && apt install -y libssl-dev ca-certificates \
    && update-ca-certificates

COPY deb/* ./
COPY bin/* /usr/bin/

RUN chmod +x /usr/bin/* \
    && apt install -y ./*.deb \
    && ln -s /usr/bin/exe-unit /usr/lib/yagna/plugins/exe-unit

ENTRYPOINT /usr/bin/yagna
