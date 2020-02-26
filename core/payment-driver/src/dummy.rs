use crate::{
    AccountBalance, Balance, Currency, PaymentAmount, PaymentConfirmation, PaymentDetails,
    PaymentDriver, PaymentDriverError, PaymentStatus, SignTx,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ethereum_types::Address;
use serde_json;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

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
                amount: 0.into(),
            },
            gas: Some(Balance {
                currency: Currency::Eth,
                amount: 0.into(),
            }),
        })
    }

    async fn schedule_payment(
        &mut self,
        invoice_id: &str,
        amount: PaymentAmount,
        recipient: Address,
        _due_date: DateTime<Utc>,
        _sign_tx: SignTx<'_>,
    ) -> Result<(), PaymentDriverError> {
        match self.payments.entry(invoice_id.to_string()) {
            Entry::Occupied(_) => Err(PaymentDriverError::AlreadyScheduled),
            Entry::Vacant(entry) => {
                entry.insert(PaymentDetails {
                    recipient,
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
            amount: 0.into(),
        })
    }
}
