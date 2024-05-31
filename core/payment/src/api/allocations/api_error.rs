use std::collections::BTreeMap;

use actix_web::HttpResponse;
use http::Uri;
use problem_details::ProblemDetails;
use serde::Serialize;
use serde_json::Value;
use ya_core_model::driver::ValidateAllocationResult;

pub type PaymentProblemDetails = ProblemDetails<BTreeMap<String, Value>>;

const CONTENT_TYPE_PROBLEM_JSON: (&str, &str) = ("Content-Type", "application/problem+json");

pub fn try_from_validation(
    result: ValidateAllocationResult,
    request_body: &impl Serialize,
    payment_triple: String,
    address: String,
) -> Option<HttpResponse> {
    let mut extensions = BTreeMap::new();

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
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/insufficient-account-funds",
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
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/insufficient-deposit-funds",
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
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/timeout-exceeds-deposit",
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
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/timeout-passed",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::MalformedDepositContract => {
            let detail = "Invalid deposit contract address";

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/malformed-deposit-contract",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::MalformedDepositId => {
            let detail = "Invalid deposit id";

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/malformed-deposit-id",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::NoDeposit { deposit_id } => {
            let detail = format!("Deposit doesn't exist under the given id {deposit_id}");

            extensions.insert("requestedDepositId".to_string(), Value::String(deposit_id));

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/no-deposit",
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
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/deposit-reused",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::DepositSpenderMismatch { deposit_spender } => {
            let detail = format!(
                "Deposit spender {deposit_spender} doesn't match allocation address {address}"
            );

            extensions.insert("depositSpender".to_string(), Value::String(deposit_spender));

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/deposit-spender-mismatch",
                ))
                .with_detail(detail)
        }
        ValidateAllocationResult::DepositValidationError(message) => {
            let detail = format!("Deposit contract rejected the deposit: {message}");

            details
                .with_type(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error",
                ))
                .with_instance(Uri::from_static(
                    "/payment-api/v1/allocations/validation-error/deposit-validation-error",
                ))
                .with_detail(detail)
        }
    };

    log::error!(
        "{}",
        details.detail.as_deref().unwrap_or("missing error detail")
    );

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

    Some(
        HttpResponse::BadRequest()
            .insert_header(CONTENT_TYPE_PROBLEM_JSON)
            .json(details.with_extensions(extensions)),
    )
}

pub fn account_not_registered(
    request_body: &impl Serialize,
    payment_triple: String,
    address: String,
) -> HttpResponse {
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

    extensions.insert("address".to_string(), Value::String(address.clone()));

    let details = ProblemDetails::new()
        .with_type(Uri::from_static(
            "/payment-api/v1/allocations/account-not-registered",
        ))
        .with_detail(format!(
            "Account {address} not registered for platform {payment_triple}"
        ))
        .with_extensions(extensions);

    HttpResponse::InternalServerError()
        .insert_header(CONTENT_TYPE_PROBLEM_JSON)
        .json(details)
}

pub fn bad_platform_parameter(
    request_body: &impl Serialize,
    error: &impl Serialize,
    requested_payment_platform: &impl Serialize,
) -> HttpResponse {
    let mut extensions = BTreeMap::new();

    extensions.insert(
        "requestBody".to_string(),
        serde_json::to_value(request_body).unwrap_or(Value::String(
            "[requestBody serialization failed]".to_string(),
        )),
    );

    extensions.insert(
        "parseError".to_string(),
        serde_json::to_value(error).unwrap_or(Value::String(
            "[parseError serialization failed]".to_string(),
        )),
    );

    extensions.insert(
        "requestedPaymentPlatform".to_string(),
        serde_json::to_value(requested_payment_platform).unwrap_or(Value::String(
            "[requestedPaymentPlatform serialization failed]".to_string(),
        )),
    );

    let details = ProblemDetails::new()
        .with_type(Uri::from_static(
            "/payment-api/v1/allocations/bad-payment-platform",
        ))
        .with_detail(format!("Payment platform doesn't parse"))
        .with_extensions(extensions);

    HttpResponse::BadRequest()
        .insert_header(CONTENT_TYPE_PROBLEM_JSON)
        .json(details)
}

pub fn server_error(request_body: &impl Serialize, error: &impl Serialize) -> HttpResponse {
    let mut extensions = BTreeMap::new();

    extensions.insert(
        "requestBody".to_string(),
        serde_json::to_value(request_body).unwrap_or(Value::String(
            "[requestBody serialization failed]".to_string(),
        )),
    );

    extensions.insert(
        "internalError".to_string(),
        serde_json::to_value(error).unwrap_or(Value::String(
            "[internalError serialization failed]".to_string(),
        )),
    );

    let details = ProblemDetails::new()
        .with_type(Uri::from_static(
            "/payment-api/v1/allocations/internal-error",
        ))
        .with_detail(format!("Unhandled internal error"))
        .with_extensions(extensions);

    HttpResponse::InternalServerError()
        .insert_header(CONTENT_TYPE_PROBLEM_JSON)
        .json(details)
}
