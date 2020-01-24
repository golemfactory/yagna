# Provider Agent

## Creating token

Run yagna:
```
cargo run --bin yagna -- service run
```
Create token:
```
cargo run --bin yagna -- app-key create "provider-agent"
```

## Running

Run yagna:
```
cargo run --bin yagna -- service run
```

Run market api test bed from repository: https://github.com/stranger80/golem-client-mock

```
dotnet build
dotnet publish
./start_api.sh
```

Send authorization key to market api test bed:
```
./app-key-token send-keys
```

List keys:

```
cargo run --bin yagna -- app-key list
```

Copy key field as `authorization_token` parameter:
```
RUST_LOG=info cargo run --bin ya-provider {authorization_token}
```

You can specify which market and activity api hosts to connect to:
```
RUST_LOG=info cargo run --bin ya-provider {authorization_token} --market-host 127.0.0.1:5001 --activity-host 127.0.0.1:7465
```