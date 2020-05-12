use actix_web::web::{Data, Json, Path, Query};
use actix_web::HttpResponse;
use ya_client::model::market::*;
use ya_persistence::executor::DbExecutor;
use ya_service_api_web::middleware::Identity;

use crate::utils::*;

use super::{
    resolve_web_error, ClientCache, PathAgreement, PathSubscription, PathSubscriptionProposal,
    QueryTimeout, QueryTimeoutMaxEvents,
};

pub fn extend_web_scope(scope: actix_web::Scope) -> actix_web::Scope {
    scope.service(subscribe)
    // .service(get_offers)
    // .service(unsubscribe)
    // .service(collect)
    // .service(counter_proposal)
    // .service(get_proposal)
    // .service(reject_proposal)
    // .service(approve_agreement)
    // .service(reject_agreement)
}

#[actix_web::post("/offers")]
async fn subscribe(
    client_cache: Data<ClientCache>,
    body: Json<Offer>,
    id: Identity,
) -> HttpResponse {
    let api = client_cache.get_provider_api(id.identity).await;
    match api.subscribe(&body.into_inner()).await {
        Ok(subscription_id) => response::created(subscription_id),
        Err(err) => resolve_web_error(err),
    }
}
//
// #[actix_web::get("/offers")]
// async fn get_offers(_db: Data<DbExecutor>, id: Identity) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let offers_result = market_api.get_offers().await;
//
//             match offers_result {
//                 Ok(offers) => response::ok(offers),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::delete("/offers/{subscription_id}")]
// async fn unsubscribe(
//     _db: Data<DbExecutor>,
//     path: Path<PathSubscription>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let subscription_id_result = market_api.unsubscribe(&path.subscription_id).await;
//
//             match subscription_id_result {
//                 Ok(_subscription_id) => response::no_content(),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::get("/offers/{subscription_id}/events")]
// async fn collect(
//     _db: Data<DbExecutor>,
//     path: Path<PathSubscription>,
//     query: Query<QueryTimeoutMaxEvents>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let events_result = market_api
//                 .collect(&path.subscription_id, query.timeout, query.max_events)
//                 .await;
//
//             match events_result {
//                 Ok(events) => response::ok(events),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::post("/offers/{subscription_id}/proposals/{proposal_id}")]
// async fn counter_proposal(
//     _db: Data<DbExecutor>,
//     path: Path<PathSubscriptionProposal>,
//     body: Json<Proposal>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let proposal_id_result = market_api
//                 .counter_proposal(&body.into_inner(), &path.subscription_id)
//                 .await;
//
//             match proposal_id_result {
//                 Ok(proposal_id) => response::created(proposal_id),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::get("/offers/{subscription_id}/proposals/{proposal_id}")]
// async fn get_proposal(
//     _db: Data<DbExecutor>,
//     path: Path<PathSubscriptionProposal>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let proposal_result = market_api
//                 .get_proposal(&path.subscription_id, &path.proposal_id)
//                 .await;
//
//             match proposal_result {
//                 Ok(proposal) => response::ok(proposal),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::delete("/offers/{subscription_id}/proposals/{proposal_id}")]
// async fn reject_proposal(
//     _db: Data<DbExecutor>,
//     path: Path<PathSubscriptionProposal>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let proposal_result = market_api
//                 .reject_proposal(&path.subscription_id, &path.proposal_id)
//                 .await;
//
//             match proposal_result {
//                 Ok(_) => response::no_content(),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::post("/agreements/{agreement_id}/approve")]
// async fn approve_agreement(
//     _db: Data<DbExecutor>,
//     path: Path<PathAgreement>,
//     query: Query<QueryTimeout>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let agreement_id_result = market_api
//                 .approve_agreement(&path.agreement_id, query.timeout)
//                 .await;
//
//             match agreement_id_result {
//                 Ok(content) => response::ok(content), // Note this is not following the market-api.yaml
//                 // Ok(_) => response::no_content(),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }
//
// #[actix_web::post("/agreements/{agreement_id}/reject")]
// async fn reject_agreement(
//     _db: Data<DbExecutor>,
//     path: Path<PathAgreement>,
//     id: Identity,
// ) -> HttpResponse {
//     match build_market_api(id) {
//         Ok(market_api) => {
//             let terminate_result = market_api.terminate_agreement(&path.agreement_id).await;
//
//             match terminate_result {
//                 Ok(_) => response::no_content(),
//                 Err(err) => resolve_web_error(err),
//             }
//         }
//         Err(err) => response::server_error(&err),
//     }
// }

// NOTE that terminate_agreement and get_agreement are already implemented in the Reuqestor side API
