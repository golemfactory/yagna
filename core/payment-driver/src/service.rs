use crate::processor::PaymentDriverProcessor;
use ya_core_model::driver::*;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub fn bind_service(db: &DbExecutor, processor: PaymentDriverProcessor) {
    log::debug!("Binding payment driver service to service bus");

    bus::ServiceBinder::new(BUS_ID, db, processor)
        .bind_with_processor(account_event)
        .bind_with_processor(init)
        .bind_with_processor(get_account_balance)
        .bind_with_processor(get_payment_status)
        .bind_with_processor(get_transaction_balance)
        .bind_with_processor(schedule_payment)
        .bind_with_processor(verify_payment);

    log::debug!("Successfully bound payment driver service to service bus");
}

pub async fn subscribe_to_identity_events() {
    if let Err(e) = bus::service(ya_core_model::identity::BUS_ID)
        .send(ya_core_model::identity::Subscribe {
            endpoint: BUS_ID.into(),
        })
        .await
    {
        log::error!("init app-key listener error: {}", e)
    }
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

async fn get_transaction_balance(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: GetTransactionBalance,
) -> Result<Balance, GenericError> {
    log::info!("get transaction balance: {:?}", msg);

    let sender = msg.sender();
    let recipient = msg.recipient();

    processor
        .get_transaction_balance(sender.as_str(), recipient.as_str())
        .await
        .map_or_else(|e| Err(GenericError::new(e)), |balance| Ok(balance))
}

async fn schedule_payment(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: SchedulePayment,
) -> Result<Ack, GenericError> {
    log::info!("schedule payment: {:?}", msg);

    let invoice_id = msg.invoice_id();
    let amount = msg.amount();
    let sender = msg.sender();
    let recipient = msg.recipient();
    let due_date = msg.due_date();

    processor
        .schedule_payment(
            invoice_id.as_str(),
            amount,
            sender.as_str(),
            recipient.as_str(),
            due_date,
        )
        .await
        .map_or_else(|e| Err(GenericError::new(e)), |()| Ok(Ack {}))
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

async fn account_event(
    _db: DbExecutor,
    processor: PaymentDriverProcessor,
    _caller: String,
    msg: ya_core_model::identity::event::Event,
) -> Result<(), ya_core_model::identity::Error> {
    log::info!("account event: {:?}", msg);
    Ok(())
}
