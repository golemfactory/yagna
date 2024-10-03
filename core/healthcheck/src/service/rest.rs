use actix_web::{HttpResponse, Responder};
use chrono::Utc;
use serde_json::json;

use std::time::Duration;

use ya_core_model::market::local::BUS_ID as MARKET_BUS_ID;
use ya_core_model::net::local::BUS_ID as NET_BUS_ID;
use ya_core_model::payment::local::BUS_ID as PAYMENT_BUS_ID;

use ya_core_model::market::GetLastBcastTs;
use ya_core_model::net::local::ListNeighbours;
use ya_core_model::payment::local::{PaymentDriverStatus, PaymentDriverStatusError};

use ya_service_bus::{timeout::IntoTimeoutFuture, typed::service, RpcEndpoint};

pub const HEALTHCHECK_API_PATH: &str = "/healthcheck";

pub fn web_scope() -> actix_web::Scope {
    actix_web::web::scope(HEALTHCHECK_API_PATH).service(healthcheck)
}

async fn payment_healthcheck() -> Result<(), HttpResponse> {
    let result = service(PAYMENT_BUS_ID)
        .call(PaymentDriverStatus {
            driver: None,
            network: None,
        })
        .timeout(Some(Duration::from_secs(5)))
        .await;

    let result = match result {
        Ok(ok) => ok,
        Err(_elapsed) => return Err(errors::internal("internal-timeout", "payments-check")),
    };

    let result = match result {
        Ok(resp) => resp,
        Err(gsb_err) => {
            log::warn!("Healtcheck failed due to {gsb_err}");
            return Err(errors::internal("gsb-error", "payments-check"));
        }
    };

    let status_properties = match result {
        Ok(props) => props,
        Err(
            payment_err @ (PaymentDriverStatusError::NoDriver(_)
            | PaymentDriverStatusError::NoNetwork(_)
            | PaymentDriverStatusError::Internal(_)),
        ) => {
            log::warn!("Healtcheck failed due to {payment_err}");
            return Err(errors::internal("payments-service-error", "payments-check"));
        }
    };

    if !status_properties.is_empty() {
        return Err(errors::payments(status_properties));
    }

    Ok(())
}

async fn relay_healtcheck() -> Result<(), HttpResponse> {
    let result = service(NET_BUS_ID)
        .send(ListNeighbours { size: 8 })
        .timeout(Some(Duration::from_secs(5)))
        .await;

    let result = match result {
        Ok(ok) => ok,
        Err(_elapsed) => return Err(errors::internal("internal-timeout", "relay-check")),
    };

    let result = match result {
        Ok(ok) => ok,
        Err(gsb_err) => {
            log::warn!("Healtcheck failed due to {gsb_err}");
            return Err(errors::internal("gsb-error", "relay-check"));
        }
    };

    let _gsb_remote_ping = match result {
        Ok(ok) => ok,
        Err(net_err) => {
            log::warn!("Healtcheck failed due to {net_err}");
            return Err(errors::internal("net-service-error", "relay-check"));
        }
    };

    Ok(())
}

async fn market_healthcheck() -> Result<(), HttpResponse> {
    let result = service(MARKET_BUS_ID)
        .call(GetLastBcastTs)
        .timeout(Some(Duration::from_secs(5)))
        .await;

    let result = match result {
        Ok(ok) => ok,
        Err(_elapsed) => return Err(errors::internal("internal-timeout", "market-check")),
    };

    let result = match result {
        Ok(ok) => ok,
        Err(gsb_err) => {
            log::warn!("Healtcheck failed due to {gsb_err}");
            return Err(errors::internal("gsb-error", "market-check"));
        }
    };

    let bcast_ts = match result {
        Ok(ok) => ok,
        Err(market_err) => {
            log::warn!("Healtcheck failed due to {market_err}");
            return Err(errors::internal("market-service-error", "market-check"));
        }
    };

    let last_bcast_age = Utc::now() - bcast_ts;
    if last_bcast_age > chrono::Duration::minutes(2) {
        return Err(errors::market_bcast_timeout(last_bcast_age));
    }

    Ok(())
}

#[actix_web::get("")]
async fn healthcheck() -> impl Responder {
    if let Err(response) = payment_healthcheck().await {
        return response;
    }
    if let Err(response) = relay_healtcheck().await {
        return response;
    }
    if let Err(response) = market_healthcheck().await {
        return response;
    }

    HttpResponse::Ok().json(json!({"status": "ok"}))
}

mod errors {
    use actix_web::HttpResponse;
    use http::Uri;
    use problem_details::ProblemDetails;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::iter::FromIterator;
    use std::str::FromStr;
    use ya_client::model::payment::DriverStatusProperty;

    const CONTENT_TYPE_PROBLEM_JSON: (&str, &str) = ("Content-Type", "application/problem+json");

    pub fn internal(instance: &str, step: &str) -> HttpResponse {
        let extensions = HashMap::<String, String>::from_iter(std::iter::once((
            "step".to_string(),
            step.to_string(),
        )));

        let problem = ProblemDetails::new()
            .with_type(Uri::from_static("/healthcheck/internal-error"))
            .with_instance(
                Uri::from_str(&format!("/healthcheck/internal-error/{instance}",))
                    .expect("Invalid URI"),
            )
            .with_extensions(extensions);

        HttpResponse::InternalServerError()
            .insert_header(CONTENT_TYPE_PROBLEM_JSON)
            .json(problem)
    }

    pub fn payments(props: Vec<DriverStatusProperty>) -> HttpResponse {
        let extensions = HashMap::<String, Value>::from_iter([
            (
                "step".to_string(),
                Value::String("payments-check".to_string()),
            ),
            (
                "problems".to_string(),
                Value::Array(
                    props
                        .into_iter()
                        .map(|prop| match prop {
                            DriverStatusProperty::CantSign { .. } => "Can't sign transaction",
                            DriverStatusProperty::InsufficientGas { .. } => "Insufficient gas",
                            DriverStatusProperty::InsufficientToken { .. } => "Insufficient token",
                            DriverStatusProperty::InvalidChainId { .. } => "Misconfigured chain",
                            DriverStatusProperty::RpcError { .. } => "Persistent RPC issues",
                            DriverStatusProperty::TxStuck { .. } => "Stuck transaction",
                        })
                        .map(ToOwned::to_owned)
                        .map(Value::String)
                        .collect(),
                ),
            ),
        ]);

        let problem = ProblemDetails::new()
            .with_detail("One on more issues blocking the operation of payments have been detected. Run `yagna payment driver status` to diagnose")
            .with_type(Uri::from_static("/healthcheck/payment-driver-errors"))
            .with_instance(Uri::from_static("/healthcheck/payment-driver-errors"))
            .with_extensions(extensions);

        HttpResponse::InternalServerError()
            .insert_header(CONTENT_TYPE_PROBLEM_JSON)
            .json(problem)
    }

    pub fn market_bcast_timeout(last_bcast_age: chrono::Duration) -> HttpResponse {
        let extensions = HashMap::<String, Value>::from_iter([
            (
                "step".to_string(),
                Value::String("market-check".to_string()),
            ),
            (
                "lastBcastAgeSecs".to_string(),
                Value::Number(last_bcast_age.num_seconds().into()),
            ),
        ]);

        let problem = ProblemDetails::new()
            .with_detail("Last received market broadcast is too old")
            .with_type(Uri::from_static("/healthcheck/market-bcast-timeout"))
            .with_instance(Uri::from_static("/healthcheck/market-bcast-timeout"))
            .with_extensions(extensions);

        HttpResponse::InternalServerError()
            .insert_header(CONTENT_TYPE_PROBLEM_JSON)
            .json(problem)
    }
}
