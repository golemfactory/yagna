# Yagna Service & CLI

The entrypoint for the Yagna Network.
Yagna Service serves public REST API and internal GSB API.
The same binary acts as a command line interface for the service.

## Yagna Service 

### Configuration

| Setting | CLI Option | Environment variable | Default | Description |
|---------|------------|----------------------|---------|-------------|
| Data folder | `-d, --datadir <path>` | `YAGNA_DATADIR` | platform specific (see `--help`) | The folder in which the Daemon's SQL storage file is to be located | 
| GSB URL | `-g, --gsb-url <url>` | `GSB_URL` | `tcp://127.0.0.1:7464` | Service Bus URL |
| REST API URL | `-a, --api-url <url>` | `YAGNA_API_URL` | `http://127.0.0.1:7465` | Yagna REST API endpoints base URL |
| Net Mk1 hub addr | N/A | `CENTRAL_NET_HOST` | `34.244.4.185:7464` | Centralized (Mk1 phase) Yagna network server address |

## Yagna CLI

Invoke `yagna --help` to see what is possible.

