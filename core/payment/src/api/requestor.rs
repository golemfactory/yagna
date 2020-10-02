use crate::api::*;
use crate::dao::*;
use crate::error::{DbError, Error};
use crate::utils::{listen_for_events, response, with_timeout};
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use serde_json::value::Value::Null;
use ya_client_model::payment::*;
use ya_core_model::payment::local::{GetAccounts, SchedulePayment, BUS_ID as LOCAL_SERVICE};
use ya_core_model::payment::public::{
    AcceptDebitNote, AcceptInvoice, AcceptRejectError, BUS_ID as PUBLIC_SERVICE,
};
use ya_core_model::payment::RpcMessageError;
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

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
        .route("/accounts", get().to(get_accounts))
}

// ************************** DEBIT NOTE **************************

async fn get_debit_notes(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: DebitNoteDao = db.as_dao();
    match dao.get_for_requestor(node_id).await {
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

async fn accept_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
    id: Identity,
) -> HttpResponse {
    let debit_note_id = path.debit_note_id.clone();
    let node_id = id.identity;
    let acceptance = body.into_inner();
    let allocation_id = acceptance.allocation_id.clone();

    let dao: DebitNoteDao = db.as_dao();
    let debit_note: DebitNote = match dao.get(debit_note_id.clone(), node_id).await {
        Ok(Some(debit_note)) => debit_note,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    if debit_note.total_amount_due != acceptance.total_amount_accepted {
        return response::bad_request(&"Invalid amount accepted");
    }

    match debit_note.status {
        DocumentStatus::Received => (),
        DocumentStatus::Rejected => (),
        DocumentStatus::Failed => (),
        DocumentStatus::Accepted => return response::ok(Null),
        DocumentStatus::Settled => return response::ok(Null),
        DocumentStatus::Issued => return response::server_error(&"Illegal status: issued"),
        DocumentStatus::Cancelled => return response::bad_request(&"Debit note cancelled"),
    }

    let activity_id = debit_note.activity_id.clone();
    let activity = match db
        .as_dao::<ActivityDao>()
        .get(activity_id.clone(), node_id)
        .await
    {
        Ok(Some(activity)) => activity,
        Ok(None) => return response::server_error(&format!("Activity {} not found", activity_id)),
        Err(e) => return response::server_error(&e),
    };
    let amount_to_pay = &debit_note.total_amount_due - &activity.total_amount_accepted.0;

    let allocation = match db
        .as_dao::<AllocationDao>()
        .get(allocation_id.clone(), node_id)
        .await
    {
        Ok(Some(allocation)) => allocation,
        Ok(None) => {
            return response::bad_request(&format!("Allocation {} not found", allocation_id))
        }
        Err(e) => return response::server_error(&e),
    };
    if amount_to_pay > allocation.remaining_amount {
        let msg = format!(
            "Not enough funds. Allocated: {} Needed: {}",
            allocation.remaining_amount, amount_to_pay
        );
        return response::bad_request(&msg);
    }

    with_timeout(query.timeout, async move {
        let issuer_id = debit_note.issuer_id;
        let accept_msg = AcceptDebitNote::new(debit_note_id.clone(), acceptance, issuer_id);
        let schedule_msg =
            SchedulePayment::from_debit_note(debit_note, allocation_id, amount_to_pay);
        match async move {
            ya_net::from(node_id)
                .to(issuer_id)
                .service(PUBLIC_SERVICE)
                .call(accept_msg)
                .await??;
            if let Some(msg) = schedule_msg {
                bus::service(LOCAL_SERVICE).send(msg).await??;
            }
            dao.accept(debit_note_id, node_id).await?;
            Ok(())
        }
        .await
        {
            Ok(_) => response::ok(Null),
            Err(Error::Rpc(RpcMessageError::AcceptReject(AcceptRejectError::BadRequest(e)))) => {
                return response::bad_request(&e);
            }
            Err(e) => return response::server_error(&e),
        }

        // TODO: Compute amount to pay and schedule payment
    })
    .await
}

async fn reject_debit_note(
    db: Data<DbExecutor>,
    path: Path<DebitNoteId>,
    query: Query<Timeout>,
    body: Json<Rejection>,
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
        dao.get_for_requestor(node_id.clone(), later_than.clone())
            .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(events) => response::ok(events),
        Err(e) => response::server_error(&e),
    }
}

// *************************** INVOICE ****************************

async fn get_invoices(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: InvoiceDao = db.as_dao();
    match dao.get_for_requestor(node_id).await {
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

async fn accept_invoice(
    db: Data<DbExecutor>,
    path: Path<InvoiceId>,
    query: Query<Timeout>,
    body: Json<Acceptance>,
    id: Identity,
) -> HttpResponse {
    let invoice_id = path.invoice_id.clone();
    let node_id = id.identity;
    let acceptance = body.into_inner();
    let allocation_id = acceptance.allocation_id.clone();

    let dao: InvoiceDao = db.as_dao();
    let invoice = match dao.get(invoice_id.clone(), node_id).await {
        Ok(Some(invoice)) => invoice,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    if invoice.amount != acceptance.total_amount_accepted {
        return response::bad_request(&"Invalid amount accepted");
    }

    match invoice.status {
        DocumentStatus::Received => (),
        DocumentStatus::Rejected => (),
        DocumentStatus::Failed => (),
        DocumentStatus::Accepted => return response::ok(Null),
        DocumentStatus::Settled => return response::ok(Null),
        DocumentStatus::Cancelled => return response::bad_request(&"Invoice cancelled"),
        DocumentStatus::Issued => return response::server_error(&"Illegal status: issued"),
    }

    let agreement_id = invoice.agreement_id.clone();
    let agreement = match db
        .as_dao::<AgreementDao>()
        .get(agreement_id.clone(), node_id)
        .await
    {
        Ok(Some(agreement)) => agreement,
        Ok(None) => {
            return response::server_error(&format!("Agreement {} not found", agreement_id))
        }
        Err(e) => return response::server_error(&e),
    };
    let amount_to_pay = &invoice.amount - &agreement.total_amount_accepted.0;

    let allocation = match db
        .as_dao::<AllocationDao>()
        .get(allocation_id.clone(), node_id)
        .await
    {
        Ok(Some(allocation)) => allocation,
        Ok(None) => {
            return response::bad_request(&format!("Allocation {} not found", allocation_id))
        }
        Err(e) => return response::server_error(&e),
    };
    // FIXME: remaining amount should be 'locked' until payment is done to avoid double spending
    if amount_to_pay > allocation.remaining_amount {
        let msg = format!(
            "Not enough funds. Allocated: {} Needed: {}",
            allocation.remaining_amount, amount_to_pay
        );
        return response::bad_request(&msg);
    }

    with_timeout(query.timeout, async move {
        let issuer_id = invoice.issuer_id;
        let accept_msg = AcceptInvoice::new(invoice_id.clone(), acceptance, issuer_id);
        let schedule_msg = SchedulePayment::from_invoice(invoice, allocation_id, amount_to_pay);
        match async move {
            ya_net::from(node_id)
                .to(issuer_id)
                .service(PUBLIC_SERVICE)
                .call(accept_msg)
                .await??;
            bus::service(LOCAL_SERVICE).send(schedule_msg).await??;
            dao.accept(invoice_id, node_id).await?;
            Ok(())
        }
        .await
        {
            Ok(_) => response::ok(Null),
            Err(Error::Rpc(RpcMessageError::AcceptReject(AcceptRejectError::BadRequest(e)))) => {
                return response::bad_request(&e)
            }
            Err(e) => return response::server_error(&e),
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
    let node_id = id.identity;
    let timeout_secs = query.timeout;
    let later_than = query.later_than.map(|d| d.naive_utc());

    let dao: InvoiceEventDao = db.as_dao();
    let getter = || async {
        dao.get_for_requestor(node_id.clone(), later_than.clone())
            .await
    };
    match listen_for_events(getter, timeout_secs).await {
        Ok(events) => response::ok(events),
        Err(e) => response::server_error(&e),
    }
}

// ************************** ALLOCATION **************************

async fn create_allocation(
    db: Data<DbExecutor>,
    body: Json<NewAllocation>,
    id: Identity,
) -> HttpResponse {
    // TODO: Handle deposits & timeouts
    // TODO: Check available funds
    let allocation = body.into_inner();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    match async move {
        let allocation_id = dao.create(allocation, node_id).await?;
        Ok(dao.get(allocation_id, node_id).await?)
    }
    .await
    {
        Ok(Some(allocation)) => response::created(allocation),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocations(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    match dao.get_for_owner(node_id).await {
        Ok(allocations) => response::ok(allocations),
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocation(
    db: Data<DbExecutor>,
    path: Path<AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    match dao.get(allocation_id, node_id).await {
        Ok(Some(allocation)) => response::ok(allocation),
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

async fn release_allocation(
    db: Data<DbExecutor>,
    path: Path<AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    match dao.release(allocation_id, node_id).await {
        Ok(true) => response::ok(Null),
        Ok(false) => response::not_found(),
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
        dao.get_for_requestor(node_id.clone(), later_than.clone())
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

// *************************** ACCOUNTS ****************************

async fn get_accounts(id: Identity) -> HttpResponse {
    let node_id = id.identity.to_string();
    let all_accounts = match bus::service(LOCAL_SERVICE).send(GetAccounts {}).await {
        Ok(Ok(accounts)) => accounts,
        Ok(Err(e)) => return response::server_error(&e),
        Err(e) => return response::server_error(&e),
    };
    let recv_accounts: Vec<Account> = all_accounts
        .into_iter()
        .filter(|account| account.send)
        .filter(|account| account.address == node_id) // TODO: Implement proper account permission system
        .map(|account| Account {
            platform: account.platform,
            address: account.address,
        })
        .collect();
    response::ok(recv_accounts)
}
