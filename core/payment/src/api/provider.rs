use crate::api::*;
use crate::dao::*;
use crate::error::{DbError, Error};
use crate::utils::provider::*;
use crate::utils::*;
use actix_web::web::{get, post, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use serde_json::value::Value::Null;
use ya_client_model::payment::*;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment::public::{SendDebitNote, SendError, SendInvoice, BUS_ID};
use ya_core_model::payment::RpcMessageError;
use ya_net::TryRemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_persistence::types::Role;
use ya_service_api_web::middleware::Identity;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/debitNotes", post().to(issue_debit_note))
        .route("/debitNotes", get().to(get_debit_notes))
        .route("/debitNotes/{debit_note_id}", get().to(get_debit_note))
        .route(
            "/debitNotes/{debit_note_id}/payments",
            get().to(get_debit_note_payments),
        )
        .route(
            "/debitNotes/{debit_note_id}/send",
            post().to(send_debit_note),
        )
        .route(
            "/debitNotes/{debit_note_id}/cancel",
            post().to(cancel_debit_note),
        )
        .route("/debitNoteEvents", get().to(get_debit_note_events))
        .route("/invoices", post().to(issue_invoice))
        .route("/invoices", get().to(get_invoices))
        .route("/invoices/{invoice_id}", get().to(get_invoice))
        .route(
            "/invoices/{invoice_id}/payments",
            get().to(get_invoice_payments),
        )
        .route("/invoices/{invoice_id}/send", post().to(send_invoice))
        .route("/invoices/{invoice_id}/cancel", post().to(cancel_invoice))
        .route("/invoiceEvents", get().to(get_invoice_events))
        .route("/payments", get().to(get_payments))
        .route("/payments/{payment_id}", get().to(get_payment))
}

// ************************** DEBIT NOTE **************************

async fn issue_debit_note(
    db: Data<DbExecutor>,
    body: Json<NewDebitNote>,
    id: Identity,
) -> HttpResponse {
    let debit_note = body.into_inner();
    let activity_id = debit_note.activity_id.clone();

    let agreement = match get_agreement_for_activity(activity_id.clone()).await {
        Ok(Some(agreement_id)) => agreement_id,
        Ok(None) => return response::bad_request(&format!("Activity not found: {}", &activity_id)),
        Err(e) => return response::server_error(&e),
    };
    let agreement_id = agreement.agreement_id.clone();

    let node_id = id.identity;
    if node_id
        != agreement
            .offer
            .provider_id
            .clone()
            .unwrap()
            .parse()
            .unwrap()
    {
        // FIXME: provider_id shouldn't be an Option
        return response::unauthorized();
    }

    match async move {
        db.as_dao::<AgreementDao>()
            .create_if_not_exists(agreement, node_id, Role::Provider)
            .await?;
        db.as_dao::<ActivityDao>()
            .create_if_not_exists(activity_id, node_id, Role::Provider, agreement_id)
            .await?;

        let dao: DebitNoteDao = db.as_dao();
        let debit_note_id = dao.create_new(debit_note, node_id).await?;
        Ok(dao.get(debit_note_id, node_id).await?)
    }
    .await
    {
        Ok(Some(debit_note)) => response::created(debit_note),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn get_debit_notes(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: DebitNoteDao = db.as_dao();
    match dao.get_for_provider(node_id).await {
        Ok(debit_notes) => response::ok(debit_notes),
        Err(e) => response::server_error(&e),
    }
}

async fn get_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    id: Identity,
) -> HttpResponse {
    let debit_note_id = path.debit_note_id.clone();
    let node_id = id.identity;
    let dao: DebitNoteDao = db.as_dao();
    match dao.get(debit_note_id, node_id).await {
        Ok(Some(debit_note)) => response::ok(debit_note),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn send_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    id: Identity,
) -> HttpResponse {
    let debit_note_id = path.debit_note_id.clone();
    let node_id = id.identity;
    let dao: DebitNoteDao = db.as_dao();
    let debit_note = match dao.get(debit_note_id.clone(), node_id).await {
        Ok(Some(debit_note)) => debit_note,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    if debit_note.status != InvoiceStatus::Issued {
        return response::ok(Null); // Debit note has been already sent
    }

    with_timeout(query.timeout, async move {
        match async move {
            let recipient_id: NodeId = debit_note.recipient_id.parse().unwrap();
            recipient_id
                .try_service(BUS_ID)?
                .call(SendDebitNote(debit_note))
                .await??;
            dao.update_status(debit_note_id, node_id, InvoiceStatus::Received)
                .await?;
            Ok(())
        }
        .await
        {
            Ok(_) => response::ok(Null),
            Err(Error::Rpc(RpcMessageError::Send(SendError::BadRequest(e)))) => {
                response::bad_request(&e)
            }
            Err(e) => response::server_error(&e),
        }
    })
    .await
}

async fn cancel_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_debit_note_events(
    db: Data<DbExecutor>,
    query: Query<EventParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.timeout;
    let later_than = query.later_than.map(|d| d.naive_utc());

    let dao: DebitNoteEventDao = db.as_dao();
    let getter = || async {
        dao.get_for_provider(node_id.clone(), later_than.clone())
            .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(events) => response::ok(events),
        Err(e) => response::server_error(&e),
    }
}

// *************************** INVOICE ****************************

async fn issue_invoice(db: Data<DbExecutor>, body: Json<NewInvoice>, id: Identity) -> HttpResponse {
    let invoice = body.into_inner();
    let agreement_id = invoice.agreement_id.clone();
    let activity_ids = invoice.activity_ids.clone().unwrap_or_default();

    let agreement = match get_agreement(agreement_id.clone()).await {
        Ok(Some(agreement)) => agreement,
        Ok(None) => {
            return response::bad_request(&format!("Agreement not found: {}", agreement_id))
        }
        Err(e) => return response::server_error(&e),
    };

    for activity_id in activity_ids.iter() {
        match get_agreement_id(activity_id.clone()).await {
            Ok(Some(id)) if id != agreement_id => {
                return response::bad_request(&format!(
                    "Activity {} belongs to agreement {} not {}",
                    activity_id, id, agreement_id
                ));
            }
            Ok(None) => {
                return response::bad_request(&format!("Activity not found: {}", activity_id))
            }
            Err(e) => return response::server_error(&e),
            _ => (),
        }
    }

    let node_id = id.identity;
    if node_id
        != agreement
            .offer
            .provider_id
            .clone()
            .unwrap()
            .parse()
            .unwrap()
    {
        // FIXME: provider_id shouldn't be an Option
        return response::unauthorized();
    }

    match async move {
        db.as_dao::<AgreementDao>()
            .create_if_not_exists(agreement, node_id, Role::Provider)
            .await?;

        let dao: ActivityDao = db.as_dao();
        for activity_id in activity_ids {
            dao.create_if_not_exists(activity_id, node_id, Role::Provider, agreement_id.clone())
                .await?;
        }

        let dao: InvoiceDao = db.as_dao();
        let invoice_id = dao.create_new(invoice, node_id).await?;
        Ok(dao.get(invoice_id, node_id).await?)
    }
    .await
    {
        Ok(Some(invoice)) => response::created(invoice),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn get_invoices(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: InvoiceDao = db.as_dao();
    match dao.get_for_provider(node_id).await {
        Ok(invoices) => response::ok(invoices),
        Err(e) => response::server_error(&e),
    }
}

async fn get_invoice(db: Data<DbExecutor>, path: Path<InvoiceId>, id: Identity) -> HttpResponse {
    let invoice_id = path.invoice_id.clone();
    let node_id = id.identity;
    let dao: InvoiceDao = db.as_dao();
    match dao.get(invoice_id, node_id).await {
        Ok(Some(invoice)) => response::ok(invoice),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn send_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
    id: Identity,
) -> HttpResponse {
    let invoice_id = path.invoice_id.clone();
    let node_id = id.identity;
    let dao: InvoiceDao = db.as_dao();
    let invoice = match dao.get(invoice_id.clone(), node_id).await {
        Ok(Some(invoice)) => invoice,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    if invoice.status != InvoiceStatus::Issued {
        return response::ok(Null); // Invoice has been already sent
    }

    with_timeout(query.timeout, async move {
        let recipient_id: NodeId = invoice.recipient_id.parse().unwrap();
        match async move {
            recipient_id
                .try_service(BUS_ID)?
                .call(SendInvoice(invoice))
                .await??;
            dao.update_status(invoice_id, node_id, InvoiceStatus::Received)
                .await?;
            Ok(())
        }
        .await
        {
            Ok(_) => response::ok(Null),
            Err(Error::Rpc(RpcMessageError::Send(SendError::BadRequest(e)))) => {
                response::bad_request(&e)
            }
            Err(e) => response::server_error(&e),
        }
    })
    .await
}

async fn cancel_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_invoice_events(
    db: Data<DbExecutor>,
    query: Query<EventParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.timeout;
    let later_than = query.later_than.map(|d| d.naive_utc());

    let dao: InvoiceEventDao = db.as_dao();
    let getter = || async {
        dao.get_for_provider(node_id.clone(), later_than.clone())
            .await
    };
    match listen_for_events(getter, timeout_secs).await {
        Ok(events) => response::ok(events),
        Err(e) => response::server_error(&e),
    }
}

// *************************** PAYMENT ****************************

async fn get_payments(
    db: Data<DbExecutor>,
    query: Query<EventParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.timeout;
    let later_than = query.later_than.map(|d| d.naive_utc());

    let dao: PaymentDao = db.as_dao();
    let getter = || async {
        dao.get_for_provider(node_id.clone(), later_than.clone())
            .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(payments) => response::ok(payments),
        Err(e) => response::server_error(&e),
    }
}

async fn get_payment(db: Data<DbExecutor>, path: Path<PaymentId>, id: Identity) -> HttpResponse {
    let payment_id = path.payment_id.clone();
    let node_id = id.identity;
    let dao: PaymentDao = db.as_dao();
    match dao.get(payment_id, node_id).await {
        Ok(Some(payment)) => response::ok(payment),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn get_debit_note_payments(db: Data<DbExecutor>, path: Path<DebitNoteId>) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_invoice_payments(db: Data<DbExecutor>, path: Path<InvoiceId>) -> HttpResponse {
    response::not_implemented() // TODO
}
