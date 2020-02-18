use chrono::{DateTime, Utc};

use ethereum_types::{Address};

use web3::contract::{Contract};
use web3::transports::Http;
use web3::futures::Future;

use crate::{PaymentDriver, PaymentDriverResult};
use crate::account::{AccountBalance, Balance, Currency};
use crate::error::PaymentDriverError;
use crate::ethereum::EthereumClient;
use crate::payment::{PaymentAmount, PaymentConfirmation, PaymentDetails, PaymentStatus};



#[allow(unused)]
pub struct GNTDriver {
    address: Address,
    ethereum_client: EthereumClient,
    gnt_contract: Contract<Http>,
}

impl GNTDriver {
    #[allow(unused)]
    fn new(
        address: Address,
        ethereum_client: EthereumClient,
        contract_address: Address
    ) -> PaymentDriverResult<GNTDriver> {
        match ethereum_client.get_contract(contract_address, include_bytes!("./contracts/gnt.json"))
        {
            Ok(contract) => Ok(GNTDriver {
                address: address,
                ethereum_client: ethereum_client,
                gnt_contract: contract,
            }),
            Err(_) => Err(PaymentDriverError::UnexpectedError{}),
        }
    }

    pub fn get_gnt_balance(
        &self,
        address: ethereum_types::Address,
    ) -> PaymentDriverResult<Balance> {
        let result = self.gnt_contract.query(
            "balanceOf",
            (address,),
            None,
            web3::contract::Options::default(),
            None,
        );
        // TODO error handling
        let balance: ethereum_types::U256 = result.wait().unwrap();
        Ok(Balance::new(
            balance,
            Currency::Gnt {},
        ))
    }

    pub fn get_eth_balance(
        &self,
        address: ethereum_types::Address,
    ) -> PaymentDriverResult<Balance> {
        let block_number = None;
        match self.ethereum_client.get_eth_balance(address, block_number) {
            Ok(amount) => Ok(Balance::new(
                amount,
                Currency::Eth {},
            )),
            Err(_) => Err(PaymentDriverError::UnexpectedError {}),
        }
    }
}

#[allow(unused)]
#[async_trait::async_trait]
impl PaymentDriver for GNTDriver {
   /// Returns account balance
   async fn get_account_balance(&self) -> PaymentDriverResult<AccountBalance> {
       unimplemented!();
   }

   /// Schedules payment
   async fn schedule_payment(
       &mut self,
       invoice_id: &str,
       amount: PaymentAmount,
       recipient: Address,
       due_date: DateTime<Utc>,
   ) -> PaymentDriverResult<()> {
       unimplemented!();
   }

   /// Returns payment status
   async fn get_payment_status(&self, invoice_id: &str) -> PaymentDriverResult<PaymentStatus> {
       unimplemented!();
   }

   /// Verifies payment
   async fn verify_payment(
       &self,
       confirmation: &PaymentConfirmation,
   ) -> PaymentDriverResult<PaymentDetails> {
       unimplemented!();
   }

   /// Returns sum of transactions from given address
   async fn get_transaction_balance(&self, payee: Address) -> PaymentDriverResult<Balance> {
       unimplemented!();
   }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_it_works() {
        assert!(true);
    }
}
