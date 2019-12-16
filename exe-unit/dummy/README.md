# ya-exe-dummy

`ya-exe-dummy` or DummyExeUnit container mock.

`ya-exe-dummy` or simply DummyExeUnit is a mock implementation of an ExeUnit - an entity
which is tasked with deploying, starting and stopping a sandboxed container used to
execute foreign, untrusted on the host.

DummyExeUnit currently supports 5 types of commands: `DEPLOY`, `START`, `RUN`, `TRANSFER`,
and `STOP`. The structure of each command is presented in the example usage below.

DummyExeUnit can be used in two ways: either executing a batch of commands encoded as JSON
from file, or interactively, using a rudimentary CLI.

## Running from JSON file input

Example JSON input which contains all supported commands has the following structure:

```json
[
  { "deploy": { "params": [] } },
  { "start": { "params": [] } },
  { "run": { "cmd": "hello" } },
  { "transfer": { "from": "dummy_src", "to": "dummy_dst" } },
  { "stop": {} }
]
```

The specified set of commands can then be executed by DummyExeUnit as follows:

```shell
cargo run --bin ya-exe-dummy -- example.json
```

## Running interactively

It is possible to converse with the DummyExeUnit interactively. This can be done by
entering an interactive CLI mode which is done automatically if no JSON file input
is specified:

```shell
cargo run --bin ya-exe-dummy
```

Then, one can specify any set of commands that will be sent to the unit in sequence,
and the response to each will be printed back to the screen:

```shell
> [ { "start": { "params": [] } } ]
received response = Ok(Ok((Active, "params={}")))
```

The CLI can be exited at any time by issuing an `exit` command:

```shell
> exit
```
