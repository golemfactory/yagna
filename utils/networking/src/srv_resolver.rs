use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

const DEFAULT_LOOKUP_DOMAIN: &'static str = "dev.golem.network";

/// Resolves prefixes in the `DEFAULT_LOOKUP_DOMAIN`, see also `resolve_record`
pub async fn resolve_yagna_record(prefix: &str) -> std::io::Result<String> {
    resolve_record(format!(
        "{}.{}",
        prefix.trim_end_matches('.'),
        DEFAULT_LOOKUP_DOMAIN
    ))
    .await
}

/// Performs lookup of the Service Record (SRV) in the Domain Name System
/// If successful responds in the format of `hostname:port`
pub async fn resolve_record(record: String) -> std::io::Result<String> {
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
