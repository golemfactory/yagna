use std::io::{Error as IoError, ErrorKind as IoErrorKind};
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

const DEFAULT_LOOKUP_DOMAIN: &'static str = "dev.golem.network";

pub async fn resolve_record(record: &str) -> std::io::Result<String> {
    let resolver: TokioAsyncResolver =
        TokioAsyncResolver::tokio(ResolverConfig::google(), ResolverOpts::default()).await?;
    let lookup = resolver
        .srv_lookup(format!(
            "{}.{}",
            record.trim_end_matches('.'),
            DEFAULT_LOOKUP_DOMAIN
        ))
        .await?;
    let srv = lookup
        .iter()
        .next()
        .ok_or_else(|| IoError::from(IoErrorKind::NotFound))?;
    let addr = format!(
        "{}:{}",
        srv.target().to_string().trim_end_matches('.'),
        srv.port()
    );

    log::debug!("Central net address: {}", addr);
    Ok(addr)
}
