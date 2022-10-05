## Contents

This directory contains a Docker image used as a VM-workaround for the repository. The ExeUnit is modified so that
each SCP call is redirected to the `docker-tap` image and all VPN communication is redirected to that container.

## Building

### project binaries

```bash
cd pump
make
```

### docker image

```bash
docker build -t docker-tap -f Dockerfile .
```
