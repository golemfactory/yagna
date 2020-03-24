# Provider Agent

## Configuration:

Provider agent can be used with .env file. Here is list of additional
environment variables that can be set:
* YAGNA_APPKEY - authorization token
* CREDIT_ADDRESS - ETH address where payments will be sent

### Command line parameters:


| Parameter      | Description   
| -------------- |------------------------------------------------|
| app-key        | Authorization token. Overrides `YAGNA_APPKEY`
| market-url     | Market api address
| activity-url   | Activity api address
| payment-url    | Payment api address
| credit-address | Ethereum account for payments. Overrides `CREDIT_ADDRESS`

## Creating token

Run yagna:
```
cargo run --bin yagna -- service run
```
Create token:
```
cargo run --bin yagna -- app-key create "provider-agent"
```

## Running

Run yagna:
```
cargo run --bin yagna -- service run
```

List keys:

```
cargo run --bin yagna -- app-key list
```

Pass key field as `authorization_token` parameter from command line
or add to .env file as `YAGNA_APPKEY`:
```
cargo run --bin ya-provider --app-key {authorization_token}
# or with .env
cargo run --bin ya-provider
```

### Running with mock requestor

Run `ya-requestor` app to mock negotiations and activity.

Note: You need to run separate yagna service with different identity,
if you want to run requestor on the same machine. The best wait is to create
separate directory with new .env file for Requestor.
```
# Get some ETH and GNT from faucet on testnet. This can last a little bit long!
cargo run --bin yagna payment init -r

# Check if you got creadit on your account:
cargo run --bin yagna payment status

# Run requestor:
cargo run --bin ya-requestor
``` 


## ExeUnits

Provider agent will load json file with ExeUnits descriptors from `exe-unit/example-exeunits.json`
that is placed in yagna repository.