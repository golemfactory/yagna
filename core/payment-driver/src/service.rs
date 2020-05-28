use crate::processor::PaymentDriverProcessor;
use ya_core_model::driver::*;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::ServiceBinder;

pub fn bind_service(db: &DbExecutor, processor: PaymentDriverProcessor) {
    log::debug!("Binding payment driver service to service bus");

    ServiceBinder::new(BUS_ID, db, processor)
        .bind_with_processor(init)
        .bind_with_processor(get_account_balance)
        .bind_with_processor(get_payment_status)
        .bind_with_processor(verify_payment);

    log::debug!("Successfully bound payment driver service to service bus");
}

async fn init(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: Init,
) -> Result<Ack, GenericError> {
    log::info!("init: {:?}", msg);

    let address = msg.address();
    let mode = msg.mode();

    processor
        .init(mode, address.as_str())
        .await
        .map_or_else(|e| Err(GenericError::new(e)), |()| Ok(Ack {}))
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

async fn get_payment_status(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: GetPaymentStatus,
) -> Result<PaymentStatus, GenericError> {
    log::info!("get payment status: {:?}", msg);

    let invoice_id = msg.invoice_id();

    processor
        .get_payment_status(invoice_id.as_str())
        .await
        .map_or_else(
            |e| Err(GenericError::new(e)),
            |payment_status| Ok(payment_status),
        )
}

async fn verify_payment(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: VerifyPayment,
) -> Result<PaymentDetails, GenericError> {
    log::info!("verify payment: {:?}", msg);

    let confirmation = msg.confirmation();

    processor.verify_payment(confirmation).await.map_or_else(
        |e| Err(GenericError::new(e)),
        |payment_details| Ok(payment_details),
    )
}
