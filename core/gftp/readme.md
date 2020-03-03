# Using gftp transfer binary

## Publishing files:

Start yagna service:
```
cargo run --bin yagna service run
```

Publish chosen file. Copy file hash from logs.
```
cargo run --bin gftp -- publish {file name}
...
Published file [LICENSE] as gftp://0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3/edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53.
...
```

## Downloading files:

```
cargo run --bin gftp -- download gftp://0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3/edb0016d9f8bafb54540da34f05a8d510de8114488f23916276bdead05509a53 -o workdir/gftp/download.txt
```

## Uploading file

Publish file for upload:

```
cargo run --bin gftp -- await-upload workdir/gftp-upload/License
...
[2020-03-03T10:17:57Z INFO  gftp] Waiting for file upload [workdir/gftp-upload/License] on url [gftp://0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3/z2IeDvgs1Q1hZ6seR0iSEsKW8kxdxQCK0eoz6DsYVznqJIl5K18NqwJPdLgesY9yR].
...
```

Upload file on provider side:
```
cargo run --release --bin gftp -- upload LICENSE gftp://0x06bf342e4d1633aac5db38817c2e938e9d6ab7f3/z2IeDvgs1Q1hZ6seR0iSEsKW8kxdxQCK0eoz6DsYVznqJIl5K18NqwJPdLgesY9yR
```
