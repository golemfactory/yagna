use crate::erc20::rpc_resolv::NameResolver::{DnsLookup, StaticList};
use anyhow::Context;
use futures::prelude::*;
use rand::prelude::*;
use rand::thread_rng;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;
use web3::transports::Http;
use web3::Web3;
use ya_payment_driver::db::models::Network;

pub struct RpcResolver {
    network_resolvers: Arc<Mutex<BTreeMap<Network, NetworkResolver>>>,
}

impl RpcResolver {
    pub fn new() -> Self {
        let network_resolvers = Arc::new(Mutex::new(BTreeMap::new()));
        Self { network_resolvers }
    }

    pub async fn clients_for(
        &self,
        network: Network,
    ) -> anyhow::Result<impl Stream<Item = Web3<Http>>> {
        let n = {
            let mut g = self
                .network_resolvers
                .lock()
                .expect("RpcResolver mutex poisoned");
            match g.entry(network) {
                Entry::Occupied(v) => v.get().clone(),
                Entry::Vacant(v) => {
                    let network_resolver = NetworkResolver::from_env(network)?;
                    v.insert(network_resolver.clone());
                    network_resolver
                }
            }
        };
        Ok(n.clients().await)
    }
}

#[derive(Clone)]
struct NetworkResolver {
    inner: Arc<RwLock<NetworkResolverInner>>,
}

struct NetworkResolverInner {
    name_resolver: NameResolver,
    last_ep: Option<(Web3<Http>, Arc<str>)>,
}

impl NetworkResolver {
    fn from_env(network: Network) -> anyhow::Result<Self> {
        let name_resolver = NameResolver::from_env(network)?;
        let last_ep = None;
        let inner = Arc::new(RwLock::new(NetworkResolverInner {
            name_resolver,
            last_ep,
        }));

        Ok(Self { inner })
    }

    pub async fn clients(&self) -> impl Stream<Item = Web3<Http>> {
        let last_ep = self.inner.read().await.last_ep.clone();
        if let Some((web3, web3url)) = last_ep {
            let resolver = self.clone();
            return ResolvStream::Ready {
                web3: Some(web3),
                web3url,
                resolver,
            }
            .stream();
        }
        let mut g = self.clone().inner.write_owned().await;
        if let Some(ts) = g.name_resolver.expires() {
            if ts < Instant::now() {
                g.name_resolver.refresh().await;
            }
        }
        let mut t = thread_rng();
        let mut names: Vec<_> = g.name_resolver.names().collect();
        names.as_mut_slice().shuffle(&mut t);
        ResolvStream::TryNames { names, g }.stream()
    }
}

enum NameResolver {
    StaticList(Vec<Arc<str>>),
    DnsLookup {
        resolver: TokioAsyncResolver,
        dns_name: String,
        last_names: Vec<Arc<str>>,
        expire: Instant,
    },
}

fn parse_env_list(names: &'_ str) -> impl Iterator<Item = Arc<str>> + '_ {
    names.split(',').filter_map(|name| {
        let name = name.trim();
        if url::Url::parse(name).is_ok() {
            Some(name.into())
        } else {
            None
        }
    })
}

const DNS_REFRESH_TIMEOUT: Duration = Duration::from_secs(300);

impl NameResolver {
    fn from_env(network: Network) -> anyhow::Result<Self> {
        let network_upper = network.to_string().to_uppercase();
        let env_name = format!("{network_upper}_GETH_ADDR");
        if let Ok(names) = env::var(env_name) {
            return Ok(StaticList(parse_env_list(&names).collect()));
        }
        let resolver = TokioAsyncResolver::tokio(ResolverConfig::google(), ResolverOpts::default())
            .context("Failed to create dns resolver for rpc-node lookup")?;
        let expire = Instant::now() - Duration::from_secs(1);
        let last_names = Default::default();
        let network_lower = network.to_string().to_lowercase();
        let dns_name = format!("{network_lower}.rpc-node.dev.golem.network");

        Ok(DnsLookup {
            resolver,
            dns_name,
            last_names,
            expire,
        })
    }

    fn expires(&self) -> Option<Instant> {
        match self {
            Self::StaticList(_) => None,
            Self::DnsLookup { expire, .. } => Some(*expire),
        }
    }

    fn names(&self) -> impl Iterator<Item = Arc<str>> + 'static {
        match self {
            Self::StaticList(items) => items.clone().into_iter(),
            Self::DnsLookup { last_names, .. } => last_names.clone().into_iter(),
        }
    }

    async fn refresh(&mut self) {
        match self {
            Self::StaticList(_) => (),
            Self::DnsLookup {
                resolver,
                expire,
                last_names,
                dns_name,
            } => {
                if let Ok(names) = resolver.txt_lookup(dns_name.as_str()).await {
                    *last_names = names
                        .iter()
                        .filter_map(|x| x.iter().next().and_then(|v| std::str::from_utf8(v).ok()))
                        .map(Into::into)
                        .collect();
                    *expire = Instant::now() + DNS_REFRESH_TIMEOUT;
                }
            }
        }
    }
}

enum ResolvStream {
    Ready {
        web3: Option<Web3<Http>>,
        web3url: Arc<str>,
        resolver: NetworkResolver,
    },
    TryNames {
        names: Vec<Arc<str>>,
        g: tokio::sync::OwnedRwLockWriteGuard<NetworkResolverInner>,
    },
}

impl ResolvStream {
    fn stream(self) -> impl Stream<Item = Web3<Http>> {
        fn do_try_names(
            mut names: Vec<Arc<str>>,
            mut g: tokio::sync::OwnedRwLockWriteGuard<NetworkResolverInner>,
        ) -> Option<(Web3<Http>, ResolvStream)> {
            while let Some(web3name) = names.pop() {
                if let Ok(web3t) = web3::transports::Http::new(&web3name) {
                    let web3 = Web3::new(web3t);
                    g.last_ep = Some((web3.clone(), web3name));
                    return Some((web3, ResolvStream::TryNames { names, g }));
                }
            }
            g.last_ep = None;

            None
        }

        stream::unfold(self, |state| async move {
            match state {
                Self::Ready {
                    mut web3,
                    web3url,
                    resolver,
                } => {
                    if let Some(web3) = web3.take() {
                        Some((
                            web3,
                            Self::Ready {
                                web3: None,
                                web3url,
                                resolver,
                            },
                        ))
                    }
                    // case 2: find other values
                    else {
                        let g = resolver.clone().inner.write_owned().await;
                        let mut t = thread_rng();
                        let mut names: Vec<_> =
                            g.name_resolver.names().filter(|u| u != &web3url).collect();
                        names.as_mut_slice().shuffle(&mut t);

                        do_try_names(names, g)
                    }
                }
                Self::TryNames { names, g } => do_try_names(names, g),
            }
        })
    }
}

#[cfg(test)]
mod integration_test {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use test_case::test_case;

    #[test_case(Network::Mainnet; "check chain_id on Eth Mainnet")]
    #[test_case(Network::Goerli; "check chain_id on Goerli")]
    #[test_case(Network::Mumbai; "check chain_id on Mumbai")]
    #[test_case(Network::Polygon; "check chain_id on Polygon Mainnet")]
    #[test_case(Network::Rinkeby; "check chain_id on Rinkeby")]
    #[tokio::test]
    #[cfg_attr(not(feature = "integration"), ignore)]
    async fn test_resolver(network: Network) {
        let resolver = NetworkResolver::from_env(network).unwrap();
        eprintln!("starting check for: {}", network);
        let cnt = AtomicUsize::new(0);
        resolver
            .clients()
            .await
            .for_each(|client: Web3<Http>| {
                let _ = cnt.fetch_add(1, Ordering::Relaxed);
                async move {
                    let chain_id = client.eth().chain_id().await;
                    assert_eq!(network as usize, chain_id.unwrap().as_usize());
                }
            })
            .await;
        assert!(cnt.into_inner() > 0);
    }
}
