use std::collections::HashSet;
use std::net::IpAddr;
use std::str::FromStr;

#[cfg(test)]
use std::time::Duration;

use hickory_resolver::config;
use hickory_resolver::{net::runtime::TokioRuntimeProvider, TokioResolver};

#[derive(Clone)]
pub struct StableResolver {
    stable_dns: IpAddr,
    resolver: TokioResolver,
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
    options.use_hosts_file = hickory_resolver::config::ResolveHosts::Never;
    options.cache_size = 0;
    let config = config::ResolverConfig::udp_and_tcp(&config::GOOGLE);
    let resolver = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(options)
        .build()?;
    let stable_dns = config::GOOGLE.ips[0];

    Ok(StableResolver {
        stable_dns,
        resolver,
    })
}

pub async fn resolver() -> anyhow::Result<StableResolver> {
    let default_resolver = TokioResolver::builder_with_config(
        config::ResolverConfig::udp_and_tcp(&config::GOOGLE),
        TokioRuntimeProvider::default(),
    )
    .with_options(Default::default())
    .build()?;
    let response = default_resolver
        .lookup_ip("stable-dns.dev.golem.network")
        .await?;
    let stable_dns = response.into_iter().next().unwrap_or(config::GOOGLE.ips[0]);
    let config =
        config::ResolverConfig::from_name_servers(vec![config::NameServerConfig::udp(stable_dns)]);
    let resolver = TokioResolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(Default::default())
        .build()?;
    Ok(StableResolver {
        stable_dns,
        resolver,
    })
}

pub const DNS_PORT: u16 = 53;

pub fn dns_servers() -> impl Iterator<Item = IpAddr> {
    use hickory_resolver::config::*;

    GOOGLE
        .ips
        .iter()
        .cloned()
        .chain(CLOUDFLARE.ips.iter().cloned())
        .chain(QUAD9.ips.iter().cloned())
}

// Do not use it in CI
#[cfg(test)]
#[ignore]
#[actix_rt::test]
async fn test_resolver() {
    let name = "accounts.google.com";
    let r = resolver().await.unwrap();
    let ac = r.ips(name).await.unwrap();
    for _i in 1..5 {
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
    for _i in 1..5 {
        actix_rt::time::sleep(Duration::from_secs(15)).await;
        r.clear_cache();
        let ac2 = r.ips(name).await.unwrap();
        assert_eq!(ac, ac2);
    }
}
