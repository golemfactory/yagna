use std::convert::TryInto;
use std::str::FromStr;
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
use ya_client_model::payment::allocation::{PaymentPlatform, PaymentPlatformEnum};
use ya_client_model::payment::*;
use ya_core_model::payment::local::{
    get_token_from_network_name, DriverName, NetworkName, ValidateAllocation,
    ValidateAllocationError, BUS_ID as LOCAL_SERVICE,
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

const DEFAULT_TESTNET_NETWORK: NetworkName = NetworkName::Holesky;
const DEFAULT_MAINNET_NETWORK: NetworkName = NetworkName::Polygon;
const DEFAULT_PAYMENT_DRIVER: DriverName = DriverName::Erc20;

fn default_payment_platform_testnet() -> String {
    format!(
        "{}-{}-{}",
        DEFAULT_PAYMENT_DRIVER,
        DEFAULT_TESTNET_NETWORK,
        get_default_token(&DEFAULT_PAYMENT_DRIVER, &DEFAULT_TESTNET_NETWORK)
    )
}

fn default_payment_platform_mainnet() -> String {
    format!(
        "{}-{}-{}",
        DEFAULT_PAYMENT_DRIVER,
        DEFAULT_MAINNET_NETWORK,
        get_default_token(&DEFAULT_PAYMENT_DRIVER, &DEFAULT_MAINNET_NETWORK)
    )
}

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

fn validate_network(network: &str) -> Result<NetworkName, String> {
    match NetworkName::from_str(network) {
        Ok(NetworkName::Rinkeby) => Err("Rinkeby is no longer supported".to_string()),
        Ok(network_name) => Ok(network_name),
        Err(_) => Err(format!("Invalid network name: {network}")),
    }
}

fn validate_driver(network: &NetworkName, driver: &str) -> Result<DriverName, String> {
    match DriverName::from_str(driver) {
        Err(_) => Err(format!("Invalid driver name {}", driver)),
        Ok(driver_name) => Ok(driver_name),
    }
}

fn get_default_token(_driver: &DriverName, network: &NetworkName) -> String {
    get_token_from_network_name(network).to_lowercase()
}

fn validate_token(driver: &DriverName, network: &NetworkName, token: &str) -> Result<(), String> {
    if token == "GLM" || token == "tGLM" {
        return Err(format!(
            "Uppercase token names are not supported. Use lowercase glm or tglm instead of {}",
            token
        ));
    }
    let token_expected = get_default_token(driver, network);
    if token != token_expected {
        return Err(format!(
            "Token {} does not match expected token {} for driver {} and network {}. \
            Note that for test networks expected token name is tglm and for production networks it is glm",
            token, token_expected, driver, network
        ));
    }
    Ok(())
}

fn bad_req_and_log(err_msg: String) -> HttpResponse {
    log::error!("{}", err_msg);
    response::bad_request(&err_msg)
}

fn payment_platform_to_string(p: &PaymentPlatform) -> Result<String, HttpResponse> {
    let platform_str = if p.driver.is_none() && p.network.is_none() && p.token.is_none() {
        let default_platform = default_payment_platform_testnet();
        log::debug!("Empty paymentPlatform object, using {default_platform}");
        default_platform
    } else if p.token.is_some() && p.network.is_none() && p.driver.is_none() {
        let token = p.token.as_ref().unwrap();
        if token == "GLM" || token == "tGLM" {
            return Err(bad_req_and_log(format!(
                "Uppercase token names are not supported. Use lowercase glm or tglm instead of {}",
                token
            )));
        } else if token == "glm" {
            let default_platform = default_payment_platform_mainnet();
            log::debug!("Selected network {default_platform} (default for glm token)");
            default_platform
        } else if token == "tglm" {
            let default_platform = default_payment_platform_testnet();
            log::debug!("Selected network {default_platform} (default for tglm token)");
            default_platform
        } else {
            return Err(bad_req_and_log(format!(
                "Only glm or tglm token values are accepted vs {token} provided"
            )));
        }
    } else {
        let network_str = p.network.as_deref().unwrap_or_else(|| {
            if let Some(token) = p.token.as_ref() {
                if token == "glm" {
                    log::debug!(
                        "Network not specified, using default {}, because token set to glm",
                        DEFAULT_MAINNET_NETWORK
                    );
                    DEFAULT_MAINNET_NETWORK.into()
                } else {
                    log::debug!(
                        "Network not specified, using default {}",
                        DEFAULT_TESTNET_NETWORK
                    );
                    DEFAULT_TESTNET_NETWORK.into()
                }
            } else {
                log::debug!(
                    "Network not specified and token not specified, using default {}",
                    DEFAULT_TESTNET_NETWORK
                );
                DEFAULT_TESTNET_NETWORK.into()
            }
        });
        let network = validate_network(network_str)
            .map_err(|err| bad_req_and_log(format!("Validate network failed (1): {err}")))?;

        let driver_str = p.driver.as_deref().unwrap_or_else(|| {
            log::debug!(
                "Driver not specified, using default {}",
                DEFAULT_PAYMENT_DRIVER
            );
            DEFAULT_PAYMENT_DRIVER.into()
        });
        let driver = validate_driver(&network, driver_str)
            .map_err(|err| bad_req_and_log(format!("Validate driver failed (1): {err}")))?;

        if let Some(token) = p.token.as_ref() {
            validate_token(&driver, &network, token)
                .map_err(|err| bad_req_and_log(format!("Validate token failed (1): {err}")))?;
            log::debug!("Selected network {}-{}-{}", driver, network, token);
            format!("{}-{}-{}", driver, network, token)
        } else {
            let default_token = get_default_token(&driver, &network);

            log::debug!(
                "Selected network with default token {}-{}-{}",
                driver,
                network,
                default_token
            );
            format!("{}-{}-{}", driver, network, default_token)
        }
    };
    Ok(platform_str)
}

fn payment_platform_validate_and_check(
    payment_platform_str: &str,
) -> Result<(DriverName, NetworkName, String), HttpResponse> {
    // payment_platform is of the form driver-network-token
    // eg. erc20-rinkeby-tglm
    let [driver_str, network_str, token_str]: [&str; 3] = payment_platform_str
        .split('-')
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|err| {
            bad_req_and_log(format!(
                "paymentPlatform must be of the form driver-network-token instead of {}",
                payment_platform_str
            ))
        })?;

    let network = validate_network(network_str)
        .map_err(|err| bad_req_and_log(format!("Validate network failed (2): {err}")))?;

    let driver = validate_driver(&network, driver_str)
        .map_err(|err| bad_req_and_log(format!("Validate driver failed (2): {err}")))?;

    validate_token(&driver, &network, token_str)
        .map_err(|err| bad_req_and_log(format!("Validate token failed (2): {err}")))?;

    Ok((driver, network, token_str.to_string()))
}

async fn create_allocation(
    db: Data<DbExecutor>,
    body: Json<NewAllocation>,
    id: Identity,
) -> HttpResponse {
    // TODO: Handle deposits & timeouts
    let allocation = body.into_inner();
    let node_id = id.identity;

    let payment_platform = match &allocation.payment_platform {
        Some(PaymentPlatformEnum::PaymentPlatformName(name)) => {
            log::debug!("Using old API payment platform name as pure str: {}", name);
            name.clone()
        }
        Some(PaymentPlatformEnum::PaymentPlatform(p)) => match payment_platform_to_string(p) {
            Ok(platform_str) => platform_str,
            Err(e) => return e,
        },
        None => {
            let default_platform = default_payment_platform_testnet();
            log::debug!("No paymentPlatform entry found, using {default_platform}");
            default_platform
        }
    };

    let address = allocation
        .address
        .clone()
        .unwrap_or_else(|| node_id.to_string());

    // payment_platform is of the form driver-network-token
    // This function rechecks depending on the flow, but the check is cheap and also counts as sanity check
    let (driver, network, token_str) = match payment_platform_validate_and_check(&payment_platform)
    {
        Ok((driver, network, token_str)) => (driver, network, token_str),
        Err(e) => return e,
    };

    log::info!(
        "Creating allocation for payment platform: {}-{}-{}",
        driver,
        network,
        token_str
    );

    let acc = Account {
        driver: driver.to_string(),
        address: address.clone(),
        network: Some(network.to_string()),
        token: None,
        send: true,
        receive: false,
    };

    if let Err(err) = init_account(acc).await {
        return bad_req_and_log(format!("Failed to init account: {err}"));
    }

    let validate_msg = ValidateAllocation {
        platform: payment_platform.clone(),
        address: address.clone(),
        amount: allocation.total_amount.clone(),
    };

    match async move { Ok(bus::service(LOCAL_SERVICE).send(validate_msg).await??) }.await {
        Ok(true) => {}
        Ok(false) => {
            return bad_req_and_log(format!("Insufficient funds to make allocation for payment platform {payment_platform}. \
             Top up your account or release all existing allocations to unlock the funds via `yagna payment release-allocations`"));
        }
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
            ValidateAllocationError::AccountNotRegistered,
        ))) => {
            return bad_req_and_log(format!(
                "Account not registered - payment platform {payment_platform}"
            ));
        }
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
        .chain(std::iter::once(MarketProperty {
            key: "golem.com.payment.protocol.version".into(),
            value: "2".into(),
        }))
        .collect();
    let constraints = properties
        .iter()
        .map(|property| ConstraintKey::new(property.key.as_str()).equal_to(ConstraintKey::new("*")))
        .chain(std::iter::once(
            ConstraintKey::new("golem.com.payment.protocol.version")
                .greater_than(ConstraintKey::new(1)),
        ))
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
