/*
    ZksyncDriver to handle payments on the zksync network.

    Please limit the logic in this file, use local mods to handle the calls.
*/
// Extrnal crates
use uuid::Uuid;

// Workspace uses
use ya_payment_driver::{
    account::{AccountsRc, AccountsRefMut},
    bus,
    driver::{async_trait, BigDecimal, PaymentDriver},
    model::{
        Ack, GenericError, GetAccountBalance, GetTransactionBalance, Init, PaymentDetails,
        SchedulePayment, VerifyPayment,
    },
    utils as driver_utils,
};

// Local uses
use crate::{zksync::wallet, DRIVER_NAME, PLATFORM_NAME};

pub struct ZksyncDriver {
    active_accounts: AccountsRc,
}

impl ZksyncDriver {
    pub fn new(accounts: AccountsRc) -> Self {
        Self {
            active_accounts: accounts,
        }
    }
}

#[async_trait(?Send)]
impl PaymentDriver for ZksyncDriver {
    // Accounts are stored on ZksyncDriver, but updated by PaymentDriver's shared logic
    fn get_accounts(&self) -> AccountsRefMut {
        self.active_accounts.borrow_mut()
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

        let details = driver_utils::to_payment_details(msg);
        let confirmation = driver_utils::to_confirmation(&details)?;
        // TODO: move to database / background task
        wallet::make_transfer(&details).await?;
        let order_id = Uuid::new_v4().to_string();
        bus::notify_payment(self, &order_id, &details, confirmation);

        log::info!(
            "Scheduled payment with success. order_id={}, details={:?}",
            &order_id,
            details
        );
        Ok(order_id)
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
        let details = driver_utils::from_confirmation(msg.confirmation())?;
        Ok(details)
    }
}
