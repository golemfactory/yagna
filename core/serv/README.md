# Yagna Daemon & CLI

The main control module for interaction of a host node (either Requestor or Provider) with the Golem Network.

TODO: place crate dependency diagram here?

## Yagna Daemon 

### Configuration

| Setting | CLI Option | Environment variable | Default | Description |
|---------|------------|----------------------|---------|-------------|
| Host URL | -a, --address <address> (not implemented) | YAGNA_HOST | 127.0.0.1 | |
| GSB port | --router-port <router-port>  (not implemented) | YAGNA_BUS_PORT | 7464 | Local TCP port number, on which the Daemon's GSB is published. |
| REST API port | -p, --http-port <http-port>  (not implemented) | YAGNA_HTTP_PORT | 7465 | TCP port on which the APIs are published. |
| Data folder | -d, --datadir <data-dir> | (n/a) | | The folder in which the Daemon's SQL storage file is to be located | 
| Net Mk1 hub URL | (n/a) | CENTRAL_NET_HOST | 10.30.10.202:7477 | The URL to the implementation of Net Mk1 central routing hub |

## Yagna CLI

### Commands

