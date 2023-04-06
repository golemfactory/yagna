# Golem

User friendly CLI for running provider.

## Under the hood

When running as a service, it runs `yagna service` and `ya-provider` as
subprocesses.

When changing settings, it calls `ya-provider`. You can still use `ya-provider`
for advanced settings and fine-tuning.

## For developers

`golem` will search for `yagna` and `ya-provider` in `$PATH`.

Example for running it from `ya-prov` subdirectory:
```bash
PATH="${PWD}/../target/debug/:${PATH}" cargo run -p golem -- --help
```
