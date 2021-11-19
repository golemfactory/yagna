--use this query to debug and view ERC20 transaction list


SELECT tx_id,
       ts.status || '(' || t.status || ')' as `status`,
       Cast ((julianday('now') - julianday(time_created)) * 24 * 60 * 60 as integer) as `Created ago`,
       Cast ((julianday('now') - julianday(time_last_action)) * 24 * 60 * 60 as integer) as `Last action ago`,
       Cast ((julianday('now') - julianday(time_sent)) * 24 * 60 * 60 as integer) as `Last sent ago`,
       Cast ((julianday('now') - julianday(time_confirmed)) * 24 * 60 * 60 as integer) as `Last confirmed ago`,
       Cast ((julianday(time_confirmed) - julianday(time_created)) * 24 * 60 * 60 as integer) as `Total process time`,
       nonce,
       starting_gas_price,
       current_gas_price,
       max_gas_price,
       final_gas_price,
       final_gas_price_exact,
       final_gas_used,
       amount_base,
       amount_base_exact,
       amount_erc20,
       amount_erc20_exact,
       tx_type,
       tmp_onchain_txs,
       final_tx,
       gas_limit,
       time_created,
       time_last_action,
       time_sent,
       time_confirmed,
       network,
       last_error_msg,
       resent_times,
       signature,
       sender,
       encoded
  FROM `transaction` as t 
  JOIN `transaction_status` as ts on ts.status_id=t.status
  ORDER BY time_created DESC
  
