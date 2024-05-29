use std::collections::BTreeMap;
use std::convert::TryInto;
use std::fmt::Display;
use std::str::FromStr;
use std::time::Duration;
// External crates
use actix_web::web::{delete, get, post, put, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use http::Uri;
use problem_details::ProblemDetails;
use serde::Serialize;
use serde_json::value::Value::Null;
use ya_client_model::NodeId;

// Workspace uses
use ya_agreement_utils::{ClauseOperator, ConstraintKey, Constraints};
use ya_client_model::payment::allocation::{PaymentPlatform, PaymentPlatformEnum};
use ya_client_model::payment::*;
use ya_core_model::driver::ValidateAllocationResult;
use ya_core_model::payment::local::{
    get_token_from_network_name, DriverName, NetworkName, ReleaseDeposit, ValidateAllocation,
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

mod token_name {
    use super::*;

    pub struct TokenName(String);

    impl Display for TokenName {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    impl TokenName {
        pub fn default(_driver: &DriverName, network: &NetworkName) -> TokenName {
            Self(get_token_from_network_name(network).to_lowercase())
        }

        pub fn from_token_string(
            driver: &DriverName,
            network: &NetworkName,
            token: &str,
        ) -> Result<Self, String> {
            if token == "GLM" || token == "tGLM" {
                return Err(format!(
                    "Uppercase token names are not supported. Use lowercase glm or tglm instead of {}",
                    token
                ));
            }
            let token_expected = Self::default(driver, network).to_string();
            if token != token_expected {
                return Err(format!(
                    "Token {} does not match expected token {} for driver {} and network {}. \
            Note that for test networks expected token name is tglm and for production networks it is glm",
                    token, token_expected, driver, network
                ));
            }
            Ok(Self(token.to_string()))
        }
    }
}

mod platform_triple {
    use super::token_name::TokenName;
    use super::*;
    use anyhow::{anyhow, bail};

    pub struct PaymentPlatformTriple {
        driver: DriverName,
        network: NetworkName,
        token: TokenName,
    }

    impl PaymentPlatformTriple {
        pub fn driver(&self) -> &DriverName {
            &self.driver
        }

        pub fn network(&self) -> &NetworkName {
            &self.network
        }

        pub fn token(&self) -> &TokenName {
            &self.token
        }

        pub fn default_testnet() -> Self {
            PaymentPlatformTriple {
                driver: DEFAULT_PAYMENT_DRIVER,
                network: DEFAULT_TESTNET_NETWORK,
                token: TokenName::default(&DEFAULT_PAYMENT_DRIVER, &DEFAULT_TESTNET_NETWORK),
            }
        }

        pub fn default_mainnet() -> Self {
            PaymentPlatformTriple {
                driver: DEFAULT_PAYMENT_DRIVER,
                network: DEFAULT_MAINNET_NETWORK,
                token: TokenName::default(&DEFAULT_PAYMENT_DRIVER, &DEFAULT_MAINNET_NETWORK),
            }
        }

        pub fn from_payment_platform_input(
            p: &PaymentPlatform,
        ) -> anyhow::Result<PaymentPlatformTriple> {
            let platform = if p.driver.is_none() && p.network.is_none() && p.token.is_none() {
                let default_platform = Self::default_testnet();
                log::debug!("Empty paymentPlatform object, using {default_platform}");
                default_platform
            } else if p.token.is_some() && p.network.is_none() && p.driver.is_none() {
                let token = p.token.as_ref().unwrap();
                if token == "GLM" || token == "tGLM" {
                    bail!(
                        "Uppercase token names are not supported. Use lowercase glm or tglm instead of {}",
                        token
                    );
                } else if token == "glm" {
                    let default_platform = Self::default_mainnet();
                    log::debug!("Selected network {default_platform} (default for glm token)");
                    default_platform
                } else if token == "tglm" {
                    let default_platform = Self::default_testnet();
                    log::debug!("Selected network {default_platform} (default for tglm token)");
                    default_platform
                } else {
                    bail!("Only glm or tglm token values are accepted vs {token} provided");
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
                    .map_err(|err| anyhow!("Validate network failed (1): {err}"))?;

                let driver_str = p.driver.as_deref().unwrap_or_else(|| {
                    log::debug!(
                        "Driver not specified, using default {}",
                        DEFAULT_PAYMENT_DRIVER
                    );
                    DEFAULT_PAYMENT_DRIVER.into()
                });
                let driver = validate_driver(&network, driver_str)
                    .map_err(|err| anyhow!("Validate driver failed (1): {err}"))?;

                if let Some(token) = p.token.as_ref() {
                    let token = TokenName::from_token_string(&driver, &network, token)
                        .map_err(|err| anyhow!("Validate token failed (1): {err}"))?;
                    log::debug!("Selected network {}-{}-{}", driver, network, token);
                    Self {
                        driver,
                        network,
                        token,
                    }
                } else {
                    let default_token = TokenName::default(&driver, &network);

                    log::debug!(
                        "Selected network with default token {}-{}-{}",
                        driver,
                        network,
                        default_token
                    );
                    Self {
                        driver,
                        network,
                        token: default_token,
                    }
                }
            };
            Ok(platform)
        }

        pub fn from_payment_platform_str(
            payment_platform_str: &str,
        ) -> anyhow::Result<PaymentPlatformTriple> {
            // payment_platform is of the form driver-network-token
            // eg. erc20-rinkeby-tglm
            let [driver_str, network_str, token_str]: [&str; 3] = payment_platform_str
                .split('-')
                .collect::<Vec<_>>()
                .try_into()
                .map_err(|err| {
                    anyhow!(
                        "paymentPlatform must be of the form driver-network-token instead of {}",
                        payment_platform_str
                    )
                })?;

            let network = validate_network(network_str)
                .map_err(|err| anyhow!("Validate network failed (2): {err}"))?;

            let driver = validate_driver(&network, driver_str)
                .map_err(|err| anyhow!("Validate driver failed (2): {err}"))?;

            let token = TokenName::from_token_string(&driver, &network, token_str)
                .map_err(|err| anyhow!("Validate token failed (2): {err}"))?;

            Ok(Self {
                driver,
                network,
                token,
            })
        }
    }

    impl Display for PaymentPlatformTriple {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}-{}-{}", self.driver, self.network, self.token)
        }
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
}

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
}

fn validate_allocation_result_to_problem_details(
    result: ValidateAllocationResult,
    request_body: &impl Serialize,
    payment_triple: String,
    address: String,
) -> Option<ProblemDetails<BTreeMap<String, serde_json::Value>>> {
    use serde_json::Value;

    let mut extensions = BTreeMap::new();

    extensions.insert(
        "requestBody".to_string(),
        serde_json::to_value(request_body).unwrap_or(Value::String(
            "[requestBody serialization failed]".to_string(),
        )),
    );

    extensions.insert(
        "paymentPlatform".to_string(),
        Value::String(payment_triple.clone()),
    );

    extensions.insert("address".to_string(), Value::String(address));

    let details = ProblemDetails::new();
    let details = match result {
        ValidateAllocationResult::Valid => return None,
        ValidateAllocationResult::InsufficientAccountFunds {
            requested_funds,
            available_funds,
            reserved_funds,
        } => {
            let detail = format!("Insufficient funds to create the allocation. Top up your account \
                or release all existing allocations to unlock the funds via `yagna payment release-allocations`");

            extensions.insert(
                "requestedFunds".to_string(),
                Value::String(requested_funds.to_string()),
            );
            extensions.insert(
                "availableFunds".to_string(),
                Value::String(available_funds.to_string()),
            );
            extensions.insert(
                "reservedFunds".to_string(),
                Value::String(reserved_funds.to_string()),
            );

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/insufficient-account-funds",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::InsufficientDepositFunds {
            requested_funds,
            available_funds,
        } => {
            let detail = "Insufficient funds on the deposit for this allocation";

            extensions.insert(
                "requestedFunds".to_string(),
                Value::String(requested_funds.to_string()),
            );
            extensions.insert(
                "availableFunds".to_string(),
                Value::String(available_funds.to_string()),
            );

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/insufficient-deposit-funds",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::TimeoutExceedsDeposit {
            requested_timeout,
            deposit_timeout,
        } => {
            let detail = "Requested allocation timeout either not set or exceeds deposit timeout";

            extensions.insert(
                "requestedTimeout".to_string(),
                match requested_timeout {
                    Some(ts) => Value::String(ts.to_rfc3339()),
                    None => Value::Null,
                },
            );
            extensions.insert(
                "depositTimeout".to_string(),
                Value::String(deposit_timeout.to_rfc3339()),
            );

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/timeout-exceeds-deposit",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::TimeoutPassed { requested_timeout } => {
            let detail = "Requested allocation timeout is in the past";

            extensions.insert(
                "requestedTimeout".to_string(),
                Value::String(requested_timeout.to_rfc3339()),
            );

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/timeout-passed",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::MalformedDepositContract => {
            let detail = "Invalid deposit contract address";

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/malformed-deposit-contract",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::MalformedDepositId => {
            let detail = "Invalid deposit id";

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/malformed-deposit-id",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::DepositReused { allocation_id } => {
            let detail = format!(
                "Submitted deposit already has a corresponding allocation {allocation_id}. \
                Consider amending the allocation if the deposit has been extended"
            );

            extensions.insert(
                "conflictingAllocationId".to_string(),
                Value::String(allocation_id),
            );

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/deposit-reused",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::DepositSpenderMismatch { deposit_spender } => {
            let detail = "Deposit spender doesn't match allocation address";

            extensions.insert("depositSpender".to_string(), Value::String(deposit_spender));

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/deposit-spender-mismatch",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::DepositValidationError(message) => {
            let detail = format!("Deposit contract rejected the deposit: {message}");

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation/deposit-validation-error",
                ))
                .with_detail(detail)
        }
    };

    Some(details.with_extensions(extensions))
}

async fn create_allocation(
    db: Data<DbExecutor>,
    body: Json<NewAllocation>,
    id: Identity,
) -> HttpResponse {
    let bad_req_and_log = |err_msg: String| -> HttpResponse {
        log::error!("{}", err_msg);
        response::bad_request(&err_msg)
    };

    // TODO: Handle deposits & timeouts
    let allocation = body.into_inner();
    let node_id = id.identity;

    let payment_triple = match &allocation.payment_platform {
        Some(PaymentPlatformEnum::PaymentPlatformName(name)) => {
            let payment_platform = match PaymentPlatformTriple::from_payment_platform_str(name) {
                Ok(p) => p,
                Err(err) => return bad_req_and_log(format!("{}", err)),
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
                Err(err) => return bad_req_and_log(format!("{}", err)),
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
        return bad_req_and_log(format!("Failed to init account: {err}"));
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
            if let Some(problem_details) = validate_allocation_result_to_problem_details(
                result,
                &allocation,
                payment_triple.to_string(),
                address.clone(),
            ) {
                log::error!(
                    "{}",
                    problem_details
                        .detail
                        .as_deref()
                        .unwrap_or("[allocation validation error with no detail]")
                );

                return HttpResponse::BadRequest().json(problem_details);
            }
        }
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
            ValidateAllocationError::AccountNotRegistered,
        ))) => {
            return bad_req_and_log(format!(
                "Account not registered - payment platform {payment_triple}"
            ));
        }
        Err(e) => return response::server_error(&e),
    }

    let dao = db.as_dao::<AllocationDao>();

    match dao
        .create(allocation, node_id, payment_triple.to_string(), address)
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
    let bad_req_and_log = |err_msg: String| -> HttpResponse {
        log::error!("{}", err_msg);
        response::bad_request(&err_msg)
    };

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
            if let Some(problem_details) = validate_allocation_result_to_problem_details(
                result,
                &allocation_update,
                payment_triple.to_string(),
                amended_allocation.address.clone(),
            ) {
                log::error!(
                    "{}",
                    problem_details
                        .detail
                        .as_deref()
                        .unwrap_or("[allocation validation error with no detail]")
                );

                return HttpResponse::BadRequest().json(problem_details);
            }
        }
        Err(Error::Rpc(RpcMessageError::ValidateAllocation(
            ValidateAllocationError::AccountNotRegistered,
        ))) => {
            return bad_req_and_log(format!(
                "Account not registered - payment platform {payment_triple}"
            ));
        }
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
