use std::convert::TryFrom;

use ya_runtime_api::server::Network;
use ya_service_bus::{typed, typed::Endpoint as GsbEndpoint};
use ya_utils_networking::vpn::{network::DuoEndpoint, Error as NetError};

use crate::error::Error;
use crate::state::DeploymentNetwork;
use crate::Result;

pub(crate) mod inet;
pub(crate) mod vpn;

impl<'a> TryFrom<&'a DeploymentNetwork> for Network {
    type Error = Error;

    fn try_from(net: &'a DeploymentNetwork) -> Result<Self> {
        let ip = net.network.addr();
        let mask = net.network.netmask();
        let gateway = net
            .network
            .hosts()
            .find(|ip_| ip_ != &ip)
            .ok_or(NetError::NetAddrTaken(ip))?;

        Ok(Network {
            addr: ip.to_string(),
            gateway: gateway.to_string(),
            mask: mask.to_string(),
            if_addr: net.node_ip.to_string(),
        })
    }
}

fn gsb_endpoint(node_id: &str, net_id: &str) -> DuoEndpoint<GsbEndpoint> {
    DuoEndpoint {
        tcp: typed::service(format!("/net/{}/vpn/{}", node_id, net_id)),
        udp: typed::service(format!("/udp/net/{}/vpn/{}/raw", node_id, net_id)),
    }
}
