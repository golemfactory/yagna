use crate::vpn::common::{hton, to_octets};
use crate::vpn::Error;
use ipnet::IpNet;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::Hash;
use std::net::IpAddr;
use std::ops::Not;

/// Internal VPN state. Neither network nor node IP duplicates are allowed.
pub struct State<E> {
    networks: HashMap<String, IpNet>,
    endpoints: BTreeMap<Box<[u8]>, E>, // IP bytes (BE) -> remote endpoint
    nodes: BTreeMap<String, HashSet<IpAddr>>, // Node id -> Vec<IP bytes (BE)>
}

impl<E> Default for State<E> {
    fn default() -> Self {
        Self {
            networks: Default::default(),
            endpoints: Default::default(),
            nodes: Default::default(),
        }
    }
}

impl<E> State<E> {
    pub fn endpoints(&self) -> &BTreeMap<Box<[u8]>, E> {
        &self.endpoints
    }

    pub fn networks(&self) -> &HashMap<String, IpNet> {
        &self.networks
    }

    pub fn nodes(&self) -> &BTreeMap<String, HashSet<IpAddr>> {
        &self.nodes
    }
}

impl<E> State<E> {
    pub fn create(&mut self, networks: HashMap<String, IpNet>) -> Result<(), Error> {
        if let Some(net_id) = intersect(self.networks.keys(), networks.keys()) {
            return Err(Error::NetIdTaken((*net_id).clone()));
        }
        if let Some(net_ip) = intersect(self.networks.values(), networks.values()) {
            return Err(Error::NetAddrTaken(net_ip.addr()));
        }
        self.networks.extend(networks.into_iter());
        Ok(())
    }

    pub fn remove<I, S>(&mut self, network_ids: I) -> Option<HashSet<IpNet>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let removed_nets = network_ids
            .into_iter()
            .fold(HashSet::new(), |mut nets, id| {
                if let Some(ip_net) = self.networks.remove(id.as_ref()) {
                    nets.insert(ip_net);
                }
                nets
            });

        removed_nets.is_empty().not().then(|| {
            let mut removed_nodes = HashSet::new();
            self.nodes.iter_mut().for_each(|(id, ips)| {
                removed_nets
                    .iter()
                    .for_each(|net| ips.retain(|ip| !net.contains(ip)));
                ips.is_empty().then(|| removed_nodes.insert(id.clone()));
            });
            self.leave(removed_nodes);
            removed_nets
        })
    }

    pub fn join<F>(&mut self, nodes: HashMap<IpAddr, String>, endpoint_fn: F) -> Result<(), Error>
    where
        F: Fn(&str, &str) -> E,
    {
        for (ip_addr, id) in nodes {
            let ip: Box<[u8]> = hton(ip_addr).into();
            if self.endpoints.contains_key(&ip) {
                return Err(Error::IpAddrTaken(ip_addr));
            }

            let (net_id, _) = self
                .networks
                .iter()
                .find(|(_, net)| net.contains(&ip_addr))
                .ok_or_else(|| Error::NetAddr(ip_addr.to_string()))?;

            self.endpoints.insert(ip, endpoint_fn(&id, net_id));
            self.nodes
                .entry(id)
                .or_insert_with(HashSet::new)
                .insert(ip_addr);
        }

        Ok(())
    }

    pub fn leave<I, S>(&mut self, node_ids: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        node_ids.into_iter().for_each(|id| {
            self.nodes.remove(id.as_ref()).map(|addrs| {
                addrs.into_iter().for_each(|a| {
                    self.endpoints.remove(&to_octets(a));
                });
            });
        });
    }
}

fn intersect<'a, T: Hash + Eq>(
    cur: impl Iterator<Item = &'a T>,
    new: impl Iterator<Item = &'a T>,
) -> Option<&'a T> {
    let cur_set = cur.collect::<HashSet<_>>();
    let new_set = new.collect::<HashSet<_>>();
    cur_set.intersection(&new_set).next().cloned()
}
