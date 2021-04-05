use crate::vpn::common::{hton, to_ip, to_octets};
use crate::vpn::Error;
use ipnet::IpNet;
use std::collections::{BTreeSet, HashMap};
use std::net::IpAddr;

pub struct Networks<E> {
    networks: HashMap<String, Network<E>>,
}

impl<E> Default for Networks<E> {
    fn default() -> Self {
        Self {
            networks: Default::default(),
        }
    }
}

impl<E: Clone> Networks<E> {
    pub fn get_mut(&mut self, id: &str) -> Result<&mut Network<E>, Error> {
        self.networks.get_mut(id).ok_or_else(|| Error::NetNotFound)
    }

    pub fn endpoint<B: AsRef<[u8]>>(&self, ip: B) -> Option<E> {
        self.as_ref()
            .values()
            .filter_map(|n| n.endpoint(ip.as_ref()))
            .next()
    }

    pub fn endpoints(&self) -> Vec<E> {
        self.networks.values().fold(Vec::new(), |mut v, n| {
            v.extend(n.endpoints.values().cloned());
            v
        })
    }

    pub fn add<S: ToString>(&mut self, id: S, network: IpNet) -> Result<(), Error> {
        let id = id.to_string();

        if self.networks.contains_key(&id) {
            return Err(Error::NetIdTaken(id));
        }
        if self
            .networks
            .values()
            .find(|n| n.as_ref() == &network || n.as_ref().contains(&network.addr()))
            .is_some()
        {
            return Err(Error::NetAddrTaken(network.addr()));
        }

        let net = Network::new(&id, network);
        self.networks.insert(id, net);
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Option<Network<E>> {
        self.networks.remove(id)
    }
}

impl<E> AsRef<HashMap<String, Network<E>>> for Networks<E> {
    fn as_ref(&self) -> &HashMap<String, Network<E>> {
        &self.networks
    }
}

pub struct Network<E> {
    id: String,
    network: IpNet,
    pub(self) addresses: BTreeSet<IpAddr>,
    pub(self) endpoints: HashMap<Box<[u8]>, E>, // IP bytes (BE) -> remote endpoint
    nodes: HashMap<String, BTreeSet<IpAddr>>,   // Node id -> Vec<IP bytes (BE)>
}

impl<E> Network<E> {
    pub fn new(id: &str, network: IpNet) -> Self {
        Self {
            id: id.to_string(),
            network,
            addresses: Default::default(),
            endpoints: Default::default(),
            nodes: Default::default(),
        }
    }

    pub fn id(&self) -> &String {
        &self.id
    }

    pub fn address(&self) -> Result<IpAddr, Error> {
        self.addresses
            .iter()
            .next()
            .cloned()
            .ok_or_else(|| Error::NetEmpty)
    }

    pub fn endpoints(&self) -> &HashMap<Box<[u8]>, E> {
        &self.endpoints
    }

    pub fn nodes(&self) -> &HashMap<String, BTreeSet<IpAddr>> {
        &self.nodes
    }

    pub fn add_address(&mut self, ip: &str) -> Result<(), Error> {
        let ip = to_ip(ip.as_ref())?;
        if !self.network.contains(&ip) {
            return Err(Error::NetAddr(ip.to_string()));
        }
        self.addresses.insert(ip);
        Ok(())
    }

    pub fn add_node<F>(&mut self, ip_addr: IpAddr, id: &str, endpoint_fn: F) -> Result<(), Error>
    where
        F: Fn(&str, &str) -> E,
    {
        if !self.network.contains(&ip_addr) {
            return Err(Error::NetAddr(ip_addr.to_string()));
        }

        let node_id = id.to_string();
        let ip: Box<[u8]> = hton(ip_addr).into();

        if self.endpoints.contains_key(&ip) {
            return Err(Error::IpAddrTaken(ip_addr));
        }

        self.endpoints.insert(ip, endpoint_fn(&node_id, &self.id));
        self.nodes
            .entry(node_id)
            .or_insert_with(Default::default)
            .insert(ip_addr);

        Ok(())
    }

    pub fn remove_node(&mut self, node_id: &str) {
        self.nodes.remove(node_id).map(|addrs| {
            addrs.into_iter().for_each(|a| {
                self.endpoints.remove(&to_octets(a));
            });
        });
    }
}

impl<E: Clone> Network<E> {
    pub fn endpoint<B: AsRef<[u8]>>(&self, ip: B) -> Option<E> {
        self.endpoints.get(ip.as_ref()).cloned()
    }
}

impl<E> AsRef<IpNet> for Network<E> {
    fn as_ref(&self) -> &IpNet {
        &self.network
    }
}
