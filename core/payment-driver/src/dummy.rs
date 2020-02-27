use crate::{
    AccountBalance, Balance, Currency, PaymentAmount, PaymentConfirmation, PaymentDetails,
    PaymentDriver, PaymentDriverError, PaymentStatus, SignTx,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::{Address, U256};
use ethsign::Signature;
use serde_json;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::convert::TryInto;

#[derive(Clone)]
pub struct DummyDriver {
    payments: HashMap<String, PaymentDetails>,
}

impl DummyDriver {
    pub fn new() -> Self {
        Self {
            payments: HashMap::new(),
        }
    }
}

#[async_trait]
impl PaymentDriver for DummyDriver {
    async fn get_account_balance(&self) -> Result<AccountBalance, PaymentDriverError> {
        Ok(AccountBalance {
            base_currency: Balance {
                currency: Currency::Gnt,
                amount: U256::from_dec_str("1000000000000000000000000").unwrap(),
            },
            gas: Some(Balance {
                currency: Currency::Eth,
                amount: U256::from_dec_str("1000000000000000000000000").unwrap(),
            }),
        })
    }

    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        recipient: Address,
        _due_date: DateTime<Utc>,
        sign_tx: SignTx<'_>,
    ) -> Result<(), PaymentDriverError> {
        match self.payments.entry(invoice_id.to_string()) {
            Entry::Occupied(_) => Err(PaymentDriverError::AlreadyScheduled),
            Entry::Vacant(entry) => {
                entry.insert(PaymentDetails {
                    recipient,
                    sender: recover_addr(sign_tx).await?,
                    amount: amount.base_currency_amount,
                    date: Some(Utc::now()),
                });
                Ok(())
            }
        }
    }

    async fn get_payment_status(
        &self,
        invoice_id: &str,
    ) -> Result<PaymentStatus, PaymentDriverError> {
        match self.payments.get(invoice_id) {
            Some(details) => Ok(PaymentStatus::Ok(PaymentConfirmation::from(
                serde_json::to_string(details).unwrap().as_bytes(),
            ))),
            None => Err(PaymentDriverError::NotFound),
        }
    }

    async fn verify_payment(
        &self,
        confirmation: &PaymentConfirmation,
    ) -> Result<PaymentDetails, PaymentDriverError> {
        let json_str = std::str::from_utf8(confirmation.confirmation.as_slice()).unwrap();
        let details = serde_json::from_str(&json_str).unwrap();
        Ok(details)
    }

    async fn get_transaction_balance(
        &self,
        _payer: Address,
    ) -> Result<Balance, PaymentDriverError> {
        Ok(Balance {
            currency: Currency::Gnt,
            amount: U256::from_dec_str("1000000000000000000000000").unwrap(),
        })
    }
}

fn sig_from_vec(vec: Vec<u8>) -> Result<Signature, std::array::TryFromSliceError> {
    let (v, vec) = vec.split_at(1);
    let (r, s) = vec.split_at(32);
    let v = v[0];
    let r: [u8; 32] = r.try_into()?;
    let s: [u8; 32] = s.try_into()?;
    Ok(Signature { v, r, s })
}

async fn recover_addr(sign_tx: SignTx<'_>) -> Result<Address, PaymentDriverError> {
    let msg: [u8; 32] = [1; 32];
    let sig = sign_tx(msg.to_vec()).await;

    let sig = match sig_from_vec(sig) {
        Ok(sig) => sig,
        Err(_) => {
            return Err(PaymentDriverError::LibraryError(
                "Invalid signature".to_string(),
            ))
        }
    };
    let pub_key = sig.recover(&msg)?;
    Ok(pub_key.address().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethsign::SecretKey;
    use futures::Future;

    #[tokio::test]
    async fn test_recover_addr() {
        let secret: [u8; 32] = [1; 32];
        let secret = SecretKey::from_raw(&secret).unwrap();
        let addr: Address = secret.public().address().into();
        let sign_tx = |msg: Vec<u8>| -> Box<dyn Future<Output = Vec<u8>> + Unpin + Send + Sync> {
            let sig = secret.sign(msg.as_slice()).unwrap();
            let mut v = Vec::with_capacity(33);
            v.push(sig.v);
            v.extend_from_slice(&sig.r[..]);
            v.extend_from_slice(&sig.s[..]);
            Box::new(futures::future::ready(v))
        };
        let recovered_addr = recover_addr(&sign_tx).await.unwrap();
        assert_eq!(recovered_addr, addr);
    }
}
