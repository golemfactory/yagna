# Provider Agent

## Central setup
We have centrally deployed (ip: `34.244.4.185`) three independent standalone modules/apps:
 - [net Mk1](https://github.com/golemfactory/yagna/blob/master/docs/net-api/net-mk1-hub.md) @ 34.244.4.185:7464 \
   (can be invoked locally with `cargo run --release --example ya_sb_router`)
 - [market Mk0](https://github.com/golemfactory/yagna/blob/master/docs/market-api/market-api-mk0-central-exchange.md) @ http://34.244.4.185:8080/market-api/v1/ \
   (can be invoked locally with `dotnet run --urls "http://0.0.0.0:5001" -p GolemClientMockAPI`)
 - simple "wasm store" @ 34.244.4.185:8000 \
   this is a http server that has two purposes: to serve binary `.zip`/`.yimg` packages (GET) and receive computation results (PUT)
   (can be invoked locally with `cargo run --release --example http-get-put --root-dir <DIR-WITH-WASM-BINARY-IMAGES>`)
   TODO: describe how to build and pack yagna wasm binary image

## Configuration

Provider agent can be used with `.env` file. [Here](https://github.com/golemfactory/yagna/wiki/DotEnv-Configuration) is list of additional environment variables that can be set.

Create separate working dir for the Provider Agent (please create `ya-prov` in the main yagna source code directory), and create `.env` file there by copying
[`.env-template`](https://github.com/golemfactory/yagna/blob/master/.env-template) from yagna repo main directory.

### Command line parameters

This can be displayed using `--help`

| Parameter      | Description   
| -------------- |------------------------------------------------|
| app-key        | Authorization token. Overrides `YAGNA_APPKEY`
| market-url     | Market api address. Overrides `YAGNA_MARKET_URL`
| activity-url   | Activity api address. Overrides `YAGNA_ACTIVITY_URL`
| payment-url    | Payment api address. Overrides `YAGNA_PAYMENT_URL`
| credit-address | Ethereum account for payments (should match NodeId). Overrides `CREDIT_ADDRESS`
| exe-unit-path  | Path to JSON descriptor file for ExeUnits. Overrides `EXE_UNIT_PATH`

### Creating app-key authentication token

To use Provider Agent we nedd to provide afromentioned `YAGNA_APPKEY`.
To obtain it we need to be in this newly created workdir `cd ya-prov`:

1. Run [yagna daemon](https://github.com/golemfactory/yagna/blob/master/core/serv/README.md):
```
cargo run --bin yagna -- service run
```
When enabling `debug` log level or higher its good to filter out core crates to `info`:
```
RUST_LOG=debug,tokio_core=info,tokio_reactor=info,hyper=info cargo run --bin yagna -- service run
```

2. Create token:

In another console, go to the same directory and run:
```
cargo run --bin yagna -- app-key create "provider-agent"
```
it will display newly created app-key eg.
```
$ cargo run --bin yagna -- app-key create "provider-agent"
58cffa9aa1e74811b223b627c7f87aac
```

3. Put this app-key into your `.env` file as a value for variable `YAGNA_APPKEY`.

4. Restart yagna daemon (Ctrl+C, then repeat step 1)

## Running the Provider Agent

Make sure you have compiled latest changes to exe-init binaries:

`cargo build --release --bin exe-unit --bin wasmtime-exeunit`

While the yagna daemon is still running (and you are in the `ya-prov` directory) you can now start Provider Agent:

`cargo run --release --bin ya-provider -- --exe-unit-path ../exe-unit/resources/local-exeunits-descriptor.json`


## Mock requestor

Run `ya-requestor` app to mock negotiations and activity.

1. Configure requestor

You need to run a separate yagna service with a different identity,
if you want to run requestor on the same machine. The best way is to create
a separate directory (please use `ya-requestor` in the main yagna
source directory) with a new `.env` copied from .env-template. You must change port
numbers in `YAGNA_API_URL`, and `YAGNA_ACTIVITY_URL` to e.g. 7768,
and the port number in `GSB_URL` to e.g. 7465 in your new `.env` file.

2. Run yagna daemon:
```
cargo run --bin yagna -- service run
```

3. Create app-key in a new console:
```
cargo run --bin yagna -- app-key create "requestor-agent"
```

4. Set the result of `app-key create` as YAGNA_APPKEY in your .env file.

5. Stop yagna daemon and run it again.

6. Get some ETH and GNT from faucet on testnet.
This can last a little bit long! If it doesn't work, try again.
```
cargo run --bin yagna payment init -r
```

7. Check if you got credit on your account:
```
cargo run --bin yagna payment status
```

8. Run requestor (commands.json contains commands to be executed on the provider):

```
cargo run --bin ya-requestor -- --exe-script ../exe-unit/examples/commands.json
```
