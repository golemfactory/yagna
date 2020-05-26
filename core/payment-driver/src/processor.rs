use crate::account::AccountBalance;
use crate::PaymentDriver;
use crate::PaymentDriverResult;
use std::sync::Arc;
use ya_persistence::executor::DbExecutor;

#[derive(Clone)]
pub struct PaymentDriverProcessor {
    driver: Arc<dyn PaymentDriver + Send + Sync + 'static>,
    db_executor: DbExecutor,
}

impl PaymentDriverProcessor {
    pub fn new<D>(driver: D, db_executor: DbExecutor) -> Self
    where
        D: PaymentDriver + Send + Sync + 'static,
    {
        Self {
            driver: Arc::new(driver),
            db_executor,
        }
    }

    pub async fn get_account_balance(&self, addr: &str) -> PaymentDriverResult<AccountBalance> {
        self.driver.get_account_balance(addr).await
    }
}
