# Hybrid network dev setup

## Local network setup diagram

```mermaid
flowchart BT
    relay[ya-relay]

    y_p <-->|GSB| relay
    y_r <-->|GSB| relay
    gftp_p -.-|GSB| gftp_r

    subgraph Requestor
        y_r[yagna]
        gftp_r[\gftp\]
        
        y_r -.-|GSB| gftp_r
        py_api(yapapi) -->|REST| y_r
        js_api(yajsapi) -->|REST| y_r
        py_api -.-|File IO| gftp_r
        js_api -.-|File IO| gftp_r

        subgraph Python App
            py_hello(hello-world \n yapapi/examples)
            py_api(yapapi)
            
            py_hello -.->|implements| py_api
        end

        subgraph Js App - Optional
            js_hello(hello-world \n yajsapi/examples)
            js_api(yajsapi)

            js_hello -.->|implements| js_api
        end
    end

    subgraph Provider
        y_p[yagna]
        prov[ya-provider \nyagna/agent/provider]
        gftp_p[\gftp\]

        y_p -->|REST| prov
        y_p -.-|GSB| gftp_p
        prov -.->|spawns| exe_vm
        prov -.->|spawns| exe_wasi
        exe_vm -.-|File IO| gftp_p
        exe_wasi -.-|File IO| gftp_p

        subgraph VM Runtime
            exe_vm(ya-exe-unit \n yagna/exe-unit) 
            vm(ya-runtime-vm)

            exe_vm -.->|spawns| vm
        end

        subgraph Wasm Runtime - Optional
            exe_wasi(ya-exe-unit \n yagna/exe-unit)
            wasi(ya-runtime-wasi)

            exe_wasi -.->|spawns| wasi
        end
    end
```

## Setup

### Prerequisites

- Linux - Only VM runtime is actively maintained and it requires Linux. It works on WSL.
- rust
- Python 3.x (and preferably [Poetry](https://python-poetry.org/docs/#installation))
- NodeJS [Optional]

Additional packages:

```bash
sudo apt install  build-essential libprotobuf-dev protobuf-compiler
```

### Projects checkout

Clone projects into workspace directory:

```sh
git clone git@github.com:golemfactory/yagna.git
git clone git@github.com:golemfactory/ya-relay.git
git clone git@github.com:golemfactory/yapapi.git
git clone git@git@github.com:golemfactory/yajsapi.git
git clone git@github.com:golemfactory/ya-runtime-wasi.git
# It is worth to have sources, but this guide will use binaries of VM runtime
git clone git@github.com:golemfactory/ya-runtime-vm.git
```

### Building projects

```sh
# Build yagna (project contains ya-provider, exe-unit, and gftp)
cd yagna;
cargo build --all;
cargo install --path core/gftp --features bin
cd -;

# Build ya-relay
cd ya-relay;
cargo build;
cd -;

# Download ya-runtime-vm binaries
wget -q $(curl -s https://api.github.com/repos/golemfactory/ya-runtime-vm/releases/latest | grep browser_download_url | grep "linux" | cut -d '"' -f 4);
mkdir ya-runtime-vm_bin;
tar -xzf $(ls ya-runtime-vm-linux-*.tar.gz) --strip-components 1 --directory ya-runtime-vm_bin

# Build ya-runtime-wasi
cd ya-runtime-wasi;
cargo build;
cd -;
```

### Configuring Requestor and Provider

#### Requestor

```sh
mkdir ya-req_hybrid;
cp .env-template ya-req_hybrid/.env

sed -e "s/__YOUR_NODE_NAME_GOES_HERE__/$USER_requestor/" .env
sed -e "s/__NET_TYPE__/hybrid/" .env

tee -a ya-req_hybrid/.env << END
RUST_LOG=debug,tokio_core=info,tokio_reactor=info,hyper=info,reqwest=info
GSB_URL=tcp://127.0.0.1:12501
YAGNA_API_URL=http://127.0.0.1:12502
YA_NET_BIND_URL=udp://0.0.0.0:12502
YA_NET_RELAY_HOST=127.0.0.1:7464
END
```

Start [yagna service](../core/serv/README.md):

```sh
cd ya-req_hybrid;
cargo run service run
```

Then generate `YAGNA_APPKEY` for requestor.

```sh
cd ya-req_hybrid;
APP_KEY=`cargo run app-key create 'requestor'`
sed -e "s/__GENERATED_APP_KEY__/$APP_KEY/" .env
```

#### Provider

```sh
mkdir ya-prov_hybrid;
cp .env-template ya-prov_hybrid/.env

sed -e "s/__YOUR_NODE_NAME_GOES_HERE__/$USER_provider/" .env
sed -e "s/__NET_TYPE__/hybrid/" .env

tee -a ya-req_hybrid/.env << END
RUST_LOG=debug,tokio_core=info,tokio_reactor=info,hyper=info,reqwest=info
RUST)bACKTRACE=1
MEAN_CYCLIC_BCAST_INTERVAL="1s"
GSB_URL=tcp://127.0.0.1:11501
YAGNA_API_URL=http://127.0.0.1:11502
YA_NET_RELAY_HOST=127.0.0.1:7464
END
```
