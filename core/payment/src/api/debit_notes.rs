use std::borrow::Cow;
use std::sync::Arc;
// Extrnal crates
use actix_web::web::{get, post, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use bigdecimal::BigDecimal;
use serde_json::value::Value::Null;
use std::time::Instant;
// Workspace uses
use metrics::{counter, timing};
use ya_client_model::payment::*;
use ya_core_model::payment::public::{
    AcceptDebitNote, AcceptRejectError, SendDebitNote, SendError, BUS_ID as PUBLIC_SERVICE,
};
use ya_core_model::payment::RpcMessageError;
use ya_net::RemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_persistence::types::Role;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::timeout::IntoTimeoutFuture;

// Local uses
use super::guard::AgreementLock;
use crate::dao::*;
use crate::error::{DbError, Error};
use crate::payment_sync::SYNC_NOTIFS_NOTIFY;
use crate::utils::provider::get_agreement_for_activity;
use crate::utils::*;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        // Shared
        .route("/debitNotes", get().to(get_debit_notes))
        .route("/debitNotes/{debit_note_id}", get().to(get_debit_note))
        .route(
            "/debitNotes/{debit_note_id}/payments",
            get().to(get_debit_note_payments),
        )
        .route("/debitNoteEvents", get().to(get_debit_note_events))
        // Provider
        .route("/debitNotes", post().to(issue_debit_note))
        .route(
            "/debitNotes/{debit_note_id}/send",
            post().to(send_debit_note),
        )
        .route(
            "/debitNotes/{debit_note_id}/cancel",
            post().to(cancel_debit_note),
        )
        // Requestor
        .route(
            "/debitNotes/{debit_note_id}/accept",
            post().to(accept_debit_note),
        )
        .route(
            "/debitNotes/{debit_note_id}/reject",
            post().to(reject_debit_note),
        )
}

async fn get_debit_notes(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    let dao: DebitNoteDao = db.as_dao();
    match dao
        .get_for_node_id(node_id, after_timestamp, max_items)
        .await
    {
        Ok(debit_notes) => response::ok(debit_notes),
        Err(e) => response::server_error(&e),
    }
}

async fn get_debit_note(
    db: Data<DbExecutor>,
    path: Path<params::DebitNoteId>,
    id: Identity,
) -> HttpResponse {
    let debit_note_id = path.debit_note_id.clone();
    let node_id = id.identity;
    let dao: DebitNoteDao = db.as_dao();
    match dao.get(debit_note_id, Some(node_id)).await {
        Ok(Some(debit_note)) => response::ok(debit_note),
        Ok(None) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

async fn get_debit_note_payments(
    db: Data<DbExecutor>,
    path: Path<params::DebitNoteId>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

async fn get_debit_note_events(
    db: Data<DbExecutor>,
    query: Query<params::EventParams>,
    req: actix_web::HttpRequest,
    id: Identity,
) -> HttpResponse {
    counter!("payment.debit_notes.events.query", 1);

    let requestor_events: Vec<Cow<'static, str>> = req
        .headers()
        .get("X-Requestor-Events")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').map(|s| Cow::Owned(s.to_owned())).collect())
        .unwrap_or_else(|| vec!["RECEIVED".into(), "CANCELLED".into()]);

    let provider_events: Vec<Cow<'static, str>> = req
        .headers()
        .get("X-Provider-Events")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').map(|s| Cow::Owned(s.to_owned())).collect())
        .unwrap_or_else(|| {
            vec![
                "ACCEPTED".into(),
                "REJECTED".into(),
                "SETTLED".into(),
                "CANCELLED".into(),
            ]
        });
    let node_id = id.identity;
    let timeout_secs = query.timeout.unwrap_or(params::DEFAULT_EVENT_TIMEOUT);
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_events = query.max_events;
    let app_session_id = &query.app_session_id;

    let dao: DebitNoteEventDao = db.as_dao();
    let getter = || async {
        dao.get_for_node_id(
            node_id,
            after_timestamp,
            max_events,
            app_session_id.clone(),
            requestor_events.clone(),
            provider_events.clone(),
        )
        .await
    };

    match listen_for_events(getter, timeout_secs).await {
        Ok(events) => response::ok(events),
        Err(e) => response::server_error(&e),
    }
}

// Provider

async fn issue_debit_note(
    db: Data<DbExecutor>,
    body: Json<NewDebitNote>,
    id: Identity,
) -> HttpResponse {
    let debit_note = body.into_inner();
    let activity_id = debit_note.activity_id.clone();

    let agreement = match get_agreement_for_activity(
        activity_id.clone(),
        ya_client_model::market::Role::Provider,
    )
    .await
    {
        Ok(Some(agreement_id)) => agreement_id,
        Ok(None) => return response::bad_request(&format!("Activity not found: {}", &activity_id)),
        Err(e) => return response::server_error(&e),
    };
    let agreement_id = agreement.agreement_id.clone();

    let node_id = id.identity;
    if &node_id != agreement.provider_id() {
        return response::unauthorized();
    }

    match async move {
        db.as_dao::<AgreementDao>()
            .create_if_not_exists(agreement, node_id, Role::Provider)
            .await?;
        db.as_dao::<ActivityDao>()
            .create_if_not_exists(activity_id.clone(), node_id, Role::Provider, agreement_id)
            .await?;

        let dao: DebitNoteDao = db.as_dao();
        let debit_note_id = dao.create_new(debit_note, node_id).await?;
        let debit_note = dao.get(debit_note_id.clone(), Some(node_id)).await?;

        log::info!("DebitNote [{debit_note_id}] for Activity [{activity_id}] issued.");
        counter!("payment.debit_notes.provider.issued", 1);
        Ok(debit_note)
    }
    .await
    {
        Ok(Some(debit_note)) => response::created(debit_note),
        Ok(None) => response::server_error(&"Database error"),
        Err(DbError::Query(e)) => response::bad_request(&e),
        Err(e) => response::server_error(&e),
    }
}

async fn send_debit_note(
    db: Data<DbExecutor>,
    path: Path<params::DebitNoteId>,
    query: Query<params::Timeout>,
    id: Identity,
) -> HttpResponse {
    let start = Instant::now();

    let debit_note_id = path.debit_note_id.clone();
    let node_id = id.identity;
    let dao: DebitNoteDao = db.as_dao();

    log::debug!("Requested send DebitNote [{}]", debit_note_id);
    counter!("payment.debit_notes.provider.sent.call", 1);

    let debit_note = match dao.get(debit_note_id.clone(), Some(node_id)).await {
        Ok(Some(debit_note)) => debit_note,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    if debit_note.status != DocumentStatus::Issued {
        return response::ok(Null); // Debit note has been already sent
    }

    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);
    let activity_id = debit_note.activity_id.clone();
    let recipient_id = debit_note.recipient_id;

    let result = with_timeout(timeout, async move {
        match async move {
            log::debug!(
                "Sending DebitNote [{}] to [{}].",
                debit_note.debit_note_id,
                debit_note.recipient_id
            );

            let debit_note_id = debit_note.debit_note_id.clone();

            ya_net::from(node_id)
                .to(debit_note.recipient_id)
                .service(PUBLIC_SERVICE)
                .call(SendDebitNote(debit_note))
                .await??;
            dao.mark_received(debit_note_id.clone(), node_id).await?;
            Ok(())
        }
        .timeout(Some(timeout))
        .await
        {
            Ok(Ok(_)) => {
                log::info!(
                    "DebitNote [{debit_note_id}] for Activity [{activity_id}] sent to [{recipient_id}]."
                );
                counter!("payment.debit_notes.provider.sent", 1);
                response::ok(Null)
            }
            Ok(Err(Error::Rpc(RpcMessageError::Send(SendError::BadRequest(e))))) => {
                response::bad_request(&e)
            }
            Ok(Err(e)) => response::server_error(&e),
            Err(_) => response::timeout(&"Timeout sending DebitNote to remote Node."),
        }
    })
    .await;

    timing!(
        "payment.debit_notes.provider.sent.time",
        start,
        Instant::now()
    );
    result
}

async fn cancel_debit_note(
    db: Data<DbExecutor>,
    path: Path<params::DebitNoteId>,
    query: Query<params::Timeout>,
) -> HttpResponse {
    response::not_implemented() // TODO
}

// Requestor

async fn accept_debit_note(
    db: Data<DbExecutor>,
    agreement_lock: Data<Arc<AgreementLock>>,
    path: Path<params::DebitNoteId>,
    query: Query<params::Timeout>,
    body: Json<Acceptance>,
    id: Identity,
) -> HttpResponse {
    let start = Instant::now();

    let debit_note_id = path.debit_note_id.clone();
    let node_id = id.identity;
    let acceptance = body.into_inner();
    let allocation_id = acceptance.allocation_id.clone();

    log::debug!("Requested accept DebitNote [{}]", debit_note_id);
    counter!("payment.debit_notes.requestor.accepted.call", 1);

    let dao: DebitNoteDao = db.as_dao();
    let sync_dao: SyncNotifsDao = db.as_dao();

    log::trace!("Querying DB for Debit Note [{}]", debit_note_id);
    let debit_note: DebitNote = match dao.get(debit_note_id.clone(), Some(node_id)).await {
        Ok(Some(debit_note)) => debit_note,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    // Required to serialize complex DB access patterns related to debit note / invoice acceptances.
    let _agreement_lock = agreement_lock.lock(debit_note.agreement_id.clone());

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
    log::trace!(
        "Querying DB for Activity [{}] for Debit Note [{}]",
        activity_id,
        debit_note_id
    );
    let activity = match db
        .as_dao::<ActivityDao>()
        .get(activity_id.clone(), node_id)
        .await
    {
        Ok(Some(activity)) => activity,
        Ok(None) => return response::server_error(&format!("Activity {} not found", activity_id)),
        Err(e) => return response::server_error(&e),
    };
    //check if invoice exists and accepted for this activity
    match db
        .as_dao::<InvoiceDao>()
        .get_by_agreement(activity.agreement_id.clone(), node_id)
        .await
    {
        Ok(Some(invoice)) => match invoice.status {
            DocumentStatus::Issued => {
                log::error!(
                    "Wrong status [{}] for invoice [{}] for Activity [{}] and agreement [{}]",
                    invoice.status,
                    invoice.invoice_id,
                    activity_id,
                    activity.agreement_id
                );
                return response::server_error(&"Wrong status for invoice");
            }
            DocumentStatus::Received => {
                log::warn!("Received debit note [{}] for freshly received invoice [{}] for Activity [{}] and agreement [{}]",
                        debit_note_id,
                        invoice.invoice_id,
                        activity_id,
                        activity.agreement_id
                    );
            }
            DocumentStatus::Accepted
            | DocumentStatus::Rejected
            | DocumentStatus::Failed
            | DocumentStatus::Settled
            | DocumentStatus::Cancelled => {
                log::info!("Received debit note [{}] for already existing invoice [{}] with status {} for Activity [{}] and agreement [{}]",
                        debit_note_id,
                        invoice.invoice_id,
                        invoice.status,
                        activity_id,
                        activity.agreement_id
                    );
                return response::ok(Null);
            }
        },
        Ok(None) => {
            //no problem, ignore
        }
        Err(e) => return response::server_error(&e),
    };
    let amount_to_pay = &debit_note.total_amount_due - &activity.total_amount_accepted.0;

    log::trace!(
        "Querying DB for Allocation [{}] for Debit Note [{}]",
        allocation_id,
        debit_note_id
    );
    let allocation = match db
        .as_dao::<AllocationDao>()
        .get(allocation_id.clone(), node_id)
        .await
    {
        Ok(AllocationStatus::Active(allocation)) => allocation,
        Ok(AllocationStatus::Gone) => {
            return response::gone(&format!(
                "Allocation {} has been already released",
                allocation_id
            ))
        }
        Ok(AllocationStatus::NotFound) => {
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

    if amount_to_pay > BigDecimal::from(0) {
        match db
            .as_dao::<AllocationDao>()
            .spend_from_allocation_transaction(SpendFromAllocationArgs {
                owner_id: node_id,
                allocation_id: allocation_id.clone(),
                agreement_id: activity.agreement_id.clone(),
                activity_id: Some(activity.id.clone()),
                amount: amount_to_pay.clone(),
            })
            .await
        {
            Ok(_) => (),
            Err(e) => return response::server_error(&e),
        }
    }

    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);
    let result = async move {
        let issuer_id = debit_note.issuer_id;
        let accept_msg = AcceptDebitNote::new(debit_note_id.clone(), acceptance, issuer_id);
        match async move {
            // Mark the debit note as accepted in DB
            log::trace!("Accepting DebitNote [{}] in DB", debit_note_id);
            dao.accept(debit_note_id.clone(), node_id).await?;
            log::trace!("DebitNote accepted successfully for [{}]", debit_note_id);

            log::debug!(
                "Sending AcceptDebitNote [{}] to [{}]",
                debit_note_id,
                issuer_id
            );
            let send_result = ya_net::from(node_id)
                .to(issuer_id)
                .service(PUBLIC_SERVICE)
                .call(accept_msg)
                .await;

            if let Ok(response) = send_result {
                log::debug!("AcceptDebitNote delivered");
                dao.mark_accept_sent(debit_note_id.clone(), node_id).await?;
                response?;
            } else {
                log::debug!("AcceptDebitNote not delivered");
                sync_dao.upsert(issuer_id).await?;
                SYNC_NOTIFS_NOTIFY.notify_one();
            }

            Ok(())
        }
        .timeout(Some(timeout))
        .await
        {
            Ok(Ok(_)) => {
                log::info!(
                    "DebitNote [{}] for Activity [{}] accepted.",
                    path.debit_note_id,
                    activity_id
                );
                counter!("payment.debit_notes.requestor.accepted", 1);
                response::ok(Null)
            }
            Ok(Err(Error::Rpc(RpcMessageError::AcceptReject(AcceptRejectError::BadRequest(
                e,
            ))))) => response::bad_request(&e),
            Ok(Err(e)) => response::server_error(&e),
            Err(_) => response::timeout(&"Timeout accepting Debit Note on remote Node."),
        }
    }
    .await;

    timing!(
        "payment.debit_notes.requestor.accepted.time",
        start,
        Instant::now()
    );
    result
}

async fn reject_debit_note(
    db: Data<DbExecutor>,
    path: Path<params::DebitNoteId>,
    query: Query<params::Timeout>,
    body: Json<Rejection>,
) -> HttpResponse {
    response::not_implemented() // TODO
}
