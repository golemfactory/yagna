use ya_core_model::payment::*;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

macro_rules! bind_gsb_method {
    ($db_executor:expr, $method:ident) => {{
        let db_ = $db_executor.clone();
        let _ = bus::bind_with_caller(&SERVICE_ID, move |addr, msg| {
            $method(db_.clone(), addr.to_string(), msg)
        });
    }};
}

pub fn bind_service(db: &DbExecutor) {
    log::info!("Binding payment service to service bus");

    bind_gsb_method!(db, send_debit_note);
    bind_gsb_method!(db, accept_debit_note);
    bind_gsb_method!(db, reject_debit_note);
    bind_gsb_method!(db, cancel_debit_note);

    bind_gsb_method!(db, send_invoice);
    bind_gsb_method!(db, accept_invoice);
    bind_gsb_method!(db, reject_invoice);
    bind_gsb_method!(db, cancel_invoice);

    bind_gsb_method!(db, send_payment);

    log::info!("Successfully bound payment service to service bus");
}

// ************************** DEBIT NOTE **************************

async fn send_debit_note(
    db: DbExecutor,
    sender: String,
    msg: SendDebitNote,
) -> Result<Ack, SendError> {
    unimplemented!() // TODO
}

async fn accept_debit_note(
    db: DbExecutor,
    sender: String,
    msg: AcceptDebitNote,
) -> Result<Ack, AcceptRejectError> {
    unimplemented!() // TODO
}

async fn reject_debit_note(
    db: DbExecutor,
    sender: String,
    msg: RejectDebitNote,
) -> Result<Ack, AcceptRejectError> {
    unimplemented!() // TODO
}

async fn cancel_debit_note(
    db: DbExecutor,
    sender: String,
    msg: CancelDebitNote,
) -> Result<Ack, CancelError> {
    unimplemented!() // TODO
}

// *************************** INVOICE ****************************

async fn send_invoice(db: DbExecutor, sender: String, msg: SendInvoice) -> Result<Ack, SendError> {
    unimplemented!() // TODO
}

async fn accept_invoice(
    db: DbExecutor,
    sender: String,
    msg: AcceptInvoice,
) -> Result<Ack, AcceptRejectError> {
    unimplemented!() // TODO
}

async fn reject_invoice(
    db: DbExecutor,
    sender: String,
    msg: RejectInvoice,
) -> Result<Ack, AcceptRejectError> {
    unimplemented!() // TODO
}

async fn cancel_invoice(
    db: DbExecutor,
    sender: String,
    msg: CancelInvoice,
) -> Result<Ack, CancelError> {
    unimplemented!() // TODO
}

// *************************** PAYMENT ****************************

async fn send_payment(db: DbExecutor, sender: String, msg: SendPayment) -> Result<Ack, SendError> {
    unimplemented!() // TODO
}
