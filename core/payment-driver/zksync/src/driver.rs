/*
    ZksyncDriver to handle payments on the zksync network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use actix::Arbiter;
use chrono::Utc;
use serde_json;
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    account::{Accounts, AccountsRc},
    bus,
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
}

#[async_trait(?Send)]
impl PaymentDriver for ZksyncDriver {
    async fn account_event(
        &self,
        _db: (),
        _caller: String,
        msg: IdentityEvent,
    ) -> Result<(), IdentityError> {
        self.active_accounts.borrow_mut().handle_event(msg);
        Ok(())
    }

    async fn get_account_balance(
        &self,
        _db: (),
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
        _db: (),
        _caller: String,
        msg: GetTransactionBalance,
    ) -> Result<BigDecimal, GenericError> {
        log::debug!("get_transaction_balance: {:?}", msg);
        //todo!()
        // TODO: Get real transaction balance
        Ok(BigDecimal::from(1_000_000_000_000_000_000u64))
    }

    async fn init(&self, _db: (), _caller: String, msg: Init) -> Result<Ack, GenericError> {
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
        _db: (),
        _caller: String,
        msg: SchedulePayment,
    ) -> Result<String, GenericError> {
        log::debug!("schedule_payment: {:?}", msg);

        let date = Utc::now();
        let details = driver_utils::to_payment_details(msg, Some(date));
        let confirmation = to_confirmation(&details)?;
        // TODO: move to database / background task
        wallet::make_transfer(&details).await?;
        let order_id = Uuid::new_v4().to_string();
        let driver_name = self.get_name();
        // Make a clone of order_id as return values
        let result = order_id.clone();

        log::info!(
            "Scheduled payment with success. order_id={}, details={:?}",
            &order_id,
            &details
        );

        // Spawned because calling payment service while handling a call from payment service
        // would result in a deadlock.
        Arbiter::spawn(async move {
            let _ = bus::notify_payment(&driver_name, &order_id, &details, confirmation)
                .await
                .map_err(|e| log::error!("{}", e));
        });
        Ok(result)
    }

    async fn verify_payment(
        &self,
        _db: (),
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
