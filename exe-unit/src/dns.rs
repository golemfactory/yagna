use std::collections::HashSet;
use std::net::IpAddr;
use std::str::FromStr;
use trust_dns_resolver::config;
use trust_dns_resolver::TokioAsyncResolver;

#[derive(Clone)]
pub struct StableResolver {
    stable_dns: IpAddr,
    resolver: TokioAsyncResolver,
}

impl StableResolver {
    pub async fn ips(&self, host_name: &str) -> anyhow::Result<HashSet<IpAddr>> {
        if let Ok(ip_addr) = IpAddr::from_str(host_name) {
            return Ok(HashSet::from([ip_addr]));
        }
        log::debug!("Resolving IP addresses of '{}'", host_name);

        let response = self.resolver.lookup_ip(host_name).await?;

        Ok(response.into_iter().collect())
    }

    pub fn stable_dns(&self) -> IpAddr {
        self.stable_dns
    }

    #[cfg(test)]
    fn clear_cache(&self) {
        self.resolver.clear_cache();
    }
}

#[cfg(test)]
async fn google_resolver() -> anyhow::Result<StableResolver> {
    let mut options: config::ResolverOpts = Default::default();
    options.use_hosts_file = false;
    options.cache_size = 0;
    let config = config::ResolverConfig::default();
    let resolver = TokioAsyncResolver::tokio(config, options)?;
    let stable_dns = config::GOOGLE_IPS[0];

    Ok(StableResolver {
        stable_dns,
        resolver,
    })
}

pub async fn resolver() -> anyhow::Result<StableResolver> {
    let default_resolver = TokioAsyncResolver::tokio(Default::default(), Default::default())?;
    let response = default_resolver
        .lookup_ip("stable-dns.dev.golem.network")
        .await?;
    let stable_dns = response.into_iter().next().unwrap_or(config::GOOGLE_IPS[0]);
    let mut config = config::ResolverConfig::new();
    config.add_name_server(config::NameServerConfig::new(
        (stable_dns, 53).into(),
        trust_dns_resolver::config::Protocol::Udp,
    ));
    let resolver = TokioAsyncResolver::tokio(config, Default::default())?;
    Ok(StableResolver {
        stable_dns,
        resolver,
    })
}

pub const DNS_PORT: u16 = 53;

pub fn dns_servers() -> impl Iterator<Item = IpAddr> {
    use trust_dns_resolver::config::*;

    GOOGLE_IPS
        .iter()
        .cloned()
        .chain(CLOUDFLARE_IPS.iter().cloned())
        .chain(QUAD9_IPS.iter().cloned())
}

// Do not use it in CI
#[cfg(test)]
#[ignore]
#[actix_rt::test]
async fn test_resolver() {
    let name = "accounts.google.com";
    let r = resolver().await.unwrap();
    let ac = r.ips(name).await.unwrap();
    for i in 1..5 {
        actix_rt::time::sleep(Duration::from_secs(30)).await;
        let ac2 = r.ips(name).await.unwrap();
        assert_eq!(ac, ac2);
    }
}

// Do not use it in CI
#[cfg(test)]
#[ignore]
#[should_panic]
#[actix_rt::test]
async fn test_fail_resolver() {
    let name = "accounts.google.com";
    let r = google_resolver().await.unwrap();
    let ac = r.ips(name).await.unwrap();
    for i in 1..5 {
        actix_rt::time::sleep(Duration::from_secs(15)).await;
        r.clear_cache();
        let ac2 = r.ips(name).await.unwrap();
        assert_eq!(ac, ac2);
    }
}
