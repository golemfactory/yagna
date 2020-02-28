use crate::dao::debit_note::DebitNoteDao;
use crate::dao::invoice::InvoiceDao;
use crate::error::DbError;
use crate::utils::*;
use futures::prelude::*;
use std::fmt::Display;
use std::future::Future;
use ya_core_model::payment::*;
use ya_model::payment::*;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;
use ya_service_bus::RpcMessage;

struct ServiceBinder<'a, 'b> {
    addr: &'b str,
    db: &'a DbExecutor,
}

impl<'a, 'b> ServiceBinder<'a, 'b> {
    fn bind<F: 'static, Msg: RpcMessage, Output: 'static>(self, f: F) -> Self
    where
        F: Fn(DbExecutor, String, Msg) -> Output,
        Output: Future<Output = Result<Msg::Item, Msg::Error>>,
        Msg::Error: Display,
    {
        let db = self.db.clone();
        let _ = bus::bind_with_caller(self.addr, move |addr, msg| {
            log::debug!("Received call to {}", Msg::ID);
            let fut = f(db.clone(), addr, msg);
            fut.map(|res| {
                match &res {
                    Ok(_) => log::debug!("Call to {} successful", Msg::ID),
                    Err(e) => log::debug!("Call to {} failed: {}", Msg::ID, e),
                }
                res
            })
        });
        self
    }
}

pub fn bind_service(db: &DbExecutor) {
    log::debug!("Binding payment service to service bus");

    let _ = ServiceBinder {
        db,
        addr: ya_core_model::payment::BUS_ID,
    }
    .bind(send_debit_note)
    .bind(accept_debit_note)
    .bind(reject_debit_note)
    .bind(cancel_debit_note)
    .bind(send_invoice)
    .bind(accept_invoice)
    .bind(reject_invoice)
    .bind(cancel_invoice)
    .bind(send_payment);

    log::debug!("Successfully bound payment service to service bus");
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
    let offeror_id = agreement.offer.provider_id.unwrap(); // FIXME: provider_id shouldn't be an Option
    let issuer_id = debit_note.issuer_id.clone();
    if sender_id != offeror_id || sender_id != issuer_id {
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
    let offeror_id = agreement.offer.provider_id.unwrap(); // FIXME: provider_id shouldn't be an Option
    let issuer_id = invoice.issuer_id.clone();
    if sender_id != offeror_id || sender_id != issuer_id {
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
    let invoice_id = msg.invoice_id;
    let acceptance = msg.acceptance;
    let dao: InvoiceDao = db.as_dao();
    let invoice: Invoice = match dao.get(invoice_id.clone()).await {
        Ok(Some(invoice)) => invoice.into(),
        Ok(None) => return Err(AcceptRejectError::ObjectNotFound),
        Err(e) => return Err(AcceptRejectError::ServiceError(e.to_string())),
    };

    let sender_id = sender.trim_start_matches("/net/");
    if sender_id != invoice.recipient_id {
        return Err(AcceptRejectError::Forbidden);
    }

    if invoice.amount != acceptance.total_amount_accepted {
        let msg = format!(
            "Invalid amount accepted. Expected: {} Actual: {}",
            invoice.amount, acceptance.total_amount_accepted
        );
        return Err(AcceptRejectError::BadRequest(msg));
    }

    match invoice.status {
        InvoiceStatus::Issued => (),
        InvoiceStatus::Received => (),
        InvoiceStatus::Rejected => (),
        InvoiceStatus::Accepted => return Ok(Ack {}),
        InvoiceStatus::Settled => return Ok(Ack {}),
        InvoiceStatus::Cancelled => {
            return Err(AcceptRejectError::BadRequest(
                "Cannot accept cancelled invoice".to_owned(),
            ))
        }
        InvoiceStatus::Failed => {
            return Err(AcceptRejectError::BadRequest(
                "Cannot accept failed invoice".to_owned(),
            ))
        }
    }

    match dao
        .update_status(invoice_id, InvoiceStatus::Accepted.into())
        .await
    {
        Ok(_) => Ok(Ack {}),
        Err(e) => Err(AcceptRejectError::ServiceError(e.to_string())),
    }
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
