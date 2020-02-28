use crate::api::*;
use crate::dao::allocation::AllocationDao;
use crate::dao::debit_note::DebitNoteDao;
use crate::dao::invoice::InvoiceDao;
use crate::dao::invoice_event::InvoiceEventDao;
use crate::dao::payment::PaymentDao;
use crate::error::{DbError, Error};
use crate::models as db_models;
use crate::processor::PaymentProcessor;
use crate::utils::{response, with_timeout};
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use serde_json::value::Value::Null;
use std::time::Duration;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment;
use ya_model::payment::*;
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/debitNotes", get().to(get_debit_notes))
        .route("/debitNotes/{debit_note_id}", get().to(get_debit_note))
        .route(
            "/debitNotes/{debit_note_id}/payments",
            get().to(get_debit_note_payments),
        )
        .route(
            "/debitNotes/{debit_note_id}/accept",
            post().to(accept_debit_note),
        )
        .route(
            "/debitNotes/{debit_note_id}/reject",
            post().to(reject_debit_note),
        )
        .route("/debitNoteEvents", get().to(get_debit_note_events))
        .route("/invoices", get().to(get_invoices))
        .route("/invoices/{invoice_id}", get().to(get_invoice))
        .route(
            "/invoices/{invoice_id}/payments",
            get().to(get_invoice_payments),
        )
        .route("/invoices/{invoice_id}/accept", post().to(accept_invoice))
        .route("/invoices/{invoice_id}/reject", post().to(reject_invoice))
        .route("/invoiceEvents", get().to(get_invoice_events))
        .route("/allocations", post().to(create_allocation))
        .route("/allocations", get().to(get_allocations))
        .route("/allocations/{allocation_id}", get().to(get_allocation))
        .route("/allocations/{allocation_id}", put().to(amend_allocation))
        .route(
            "/allocations/{allocation_id}",
            delete().to(release_allocation),
        )
        .route("/payments", get().to(get_payments))
        .route("/payments/{payment_id}", get().to(get_payment))
}

// ************************** DEBIT NOTE **************************

async fn get_debit_notes(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let dao: DebitNoteDao = db.as_dao();
    match dao.get_received(recipient_id).await {
        Ok(debit_notes) => response::ok(
            debit_notes
                .into_iter()
                .map(|d| d.into())
                .collect::<Vec<DebitNote>>(),
        ),
        Err(e) => response::server_error(&e),
    }
}

async fn get_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    id: Identity,
) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let dao: DebitNoteDao = db.as_dao();
    match dao.get(path.debit_note_id.clone()).await {
        Ok(Some(debit_note)) if debit_note.recipient_id == recipient_id => {
            response::ok::<DebitNote>(debit_note.into())
        }
        Err(e) => response::server_error(&e),
        _ => response::not_found(),
    }
}

async fn accept_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn reject_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Rejection>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_debit_note_events(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    response::not_implemented() // TODO
}

// *************************** INVOICE ****************************

async fn get_invoices(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let dao: InvoiceDao = db.as_dao();
    match dao.get_received(recipient_id).await {
        Ok(invoices) => response::ok(
            invoices
                .into_iter()
                .map(|d| d.into())
                .collect::<Vec<Invoice>>(),
        ),
        Err(e) => response::server_error(&e),
    }
}

async fn get_invoice(db: Data<DbExecutor>, path: Path<InvoiceId>, id: Identity) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let dao: InvoiceDao = db.as_dao();
    match dao.get(path.invoice_id.clone()).await {
        Ok(Some(invoice)) if invoice.invoice.recipient_id == recipient_id => {
            response::ok::<Invoice>(invoice.into())
        }
        Err(e) => response::server_error(&e),
        _ => response::not_found(),
    }
}

async fn accept_invoice(
    db: Data<DbExecutor>,
    processor: Data<PaymentProcessor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
    id: Identity,
) -> HttpResponse {
    let invoice_id = path.invoice_id.clone();
    let recipient_id = id.identity.to_string();
    let acceptance = body.into_inner();
    let allocation_id = acceptance.allocation_id.clone();

    let dao: InvoiceDao = db.as_dao();
    let invoice: Invoice = match dao.get(invoice_id.clone()).await {
        Ok(Some(invoice)) if invoice.invoice.recipient_id == recipient_id => invoice.into(),
        Err(e) => return response::server_error(&e),
        _ => return response::not_found(),
    };

    let node_id = id.identity;
    if Some(node_id) != invoice.recipient_id.parse().ok() {
        return response::unauthorized();
    }

    if invoice.amount != acceptance.total_amount_accepted {
        return response::bad_request(&"Invalid amount accepted");
    }

    match invoice.status {
        InvoiceStatus::Received => (),
        InvoiceStatus::Rejected => (),
        InvoiceStatus::Failed => (),
        InvoiceStatus::Accepted => return response::ok(Null),
        InvoiceStatus::Settled => return response::ok(Null),
        InvoiceStatus::Issued => return response::server_error(&"Illegal status: issued"),
        InvoiceStatus::Cancelled => return response::bad_request(&"Invoice cancelled"),
    }

    let allocation_dao: AllocationDao = db.as_dao();
    let allocation: Allocation = match allocation_dao.get(allocation_id.clone()).await {
        Ok(Some(allocation)) => allocation.into(),
        Ok(None) => {
            return response::bad_request(&format!("Allocation {} not found", allocation_id))
        }
        Err(e) => return response::server_error(&e),
    };
    if invoice.amount > allocation.remaining_amount {
        let msg = format!(
            "Not enough funds. Allocated: {} Needed: {}",
            allocation.remaining_amount, invoice.amount
        );
        return response::bad_request(&msg);
    }

    with_timeout(query.timeout, async move {
        let issuer_id: NodeId = invoice.issuer_id.parse().unwrap();
        let msg = payment::AcceptInvoice {
            invoice_id: invoice_id.clone(),
            acceptance,
        };
        match async move {
            issuer_id.service(payment::BUS_ID).call(msg).await??;
            Ok(())
        }
        .await
        {
            Err(Error::Rpc(payment::RpcMessageError::AcceptReject(
                payment::AcceptRejectError::BadRequest(e),
            ))) => return response::bad_request(&e),
            Err(e) => return response::server_error(&e),
            _ => (),
        }

        if let Err(e) = processor.schedule_payment(invoice, allocation_id).await {
            return response::server_error(&e);
        }

        match dao
            .update_status(invoice_id, InvoiceStatus::Accepted.into())
            .await
        {
            Ok(_) => response::ok(Null),
            Err(e) => response::server_error(&e),
        }
    })
    .await
}

async fn reject_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
    body: Json<Rejection>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_invoice_events(
    db: Data<DbExecutor>,
    query: Query<EventParams>,
    id: Identity,
) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let timeout = query.timeout;
    let later_than = query.later_than.map(|d| d.naive_utc());
    let dao: InvoiceEventDao = db.as_dao();

    match dao
        .get_for_recipient(recipient_id.clone(), later_than.clone())
        .await
    {
        Err(e) => return response::server_error(&e),
        Ok(events) if events.len() > 0 || timeout == 0 => {
            return response::ok::<Vec<InvoiceEvent>>(events.into_iter().map(Into::into).collect())
        }
        _ => (),
    }

    let timeout = Duration::from_secs(timeout.into());
    let result = tokio::time::timeout(timeout, async move {
        loop {
            tokio::time::delay_for(Duration::from_secs(1)).await;
            match dao
                .get_for_recipient(recipient_id.clone(), later_than.clone())
                .await
            {
                Err(e) => break Err(e),
                Ok(events) if events.len() > 0 => break Ok(events),
                _ => (),
            }
        }
    })
    .await
    .unwrap_or(Ok(vec![]));
    match result {
        Err(e) => response::server_error(&e),
        Ok(events) => {
            response::ok::<Vec<InvoiceEvent>>(events.into_iter().map(Into::into).collect())
        }
    }
}

// ************************** ALLOCATION **************************

async fn create_allocation(db: Data<DbExecutor>, body: Json<NewAllocation>) -> HttpResponse {
    // TODO: Handle deposits & timeouts
    // TODO: Allocations should have owners (identities)
    let allocation: db_models::NewAllocation = body.into_inner().into();
    let allocation_id = allocation.id.clone();
    let dao: AllocationDao = db.as_dao();
    match async move {
        dao.create(allocation).await?;
        Ok(dao.get(allocation_id).await?)
    }
    .await
    {
        Ok(Some(allocation)) => response::created::<Allocation>(allocation.into()),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocations(db: Data<DbExecutor>) -> HttpResponse {
    let dao: AllocationDao = db.as_dao();
    match dao.get_all().await {
        Ok(allocations) => {
            response::ok::<Vec<Allocation>>(allocations.into_iter().map(Into::into).collect())
        }
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocation(db: Data<DbExecutor>, path: Path<AllocationId>) -> HttpResponse {
    let dao: AllocationDao = db.as_dao();
    match dao.get(path.allocation_id.clone()).await {
        Ok(Some(allocation)) => response::ok::<Allocation>(allocation.into()),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn amend_allocation(
    db: Data<DbExecutor>,
    path: Path<AllocationId>,
    body: Json<Allocation>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn release_allocation(db: Data<DbExecutor>, path: Path<AllocationId>) -> HttpResponse {
    let dao: AllocationDao = db.as_dao();
    match dao.delete(path.allocation_id.clone()).await {
        Ok(_) => response::ok(Null),
        Err(e) => response::server_error(&e),
    }
}

// *************************** PAYMENT ****************************

async fn get_payments(
    db: Data<DbExecutor>,
    query: Query<EventParams>,
    id: Identity,
) -> HttpResponse {
    let payer_id = id.identity.to_string();
    let dao: PaymentDao = db.as_dao();
    match dao.get_sent(payer_id).await {
        Ok(payments) => {
            response::ok::<Vec<Payment>>(payments.into_iter().map(Into::into).collect())
        }
        Err(e) => response::server_error(&e),
    }
}

async fn get_payment(db: Data<DbExecutor>, path: Path<PaymentId>, id: Identity) -> HttpResponse {
    let payer_id = id.identity.to_string();
    let dao: PaymentDao = db.as_dao();
    match dao.get(path.payment_id.clone()).await {
        Ok(Some(payment)) if payment.payment.payer_id == payer_id => {
            response::ok::<Payment>(payment.into())
        }
        Err(e) => response::server_error(&e),
        _ => response::not_found(),
    }
}

async fn get_debit_note_payments(db: Data<DbExecutor>, path: Path<DebitNoteId>) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_invoice_payments(db: Data<DbExecutor>, path: Path<InvoiceId>) -> HttpResponse {
    response::not_implemented() // TODO
}
