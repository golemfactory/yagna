## Current ERC20 transactions flow 
Date: 2021-10-22

Disclaimer: This is dev documentation not officially maintained, it is intended for internal development.

Testing transfers:
This command will send 0.0001 GLMs from internal wallet to address 0x89Ef977db64A2597bA57E3eb4b717D3bAAeBaeC3 (use your own address for testing)
Note that service have to be running otherwise you get no connection error.

```
yagna.exe payment transfer --amount 0.0001 --driver erc20 --network mumbai --to-address 0x89Ef977db64A2597bA57E3eb4b717D3bAAeBaeC3
```

You can specify extra options 
* --gas-price (starting gas price in gwei)
* --max-gas-price (maximum allowed gas price in gwei)
* --gas-limit (limit of gas used in transaction). Better to leave default as it is not affecting cost of transaction. This is convenient for testing errors on blockchain.

```
yagna.exe payment transfer --amount 0.0001 --gas-price 1.1 --max-gas-price 60.4 --gas-limit 80000 --driver erc20 --network mumbai --to-address 0x89Ef977db64A2597bA57E3eb4b717D3bAAeBaeC3
```

Networks currently supported:
* mainnnet (ETH mainnet, do not use)
* rinkeby (ETH testnet, good support)
* goerli (ETH testnet)
* mumbai (Polygon testnet)
* polygon (Polygon mainnet)

## Implementation

DB fields explained

```sql
    tx_id TEXT NOT NULL PRIMARY KEY,
    sender TEXT NOT NULL,
    nonce INTEGER NOT NULL DEFAULT -1,
    status INTEGER NOT NULL,
    tx_type INTEGER NOT NULL,
    tmp_onchain_txs TEXT NULL,
    final_tx TEXT NULL,
    starting_gas_price DOUBLE NULL,
    current_gas_price DOUBLE NULL,
    max_gas_price DOUBLE NULL,
    final_gas_price DOUBLE NULL,
    final_gas_used INTEGER NULL,
    gas_limit INTEGER NULL,
    time_created DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    time_last_action DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    time_sent DATETIME NULL,
    time_confirmed DATETIME NULL,
    network INTEGER NOT NULL DEFAULT 4,
    last_error_msg TEXT NULL,
    resent_times INT DEFAULT 0,
    signature TEXT NULL,
    encoded TEXT NOT NULL,
```

* tx_id - unique UUID4 generated for trasnsaction
* sender - internal yagna address used for sending transaction
* nonce - Ethereum nonce assigned to transaction
* status - status of the transaction:
  * CREATED(1) - the transaction is submitted to db and wait to be sent
  * SENT(2) - the transaction is successfully sent to the blockchain network 
  * PENDING(3) - the transaction is found on blockchain but waiting for execution 
  * CONFIRMED(4) - the transaction is confirmed and succeeded
  * ERRORSENT(10) - transaction failed to be sent on-chain (not consuming nonce, has to be repeated)
  * ERRORONCHAIN(11) - transaction is confirmed but failed on-chain (consuming nonce, cannot be repeated until new transaction is assigned to payment)
* tx_type (transfer or faucet transaction)
* tmp_onchain_txs - hashes of all transactions that are sent to the chain (important for checking transaction status when gas is increased)
* final_tx - onchain transaction hash only when transaction is CONFIRMED or ERRORONCHAIN. tmp_onchain_txs are removed to reduce clutter.
* starting_gas_price - gas in Gwei
* current_gas_price - starts with null then it has assigned higher level of gas until transaction is processed
* max_gas_price - limit for current_gas_price
* final_gas_price - assigned after transaction is CONFIRMED or ERRORONCHAIN.
* final_gas_used - assigned after transaction is CONFIRMED or ERRORONCHAIN. use final_gas_used * final_gas_price to have transaction cost in Gwei  
* gas_limit - assigned max gas for transaction. If set to low ends with error during transaction sent or error on chain depending on set value.
* time_created - UTC time when entry is created.
* time_last_action - UTC time of last change of the entry.
* time_sent - UTC time of last succesfull sent.
* time_confirmed - UTC time of transaction confirmation (can be also when error on-chain)
* network - id of the Network (for example: 80001 is Mumbai, 137 is Polygon)
* last_error_msg - last error during sending or error onchain, Nulled when trasnaction is successfull
* resent_times - not used right now, intended to limit transaction retries
* signature - transaction signature
* encoded - YagnaRawTransaction encoded in json





