use lazy_static::lazy_static;
use regex::Regex;
use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use std::iter::FromIterator;
use std::net::IpAddr;

use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;
use url::Url;

const DEFAULT_LOOKUP_DOMAIN: &'static str = "dev.golem.network";

/// Resolves prefixes in the `DEFAULT_LOOKUP_DOMAIN`, see also `resolve_record`
pub async fn resolve_yagna_srv_record(prefix: &str) -> std::io::Result<String> {
    resolve_srv_record(&format!(
        "{}.{}",
        prefix.trim_end_matches('.'),
        DEFAULT_LOOKUP_DOMAIN
    ))
    .await
}

/// Performs lookup of the Service Record (SRV) in the Domain Name System
/// If successful responds in the format of `hostname:port`
pub async fn resolve_srv_record(record: &str) -> std::io::Result<String> {
    let resolver: TokioAsyncResolver =
        TokioAsyncResolver::tokio(ResolverConfig::google(), ResolverOpts::default()).await?;
    let lookup = resolver.srv_lookup(record).await?;
    let srv = lookup
        .iter()
        .next()
        .ok_or_else(|| IoError::from(IoErrorKind::NotFound))?;
    let addr = format!(
        "{}:{}",
        srv.target().to_string().trim_end_matches('.'),
        srv.port()
    );

    log::debug!("Resolved address: {}", addr);
    Ok(addr)
}

/// Replace domain name in URL with resolved IP address
/// Hack required on windows to bypass failing resolution on Windows 10
/// Not needed when https://github.com/actix/actix-web/issues/1047 is resolved
pub async fn resolve_dns_record(request_url: &str) -> anyhow::Result<String> {
    let request_host = Url::parse(request_url)?
        .host()
        .ok_or(anyhow::anyhow!("Invalid url: {}", request_url))?
        .to_string();

    let address = resolve_dns_record_host(&request_host).await?;
    Ok(request_url.replace(&request_host, &address))
}

pub async fn resolve_dns_record_host(host: &str) -> anyhow::Result<String> {
    let resolver =
        TokioAsyncResolver::tokio(ResolverConfig::google(), ResolverOpts::default()).await?;

    let response = resolver.lookup_ip(host).await?;
    let address = response
        .iter()
        .next()
        .ok_or(anyhow::anyhow!("DNS resolution failed for host: {}", host))?
        .to_string();
    Ok(address)
}

/// Try resolving hostname with `resolve_dns_record` or `resolve_dns_record_host`.
/// Returns the original URL if it fails.
pub async fn try_resolve_dns_record(request_url_or_host: &str) -> String {
    lazy_static! {
        static ref SCHEME_RE: Regex = Regex::new("(?i)^[a-z0-9\\-\\.]+?:").unwrap();
    }
    match {
        if SCHEME_RE.is_match(request_url_or_host) {
            resolve_dns_record(request_url_or_host).await
        } else {
            resolve_dns_record_host(request_url_or_host).await
        }
    } {
        Ok(url) => url,
        Err(e) => {
            log::warn!(
                "Error resolving hostname: {} url={}",
                e,
                request_url_or_host
            );
            request_url_or_host.to_owned()
        }
    }
}

/// Resolve all known IP addresses of a given domain
pub async fn resolve_domain_name<T: FromIterator<IpAddr>>(domain: &str) -> anyhow::Result<T> {
    let resolver =
        TokioAsyncResolver::tokio(ResolverConfig::google(), ResolverOpts::default()).await?;
    let response = resolver.lookup_ip(domain).await?;
    Ok(T::from_iter(response.iter()))
}
