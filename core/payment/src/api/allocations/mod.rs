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
use ya_client_model::payment::allocation::PaymentPlatformEnum;
use ya_client_model::payment::*;
use ya_core_model::payment::local::{
    DriverName, NetworkName, ReleaseDeposit, ValidateAllocation, ValidateAllocationError,
    BUS_ID as LOCAL_SERVICE,
};
use ya_core_model::payment::RpcMessageError;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::accounts::{init_account, Account};
use crate::dao::*;
use crate::error::Error;
use crate::utils::response;

const DEFAULT_TESTNET_NETWORK: NetworkName = NetworkName::Holesky;
const DEFAULT_MAINNET_NETWORK: NetworkName = NetworkName::Polygon;
const DEFAULT_PAYMENT_DRIVER: DriverName = DriverName::Erc20;

mod api_error;
mod platform_triple;
mod token_name;

use platform_triple::PaymentPlatformTriple;

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
        .route(
            "/allocations/{allocation_id}/orders",
            get().to(get_pay_allocation_orders),
        )
}

async fn create_allocation(
    db: Data<DbExecutor>,
    body: Json<NewAllocation>,
    id: Identity,
) -> HttpResponse {
    let allocation = body.into_inner();
    let node_id = id.identity;

    let payment_triple = match &allocation.payment_platform {
        Some(PaymentPlatformEnum::PaymentPlatformName(name)) => {
            let payment_platform = match PaymentPlatformTriple::from_payment_platform_str(name) {
                Ok(p) => p,
                Err(err) => {
                    log::error!("Payment platform string parse failed: {err}");
                    return api_error::bad_platform_parameter(&allocation, &err.to_string(), &name);
                }
            };
            log::debug!(
                "Successfully parsed API payment platform name: {}",
                payment_platform
            );
            payment_platform
        }
        Some(PaymentPlatformEnum::PaymentPlatform(payment_platform)) => {
            match PaymentPlatformTriple::from_payment_platform_input(payment_platform) {
                Ok(platform_str) => platform_str,
                Err(err) => {
                    log::error!("Payment platform object parse failed: {err}");
                    return api_error::bad_platform_parameter(
                        &allocation,
                        &err.to_string(),
                        &payment_platform,
                    );
                }
            }
        }
        None => {
            let default_platform = PaymentPlatformTriple::default_testnet();
            log::debug!("No paymentPlatform entry found, using {default_platform}");
            default_platform
        }
    };

    let address = allocation
        .address
        .clone()
        .unwrap_or_else(|| node_id.to_string());

    log::info!(
        "Creating allocation for payment platform: {}",
        payment_triple
    );

    let acc = Account {
        driver: payment_triple.driver().to_string(),
        address: address.clone(),
        network: Some(payment_triple.network().to_string()),
        token: None,
        send: true,
        receive: false,
    };

    if let Err(err) = init_account(acc).await {
        return api_error::server_error(&allocation, &err.to_string());
    }

    let validate_msg = ValidateAllocation {
        platform: payment_triple.to_string(),
        address: address.clone(),
        amount: allocation.total_amount.clone(),
        timeout: allocation.timeout,
        deposit: allocation.deposit.clone(),
        new_allocation: true,
    };

    match async move { Ok(bus::service(LOCAL_SERVICE).send(validate_msg).await??) }.await {
        Ok(result) => {
            if let Some(error_response) = api_error::try_from_validation(
                result,
                &allocation,
                payment_triple.to_string(),
                address.clone(),
            ) {
                return error_response;
            }
        }
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
            ValidateAllocationError::AccountNotRegistered,
        ))) => {
            log::error!(
                "Account {} not registered on platform {}",
                address.clone(),
                payment_triple
            );

            return api_error::account_not_registered(
                &allocation,
                payment_triple.to_string(),
                address.clone(),
            );
        }
        Err(e) => return api_error::server_error(&allocation, &e.to_string()),
    }

    let dao = db.as_dao::<AllocationDao>();

    match dao
        .create(
            allocation.clone(),
            node_id,
            payment_triple.to_string(),
            address,
        )
        .await
    {
        Ok(allocation_id) => match dao.get(allocation_id, node_id).await {
            Ok(AllocationStatus::Active(allocation)) => {
                let allocation_id = allocation.allocation_id.clone();

                release_allocation_after(db.clone(), allocation_id, allocation.timeout, node_id)
                    .await;

                response::created(allocation)
            }
            Ok(AllocationStatus::NotFound) => {
                api_error::server_error(&allocation, &"Database Error")
            }
            Ok(AllocationStatus::Gone) => api_error::server_error(&allocation, &"Database Error"),
            Err(e) => api_error::server_error(&allocation, &e.to_string()),
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
    match dao
        .get_for_owner(node_id, after_timestamp, max_items, Some(false))
        .await
    {
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

    let deposit = match (old_allocation.deposit.clone(), update.deposit) {
        (Some(deposit), None) => Some(deposit),
        (Some(mut deposit), Some(deposit_update)) => {
            deposit.validate = deposit_update.validate;
            Some(deposit)
        }
        (None, None) => None,
        (None, Some(_deposit_update)) => {
            return Err("Cannot update deposit of an allocation created without one");
        }
    };

    Ok(Allocation {
        total_amount,
        remaining_amount,
        timeout: update.timeout.or(old_allocation.timeout),
        deposit,
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
    let allocation_update: AllocationUpdate = body.into_inner();
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
        match amend_allocation_fields(current_allocation.clone(), allocation_update.clone()) {
            Ok(allocation) => allocation,
            Err(e) => return response::bad_request(&e),
        };

    let payment_triple = amended_allocation.payment_platform.clone();

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
        timeout: amended_allocation.timeout,
        deposit: amended_allocation.deposit.clone(),
        new_allocation: false,
    };
    match async move { Ok(bus::service(LOCAL_SERVICE).send(validate_msg).await??) }.await {
        Ok(result) => {
            if let Some(error_response) = api_error::try_from_validation(
                result,
                &allocation_update,
                payment_triple.to_string(),
                amended_allocation.address.clone(),
            ) {
                return error_response;
            }
        }
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
            ValidateAllocationError::AccountNotRegistered,
        ))) => {
            log::error!(
                "Account {} not registered on platform {}",
                amended_allocation.address,
                payment_triple
            );

            return api_error::account_not_registered(
                &allocation_update,
                payment_triple.to_string(),
                amended_allocation.address.clone(),
            );
        }
        Err(e) => return api_error::server_error(&allocation_update, &e.to_string()),
    }

    match dao.replace(amended_allocation, node_id).await {
        Ok(true) => {}
        Ok(false) => {
            return api_error::server_error(
                &allocation_update,
                &"Allocation not present despite preconditions being already ensured",
            );
        }
        Err(e) => return api_error::server_error(&allocation_update, &e.to_string()),
    }

    get_allocation(db, path, id).await
}

async fn release_allocation(
    db: Data<DbExecutor>,
    path: Path<params::AllocationId>,
    id: Identity,
) -> HttpResponse {
    let allocation_id = path.allocation_id.clone();
    let node_id = id.identity;
    let dao = db.as_dao::<AllocationDao>();

    match dao.release(allocation_id.clone(), node_id).await {
        Ok(AllocationReleaseStatus::Released { deposit, platform }) => {
            if let Some(deposit) = deposit {
                let release_result = bus::service(LOCAL_SERVICE)
                    .send(ReleaseDeposit {
                        from: id.identity.to_string(),
                        deposit_id: deposit.id,
                        deposit_contract: deposit.contract,
                        platform,
                    })
                    .await;
                match release_result {
                    Ok(Ok(_)) => response::ok(Null),
                    Err(e) => response::server_error(&e),
                    Ok(Err(e)) => response::server_error(&e),
                }
            } else {
                response::ok(Null)
            }
        }
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

    // Populate payment platform properties / constraint.
    let mut properties: Vec<MarketProperty> = allocations
        .iter()
        .map(|allocation| MarketProperty {
            key: format!(
                "golem.com.payment.platform.{}.address",
                allocation.payment_platform
            ),
            value: allocation.address.clone(),
        })
        .collect();
    let platform_clause = Constraints::new_clause(
        ClauseOperator::Or,
        properties
            .iter()
            .map(|property| {
                ConstraintKey::new(property.key.as_str()).equal_to(ConstraintKey::new("*"))
            })
            .collect(),
    );

    // Populate payment protocol version property / constraint.
    properties.push(MarketProperty {
        key: "golem.com.payment.protocol.version".into(),
        value: "3".into(),
    });

    // Validating payments from deposit contracts requires a new version
    // of the erc20 driver, so we determine the required version based
    // on any allocations using deposits.
    let required_protocol_ver = if allocations
        .iter()
        .any(|allocation| allocation.deposit.is_some())
    {
        3
    } else {
        2
    };

    let protocol_clause = Constraints::new_single(
        // >= constraint is not supported so we use > with decremented value
        ConstraintKey::new("golem.com.payment.protocol.version")
            .greater_than(ConstraintKey::new(required_protocol_ver - 1)),
    );

    let constraints = vec![platform_clause.to_string(), protocol_clause.to_string()];
    response::ok(MarketDecoration {
        properties,
        constraints,
    })
}

pub async fn release_allocation_after(
    db: Data<DbExecutor>,
    allocation_id: String,
    allocation_timeout: Option<DateTime<Utc>>,
    node_id: NodeId,
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
    node_id: NodeId,
) {
    match db
        .as_dao::<AllocationDao>()
        .release(allocation_id.clone(), node_id)
        .await
    {
        Ok(AllocationReleaseStatus::Released { deposit, platform }) => {
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

async fn get_pay_allocation_orders(
    db: Data<DbExecutor>,
    path: Path<String>,
    id: Identity,
) -> HttpResponse {
    let node_id = id.identity;
    let allocation_id = path.into_inner();
    let dao: BatchDao = db.as_dao();
    match dao
        .get_batch_items(node_id, None, None, Some(allocation_id), None, None)
        .await
    {
        Ok(items) => response::ok(items),
        Err(e) => response::server_error(&e),
    }
}
