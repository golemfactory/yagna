use std::time::UNIX_EPOCH;
// Extrnal crates
use actix_web::{HttpResponse, Scope};

// Workspace uses
use ya_client_model::payment::*;
use ya_core_model::payment::local::{GetAccounts, GetStatus, BUS_ID as LOCAL_SERVICE};
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::utils::*;

use crate::api::allocations::DEFAULT_PAYMENT_DRIVER;
use actix_web::web::{Data, Query};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ya_persistence::executor::DbExecutor;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(get_provider_accounts)
        .service(get_requestor_accounts)
        .service(get_wallet_status)
}

#[actix_web::get("/providerAccounts")]
async fn get_provider_accounts(id: Identity) -> HttpResponse {
    let node_id = id.identity.to_string();
    let all_accounts = match bus::service(LOCAL_SERVICE).send(GetAccounts {}).await {
        Ok(Ok(accounts)) => accounts,
        Ok(Err(e)) => return response::server_error(&e),
        Err(e) => return response::server_error(&e),
    };
    let recv_accounts: Vec<Account> = all_accounts
        .into_iter()
        .filter(|account| account.receive)
        //.filter(|account| account.address == node_id) // TODO: Implement proper account permission system
        .collect();
    response::ok(recv_accounts)
}

#[actix_web::get("/requestorAccounts")]
async fn get_requestor_accounts(db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    let node_id = id.identity.to_string();
    let all_accounts = match bus::service(LOCAL_SERVICE).send(GetAccounts {}).await {
        Ok(Ok(accounts)) => accounts,
        Ok(Err(e)) => return response::server_error(&e),
        Err(e) => return response::server_error(&e),
    };
    let recv_accounts: Vec<Account> = all_accounts
        .into_iter()
        .filter(|account| account.address == node_id) // TODO: Implement proper account permission system
        .collect();
    response::ok(recv_accounts)
}

/// TODO: Should be in `ya-client-model`, but for faster development it's here.
///       It's should be decided if this endpoint should be merged to master.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuery {
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default)]
    pub driver: Option<String>,
}

#[actix_web::get("/account/status")]
async fn get_wallet_status(
    db: Data<DbExecutor>,
    id: Identity,
    query: Query<AccountQuery>,
) -> HttpResponse {
    let query = query.into_inner();
    let address = query.address.unwrap_or(id.identity.to_string());

    if address != id.identity.to_string() {
        return response::bad_request(&"Attempting to get wallet status using wrong identity");
    }

    let status = match bus::service(LOCAL_SERVICE)
        .call(GetStatus {
            address,
            driver: query.driver.unwrap_or(DEFAULT_PAYMENT_DRIVER.to_string()),
            network: query.network,
            token: None,
            after_timestamp: DateTime::<Utc>::from(UNIX_EPOCH).timestamp(),
        })
        .await
    {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => return response::server_error(&e),
        Err(e) => return response::server_error(&e),
    };
    response::ok(status)
}
