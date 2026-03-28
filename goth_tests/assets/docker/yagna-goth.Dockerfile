FROM ubuntu:latest

COPY deb/* ./
COPY bin/* /usr/bin/

RUN chmod +x /usr/bin/* \
    && apt install -y ./*.deb \
    && ln -s /usr/bin/exe-unit /usr/lib/yagna/plugins/exe-unit

ENTRYPOINT /usr/bin/yagna
