# Provider Agent

## Central setup
We have centrally deployed (ip: `34.244.4.185`) three independent standalone modules/apps:
 - [net Mk1](https://github.com/golemfactory/yagna/blob/master/docs/net-api/net-mk1-hub.md) @ 34.244.4.185:7464 \
   (can be invoked locally with `cargo run --release --example ya_sb_router`)
 - [market Mk0](https://github.com/golemfactory/yagna/blob/master/docs/market-api/market-api-mk0-central-exchange.md) @ http://34.244.4.185:8080/market-api/v1/ \
   (can be invoked locally with `dotnet run --urls "http://0.0.0.0:5001" -p GolemClientMockAPI` 
 - simple "wasm store" \
   this is a http server that has two purposes: to serve binary `.zip`/`.yimg` packages (GET) and receive computation results (PUT)
   (can be invoked locally with `cargo run --release --example http-get-put --root-dir <DIR-WITH-WASM-BINARY-IMAGES>`
   TODO: describe how to build and pack yagna wasm binary image

## Configuration:

Provider agent can be used with `.env` file. [Here](https://github.com/golemfactory/yagna/wiki/DotEnv-Configuration) is list of additional environment variables that can be set.

Best practice is to create separate working dir for the Provider Agent (say `~/ya-prov`), and create there `.env` file by copying
[`.env-template`](https://github.com/golemfactory/yagna/blob/master/.env-template) from yagna repo main directory.

### Command line parameters:

This can be displayed using `--help`

| Parameter      | Description   
| -------------- |------------------------------------------------|
| app-key        | Authorization token. Overrides `YAGNA_APPKEY`
| market-url     | Market api address. Overrides `YAGNA_MARKET_URL`
| activity-url   | Activity api address. Overrides `YAGNA_ACTIVITY_URL`
| payment-url    | Payment api address. Overrides `YAGNA_PAYMENT_URL`
| credit-address | Ethereum account for payments. Overrides `CREDIT_ADDRESS`

### Creating app-key authentication token

To use Provider Agent we nedd to provide afromentioned `YAGNA_APPKEY`.
To obtain it we need be in this newly created workdir `cd ~/ya-prov` :

1. Run [yagna daemon](https://github.com/golemfactory/yagna/blob/master/core/serv/README.md):
```
cargo run --bin yagna -- service run
```
or optionaly with RUST_LOG tweaks:
```
RUST_LOG=debug,tokio_core=info,hyper=info,tokio_reactor=info cargo run --bin yagna -- service run
```

2. Create token:

In another console, go to the same directory and
```
cargo run --bin yagna -- app-key create "provider-agent"
```
it will display newly created app-key eg.
```
$ cargo run --bin yagna -- app-key create "provider-agent"
    Finished dev [unoptimized + debuginfo] target(s) in 0.35s
     Running `/Users/tworec/git/yagna/target/debug/yagna app-key create provider-agent`
58cffa9aa1e74811b223b627c7f87aac
```

3. put this app-key into your `.env` file as a value for variable `YAGNA_APPKEY`.


## Running the Provider Agent

While the yagna daemon is still running, and you are in in `~/ya-prov` you can now start Provider Agent 

`cargo run --bin ya-provider`

`RUST_LOG=debug cargo run --bin ya-provider -- --exe-unit-path ../exe-unit/resources/local-exeunits-descriptor.json`


### Running with mock requestor

Run `ya-requestor` app to mock negotiations and activity.

Note: You need to run separate yagna service with different identity,
if you want to run requestor on the same machine. The best wait is to create
separate directory with new .env file for Requestor.
You must change gsb port `GSB_URL=tcp://127.0.0.1:7766` in your `.env` for requestor.
```
# Get some ETH and GNT from faucet on testnet. This can last a little bit long!
cargo run --bin yagna payment init -r

# Check if you got creadit on your account:
cargo run --bin yagna payment status

# Run requestor:
cargo run --bin ya-requestor
``` 


## ExeUnits

Provider agent will load json file with ExeUnits descriptors from `exe-unit/example-exeunits.json`
that is placed in yagna repository.
