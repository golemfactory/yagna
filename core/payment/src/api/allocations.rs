use std::convert::TryInto;
use std::time::Duration;
// External crates
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use serde_json::value::Value::Null;
use ya_client_model::NodeId;

// Workspace uses
use ya_agreement_utils::{ClauseOperator, ConstraintKey, Constraints};
use ya_client_model::payment::*;
use ya_core_model::payment::local::{
    ValidateAllocation, ValidateAllocationError, BUS_ID as LOCAL_SERVICE,
};
use ya_core_model::payment::RpcMessageError;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::accounts::{init_account, Account};
use crate::dao::*;
use crate::error::{DbError, Error};
use crate::utils::response;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .route("/allocations", post().to(create_allocation))
        .route("/allocations", get().to(get_allocations))
        .route("/allocations/{allocation_id}", get().to(get_allocation))
        .route("/allocations/{allocation_id}", put().to(amend_allocation))
        .route(
            "/allocations/{allocation_id}",
            delete().to(release_allocation),
        )
        .route("/demandDecorations", get().to(get_demand_decorations))
}

async fn create_allocation(
    db: Data<DbExecutor>,
    body: Json<NewAllocation>,
    id: Identity,
) -> HttpResponse {
    // TODO: Handle deposits & timeouts
    let allocation = body.into_inner();
    let node_id = id.identity;
    let mut payment_platform = match &allocation.payment_platform {
        Some(platform) => platform.clone(),
        None => return response::bad_request(&"payment platform must be provided"),
    };

    log::debug!("payment platform: {payment_platform}");

    let address = allocation
        .address
        .clone()
        .unwrap_or_else(|| node_id.to_string());

    // If the request contains information about the payment platform, initialize the account
    // by setting the `send` field to `true`, as it is implied by the intent behing allocation of funds.

    // payment_platform is of the form driver-network-token
    // eg. erc20-rinkeby-tglm
    let platform = payment_platform.clone();
    let [mut driver, network, token]: [&str; 3] =
        match platform.split('-').collect::<Vec<_>>().try_into() {
            Ok(arr) => arr,
            Err(_e) => {
                return response::bad_request(
                    &"paymentPlatform must be of the form driver-network-token",
                )
            }
        };

    // erc20 was removed
    if driver == "erc20" {
        driver = "erc20next".into();
        payment_platform = format!("{driver}-{network}-{token}");
    }

    let acc = Account {
        driver: driver.to_owned(),
        address: address.clone(),
        network: Some(network.to_owned()),
        token: None,
        send: true,
        receive: false,
    };

    if let Err(e) = init_account(acc).await {
        log::error!("Error initializing account: {:?}", e);
        return response::server_error(&e);
    }

    let validate_msg = ValidateAllocation {
        platform: payment_platform.clone(),
        address: address.clone(),
        amount: allocation.total_amount.clone(),
    };
    match async move { Ok(bus::service(LOCAL_SERVICE).send(validate_msg).await??) }.await {
        Ok(true) => {}
        Ok(false) => return response::bad_request(&"Insufficient funds to make allocation. Top up your account or release all existing allocations to unlock the funds via `yagna payment release-allocations`"),
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
                           ValidateAllocationError::AccountNotRegistered,
                       ))) => return response::bad_request(&"Account not registered"),
        Err(e) => return response::server_error(&e),
    }

    let dao = db.as_dao::<AllocationDao>();

    match dao
        .create(allocation, node_id, payment_platform, address)
        .await
    {
        Ok(allocation_id) => match dao.get(allocation_id, node_id).await {
            Ok(AllocationStatus::Active(allocation)) => {
                let allocation_id = allocation.allocation_id.clone();

                release_allocation_after(
                    db.clone(),
                    allocation_id,
                    allocation.timeout,
                    Some(node_id),
                )
                .await;

                response::created(allocation)
            }
            Ok(AllocationStatus::NotFound) => response::server_error(&"Database error"),
            Ok(AllocationStatus::Gone) => response::server_error(&"Database error"),
            Err(DbError::Query(e)) => response::bad_request(&e),
            Err(e) => response::server_error(&e),
        },
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocations(
    db: Data<DbExecutor>,
    query: Query<params::FilterParams>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let after_timestamp = query.after_timestamp.map(|d| d.naive_utc());
    let max_items = query.max_items;
    let dao: AllocationDao = db.as_dao();
    match dao.get_for_owner(node_id, after_timestamp, max_items).await {
        Ok(allocations) => response::ok(allocations),
        Err(e) => response::server_error(&e),
    }
}

async fn get_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();

    match dao.get(allocation_id.clone(), node_id).await {
        Ok(AllocationStatus::Active(allocation)) => response::ok(allocation),
        Ok(AllocationStatus::Gone) => response::gone(&format!(
            "Allocation {} has been already released",
            allocation_id
        )),
        Ok(AllocationStatus::NotFound) => response::not_found(),
        Err(e) => response::server_error(&e),
    }
}

fn amend_allocation_fields(
    old_allocation: Allocation,
    update: AllocationUpdate,
) -> Result<Allocation, &'static str> {
    let total_amount = update
        .total_amount
        .unwrap_or_else(|| old_allocation.total_amount.clone());
    let remaining_amount = total_amount.clone() - &old_allocation.spent_amount;

    if remaining_amount < BigDecimal::from(0) {
        return Err("New allocation would be smaller than the already spent amount");
    }
    if let Some(timeout) = update.timeout {
        if timeout < chrono::offset::Utc::now() {
            return Err("New allocation timeout is in the past");
        }
    }

    Ok(Allocation {
        total_amount,
        remaining_amount,
        timeout: update.timeout.or(old_allocation.timeout),
        ..old_allocation
    })
}

async fn amend_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    body: Json<AllocationUpdate>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;
    let new_allocation: AllocationUpdate = body.into_inner();
    let dao: AllocationDao = db.as_dao();

    let current_allocation = match dao.get(allocation_id.clone(), node_id).await {
        Ok(AllocationStatus::Active(allocation)) => allocation,
        Ok(AllocationStatus::Gone) => {
            return response::gone(&format!(
                "Allocation {allocation_id} has been already released",
            ))
        }
        Ok(AllocationStatus::NotFound) => return response::not_found(),
        Err(e) => return response::server_error(&e),
    };

    let amended_allocation =
        match amend_allocation_fields(current_allocation.clone(), new_allocation) {
            Ok(allocation) => allocation,
            Err(e) => return response::bad_request(&e),
        };

    // validation will take into account all existing allocation, including the one
    // being currently modified. This means we only need to validate the increase.
    let amount_to_validate =
        amended_allocation.total_amount.clone() - &current_allocation.total_amount;

    let validate_msg = ValidateAllocation {
        platform: amended_allocation.payment_platform.clone(),
        address: amended_allocation.address.clone(),
        amount: if amount_to_validate > BigDecimal::from(0) {
            amount_to_validate
        } else {
            0.into()
        },
    };
    match async move { Ok(bus::service(LOCAL_SERVICE).send(validate_msg).await??) }.await {
        Ok(true) => {}
        Ok(false) => return response::bad_request(&"Insufficient funds to make allocation. Top up your account or release all existing allocations to unlock the funds via `yagna payment release-allocations`"),
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
                           ValidateAllocationError::AccountNotRegistered,
                       ))) => return response::bad_request(&"Account not registered"),
        Err(e) => return response::server_error(&e),
    }

    match dao.replace(amended_allocation, node_id).await {
        Ok(true) => {}
        Ok(false) => {
            return response::server_error(
                &"Allocation not present despite preconditions being already ensured",
            )
        }
        Err(e) => return response::server_error(&e),
    }

    get_allocation(db, path, id).await
}

async fn release_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = Some(id.identity);
    let dao = db.as_dao::<AllocationDao>();

    match dao.release(allocation_id.clone(), node_id).await {
        Ok(AllocationReleaseStatus::Released) => response::ok(Null),
        Ok(AllocationReleaseStatus::NotFound) => response::not_found(),
        Ok(AllocationReleaseStatus::Gone) => response::gone(&format!(
            "Allocation {} has been already released",
            allocation_id
        )),
        Err(e) => response::server_error(&e),
    }
}

async fn get_demand_decorations(
    db: Data<DbExecutor>,
    path: Query<params::AllocationIds>,
    id: Identity,
) -> HttpResponse {
    let allocation_ids = path.allocation_ids.clone();
    let node_id = id.identity;
    let dao: AllocationDao = db.as_dao();
    let allocations = match dao.get_many(allocation_ids, node_id).await {
        Ok(allocations) => allocations,
        Err(e) => return response::server_error(&e),
    };
    if allocations.len() != path.allocation_ids.len() {
        return response::not_found();
    }

    let properties: Vec<MarketProperty> = allocations
        .into_iter()
        .map(|allocation| MarketProperty {
            key: format!(
                "golem.com.payment.platform.{}.address",
                allocation.payment_platform
            ),
            value: allocation.address,
        })
        .collect();
    let constraints = properties
        .iter()
        .map(|property| ConstraintKey::new(property.key.as_str()).equal_to(ConstraintKey::new("*")))
        .collect();
    let constraints = vec![Constraints::new_clause(ClauseOperator::Or, constraints).to_string()];
    response::ok(MarketDecoration {
        properties,
        constraints,
    })
}

pub async fn release_allocation_after(
    db: Data<DbExecutor>,
    allocation_id: String,
    allocation_timeout: Option<DateTime<Utc>>,
    node_id: Option<NodeId>,
) {
    tokio::task::spawn(async move {
        if let Some(timeout) = allocation_timeout {
            //FIXME when upgrading to tokio 1.0 or greater. In tokio 0.2 timer panics when maximum duration of delay is exceeded.
            let max_duration: i64 = 1 << 35;

            loop {
                let time_diff = timeout.timestamp_millis() - Utc::now().timestamp_millis();

                if time_diff.is_negative() {
                    break;
                }

                let timeout = time_diff.min(max_duration) as u64;
                tokio::time::sleep(Duration::from_millis(timeout)).await;
            }

            forced_release_allocation(db, allocation_id, node_id).await;
        }
    });
}

pub async fn forced_release_allocation(
    db: Data<DbExecutor>,
    allocation_id: String,
    node_id: Option<NodeId>,
) {
    match db
        .as_dao::<AllocationDao>()
        .release(allocation_id.clone(), node_id)
        .await
    {
        Ok(AllocationReleaseStatus::Released) => {
            log::info!("Allocation {} released.", allocation_id);
        }
        Err(e) => {
            log::warn!(
                "Releasing allocation {} failed. Db error occurred: {}",
                allocation_id,
                e
            );
        }
        _ => (),
    }
}
