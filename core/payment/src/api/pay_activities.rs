use std::collections::HashMap;
// External crates
use crate::dao::*;
use crate::utils::*;
use actix_web::web::{get, Data, Path, Query};
use actix_web::{HttpResponse, Scope};
use anyhow::anyhow;
use ya_client_model::payment::{params};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use crate::models::debit_note::DebitNoteForApi;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/payActivities", get().to(get_pay_activities))
        .route("/payActivities/{activity_id}", get().to(get_pay_activity))
        .route(
            "/payActivities/{activity_id}/debitNotes",
            get().to(get_activity_debit_notes),
        )
        .route(
            "/payActivities/{activity_id}/invoice",
            get().to(get_activity_invoice),
        )
        .route(
            "/payActivities/{activity_id}/orders",
            get().to(get_pay_activity_orders),
        )
}

async fn get_pay_activities(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let dao: ActivityDao = db.as_dao();
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    match dao
        .get_for_node_id(node_id, after_timestamp, max_items)
        .await
    {
        Ok(activities) => response::ok(activities),
        Err(e) => response::server_error(&e),
    }
}

async fn get_pay_activity(db: Data<DbExecutor>, path: Path<String>, id: Identity) -> HttpResponse {
    let node_id = id.identity;
    let activity_id = path.into_inner();
    let dao: ActivityDao = db.as_dao();
    match dao.get(activity_id, node_id).await {
        Ok(activity) => response::ok(activity),
        Err(e) => response::server_error(&e),
    }
}

pub async fn get_debit_note_chain(
    debit_list: Vec<DebitNoteForApi>,
) -> Result<Vec<DebitNoteForApi>, anyhow::Error> {
    if debit_list.is_empty() {
        return Ok(Vec::new());
    }
    let mut debit_note_chain = Vec::<DebitNoteForApi>::new();
    let mut debit_by_id = HashMap::new();
    let mut debit_by_prev_id = HashMap::new();

    for debit in debit_list.iter() {
        log::info!(
            "Debit note id: {} prev id: {:?}",
            debit.debit_note_id,
            debit.previous_debit_note_id
        );
        if debit_by_id
            .insert(debit.debit_note_id.clone(), debit.clone())
            .is_some()
        {
            return Err(anyhow!(
                "Duplicate debit note with id {}",
                debit.debit_note_id
            ));
        }
        if let Some(prev_id) = &debit.previous_debit_note_id {
            if debit_by_prev_id
                .insert(prev_id.clone(), debit.clone())
                .is_some()
            {
                return Err(anyhow!("Duplicate debit note with previous id {}", prev_id));
            }
        }
    }
    //find debit note that is not a previous debit note
    let mut not_previous_list: Vec<DebitNoteForApi> = Vec::new();
    for debit in debit_list.iter() {
        if !debit_by_prev_id.contains_key(&debit.debit_note_id) {
            not_previous_list.push(debit.clone());
            break;
        }
    }
    if not_previous_list.len() > 1 {
        return Err(anyhow!(
            "Expected exactly one debit note with no previous debit note, found {}",
            not_previous_list.len()
        ));
    }

    let start_debit_note = not_previous_list
        .into_iter()
        .next()
        .ok_or(anyhow!("Debit note with no previous debit note not found"))?;

    debit_note_chain.push(start_debit_note.clone());
    let mut prev_debit_note_id = start_debit_note.previous_debit_note_id.clone();
    while let Some(next_debit_note_id) = &prev_debit_note_id {
        let next_debit_note = match debit_by_id.get(next_debit_note_id) {
            Some(debit_note) => debit_note.clone(),
            None => {
                return Err(anyhow!(
                    "Debit note {} not found when building debit note chain",
                    next_debit_note_id
                ))
            }
        };

        debit_note_chain.push(next_debit_note.clone());
        prev_debit_note_id = next_debit_note.previous_debit_note_id.clone();
    }
    Ok(debit_note_chain)
}

async fn get_activity_invoice(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let activity_id = path.into_inner();
    let dao: ActivityDao = db.as_dao();
    let Some(activity) = (match dao.get(activity_id, node_id).await {
        Ok(activity) => activity,
        Err(e) => return response::server_error(&e),
    }) else {
        return response::server_error(&"Activity not found");
    };

    let dao: InvoiceDao = db.as_dao();
    dao.get_by_agreement(activity.agreement_id, node_id)
        .await
        .map(response::ok)
        .unwrap_or_else(|e| response::server_error(&e))
}

async fn get_activity_debit_notes(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let activity_id = path.into_inner();
    let dao: ActivityDao = db.as_dao();
    let Some(activity) = (match dao.get(activity_id, node_id).await {
        Ok(activity) => activity,
        Err(e) => return response::server_error(&e),
    }) else {
        return response::server_error(&"Activity not found");
    };

    let dao: DebitNoteDao = db.as_dao();
    let chain = match dao
        .list(Some(activity.role), None, None, Some(activity.id))
        .await
    {
        Ok(chain) => chain,
        Err(e) => return response::server_error(&e),
    };
    get_debit_note_chain(chain)
        .await
        .map(response::ok)
        .unwrap_or_else(|e| response::server_error(&e))
}

async fn get_pay_activity_orders(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let activity_id = path.into_inner();
    let dao: BatchDao = db.as_dao();
    match dao
        .get_batch_items(node_id, None, None, None, Some(activity_id))
        .await
    {
        Ok(items) => response::ok(items),
        Err(e) => response::server_error(&e),
    }
}
