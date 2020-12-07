use crate::processor::GNTDriverProcessor;
use crate::{DRIVER_NAME, PLATFORM_NAME};
use bigdecimal::BigDecimal;
use ya_core_model::driver::*;
use ya_core_model::payment::local as payment_srv;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed::service;
use ya_service_bus::{typed as bus, RpcEndpoint};

pub fn bind_service(db: &DbExecutor, processor: GNTDriverProcessor) {
    log::debug!("Binding payment driver service to service bus");

    bus::ServiceBinder::new(&driver_bus_id(DRIVER_NAME), db, processor)
        .bind_with_processor(account_event)
        .bind_with_processor(init)
        .bind_with_processor(get_account_balance)
        .bind_with_processor(get_transaction_balance)
        .bind_with_processor(schedule_payment)
        .bind_with_processor(verify_payment)
        .bind_with_processor(validate_allocation);

    log::debug!("Successfully bound payment driver service to service bus");
}

pub async fn subscribe_to_identity_events() -> anyhow::Result<()> {
    bus::service(ya_core_model::identity::BUS_ID)
        .send(ya_core_model::identity::Subscribe {
            endpoint: driver_bus_id(DRIVER_NAME),
        })
        .await??;
    Ok(())
}

pub async fn register_in_payment_service() -> anyhow::Result<()> {
    log::debug!("Registering driver in payment service...");
    let message = payment_srv::RegisterDriver {
        driver_name: DRIVER_NAME.to_string(),
        platform: PLATFORM_NAME.to_string(),
        recv_init_required: false,
    };
    service(payment_srv::BUS_ID).send(message).await?.unwrap(); // Unwrap on purpose because it's NoError
    log::debug!("Successfully registered driver in payment service.");

    Ok(())
}

async fn init(
    _db: DbExecutor,
    processor: GNTDriverProcessor,
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
    processor: GNTDriverProcessor,
    _caller: String,
    msg: GetAccountBalance,
) -> Result<BigDecimal, GenericError> {
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

async fn get_transaction_balance(
    _db: DbExecutor,
    processor: GNTDriverProcessor,
    _caller: String,
    msg: GetTransactionBalance,
) -> Result<BigDecimal, GenericError> {
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
    processor: GNTDriverProcessor,
    _caller: String,
    msg: SchedulePayment,
) -> Result<String, GenericError> {
    log::info!("schedule payment: {:?}", msg);

    let amount = msg.amount();
    let sender = msg.sender();
    let recipient = msg.recipient();
    let due_date = msg.due_date();

    processor
        .schedule_payment(amount, sender.as_str(), recipient.as_str(), due_date)
        .await
        .map_or_else(|e| Err(GenericError::new(e)), |r| Ok(r))
}

async fn verify_payment(
    _db: DbExecutor,
    processor: GNTDriverProcessor,
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

async fn validate_allocation(
    _db: DbExecutor,
    processor: GNTDriverProcessor,
    _caller: String,
    msg: ValidateAllocation,
) -> Result<bool, GenericError> {
    log::debug!("Validate allocation: {:?}", msg);
    let ValidateAllocation {
        address,
        amount,
        existing_allocations,
    } = msg;
    processor
        .validate_allocation(address, amount, existing_allocations)
        .await
        .map_err(GenericError::new)
}

async fn account_event(
    _db: DbExecutor,
    processor: GNTDriverProcessor,
    _caller: String,
    msg: ya_core_model::identity::event::Event,
) -> Result<(), ya_core_model::identity::Error> {
    log::debug!("account event: {:?}", msg);
    let _ = match msg {
        ya_core_model::identity::event::Event::AccountLocked { identity } => {
            processor.account_locked(identity).await
        }
        ya_core_model::identity::event::Event::AccountUnlocked { identity } => {
            processor.account_unlocked(identity).await
        }
    }
    .map_err(|e| log::error!("Identity event listener error: {:?}", e));

    Ok(())
}
