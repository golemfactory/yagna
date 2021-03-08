use crate::vpn::common::hton;
use crate::vpn::Error;
use ipnet::IpNet;
use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;
use std::str::FromStr;

pub struct State<E> {
    networks: HashMap<String, IpNet>,
    nodes: BTreeMap<Box<[u8]>, E>, // IP_BYTES_BE -> NODE ENDPOINT
    nodes_rev: BTreeMap<String, Vec<Box<[u8]>>>, // NODE_ID -> Vec<IP_BYTES_BE>
}

impl<E> State<E> {
    pub fn new(networks: HashMap<String, IpNet>) -> Self {
        Self {
            networks,
            nodes: Default::default(),
            nodes_rev: Default::default(),
        }
    }

    pub fn endpoint<B>(&self, ip: B) -> Option<&E>
    where
        B: AsRef<[u8]>,
    {
        self.nodes.get(ip.as_ref())
    }

    pub fn endpoints(&self) -> impl Iterator<Item = &E> {
        self.nodes.values()
    }

    pub fn networks(&self) -> &HashMap<String, IpNet> {
        &self.networks
    }
}

impl<E> State<E> {
    pub fn join<I, F, K, V>(&mut self, nodes: I, endpoint: F) -> Result<(), Error>
    where
        I: IntoIterator<Item = (K, V)>,
        F: Fn(&str, &str) -> E,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        for result in nodes
            .into_iter()
            .map(|(ip, e)| parse_ip(ip.as_ref()).map(|ip_addr| (ip_addr, e)))
        {
            let (ip_addr, node_id) = result?;
            let (net_id, _) = self
                .networks
                .iter()
                .find(|(_, net)| net.contains(&ip_addr))
                .ok_or_else(|| Error::NetAddrInvalid(ip_addr.to_string()))?;

            let id = node_id.as_ref().to_string();
            let ip: Box<[u8]> = hton(ip_addr).into();

            self.nodes.insert(ip.clone(), endpoint(&id, net_id));
            self.nodes_rev.entry(id).or_insert_with(Vec::new).push(ip);
        }

        Ok(())
    }

    pub fn leave<I, K>(&mut self, ids: I)
    where
        I: Iterator<Item = K>,
        K: AsRef<str>,
    {
        ids.for_each(|id| {
            self.nodes_rev.remove(id.as_ref()).map(|addrs| {
                addrs.into_iter().for_each(|a| {
                    self.nodes.remove(&a);
                });
            });
        });
    }
}

fn parse_ip(ip: &str) -> Result<IpAddr, Error> {
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
