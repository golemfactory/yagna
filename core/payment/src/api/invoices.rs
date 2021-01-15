// Extrnal crates
use actix_web::web::{get, post, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use serde_json::value::Value::Null;

// Workspace uses
use metrics::counter;
use ya_client_model::payment::*;
use ya_core_model::payment::local::{SchedulePayment, BUS_ID as LOCAL_SERVICE};
use ya_core_model::payment::public::{
    AcceptInvoice, AcceptRejectError, CancelError, CancelInvoice, SendError, SendInvoice,
    BUS_ID as PUBLIC_SERVICE,
};
use ya_core_model::payment::RpcMessageError;
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_persistence::types::Role;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::dao::*;
use crate::error::{DbError, Error};
use crate::utils::provider::get_agreement_id;
use crate::utils::*;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        // Shared
        .route("/invoices", get().to(get_invoices))
        .route("/invoices/{invoice_id}", get().to(get_invoice))
        .route(
            "/invoices/{invoice_id}/payments",
            get().to(get_invoice_payments),
        )
        .route("/invoiceEvents", get().to(get_invoice_events))
        // Provider
        .route("/invoices", post().to(issue_invoice))
        .route("/invoices/{invoice_id}/send", post().to(send_invoice))
        .route("/invoices/{invoice_id}/cancel", post().to(cancel_invoice))
        // Requestor
        .route("/invoices/{invoice_id}/accept", post().to(accept_invoice))
        .route("/invoices/{invoice_id}/reject", post().to(reject_invoice))
}

async fn get_invoices(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    let dao: InvoiceDao = db.as_dao();
    match dao
        .get_for_node_id(node_id, after_timestamp, max_items)
        .await
    {
        Ok(invoices) => response::ok(invoices),
        Err(e) => response::server_error(&e),
    }
}

async fn get_invoice(
    db: Data<DbExecutor>,
    path: Path<params::InvoiceId>,
    id: Identity,
) -> HttpResponse {
    let invoice_id = path.invoice_id.clone();
    let node_id = id.identity;
    let dao: InvoiceDao = db.as_dao();
    match dao.get(invoice_id, node_id).await {
        Ok(Some(invoice)) => response::ok(invoice),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn get_invoice_payments(db: Data<DbExecutor>, path: Path<params::InvoiceId>) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_invoice_events(
    db: Data<DbExecutor>,
    query: Query<params::EventParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.poll_timeout.unwrap_or(params::DEFAULT_EVENT_TIMEOUT);
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_events = query.max_events;
    let app_session_id = &query.app_session_id;

    let dao: InvoiceEventDao = db.as_dao();
    let getter = || async {
        dao.get_for_node_id(
            node_id.clone(),
            after_timestamp.clone(),
            max_events.clone(),
            app_session_id.clone(),
        )
        .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(events) => response::ok(events),
        Err(e) => response::server_error(&e),
    }
}

// Provider

async fn issue_invoice(db: Data<DbExecutor>, body: Json<NewInvoice>, id: Identity) -> HttpResponse {
    let invoice = body.into_inner();
    let agreement_id = invoice.agreement_id.clone();
    let activity_ids = invoice.activity_ids.clone().unwrap_or_default();

    let agreement = match get_agreement(agreement_id.clone(), ya_core_model::Role::Provider).await {
        Ok(Some(agreement)) => agreement,
        Ok(None) => {
            return response::bad_request(&format!("Agreement not found: {}", agreement_id))
        }
        Err(e) => return response::server_error(&e),
    };

    for activity_id in activity_ids.iter() {
        match get_agreement_id(activity_id.clone(), ya_core_model::Role::Provider).await {
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
    if &node_id != agreement.provider_id() {
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
        let invoice = dao.get(invoice_id, node_id).await?;

        counter!("payment.invoices.provider.issued", 1);
        Ok(invoice)
    }
    .await
    {
        Ok(Some(invoice)) => response::created(invoice),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn send_invoice(
    db: Data<DbExecutor>,
    path: Path<params::InvoiceId>,
    query: Query<params::Timeout>,
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

    if invoice.status != DocumentStatus::Issued {
        return response::ok(Null); // Invoice has been already sent
    }
    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);
    with_timeout(timeout, async move {
        match async move {
            ya_net::from(node_id)
                .to(invoice.recipient_id)
                .service(PUBLIC_SERVICE)
                .call(SendInvoice(invoice))
                .await??;
            dao.mark_received(invoice_id, node_id).await?;

            counter!("payment.invoices.provider.sent", 1);
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
    path: Path<params::InvoiceId>,
    query: Query<params::Timeout>,
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

    match invoice.status {
        DocumentStatus::Issued => (),
        DocumentStatus::Received => (),
        DocumentStatus::Rejected => (),
        DocumentStatus::Cancelled => return response::ok(Null),
        DocumentStatus::Accepted | DocumentStatus::Settled | DocumentStatus::Failed => {
            return response::conflict(&"Invoice already accepted by requestor")
        }
    }

    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);
    with_timeout(timeout, async move {
        match async move {
            ya_net::from(node_id)
                .to(invoice.recipient_id)
                .service(PUBLIC_SERVICE)
                .call(CancelInvoice {
                    invoice_id: invoice_id.clone(),
                    recipient_id: invoice.recipient_id,
                })
                .await??;
            dao.cancel(invoice_id, node_id).await?;

            counter!("payment.invoices.provider.cancelled", 1);
            Ok(())
        }
        .await
        {
            Ok(_) => response::ok(Null),
            Err(Error::Rpc(RpcMessageError::Cancel(CancelError::Conflict))) => {
                response::conflict(&"Invoice already accepted by requestor")
            }
            Err(e) => response::server_error(&e),
        }
    })
    .await
}

// Requestor

async fn accept_invoice(
    db: Data<DbExecutor>,
    path: Path<params::InvoiceId>,
    query: Query<params::Timeout>,
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

    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);
    with_timeout(timeout, async move {
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

            counter!("payment.invoices.requestor.accepted", 1);
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
    path: Path<params::InvoiceId>,
    query: Query<params::Timeout>,
    body: Json<Rejection>,
) -> HttpResponse {
    response::not_implemented() // TODO
}
