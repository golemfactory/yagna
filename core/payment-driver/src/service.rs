use crate::processor::PaymentDriverProcessor;
use ya_core_model::driver::*;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

pub fn bind_service(db: &DbExecutor, processor: PaymentDriverProcessor) {
    log::debug!("Binding payment driver service to service bus");

    ServiceBinder::new(BUS_ID, db, processor).bind_with_processor(get_account_balance);
    log::debug!("Successfully bound payment driver service to service bus");
}

async fn get_account_balance(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: GetAccountBalance,
) -> Result<AccountBalance, GenericError> {
    log::info!("get account balance: {:?}", msg);

    let addr = msg.address();

    processor
        .get_account_balance(addr.as_str())
        .await
        .map_or_else(
            |e| Err(GenericError::new(e)),
            |account_balance| Ok(account_balance),
        )
}
