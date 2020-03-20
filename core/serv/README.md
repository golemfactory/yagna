# Yagna Daemon & CLI

The main control module for interaction of a host node (either Requestor or Provider) with the Golem Network.

TODO: place crate dependency diagram here?

## Yagna Daemon 

### Configuration

| Setting | CLI Option | Environment variable | Default | Description |
|---------|------------|----------------------|---------|-------------|
| Data folder | `-d, --datadir <path>` | `YAGNA_DATADIR` | platform specific (see `--help`) | The folder in which the Daemon's SQL storage file is to be located | 
| REST API URL | `-a, --api-url <url>` | `YAGNA_API_URL` | `http://127.0.0.1:7465` | Yagna REST API endpoints base URL |
| GSB URL | `-g, --gsb-url <url>` | `GSB_URL` | `tcp://127.0.0.1:7464` | Service Bus URL |
| Net Mk1 hub addr | N/A | `CENTRAL_NET_ADDR` | `34.244.4.185:7464` | Centralized (Mk1 phase) Yagna network server address |

## Yagna CLI

### Commands

