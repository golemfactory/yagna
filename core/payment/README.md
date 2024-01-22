## Payment Service

This crate is a service to be loaded in Yagna to handle payment scenario's.
The payment service is the main service `yagna` will be talking to, but not directly handling the payments.
The payments are made by drivers loaded in the service.

### Drivers

Currently these drivers are available to use:
- Erc20
- Erc20
- Dummy

By default only the Erc20 & Erc20 drivers are enabled, extra drivers need to be specifically loaded with a feature flag.

## DO NOT USE DUMMY DRIVER FOR BUILDS THAT WILL BE DISTRIBUTED!!!

You can enable multiple drivers at the same time, use this table for the required feature flags and platform parameters:

| Driver name | Feature flag   | Public explorer                                                                            | Local | Testnet | Mainnet |
| ----------- | -------------- | ------------------------------------------------------------------------------------------ | ----- | ------- | ------- |
| erc20       | `erc20-driver` | [etherscan](https://rinkeby.etherscan.io/token/0xd94e3dc39d4cad1dad634e7eb585a57a19dc7efe) | x     | x       |         |
| erc20       | `erc20-driver` | [etherscan](https://rinkeby.etherscan.io/token/0xd94e3dc39d4cad1dad634e7eb585a57a19dc7efe) | x     | x       |         |
| dummy       | `dummy-driver` | None                                                                                       | x     |         |         |

### Examples:

Build with erc20 and erc20 drivers:
```
cargo build --release
```