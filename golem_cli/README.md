# Golem

User friendly CLI for running provider.

## Under the hood

When running as a service, it runs `yagna service` and `ya-provider` as
subprocesses.

When changing settings, it calls `ya-provider`. You can still use `ya-provider`
for advanced settings and fine-tuning.

## Stopping the provider

`golemsp stop` stops `ya-provider`. When the provider was started by a running
`golemsp run` process, that supervisor observes the provider exit and then stops
its yagna child process. A yagna service started independently is left running.

Use `--graceful` to stop accepting new Agreements, notify Requestors, and give
current tasks the configured termination grace period before the provider
terminates the remaining Agreements:

```bash
golemsp stop --graceful
```

The termination notice is informational. It does not prevent either party from
terminating an Agreement immediately for any reason.

Use `--timeout` to place an additional upper bound, in seconds, on how long the
command waits before stopping `ya-provider`. On Unix, the command sends SIGTERM
at that point and allows 15 seconds for cleanup before escalating to SIGKILL:

```bash
golemsp stop --graceful --timeout 300
```

On Windows, a non-graceful stop uses `TerminateProcess`. Use `--graceful` when
running work must receive the configured termination grace period.

## Configuration difference between running without `golemsp`

| golemsp                                                                                                                | ya-provider                                              |
|------------------------------------------------------------------------------------------------------------------------|----------------------------------------------------------|
| Creates app-key named `golem-cli` and automatically passes it to Provider.                                             | Requires manual app-key setup.                           |
| Runs with directories auto cleanup options, which remove task directory after each Activity and Agreement is finished. | Keeps tasks directories.                                 |
| Overrides `EXE_UNIT_PATH` to use always `.local/lib/yagna/plugins` in home directory (system dependent).               |                                                          |
| Runs Provider on all payment networks in group testnet or mainnet.                                                     | Uses default payment network if not specified otherwise. |
| Runs `yagna payment init --receiver` for all payment networks.                                                         | Requires manual accounts initialization.                 |


## For developers

`golem` will search for `yagna` and `ya-provider` in `$PATH`.

Example for running it from `ya-prov` subdirectory:
```bash
PATH="${PWD}/../target/debug/:${PATH}" cargo run -p golem -- --help
```
