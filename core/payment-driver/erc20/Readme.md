# Erc20 Payment driver
## Functionality
A payment driver is an abstraction over any operations relating to funds, which includes:
* Scheduling transfers to run at any point in the future.
* Verifying transfers done by other parties.
* Checking account balance.
* Reporting status of scheduled transactions and the account.

The Erc20 driver is such an abstraction built on top of the [ERC20 standard](https://ethereum.org/en/developers/docs/standards/tokens/erc-20/).

## Implementation
The core implementation is in [erc20_payment_lib](https://github.com/golemfactory/erc20_payment_lib), this crate only serves as an interface connecting
it to yagna.

## Configuration
### Via environment variables
#### Global settings
* `ERC20NEXT_SENDOUT_INTERVAL_SECS` -- The maximum interval at which transactions are batched and processed. A longer duration may conserve gas at the expense
of delivering payments at a later date.
#### Per-chain settings
In environment variables below, substitute `{CHAIN}` for the actual chain you wish to configure and `{GLM}` for the GLM symbol used on the chain.
To avoid confusion, `TGLM` is used on test chains that can mint GLM and `GLM` on non-test chains.
See `config-payments.toml` for the list of supported chains and token symbols.
* `{CHAIN}_GETH_ADDR` -- List of comma-separated RPC endpoints to be used.
* `{CHAIN}_PRIORITY_FEE` -- [priority fee](https://ethereum.org/nl/developers/docs/gas/#priority-fee).
* `{CHAIN}_MAX_FEE_PER_GAS` -- [max fee per gas](https://ethereum.org/nl/developers/docs/gas/#maxfee).
* `{CHAIN}_{SYMBOL}_CONTRACT_ADDRESS` -- Address of the GLM contract.
* `{CHAIN}_MULTI_PAYMENT_CONTRACT_ADDRESS` -- Address of a custom Golem contract allowing for executing multiple transfers at once.
* `ERC20NEXT_{CHAIN}_REQUIRED_CONFIRMATIONS` -- The number of confirmation blocks required to consider a transaction complete.

Be aware that options not prefixed with `ERC20NEXT` are also applicable to the old Erc20 driver.

### Via TOML file
* The default configuration can be seen in `config-payments.toml`.
* It can be overriden by placing a `config-payments.toml` file in yagna data directory. This is not recommended and is not guaranteed to work across versions.

## Statuses
The Erc20 driver can report a selection of statuses which indicate possible issues.
* `InsufficientGas`:
  * An account does not have sufficient gas to execute further transactions.
  * Contains: `driver`, `network`, `address`, `neededGasEst`.
* `InsufficientToken`:
  * An account does not have sufficient funds to execute further transactions.
  * Contains: `driver`, `network`, `address`, `neededTokenEst`.
* `InvalidChainId`:
  * A transaction has been scheduled on a chain that is not present in the configuration. This can only happen if `payments-config.toml` has been changed in an incorrect manner.
  * Contains: `driver`, `chainId`.
* `CantSign`:
  * The transaction cannot be signed. This means that yagna cannot access the identitiy used for this transfer, which can be caused by it being removed or locked.
  * Contains: `driver`, `network`, `address`.
* `TxStuck`:
  * A transaction cannot proceed despite being sent to the blockchain. The most likely reason is too low `max fee` setting.
  * Contains: `driver`, `network`.
* `RpcError`:
  * An RPC endpoint is unreliable. Consider using a better selection of endpoints.
  * `driver`, `network`