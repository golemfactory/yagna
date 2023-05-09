#![allow(clippy::let_unit_value)]

use crate::message::*;
use crate::network::{VpnSupervisorRef};
use actix_web::{web, HttpResponse, Responder, ResponseError};
use serde::{Deserialize, Serialize};
use ya_client_model::net::*;
use ya_client_model::ErrorMessage;
use ya_service_api_web::middleware::Identity;
use ya_utils_networking::vpn::{Error as VpnError};

type Result<T> = std::result::Result<T, ApiError>;


const API_ROOT_PATH: &str = "/net-api";

mod connect_tcp;
mod listen_tcp;

pub fn web_scope(vpn_sup: web::Data<VpnSupervisorRef>) -> actix_web::Scope {
    let api_v1_subpath = api_subpath(NET_API_V1_VPN_PATH);
    let api_v2_subpath = api_subpath(NET_API_V2_VPN_PATH);

    web::scope(API_ROOT_PATH)
        .app_data(vpn_sup)
        .service(vpn_web_scope(api_v1_subpath))
        .service(vpn_web_scope(api_v2_subpath))
}

fn api_subpath(path: &str) -> &str {
    path.trim_start_matches(API_ROOT_PATH)
}

fn vpn_web_scope(path: &str) -> actix_web::Scope {
    web::scope(path)
        .service(get_networks)
        .service(create_network)
        .service(get_network)
        .service(remove_network)
        .service(get_addresses)
        .service(add_address)
        .service(get_nodes)
        .service(add_node)
        .service(remove_node)
        .service(connect_tcp::connect_tcp)
        .service(listen_tcp::listen_tcp)
}

/// Retrieves existing virtual private networks.
#[actix_web::get("/net")]
async fn get_networks(
    vpn_sup: web::Data<VpnSupervisorRef>,
    identity: Identity,
) -> impl Responder {
    let networks = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_networks(&identity.identity)
    };
    Ok::<_, ApiError>(web::Json(networks))
}

/// Creates a new virtual private network.
#[actix_web::post("/net")]
async fn create_network(
    vpn_sup: web::Data<VpnSupervisorRef>,
    model: web::Json<NewNetwork>,
    identity: Identity,
) -> impl Responder {
    let network = model.into_inner();
    let mut supervisor = vpn_sup.write().await;
    let network = supervisor
        .create_network(identity.identity, network)
        .await?;
    Ok::<_, ApiError>(web::Json(network))
}

/// Retrieves an existing virtual private network.
#[actix_web::get("/net/{net_id}")]
async fn get_network(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let network = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_blueprint(&identity.identity, &path.net_id)?
    };
    Ok::<_, ApiError>(web::Json(network))
}

/// Removes an existing virtual private network.
#[actix_web::delete("/net/{net_id}")]
async fn remove_network(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let mut supervisor = vpn_sup.write().await;
        supervisor.remove_network(&identity.identity, &path.net_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}

/// Retrieves requestor's addresses within a virtual private network.
#[actix_web::get("/net/{net_id}/addresses")]
async fn get_addresses(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let response = vpn.send(GetAddresses {}).await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Assigns a new address for the requestor within a virtual private network.
#[actix_web::post("/net/{net_id}/addresses")]
async fn add_address(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetwork>,
    model: web::Json<Address>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let address = model.into_inner().ip;
    let response = vpn.send(AddAddress { address }).await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Retrieves requestor's addresses within a virtual private network.
#[actix_web::get("/net/{net_id}/nodes")]
async fn get_nodes(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetwork>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let response = vpn.send(GetNodes {}).await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Adds a node to an existing virtual private network.
#[actix_web::post("/net/{net_id}/nodes")]
async fn add_node(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetwork>,
    model: web::Json<Node>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let vpn = {
        let supervisor = vpn_sup.read().await;
        supervisor.get_network(&identity.identity, &path.net_id)?
    };
    let node = model.into_inner();
    let response = vpn
        .send(AddNode {
            id: node.id,
            address: node.ip,
        })
        .await??;
    Ok::<_, ApiError>(web::Json(response))
}

/// Removes an existing node from a virtual private network
#[actix_web::delete("/net/{net_id}/nodes/{node_id}")]
async fn remove_node(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathNetworkNode>,
    identity: Identity,
) -> impl Responder {
    let path = path.into_inner();
    let fut = {
        let mut supervisor = vpn_sup.write().await;
        supervisor.remove_node(&identity.identity, &path.net_id, path.node_id)?
    };
    Ok::<_, ApiError>(web::Json(fut.await?))
}



#[derive(thiserror::Error, Debug)]
enum ApiError {
    #[error("VPN communication error: {0:?}")]
    ChannelError(#[from] actix::MailboxError),
    #[error("Request error: {0:?}")]
    WebError(#[from] actix_web::Error),
    #[error(transparent)]
    Vpn(#[from] VpnError),
}

impl ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        match self {
            Self::Vpn(err) => match err {
                VpnError::IpAddrTaken(_) => HttpResponse::Conflict().json(ErrorMessage::new(err)),
                VpnError::NetIdTaken(_) => HttpResponse::Conflict().json(ErrorMessage::new(err)),
                VpnError::NetNotFound => HttpResponse::NotFound().json(ErrorMessage::new(err)),
                VpnError::ConnectionTimeout => HttpResponse::GatewayTimeout().finish(),
                VpnError::Forbidden => HttpResponse::Forbidden().finish(),
                VpnError::Cancelled => {
                    HttpResponse::InternalServerError().json(ErrorMessage::new(err))
                }
                _ => HttpResponse::BadRequest().json(ErrorMessage::new(err)),
            },
            Self::ChannelError(_) | Self::WebError(_) => {
                HttpResponse::BadRequest().json(ErrorMessage::new(self))
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PathNetwork {
    net_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PathNetworkNode {
    net_id: String,
    node_id: String,
}



#[test]
fn test_to_detect_breaking_ya_client_const_changes() {
    assert!(
        api_subpath(NET_API_V1_VPN_PATH).len() < NET_API_V1_VPN_PATH.len(),
        "ya-client const NET_API_V1_VPN_PATH changed"
    );
    assert!(
        api_subpath(NET_API_V2_VPN_PATH).len() < NET_API_V2_VPN_PATH.len(),
        "ya-client const NET_API_V2_VPN_PATH changed"
    )
}
