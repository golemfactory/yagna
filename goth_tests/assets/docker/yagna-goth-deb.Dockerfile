FROM ubuntu:latest

COPY deb/* ./
RUN chmod +x /usr/bin/* \
    && yes | apt install -y ./*.deb

ENTRYPOINT /usr/bin/yagna
