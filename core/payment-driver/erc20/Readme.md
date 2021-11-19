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

## Assigning nonces

To process transaction in ethereum network you have to assign nonce which has to be strictly one greater than previous nonce.
This makes process of sending transaction tricky and complicated, because when you send two transactions with the same nonce one of them will fail. Other case is when one transaction is blocked or waiting others with higher nonce will get stuck too.
Currently for every transaction nonce is assigned and not changed until transaction will consume nonce on chain.

Huge issue: When transaction is malformed and it get stuck, resend will not help and all transactions are blocked.

With the implementation of the driver we are trying to resolve cases automatically but the process is complicated and not perfect right now.

Good news: Nonce prevents double spending so we don't have to worry about that (unless we change the nonce)

Note: Error on chain will consume nonce and gas and new nonce has to be assigned to proceed with transaction, which is currently not handled.

## Bumping gas prices

Problem on Ethereum network is as follows:
If you sent transaction you have only vague idea when transaction will be performed. 
There is function that is estimating current gas price but it is far from perfect.
Also we want to pay for gas as little as possible. 
For example on Polygon Network if you sent transaction for 30.01Gwei you can be pretty sure it get proceeded fairly smoothly
right now, but when the network is congested transaction can get stuck for indefinite amount of time.
When network usage is lower you can try to make transaction for lower gas fee (for example 20Gwei).
To resolve this issue we implemented automatic gas bumping. Every set amount of time gas is increased to increase probability of transaction getting processed.
That way we can save money on gas and also have bigger chance of transactions not getting stuck.

Currently we proposed following gas levels for polygon network:
```
10.011, 
15.011, 
20.011, 
25.011, 
30.011, //minimum suggested gas price 
33.011, 
36.011, 
40.011, 
50.011, 
60.011, 
80.011,
100.011
```
Note that we add 0.011 to increase the chance of getting inline of someone setting whole number as gas price (which people tend to do). It costs almost nothing but significally increase your chance of transaction beeing processed.

Note that on test networks gas doesn't matter and transaction is processed instantly regardless of gas set. So to test this
feature you have to use Polygon network and pay some Matic for gas.

## List of known errors:

Error when sending when gas-limit set too low
```
RPC error: Error { code: ServerError(-32000), message: "intrinsic gas too low", data: None }
```
```
RPC error: Error { code: ServerError(-32000), message: "already known", data: None }
```
```
RPC error: Error { code: ServerError(-32000), message: "nonce too low", data: None }
```







