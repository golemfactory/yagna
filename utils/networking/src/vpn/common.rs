use std::convert::TryInto;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

use ipnet::IpNet;

use ya_relay_stack::Error;

pub const DEFAULT_MAX_FRAME_SIZE: usize = 1502;
pub const DEFAULT_IPV4_NET_MASK: &str = "255.255.255.0";

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

pub fn to_ip(ip: &str) -> Result<IpAddr, Error> {
    let ip = IpAddr::from_str(ip.as_ref()).map_err(Error::from)?;

    if ip.is_loopback() || ip.is_unspecified() || ip.is_multicast() {
        return Err(Error::IpAddrNotAllowed(ip).into());
    } else if let IpAddr::V4(ip4) = &ip {
        if ip4.is_broadcast() {
            return Err(Error::IpAddrNotAllowed(ip).into());
        }
    }

    Ok(ip)
}

pub fn to_octets(ip: IpAddr) -> Box<[u8]> {
    match ip {
        IpAddr::V4(ipv4) => ipv4.octets().into(),
        IpAddr::V6(ipv6) => ipv6.octets().into(),
    }
}

pub fn to_net<S: AsRef<str>>(ip: &str, mask: Option<S>) -> Result<IpNet, Error> {
    let result = match ip.find('/') {
        Some(_) => IpNet::from_str(ip),
        None => {
            let ip = IpAddr::from_str(ip)?;
            let cidr = match &ip {
                IpAddr::V4(_) => {
                    let mask = mask
                        .as_ref()
                        .map(|s| s.as_ref())
                        .unwrap_or(DEFAULT_IPV4_NET_MASK);
                    let octets = Ipv4Addr::from_str(mask)?.octets();
                    u32::from_ne_bytes(octets).to_be().leading_ones()
                }
                IpAddr::V6(_) => 128,
            };
            IpNet::from_str(&format!("{}/{}", ip, cidr))
        }
    };
    Ok(result.map_err(|_| Error::NetAddr(ip.to_string()))?)
}
