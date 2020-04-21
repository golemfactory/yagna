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

This can be displayed using `cargo run --bin ya-provider run --help`

| Parameter      | Description   
| -------------- |------------------------------------------------|
| app-key        | Authorization token. Overrides `YAGNA_APPKEY`
| market-url     | Market api address. Overrides `YAGNA_MARKET_URL`
| activity-url   | Activity api address. Overrides `YAGNA_ACTIVITY_URL`
| payment-url    | Payment api address. Overrides `YAGNA_PAYMENT_URL`
| exe-unit-path  | Path to JSON descriptor file for ExeUnits. Overrides `EXE_UNIT_PATH`
| node-name      | Node name to use in agreements.
| subnet         | You can set this value to filter nodes with other identifiers than selected. Useful for test purposes.
| credit-address | Ethereum account for payments (should match NodeId). Overrides `CREDIT_ADDRESS`(it will be removed in next release).

### Creating app-key authentication token

To obtain `YAGNA_APPKEY` we need to be in this newly created workdir `cd ya-prov`:

1. Run [yagna service](https://github.com/golemfactory/yagna/blob/master/core/serv/README.md):
    ```
    cargo run --bin yagna -- service run
    ```
    If you want to set `debug` log level or higher its good to filter out core crates to `info`:
    ```
    RUST_LOG=debug,tokio_core=info,tokio_reactor=info,hyper=info cargo run --bin yagna -- service run
    ```

2. Create app-key token

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

## Presets

Provider uses presets to create market offers. In current version presets are
defined in `presets.json` file, that should be placed in working directory.
You can copy example presets from `agent/provider/examples/presets.json`.

You can list presets by running command:
`cargo run --bin ya-provider preset list`

The result will be something like this:
```
Available Presets:

Name:               wasm-preset
ExeUnit:            wasmtime
Pricing model:      linear
Coefficients:
    Duration        1.4 GNT
    CPU             3.5 GNT
    Init price      0.3 GNT

Name:               amazing-offer
ExeUnit:            wasmtime
Pricing model:      linear
Coefficients:
    Duration        0.1 GNT
    CPU             0.2 GNT
    Init price      1 GNT

Name:               lame-offer
ExeUnit:            wasmtime
Pricing model:      linear
Coefficients:
    Duration        0 GNT
    CPU             0 GNT
    Init price      0 GNT

Name:               high-cpu
ExeUnit:            wasmtime
Pricing model:      linear
Coefficients:
    Duration        0.01 GNT
    CPU             1.2 GNT
    Init price      1.5 GNT
```

Coefficients describe unit price of ExeUnit metrics:

* [1] `golem.usage.duration_sec`
* [2] `golem.usage.cpu_sec`
* [3] constant price per created activity 

When running provider, you must list all presets, that you want to use.

### Creating presets

You can create preset in interactive mode:

`cargo run --bin ya-provider preset create`

or set all parameters non interactively:

`cargo run --bin ya-provider preset create --nointeractive --preset-name wasm-offer --exeunit wasmtime --pricing linear --price Duration=1.2 --price CPU=3.4 "Init price"=0.2`

If you don't specify any of price values, it will be defaulted to 0.0.  


### Updating presets

Updating in interactive mode:

`cargo run --bin ya-provider preset update new-preset`

or using command line parameters:

`cargo run --bin ya-provider preset update wasm-preset --nointeractive --exeunit wasmtime --pricing linear --price Duration=1.3 --price CPU=3.5 "Init price"=0.3`

You can omit some parameters and the will be filled with previous values.

### Removing presets

`cargo run --bin ya-provider preset remove new-preset`

### Listing metrics

You can list available metrics with command:

`cargo run --bin ya-provider preset list-metrics`

You will get something like this:

```
Duration       golem.usage.duration_sec
CPU            golem.usage.cpu_sec
```
Left column is name of preset that should be used in commands. On the right side
you can see agreement property, that will be set in usage vector.

## Running the Provider Agent

Make sure you have compiled latest changes to exe-unit binaries:

`cargo build --release --bin exe-unit --bin wasmtime-exeunit`

While the yagna service is still running (and you are in the `ya-prov` directory) you can now start Provider Agent.
You must enumerate all presets, you want Provider Agent to publish as Offers on the Market:

`cargo run --release --bin ya-provider -- --exe-unit-path ../exe-unit/resources/local-exeunits-descriptor.json run high-cpu amazing-offer`

## Mock requestor

Run `ya-requestor` app to mock negotiations, activity and payments.

### Configure requestor

You need to run a separate yagna service with a different identity,
if you want to run requestor on the same machine. The best way is to create
a separate directory (e.g. `ya-req` in the main yagna
source directory) with a new `.env` copied from `.env-template`. 
In this `.env` file you must change port numbers not to interfere with provider: 
 * `GSB_URL` to e.g. 7474
 * `YAGNA_API_URL` to e.g. 7475

### Run yagna service:
```
cargo run --bin yagna -- service run
```

3. Create app-key in a new console:
```
cargo run --bin yagna -- app-key create "requestor-agent"
```

4. Set the result as `YAGNA_APPKEY` value in your `.env` file.

6. Get some ETH and GNT from faucet on testnet (rinkeby).
This can last a little bit long! Retry if not succeed at first.
```
cargo run --bin yagna payment init -r
```

7. Check if you got credit on your account:
```
cargo run --bin yagna payment status
```
Or go to the Rinkeby's etherscan: https://rinkeby.etherscan.io/address/0xdeadbeef00000000000000000000000000000000
(Replace the address with the generated node id for the requestor agent -- a result of `cargo run --bin yagna id show`)

8. Run requestor (commands.json contains commands to be executed on the provider):

```
cargo run --bin ya-requestor -- --exe-script ../exe-unit/examples/commands.json
```

## ExeUnits

You can list available ExeUnits with command:

`cargo run --bin ya-provider exe-unit list`
Result:
```
Available ExeUnits:

Name:          wasmtime
Version:       0.1.0
Supervisor:    /home/nieznanysprawiciel/Repos/Golem/yagna/target/debug/exe-unit
Runtime:       /home/nieznanysprawiciel/Repos/Golem/yagna/target/debug/wasmtime-exeunit
Description:   This is just a sample descriptor for wasmtime exeunit used by ya-provider
Properties:
    wasm.wasi.version@v           "0.9.0"
```