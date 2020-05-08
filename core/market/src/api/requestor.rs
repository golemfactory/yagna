use super::{
    resolve_web_error, PathAgreement, PathSubscription, PathSubscriptionProposal, QueryTimeout,
    QueryTimeoutMaxEvents,
};
use crate::utils::response;
use actix_web::web::{Data, Json, Path, Query};
use actix_web::HttpResponse;
use std::time::Duration;
use ya_client::model::market::*;
use ya_client::{
    market::MarketRequestorApi,
    web::{WebAuth, WebClient},
    Result,
};
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope
        .service(subscribe)
        .service(get_demands)
        .service(unsubscribe)
        .service(collect)
        .service(counter_proposal)
        .service(get_proposal)
        .service(reject_proposal)
        .service(create_agreement)
        .service(get_agreement)
        .service(confirm_agreement)
        .service(wait_for_approval)
        .service(cancel_agreement)
        .service(terminate_agreement)
}

// ****************************************************

fn build_market_api(id: Identity) -> Result<MarketRequestorApi> {
    let client_result = WebClient::builder()
        .auth(WebAuth::Bearer(super::encode_jwt(id)))
        .timeout(Duration::from_secs(5))
        .build();

    match client_result {
        Ok(client) => Ok(client.interface().unwrap()),
        Err(err) => Err(err),
    }
}

// Failed experiments with passing the Api call as closure...
//
// async fn forward_web_request<T, R, F: Future<Output = Result<T>>>(
//     db: Data<DbExecutor>,
//     f: impl FnOnce(MarketRequestorApi, Json<R>) -> F,
//     id: Identity,
//     body: Json<R>,
// ) -> HttpResponse
// where T : Serialize {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let subscription_id_result = f(market_api, body).await;

//             match subscription_id_result {
//                 Ok(subscription_id) => response::created(subscription_id),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }

// #[actix_web::post("/demands")]
// async fn subscribe(_db: Data<DbExecutor>, body: Json<Demand>, id: Identity) -> HttpResponse {
//     forward_web_request(_db, move |market_api, body_parm| market_api.subscribe(&body_parm.into_inner().clone()), id, body).await
// }

#[actix_web::post("/demands")]
async fn subscribe(_db: Data<DbExecutor>, body: Json<Demand>, id: Identity) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let subscription_id_result = market_api.subscribe(&body.into_inner()).await;

            match subscription_id_result {
                Ok(subscription_id) => response::created(subscription_id),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::get("/demands")]
async fn get_demands(_db: Data<DbExecutor>, id: Identity) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let demands_result = market_api.get_demands().await;

            match demands_result {
                Ok(demands) => response::ok(demands),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::delete("/demands/{subscription_id}")]
async fn unsubscribe(
    _db: Data<DbExecutor>,
    path: Path<PathSubscription>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let subscription_id_result = market_api.unsubscribe(&path.subscription_id).await;

            match subscription_id_result {
                Ok(_subscription_id) => response::no_content(),
                Err(err) => resolve_web_error(err),
                //response::server_error(&err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::get("/demands/{subscription_id}/events")]
async fn collect(
    _db: Data<DbExecutor>,
    path: Path<PathSubscription>,
    query: Query<QueryTimeoutMaxEvents>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let events_result = market_api
                .collect(&path.subscription_id, query.timeout, query.max_events)
                .await;

            match events_result {
                Ok(events) => response::ok(events),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::post("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn counter_proposal(
    _db: Data<DbExecutor>,
    path: Path<PathSubscriptionProposal>,
    body: Json<Proposal>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let proposal_id_result = market_api
                .counter_proposal(&body.into_inner(), &path.subscription_id)
                .await;

            match proposal_id_result {
                Ok(proposal_id) => response::created(proposal_id),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::get("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn get_proposal(
    _db: Data<DbExecutor>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let proposal_result = market_api
                .get_proposal(&path.subscription_id, &path.proposal_id)
                .await;

            match proposal_result {
                Ok(proposal) => response::ok(proposal),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::delete("/demands/{subscription_id}/proposals/{proposal_id}")]
async fn reject_proposal(
    _db: Data<DbExecutor>,
    path: Path<PathSubscriptionProposal>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let proposal_result = market_api
                .reject_proposal(&path.subscription_id, &path.proposal_id)
                .await;

            match proposal_result {
                Ok(_) => response::no_content(),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::post("/agreements")]
async fn create_agreement(
    _db: Data<DbExecutor>,
    body: Json<AgreementProposal>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let agreement_id_result = market_api.create_agreement(&body.into_inner()).await;

            match agreement_id_result {
                Ok(agreement_id) => response::created(agreement_id),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::get("/agreements/{agreement_id}")]
async fn get_agreement(
    _db: Data<DbExecutor>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let agreement_result = market_api.get_agreement(&path.agreement_id).await;

            match agreement_result {
                Ok(agreement) => response::ok(agreement),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::post("/agreements/{agreement_id}/confirm")]
async fn confirm_agreement(
    _db: Data<DbExecutor>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let agreement_id_result = market_api.confirm_agreement(&path.agreement_id).await;

            match agreement_id_result {
                Ok(_) => response::no_content(),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::post("/agreements/{agreement_id}/wait")]
async fn wait_for_approval(
    _db: Data<DbExecutor>,
    path: Path<PathAgreement>,
    query: Query<QueryTimeout>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let approval_result = market_api
                .wait_for_approval(&path.agreement_id, query.timeout)
                .await;

            match approval_result {
                Ok(approval) => response::ok(approval),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::delete("/agreements/{agreement_id}")]
async fn cancel_agreement(
    _db: Data<DbExecutor>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let cancel_result = market_api.cancel_agreement(&path.agreement_id).await;

            match cancel_result {
                Ok(_) => response::no_content(),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}

#[actix_web::delete("/agreements/{agreement_id}/terminate")]
async fn terminate_agreement(
    _db: Data<DbExecutor>,
    path: Path<PathAgreement>,
    id: Identity,
) -> HttpResponse {
    match build_market_api(id) {
        Ok(market_api) => {
            let terminate_result = market_api.terminate_agreement(&path.agreement_id).await;

            match terminate_result {
                Ok(_) => response::no_content(),
                Err(err) => resolve_web_error(err),
            }
        }
        Err(err) => response::server_error(&err),
    }
}
