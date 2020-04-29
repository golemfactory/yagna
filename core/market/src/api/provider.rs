use crate::utils::*;
use actix_web::web::{get, post, Data, Json, Path, Query};
use actix_web::{HttpResponse, Scope};
use serde_json::value::Value::Null;
use ya_core_model::ethaddr::NodeId;
use ya_core_model::payment::public::{SendDebitNote, SendError, SendInvoice, BUS_ID};
use ya_core_model::payment::RpcMessageError;
use ya_model::market::*;
use ya_net::TryRemoteEndpoint;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;
use ya_service_bus::{timeout::IntoTimeoutFuture, RpcEndpoint};

pub fn register_endpoints(scope: Scope) -> Scope {
    scope.route("/offers", post().to(subscribe))
}

// ************************** **************************

async fn subscribe(db: Data<DbExecutor>, body: Json<Offer>, id: Identity) -> HttpResponse {
    response::ok(())
}
