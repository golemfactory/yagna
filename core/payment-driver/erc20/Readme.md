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






