# ToC

 1. [Introduction](#provider-agent)
 1. [Handbook](#handbook)
    1. [ExeUnits](#exeunits)
    1. [Presets](#presets)
    1. [Running](#running-the-provider-agent)

# Provider Agent

This is a reference Yagna Provider Agent implementation.
Provider Agent is a module which effectively controls the behaviour of
the Provider Node in the Yagna Network. It includes rules and logic for:

* Offer formulation
* Demand validation and evaluation
* Agreement negotiation
* Payment regimes
* Node Resources management
* Activity workflow control
* ExeUnit instantiation and control
* Invoice/Debit Note generation

# Handbook

Please refer <https://handbook.golem.network/> for instruction how to use Provider.

### Offer formulation

It is rather straightforward and minimal:

* at most two constrains:
  * requires `golem.srv.comp.expiration` to be set
  * if provided (via env or CLI) sets also `golem.node.debug.subnet`
* properties:
* linear pricing (see sample below: 0.01 GLM/sec + 1.2 GLM/CPUsec + 1.5 GLM const)
* hardware: memory and storage (sample below: 1 gib RAM and 10 gib disk)
* node name set via env or CLI
* runtime (sample below: wasmtime)
  
Provider subscribes to the network as many Offers as presets enumerated from CLI.

#### Sample Offer

```json
      "properties": {
        "golem": {
          "com": {
            "pricing": {
              "model": "linear",
              "model.linear": {
                "coeffs": [
                  0.01,
                  1.2,
                  1.5
                ]
              }
            },
            "scheme": "payu",
            "scheme.payu": {
              "debit-note.interval-sec?": 120.0
            },
            "usage": {
              "vector": [
                "golem.usage.duration_sec",
                "golem.usage.cpu_sec"
              ]
            }
          },
          "inf": {
            "mem": {
              "gib": 1.0
            },
            "storage": {
              "gib": 10.0
            }
          },
          "node": {
            "id": {
              "name": "__YOUR_NODE_NAME_GOES_HERE__"
            }
          },
          "runtime": {
            "name": "wasmtime",
            "version": "0.1.0",
            "wasm.wasi.version@v": "0.9.0"
          }
        }
      },
      "constraints": "(golem.srv.comp.expiration>0)"
    }
```

### Market Strategy

Current implementation has two naive market strategies:

* accepting all proposals and agreements
* accepting limited number of agreements; will reject proposals and agreements above the limit

Provider Agent uses (hardcode) the second with limit of 1 agreement.
It will accept all Proposals until first agreement approval.

Upon agreement termination (in case of failure, expiration or successful finish)
Provider Agent will start accepting Proposals again until agreement confirmation; and so on.

### Activity

Provider agent allow just one activity per agreement.
On activity finish Provider Agent will initiate Agreement termination.
This is workaround because `terminate_agreement` operation is not supported yet in Market API.

### Payments

Provider agent issues Debit Notes every `scheme.payu.debit-note.interval-sec?` (120s by default).
The property is a subject of negotiations.

During negotiation Requestor and Provider both agree on `timeout` at which Debit Notes are accepted by
Requestor. A property responsible for this is named `golem.com.payment.debit-notes.accept-timeout?`.
Provider starts negotiations with [4min](src/market/negotiator/factory.rs#L27) and it might be only
lower ie. Requestor might propose lower value, which Provider will accept as long as it is more than
`5sec`. Base value for `timeout` negotiations is controlled via [CLI](src/market/negotiator/factory.rs#L27)
and ENV `DEBIT_NOTE_ACCEPTANCE_DEADLINE`. Provider is then entitled to break the Agreement after
negotiated `timeout` elapses and Debit Note is **not** accepted.

What's more, Provider is entitled to break the Agreement, when there is no Activity
for [90s](src/tasks/config.rs#L7) (ie. idle Agreement).

Provider issues Invoice **only once**, after the Agreement is terminated.

## Configuration

Provider agent can be used with `.env` file. [Here](https://github.com/golemfactory/yagna/wiki/DotEnv-Configuration)
is list of additional environment variables that can be set.

Create separate working dir for the Provider Agent (please create `ya-prov` in the main yagna source code directory),
and create `.env` file there by copying
[`.env-template`](https://github.com/golemfactory/yagna/blob/master/.env-template) from yagna repo main directory:

```bash
mkdir ya-prov && cd ya-prov && cp ../.env-template .env
```

and change `NODE_NAME` there.

### Command line parameters

This can be displayed using `cargo run -p ya-provider -- run --help`

| Parameter      | Description | Env var |
| -------------- | ----------- | ------- |
| app-key        | Authorization token. |`YAGNA_APPKEY`|
| market-url     | Market api address. |`YAGNA_MARKET_URL`|
| activity-url   | Activity api address. |`YAGNA_ACTIVITY_URL`|
| payment-url    | Payment api address. |`YAGNA_PAYMENT_URL`|
| data-dir       | Path to a directory where configuration files are stored. |`DATA_DIR`|
| node-name      | Node name to use in agreements. |`NODE_NAME`|
| subnet         | You can set this value to filter nodes with other identifiers than selected. Useful for test purposes. |`SUBNET`|
| exe-unit-path  | Path to JSON descriptor file for ExeUnits. |`EXE_UNIT_PATH`|

### Creating app-key authentication token

To obtain `YAGNA_APPKEY` we need to be in this newly created workdir `cd ya-prov`:

1. Run [yagna service](https://github.com/golemfactory/yagna/blob/master/core/serv/README.md):

    ```bash
    cargo run service run
    ```
    If you want to set `debug` log level or higher its good to filter out core crates to `info`:

    ```bash
    RUST_LOG=debug,tokio_core=info,tokio_reactor=info,hyper=info,web3=info cargo run service run
    ```

2. Create app-key token

    In another console, go to the same directory and run:\
    (it will change your `.env` file with newly created app-key)

    ```bash
    APP_KEY=`cargo run app-key create 'provider-agent'`
    sed -e "s/__GENERATED_APP_KEY__/$APP_KEY/" -i.bckp .env
    ```

## ExeUnits

 1. [WASI](#wasi-wasmtime)
 1. [Runtime SDK](https://github.com/golemfactory/ya-runtime-sdk#deploying)
 1. [VM](#vm-docker)

### WASI (wasmtime)

This is the first ExeUnit we've prepared for you.
You need to clone its repository and build.
In following sections we assume you've cloned it to the same directory where `yagna` is cloned.

```
cd ../..  # assuming you are in ./yagna/ya-prov
git clone git@github.com:golemfactory/ya-runtime-wasi.git
cd ya-runtime-wasi
cargo build
cd ../yagna/ya-prov
```

You also need to build ExeUnit supervisor.

```bash
cargo build -p ya-exe-unit
```

You can list available ExeUnits with command:

```bash
$ cargo run -p ya-provider -- exe-unit list

Available ExeUnits:

Name:          wasmtime
Version:       0.1.0
Supervisor:    /Users/tworec/git/yagna/target/debug/exe-unit
Runtime:       /Users/tworec/git/ya-runtime-wasi/target/debug/ya-runtime-wasi
Description:   This is just a sample descriptor for wasmtime exeunit used by ya-provider
Properties:
    wasm.wasi.version@v           "0.9.0"
```

### VM (docker)

Please refer to [vm repo documentation](https://github.com/golemfactory/ya-runtime-vm).
Afterwards you'll need to update your `exeunits-descriptor.json` (defined as `EXE_UNIT_PATH`
in `.env` or os env).

Sample descriptor entry:

```json
  {
    "name": "vm",
    "version": "0.2.0",
    "supervisor-path": "exe-unit",
        "runtime-path": "/home/user/.local/lib/yagna/plugins/ya-runtime-vm/ya-runtime-vm",
    "description": "vm runtime",
    "extra-args": ["--cap-handoff"]
  }
```

## Presets

Provider uses presets to create market offers. On the first run, the Provider Agent will create
a `default` preset. Presets are saved in a `presets.json` file, located in application's data directory.

You can list presets by running command:
`cargo run -p ya-provider -- preset list`

The result will be something like this:

```
Available Presets:

Name:               default
ExeUnit:            wasmtime
Pricing model:      linear
Coefficients:
    Duration        0.1 GLM
    CPU             0.2 GLM
    Init price        1 GLM

```

Coefficients describe unit price of ExeUnit metrics:

* Duration - `golem.usage.duration_sec`
* CPU - `golem.usage.cpu_sec`
* Init price - constant price per created activity

In order to publish an offer based on a preset, that preset needs to be activated first.

### Active presets

To list all active presets, type:

```bash
cargo run -p ya-provider -- preset active
```

### Creating presets

You can create preset in the interactive mode:

```bash
cargo run -p ya-provider -- preset create
```

...and non-interactively also:

```bash
cargo run -p ya-provider -- preset create \
    --no-interactive \
    --preset-name new-preset \
    --exe-unit wasmtime \
    --pricing linear \
    --price Duration=1.2 CPU=3.4 "Init price"=0.2
```

If you don't specify any of price values, it will be defaulted to `0.0`.  

### Updating presets

Note: updating a preset will cancel (unsubscribe) all related offer subscriptions.

Updating in interactive mode:

```bash
cargo run -p ya-provider -- preset update --name new-preset
```

or using command line parameters:

```bash
cargo run -p ya-provider -- preset update --name new-preset \
    --no-interactive \
    --exe-unit wasmtime \
    --pricing linear \
    --price Duration=1.3 CPU=3.5 "Init price"=0.3
```

You can omit some parameters and the will be filled with previous values.

### Removing presets

Note: removing a preset will cancel (unsubscribe) all related offer subscriptions.

```bash
cargo run -p ya-provider -- preset remove new-preset
```

### Activating and deactivating presets

When you activate a preset, a new offer will be published (subscribed) to the marketplace.

```bash
cargo run -p ya-provider -- preset activate new-preset
```

Note: deactivating a preset will cancel all related offer subscriptions.

```bash
cargo run -p ya-provider -- preset deactivate new-preset
```

### Listing metrics

You can list available metrics with command:

```bash
$ cargo run -p ya-provider -- preset list-metrics

Duration       golem.usage.duration_sec
CPU            golem.usage.cpu_sec
```

Left column is name of preset that should be used in commands. On the right side
you can see agreement property, that will be set in usage vector.

## Hardware profiles

Hardware profiles control the maximum amount of hardware resources assigned to computations.
Provider Agent **does not allow you to allocate all of your system resources**. The remaining
logical CPU cores, memory and storage will be utilized by the background applications,
the operating system and Provider Agent itself.

Note: updating or activating another hardware profile will cancel (unsubscribe) all current offer subscriptions.

The available sub-commands for `profile` are:

```
list        List available profiles
active      Show the name of an active profile
create      Create a new profile
update      Update a profile
remove      Remove an existing profile
activate    Activate a profile
```

### Listing profiles

```bash
cargo run -p ya-provider -- profile list
```

will print an output similar to:

```
{
  "default": {
    "cpu_threads": 3,
    "mem_gib": 10.9375,
    "storage_gib": 73.57168884277344
  }
}
```

### Display the active profile

```bash
cargo run -p ya-provider -- profile active
```

will print:

```
"default"
```

### Creating a profile

Usage:

```bash
cargo run -p ya-provider -- profile create \
    <name> \
    --cpu-threads <cpu-threads> \
    --mem-gib <mem-gib> \
    --storage-gib <storage-gib>
```

E.g.:

```bash
cargo run -p ya-provider -- profile create half --cpu-threads 2  --mem-gib 8. --storage-gib 256.
```

### Updating a profile

Note: updating a profile will cancel all current offer subscriptions.

Usage is similar to profile creation.

E.g.:

```bash
cargo run -p ya-provider -- profile update --name half --cpu-threads 3  --mem-gib 5. --storage-gib 128.
```

### Removing a profile

Note: removing an active profile will cancel all current offer subscriptions.

E.g.:

```bash
cargo run -p ya-provider -- profile remove half
```

### Activating a profile

Note: activating a different profile will cancel all current offer subscriptions.

E.g.:

```bash
cargo run -p ya-provider -- profile activate some_other_profile
```

## Running the Provider Agent

While the yagna service is still running (and you are in the `ya-prov` directory)
you can now start Provider Agent.

```bash
cargo run -p ya-provider -- run
```

## Central setup

We have centrally deployed (@ yacn2.dev.golem.network) three independent standalone modules/apps:

* [net Mk1](https://github.com/golemfactory/yagna/blob/master/docs/net-api/net-mk1-hub.md) @ yacn2.dev.golem.network:7464 \
   (can be run locally with `cargo run --release -p ya-sb-router --example ya_sb_router -- -l tcp://0.0.0.0:7464`
   from the [ya-service-bus](http://github.com/golemfactory/ya-service-bus) repository)
* simple "images store" @ yacn2.dev.golem.network:8000 \
   this is a http server that has two purposes: to serve binary `.zip`/`.yimg` packages (GET) and receive computation results (PUT)
   (can be run locally with `cargo run --release -p ya-exe-unit --example http-get-put -- --root-dir <DIR-WITH-WASM-BINARY-IMAGES>`)
* ya-zksync-faucet @ yacn2.dev.golem.network:5778
    eg. `curl http://yacn2.dev.golem.network:5778/zk/donatex/0xf63579d46eedee31d9db380a38addd58fdf414fd`
