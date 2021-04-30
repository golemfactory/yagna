// Extrnal crates
use actix_web::web::{get, post, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use serde_json::value::Value::Null;
use std::time::Instant;

// Workspace uses
use metrics::{counter, timing};
use ya_client_model::payment::*;
use ya_core_model::payment::local::{SchedulePayment, BUS_ID as LOCAL_SERVICE};
use ya_core_model::payment::public::{
    AcceptDebitNote, AcceptRejectError, SendDebitNote, SendError, BUS_ID as PUBLIC_SERVICE,
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
    match dao.get(debit_note_id, node_id).await {
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
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let timeout_secs = query.timeout.unwrap_or(params::DEFAULT_EVENT_TIMEOUT);
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_events = query.max_events;
    let app_session_id = &query.app_session_id;

    let dao: DebitNoteEventDao = db.as_dao();
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

async fn issue_debit_note(
    db: Data<DbExecutor>,
    body: Json<NewDebitNote>,
    id: Identity,
) -> HttpResponse {
    let debit_note = body.into_inner();
    let activity_id = debit_note.activity_id.clone();

    let agreement = match get_agreement_for_activity(
        activity_id.clone(),
        ya_core_model::Role::Provider,
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
            .create_if_not_exists(activity_id, node_id, Role::Provider, agreement_id)
            .await?;

        let dao: DebitNoteDao = db.as_dao();
        let debit_note_id = dao.create_new(debit_note, node_id).await?;
        let debit_note = dao.get(debit_note_id, node_id).await?;

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

    let debit_note = match dao.get(debit_note_id.clone(), node_id).await {
        Ok(Some(debit_note)) => debit_note,
        Ok(None) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    if debit_note.status != DocumentStatus::Issued {
        return response::ok(Null); // Debit note has been already sent
    }

    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);

    let result = with_timeout(timeout, async move {
        match async move {
            log::debug!(
                "Sending DebitNote [{}] to [{}].",
                debit_note_id,
                debit_note.recipient_id
            );

            ya_net::from(node_id)
                .to(debit_note.recipient_id)
                .service(PUBLIC_SERVICE)
                .call(SendDebitNote(debit_note))
                .await??;
            dao.mark_received(debit_note_id, node_id).await?;
            Ok(())
        }
        .await
        {
            Ok(_) => {
                log::info!("DebitNote [{}] sent.", path.debit_note_id);
                counter!("payment.debit_notes.provider.sent", 1);
                response::ok(Null)
            }
            Err(Error::Rpc(RpcMessageError::Send(SendError::BadRequest(e)))) => {
                response::bad_request(&e)
            }
            Err(e) => response::server_error(&e),
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
    log::trace!("Querying DB for Debit Note [{}]", debit_note_id);
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
    let amount_to_pay = &debit_note.total_amount_due - &activity.total_amount_scheduled.0;

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

    let timeout = query.timeout.unwrap_or(params::DEFAULT_ACK_TIMEOUT);
    let result = with_timeout(timeout, async move {
        let issuer_id = debit_note.issuer_id;
        let accept_msg = AcceptDebitNote::new(debit_note_id.clone(), acceptance, issuer_id);
        let schedule_msg =
            SchedulePayment::from_debit_note(debit_note, allocation_id, amount_to_pay);
        match async move {
            log::trace!(
                "Sending AcceptDebitNote [{}] to [{}]",
                debit_note_id,
                issuer_id
            );
            ya_net::from(node_id)
                .to(issuer_id)
                .service(PUBLIC_SERVICE)
                .call(accept_msg)
                .await??;
            if let Some(msg) = schedule_msg {
                log::trace!("Calling SchedulePayment [{}] locally", debit_note_id);
                bus::service(LOCAL_SERVICE).send(msg).await??;
            }
            log::trace!("Accepting Debit Note [{}] in DB", debit_note_id);
            dao.accept(debit_note_id.clone(), node_id).await?;
            log::trace!("Debit Note accepted successfully for [{}]", debit_note_id);
            Ok(())
        }
        .await
        {
            Ok(_) => {
                log::info!("DebitNote [{}] accepted.", path.debit_note_id);
                counter!("payment.debit_notes.requestor.accepted", 1);
                response::ok(Null)
            }
            Err(Error::Rpc(RpcMessageError::AcceptReject(AcceptRejectError::BadRequest(e)))) => {
                return response::bad_request(&e);
            }
            Err(e) => return response::server_error(&e),
        }
    })
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
