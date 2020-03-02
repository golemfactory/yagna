# Using gftp transfer binary

## Publishing files:

Start yagna service:
```
cargo run --bin yagna service run
```

Publish chosen file. Copy file hash from logs.
```
cargo run --bin gftp -- publish -f {file name}
...
[2020-02-27T13:18:14Z INFO  gftp_server] Published file [LICENSE], hash [edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53]
...
```

Check your node address:
```
cargo run --bin yagna -- id list
...
┌───────────┬──────────┬─────────┬──────────────────────────────────────────────┐
│  default  │  locked  │  alias  │  address                                     │
├───────────┼──────────┼─────────┼──────────────────────────────────────────────┤
│  X        │          │         │  0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3  │
└───────────┴──────────┴─────────┴──────────────────────────────────────────────┘
```

File is available under address:
```
/net/{node-id}/gftp/{hash}
```
For example:
```
/net/0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3/gftp/edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53
```

## Downloading files:

```
cargo run --bin gftp -- download -u /net/0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3/gftp/edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53 -o workdir/gftp/download.txt
```