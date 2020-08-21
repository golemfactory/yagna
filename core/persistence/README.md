# Yagna persistence db schema

This module is an implementation of Yagna Daemon persistence layer,
required to record and maintain the aspects of component services and their APIs.

It is based on SQLite3 and Diesel libraries.

## Diesel

To create or change schema in the component service pls go to its crate directory
and follow [Diesel tutorial](https://diesel.rs/guides/getting-started/).

You need to install Diesel CLI with SQLite:
```
cargo install diesel_cli --no-default-features --features sqlite
```
