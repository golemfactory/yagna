# Command progress reporting

ExeUnit behaves according to specification defined [here](https://golemfactory.github.io/golem-architecture/specs/command-progress.html)
and support progress reporting for commands: `deploy` and `transfer`.

This document aims to describe implementation details not covered by specification.

## Specification


| Name                        | Description          |
|-----------------------------|----------------------|
| Minimum ExeUnit version     | {TODO}               |
| Minimum Runtime API version | Always compatible    |
| Minimum yagna version       | {TODO}               |
| Minimum provider version    | Always compatible    |
| Supported commands          | `deploy`, `transfer` |


## [ProgressArgs](https://golemfactory.github.io/ya-client/index.html?urls.primaryName=Activity%20API#/model-ProgressArgs)

ExeUnit supports only `update-interval`. If value is not set, `1s` default will be used.

`update-step` is not implemented.

## [Runtime event](https://golemfactory.github.io/ya-client/index.html?urls.primaryName=Activity%20API#model-RuntimeEventKindProgress)

### Steps

`Deploy` and `transfer` command consist of only single step.

### Progress

- Progress is reported as `Bytes`. Fields is never a `None`.
- Size of file is always checked and put as second element of tuple.
- Initially `Size` element of tuple is set to `None` and if progress with `message` field is sent
  than it can be received by Requestor agent

### Message

Two messages are currently possible:
- `Deployed image from cache`
- `Retry in {}s because of error: {err}` - indicates error during transfer, which will result in retry.

When sending message, the rest of `CommandProgress` structure fields will be set to latest values.

## Requestor Example

PoC implementation using yapapi: https://github.com/golemfactory/yapapi/pull/1153
