/*
    ZksyncDriver to handle payments on the zksync network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use actix::Arbiter;
use chrono::Utc;
use futures3::prelude::*;
use serde_json;
use uuid::Uuid;
use ethereum_types::{Address, H160, H256, U256};

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsRc},
    bus,
    dao::{DbExecutor, payment::PaymentDao},
    db::models::{PaymentEntity, PAYMENT_STATUS_NOT_YET},
    driver::{async_trait, BigDecimal, IdentityError, IdentityEvent, PaymentDriver},
    model::{
        Ack, GenericError, GetAccountBalance, GetTransactionBalance, Init, PaymentConfirmation,
        PaymentDetails, SchedulePayment, VerifyPayment,
    },
    utils as driver_utils,
};

// Local uses
use crate::{zksync::wallet, DRIVER_NAME, PLATFORM_NAME};

pub struct ZksyncDriver {
    active_accounts: AccountsRc,
}

impl ZksyncDriver {
    pub fn new() -> Self {
        Self {
            active_accounts: Accounts::new_rc(),
        }
    }

    async fn process_payments() {
        for address in act.active_accounts.borrow().list_accounts() {
            log::trace!("payment job for: {:?}", address);
            match act.active_accounts.borrow().get_node_id(address.as_str()) {
                None => continue,
                Some(node_id) => {
                    Arbiter::spawn(async move {
                        match db
                            .as_dao::<PaymentDao>()
                            .get_pending_payments(account.clone())
                            .await
                        {
                            Err(e) => log::error!(
                                "Failed to fetch pending payments for {:?} : {:?}",
                                account,
                                e
                            ),
                            Ok(payments) => {
                                if !payments.is_empty() {
                                    log::info!("Processing {} Payments", payments.len());
                                    log::debug!("Payments details: {:?}", payments);
                                }
                                for payment in payments {
                                    let _ = process_payment(
                                        payment.clone(),
                                        client.clone(),
                                        gnt_contract.clone(),
                                        tx_sender.clone(),
                                        db.clone(),
                                        sign_tx,
                                    )
                                    .await
                                    .map_err(|e| {
                                        log::error!("Failed to process payment: {:?}, error: {:?}", payment, e)
                                    });
                                }
                            }
                        };
                        //process_payments().await;
                    });
                }
            }
        }
    }
}

#[async_trait(?Send)]
impl PaymentDriver for ZksyncDriver {
    async fn account_event(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError> {
        self.active_accounts.borrow_mut().handle_event(msg);
        Ok(())
    }

    async fn get_account_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetAccountBalance,
    ) -> Result<BigDecimal, GenericError> {
        log::debug!("get_account_balance: {:?}", msg);

        let balance = wallet::account_balance(&msg.address()).await?;

        log::debug!("get_account_balance - result: {}", &balance);
        Ok(balance)
    }

    fn get_name(&self) -> String {
        DRIVER_NAME.to_string()
    }

    fn get_platform(&self) -> String {
        PLATFORM_NAME.to_string()
    }

    async fn get_transaction_balance(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: GetTransactionBalance,
    ) -> Result<BigDecimal, GenericError> {
        log::debug!("get_transaction_balance: {:?}", msg);
        //todo!()
        // TODO: Get real transaction balance
        Ok(BigDecimal::from(1_000_000_000_000_000_000u64))
    }

    async fn init(&self, _db: DbExecutor, _caller: String, msg: Init) -> Result<Ack, GenericError> {
        log::debug!("init: {:?}", msg);

        wallet::init_wallet(&msg).await?;

        let address = msg.address().clone();
        let mode = msg.mode();
        bus::register_account(self, &address, mode).await?;

        log::info!(
            "Initialised payment account. mode={:?}, address={}, driver={}, platform={}",
            mode,
            &address,
            DRIVER_NAME,
            PLATFORM_NAME
        );
        Ok(Ack {})
    }

    async fn schedule_payment(
        &self,
        db: DbExecutor,
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);

        let order_id: String = format!("{}", Uuid::new_v4());
        let sender = msg.sender().to_owned();
        let recipient = msg.recipient().to_owned();
        let gnt_amount = big_dec_to_u256(msg.amount()).unwrap();
        let gas_amount = Default::default();

        let payment = PaymentEntity {
            amount: u256_to_big_endian_hex(gnt_amount),
            gas: u256_to_big_endian_hex(gas_amount),
            order_id: order_id.clone(),
            payment_due_date: msg.due_date().naive_utc(),
            sender: sender.clone(),
            recipient: recipient.clone(),
            status: PAYMENT_STATUS_NOT_YET,
            tx_id: None,
        };
        async move {
            db.as_dao::<PaymentDao>().insert(payment).await.map_err(GenericError::new)?;
            Ok(order_id)
        }
        .boxed_local().await
    }

    async fn verify_payment(
        &self,
        _db: DbExecutor,
        _caller: String,
        msg: VerifyPayment,
    ) -> Result<PaymentDetails, GenericError> {
        log::debug!("verify_payment: {:?}", msg);
        // todo!()
        // wallet::verify_transfer(msg).await?
        let details = from_confirmation(msg.confirmation())?;
        Ok(details)
    }
}

// Used by the DummyDriver to have a 2 way conversion between details & confirmation
fn to_confirmation(details: &PaymentDetails) -> Result<Vec<u8>, GenericError> {
    Ok(serde_json::to_string(details)
        .map_err(GenericError::new)?
        .into_bytes())
}

fn from_confirmation(confirmation: PaymentConfirmation) -> Result<PaymentDetails, GenericError> {
    let json_str =
        std::str::from_utf8(confirmation.confirmation.as_slice()).map_err(GenericError::new)?;
    let details = serde_json::from_str(&json_str).map_err(GenericError::new)?;
    Ok(details)
}

pub fn u256_to_big_endian_hex(value: U256) -> String {
    let mut bytes = [0u8; 32];
    value.to_big_endian(&mut bytes);
    hex::encode(&bytes)
}

const PRECISION: u64 = 1_000_000_000_000_000_000;
use num::bigint::ToBigInt;
pub fn big_dec_to_u256(v: BigDecimal) -> Result<U256, GenericError> {
    let v = v * Into::<BigDecimal>::into(PRECISION);
    let v = v.to_bigint().unwrap();
    let v = &v.to_string();
    Ok(U256::from_dec_str(v).unwrap())
}
