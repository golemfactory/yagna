// Extrnal crates
use actix_web::{HttpResponse, Scope};

// Workspace uses
use ya_client_model::payment::*;
use ya_core_model::payment::local::{GetAccounts, BUS_ID as LOCAL_SERVICE};
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{typed as bus, RpcEndpoint};

// Local uses
use crate::utils::*;

use actix_web::web::Data;
use ya_persistence::executor::DbExecutor;

pub fn register_endpoints(scope: Scope) -> Scope {
    scope
        .service(get_provider_accounts)
        .service(get_requestor_accounts)
}

#[actix_web::get("/providerAccounts")]
async fn get_provider_accounts(id: Identity) -> HttpResponse {
    let node_id = id.identity.to_string();
    let all_accounts = match bus::service(LOCAL_SERVICE).send(GetAccounts).await {
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
    let all_accounts = match bus::service(LOCAL_SERVICE).send(GetAccounts).await {
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
