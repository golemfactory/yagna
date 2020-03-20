# Yagna Daemon & CLI

The main control module for interaction of a host node (either Requestor or Provider) with the Golem Network.

TODO: place crate dependency diagram here?

## Yagna Daemon 

### Configuration

| Setting | CLI Option | Environment variable | Default | Description |
|---------|------------|----------------------|---------|-------------|
| Data folder | -d, --datadir <data-dir> | YAGNA_DATADIR | | The folder in which the Daemon's SQL storage file is to be located | 
| Host URL | -a, --api-url <api-url> | YAGNA_API_URL | http://127.0.0.1:7465 | |
| GSB URL | -g, --gsb-url <tcp:://url> | YAGNA_BUS_PORT | 7464 | Local TCP port number, on which the Daemon's GSB is published. |
| Net Mk1 hub addr | --net-addr <host_port> | CENTRAL_NET_ADDR | 34.244.4.185:7464 | Centralized (Mk1 phase) Yagna network server address |

## Yagna CLI

### Commands

