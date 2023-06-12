#![allow(unused)]

use super::Result;
use crate::message::TcpListen;
use crate::network::VpnSupervisorRef;
use actix_web::error::ErrorInternalServerError;
use actix_web::{get, web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::time::Duration;
use ya_service_api_web::middleware::Identity;
use ya_utils_networking::vpn::Protocol;

#[derive(Deserialize)]
pub(super) struct PathListen {
    net_id: String,
    port: u16,
}

#[get("/net/{net_id}/listen-tcp/{port}")]
pub(super) async fn listen_tcp(
    vpn_sup: web::Data<VpnSupervisorRef>,
    path: web::Path<PathListen>,
    req: HttpRequest,
    stream: web::Payload,
    identity: Identity,
) -> Result<impl Responder> {
    let vpn = {
        vpn_sup
            .read()
            .await
            .get_network(&identity.identity, &path.net_id)?
        //.map_err(ErrorInternalServerError)?
    };
    let acceptor = vpn
        .send(TcpListen {
            protocol: Protocol::Tcp,
            address: Ipv4Addr::UNSPECIFIED.into(),
            port: path.port,
        })
        .await
        .map_err(ErrorInternalServerError)?
        .map_err(ErrorInternalServerError)?;

    tokio::time::sleep(Duration::from_secs(600)).await;
    Ok(web::Json(()))
}
