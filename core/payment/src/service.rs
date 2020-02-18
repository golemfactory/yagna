use crate::dao::debit_note::DebitNoteDao;
use crate::dao::invoice::InvoiceDao;
use crate::error::DbError;
use crate::utils::*;
use futures::prelude::*;
use ya_core_model::payment::*;
use ya_model::payment::InvoiceStatus;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

macro_rules! bind_gsb_method {
    ($db_executor:expr, $method:ident) => {{
        let db_ = $db_executor.clone();
        let _ = bus::bind_with_caller(&format!("/public{}", &SERVICE_ID), move |addr, msg| {
            log::info!(stringify!(Received call to $method));
            let future = $method(db_.clone(), addr.to_string(), msg);
            future.map(|res| {
                match &res {
                    Ok(_) => log::info!("Call successful"),
                    Err(e) => log::info!("Error occurred: {}", e.to_string())
                }
                res
            })
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
    let mut debit_note = msg.0;
    let agreement = match get_agreement(debit_note.agreement_id.clone()).await {
        Err(e) => {
            return Err(SendError::ServiceError(e.to_string()));
        }
        Ok(None) => {
            return Err(SendError::BadRequest(format!(
                "Agreement {} not found",
                debit_note.agreement_id
            )));
        }
        Ok(Some(agreement)) => agreement,
    };
    let sender_id = sender.trim_start_matches("/net/");
    let offeror_id = agreement.offer.provider_id.unwrap();
    let issuer_id = debit_note.issuer_id.clone();
    if sender_id != offeror_id || sender_id != issuer_id {
        // FIXME: provider_id shouldn't be an Option
        return Err(SendError::BadRequest("Invalid sender node ID".to_owned()));
    }

    let dao: DebitNoteDao = db.as_dao();
    debit_note.status = InvoiceStatus::Received;
    match dao.insert(debit_note.into()).await {
        Ok(_) => Ok(Ack {}),
        Err(DbError::Query(e)) => Err(SendError::BadRequest(e.to_string())),
        Err(e) => Err(SendError::ServiceError(e.to_string())),
    }
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
    let mut invoice = msg.0;
    let agreement = match get_agreement(invoice.agreement_id.clone()).await {
        Err(e) => {
            return Err(SendError::ServiceError(e.to_string()));
        }
        Ok(None) => {
            return Err(SendError::BadRequest(format!(
                "Agreement {} not found",
                invoice.agreement_id
            )));
        }
        Ok(Some(agreement)) => agreement,
    };
    let sender_id = sender.trim_start_matches("/net/");
    let offeror_id = agreement.offer.provider_id.unwrap();
    let issuer_id = invoice.issuer_id.clone();
    if sender_id != offeror_id || sender_id != issuer_id {
        // FIXME: provider_id shouldn't be an Option
        return Err(SendError::BadRequest("Invalid sender node ID".to_owned()));
    }

    let dao: InvoiceDao = db.as_dao();
    invoice.status = InvoiceStatus::Received;
    match dao.insert(invoice.into()).await {
        Ok(_) => Ok(Ack {}),
        Err(DbError::Query(e)) => Err(SendError::BadRequest(e.to_string())),
        Err(e) => Err(SendError::ServiceError(e.to_string())),
    }
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
