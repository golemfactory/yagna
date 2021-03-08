use crate::vpn::error::Error;
use ipnet::IpNet;
use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

pub const MTU: usize = 14 + 65521; // ether frame + payload

#[inline(always)]
pub fn hton(ip: IpAddr) -> Box<[u8]> {
    match ip {
        IpAddr::V4(ip) => ip.octets().into(),
        IpAddr::V6(ip) => ip.octets().into(),
    }
}

#[inline(always)]
pub fn ntoh(data: &[u8]) -> Option<IpAddr> {
    if data.len() == 4 {
        let bytes: [u8; 4] = data[0..4].try_into().unwrap();
        Some(IpAddr::V4(Ipv4Addr::from(u32::from_be_bytes(bytes))))
    } else if data.len() == 16 {
        let bytes: [u8; 16] = data[0..16].try_into().unwrap();
        Some(IpAddr::V6(Ipv6Addr::from(bytes)))
    } else {
        None
    }
}

#[inline(always)]
pub fn to_cidr(mask: [u8; 4]) -> u32 {
    u32::from_ne_bytes(mask).to_be().leading_ones()
}

pub fn to_net(ip: &str, mask: &str) -> Result<IpNet, Error> {
    let result = match ip.find('/') {
        Some(_) => IpNet::from_str(ip),
        None => {
            let ip = IpAddr::from_str(&ip)?;
            let cidr = match &ip {
                IpAddr::V4(_) => to_cidr(Ipv4Addr::from_str(&mask)?.octets()),
                IpAddr::V6(_) => 128,
            };
            IpNet::from_str(&format!("{}/{}", ip, cidr))
        }
    };
    Ok(result.map_err(|_| Error::NetAddrInvalid(ip.to_string()))?)
}
