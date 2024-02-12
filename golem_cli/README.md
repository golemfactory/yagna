# Golem

User friendly CLI for running provider.

## Under the hood

When running as a service, it runs `yagna service` and `ya-provider` as
subprocesses.

When changing settings, it calls `ya-provider`. You can still use `ya-provider`
for advanced settings and fine-tuning.

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
