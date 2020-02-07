# ya-exe-dummy

`ya-exe-dummy` or DummyExeUnit container mock.

`ya-exe-dummy` or simply DummyExeUnit is a mock implementation of an ExeUnit - an entity
which is tasked with deploying, starting and stopping a sandboxed container used to
execute foreign, untrusted on the host.

DummyExeUnit currently supports 5 types of commands: `DEPLOY`, `START`, `RUN`, `TRANSFER`,
and `STOP`. The structure of each command is presented in the example usage below.

DummyExeUnit can be used in three ways:
1. running interactively using a rudimentary CLI,
2. executing a batch of commands from JSON file, and
3. binding to Golem Service Bus at a specified Service ID

## Running interactively

It is possible to converse with the DummyExeUnit interactively. This can be done by
entering an interactive CLI mode which is done automatically if no JSON file input
is specified:

```shell
cargo run --bin ya-exe-dummy -- cli
```

Then, one can specify any set of commands that will be sent to the unit in sequence,
and the response to each will be printed back to the screen:

```shell
> [ { "start": { "args": [] } } ]
received response = Ok(Ok((Active, "args={}")))
```

The CLI can be exited at any time by issuing an `exit` command:

```shell
> exit
```

## Executing a batch of commands from JSON file

Example JSON input which contains all supported commands has the following structure:

```json
[
  { "deploy": {} },
  { "start": { "args": [] } },
  { "run": { "entry_point": "", "args": [] } },
  { "transfer": { "from": "dummy_src", "to": "dummy_dst" } },
  { "stop": {} }
]
```

The specified set of commands can then be executed by DummyExeUnit as follows:

```shell
cargo run --bin ya-exe-dummy -- from-file example.json
```

## Binding to Golem Service Bus at a specified Service ID

Assuming you have GSB Router up and running, you can hook up to the GSB with
the following command:

```shell
cargo run --bin ya-exe-dummy -- gsb /local/dummy
```

### Testing over GSB

For testing, you can firstly launch the GSB Router like so:

```shell
cargo run --example ya-sb-router
```

Then, you can bind the DummyExeUnit to the GSB like so:

```shell
cargo run --bin ya-exe-dummy -- gsb /local/exeunit
```

You can send example commands like so:

```shell
cargo run --example test_actix_service -- client service-bus/bus/examples/data/script.json
```
