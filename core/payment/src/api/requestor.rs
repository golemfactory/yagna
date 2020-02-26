use crate::api::*;
use crate::dao::allocation::AllocationDao;
use crate::dao::debit_note::DebitNoteDao;
use crate::dao::invoice::InvoiceDao;
use crate::dao::payment::PaymentDao;
use crate::error::{DbError, Error};
use crate::models as db_models;
use crate::processor::PaymentProcessor;
use crate::utils::with_timeout;
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
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
        Ok(debit_notes) => HttpResponse::Ok().json(
            debit_notes
                .into_iter()
                .map(|d| d.into())
                .collect::<Vec<DebitNote>>(),
        ),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
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
            HttpResponse::Ok().json(Into::<DebitNote>::into(debit_note))
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
        _ => HttpResponse::NotFound().finish(),
    }
}

async fn accept_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn reject_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Rejection>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_debit_note_events(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

// *************************** INVOICE ****************************

async fn get_invoices(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let dao: InvoiceDao = db.as_dao();
    match dao.get_received(recipient_id).await {
        Ok(invoices) => HttpResponse::Ok().json(
            invoices
                .into_iter()
                .map(|d| d.into())
                .collect::<Vec<Invoice>>(),
        ),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

async fn get_invoice(db: Data<DbExecutor>, path: Path<InvoiceId>, id: Identity) -> HttpResponse {
    let recipient_id = id.identity.to_string();
    let dao: InvoiceDao = db.as_dao();
    match dao.get(path.invoice_id.clone()).await {
        Ok(Some(invoice)) if invoice.invoice.recipient_id == recipient_id => {
            HttpResponse::Ok().json(Into::<Invoice>::into(invoice))
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
        _ => HttpResponse::NotFound().finish(),
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
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
        _ => return HttpResponse::NotFound().finish(),
    };

    let node_id = id.identity;
    if Some(node_id) != invoice.recipient_id.parse().ok() {
        return HttpResponse::Unauthorized().body(format!(
            "Identity {:?} is not authorized to send this debit note",
            node_id,
        ));
    }

    if invoice.amount != acceptance.total_amount_accepted {
        return HttpResponse::BadRequest().finish();
    }

    match invoice.status {
        InvoiceStatus::Received => (),
        InvoiceStatus::Rejected => (),
        InvoiceStatus::Accepted => return HttpResponse::Ok().finish(),
        InvoiceStatus::Settled => return HttpResponse::Ok().finish(),
        InvoiceStatus::Issued => return HttpResponse::InternalServerError().finish(),
        InvoiceStatus::Cancelled => return HttpResponse::BadRequest().finish(),
        InvoiceStatus::Failed => return HttpResponse::BadRequest().finish(),
    }

    let allocation_dao: AllocationDao = db.as_dao();
    let allocation: Allocation = match allocation_dao.get(allocation_id.clone()).await {
        Ok(Some(allocation)) => allocation.into(),
        Ok(None) => return HttpResponse::BadRequest().finish(),
        Err(e) => return HttpResponse::InternalServerError().body(e.to_string()),
    };
    if invoice.amount > allocation.remaining_amount {
        let msg = format!(
            "Not enough funds. Allocated: {} Needed: {}",
            allocation.remaining_amount, invoice.amount
        );
        return HttpResponse::BadRequest().body(msg);
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
            ))) => return { HttpResponse::BadRequest().body(e) },
            Err(e) => return { HttpResponse::InternalServerError().body(e.to_string()) },
            _ => (),
        }

        if let Err(e) = processor.schedule_payment(invoice, allocation_id).await {
            return HttpResponse::InternalServerError().body(e.to_string());
        }

        match dao
            .update_status(invoice_id, InvoiceStatus::Accepted.into())
            .await
        {
            Ok(_) => HttpResponse::Ok().finish(),
            Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
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
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoice_events(db: Data<DbExecutor>, query: Query<EventParams>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

// ************************** ALLOCATION **************************

async fn create_allocation(db: Data<DbExecutor>, body: Json<NewAllocation>) -> HttpResponse {
    // TODO: Handle deposits & timeouts
    let allocation: db_models::NewAllocation = body.into_inner().into();
    let allocation_id = allocation.id.clone();
    let dao: AllocationDao = db.as_dao();
    match async move {
        dao.create(allocation).await?;
        Ok(dao.get(allocation_id).await?)
    }
    .await
    {
        Ok(Some(allocation)) => HttpResponse::Created().json(Into::<Allocation>::into(allocation)),
        Ok(None) => HttpResponse::InternalServerError().body("Database error"),
        Err(DbError::Query(e)) => HttpResponse::BadRequest().body(e.to_string()),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

async fn get_allocations(db: Data<DbExecutor>) -> HttpResponse {
    let dao: AllocationDao = db.as_dao();
    match dao.get_all().await {
        Ok(allocations) => HttpResponse::Ok().json(
            allocations
                .into_iter()
                .map(Into::into)
                .collect::<Vec<Allocation>>(),
        ),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

async fn get_allocation(db: Data<DbExecutor>, path: Path<AllocationId>) -> HttpResponse {
    let dao: AllocationDao = db.as_dao();
    match dao.get(path.allocation_id.clone()).await {
        Ok(Some(allocation)) => HttpResponse::Ok().json(Into::<Allocation>::into(allocation)),
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

async fn amend_allocation(
    db: Data<DbExecutor>,
    path: Path<AllocationId>,
    body: Json<Allocation>,
) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn release_allocation(db: Data<DbExecutor>, path: Path<AllocationId>) -> HttpResponse {
    let dao: AllocationDao = db.as_dao();
    match dao.delete(path.allocation_id.clone()).await {
        Ok(_) => HttpResponse::Ok().finish(),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
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
        Ok(payments) => HttpResponse::Ok().json(
            payments
                .into_iter()
                .map(Into::into)
                .collect::<Vec<Payment>>(),
        ),
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
    }
}

async fn get_payment(db: Data<DbExecutor>, path: Path<PaymentId>, id: Identity) -> HttpResponse {
    let payer_id = id.identity.to_string();
    let dao: PaymentDao = db.as_dao();
    match dao.get(path.payment_id.clone()).await {
        Ok(Some(payment)) if payment.payment.payer_id == payer_id => {
            HttpResponse::Ok().json(Into::<Payment>::into(payment))
        }
        Err(e) => HttpResponse::InternalServerError().body(e.to_string()),
        _ => HttpResponse::NotFound().finish(),
    }
}

async fn get_debit_note_payments(db: Data<DbExecutor>, path: Path<DebitNoteId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}

async fn get_invoice_payments(db: Data<DbExecutor>, path: Path<InvoiceId>) -> HttpResponse {
    HttpResponse::NotImplemented().finish() // TODO
}
