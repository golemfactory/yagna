## Payment Service

This crate is a service to be loaded in Yagna to handle payment scenario's.
The payment service is the main service `yagna` will be talking to, but not directly handling the payments.
The payments are made by drivers loaded in the service.

### Drivers

Currently these drivers are available to use:
- Erc20
- Dummy
- ZkSync

By default the Erc20 and ZkSync drivers are selected, extra drivers need to be specifically loaded with a feature flag.

## DO NOT USE DUMMY DRIVER FOR BUILDS THAT WILL BE DISTRIBUTED!!!

You can enable multiple drivers at the same time, use this table for the required feature flags and platform parameters:

|Driver name|Feature flag|Public explorer|Local|Testnet|Mainnet|
|-|-|-|-|-|-|
|zksync|`zksync-driver`|[zkscan](https://rinkeby.zkscan.io/)|x|x||
|erc20|`erc20-driver`|[etherscan](https://rinkeby.etherscan.io/token/0xd94e3dc39d4cad1dad634e7eb585a57a19dc7efe)|x|x||
|dummy|`dummy-driver`|None|x|||

### Examples:

Build with zksync + erc20 driver:
```
cargo build --release
```

Build with only zksync ( not recommended )
```
cargo build --release --no-default-features --features zksync-driver
```
