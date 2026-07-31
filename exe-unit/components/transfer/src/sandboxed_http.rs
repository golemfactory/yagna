use actix_http::encoding::Decoder;
use actix_http::header::{self, HeaderName, HeaderValue, TryIntoHeaderPair};
use actix_http::{Method, Payload, StatusCode};
use awc::ClientResponse;
use bytes::Bytes;
use futures::Stream;
use ipnet::IpNet;
use std::cell::OnceCell;
use std::env;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::rc::Rc;
use trust_dns_resolver::config::{ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;
use url::Url;

use crate::error::{Error, HttpError};

const ALLOWED_IPS_ENV: &str = "YA_TRANSFER_ALLOWED_IPS";
const MAX_REDIRECTS_ENV: &str = "YA_TRANSFER_MAX_REDIRECTS";
const DEFAULT_MAX_REDIRECTS: usize = 5;

pub type SandboxedHttpResponse = ClientResponse<Decoder<Payload>>;

/// HTTP client restricted to public Internet destinations.
///
/// DNS queries use Cloudflare DNS-over-HTTPS without consulting the system
/// resolver or hosts file. The validated address is pinned to the request.
#[derive(Clone)]
pub struct SandboxedHttpClient {
    inner: awc::Client,
    resolver: Rc<OnceCell<Result<TokioAsyncResolver, String>>>,
    allowed_networks: Rc<[IpNet]>,
    max_redirects: usize,
}

impl Default for SandboxedHttpClient {
    fn default() -> Self {
        SandboxedHttpClientBuilder::new().build()
    }
}

pub struct SandboxedHttpClientBuilder {
    allowed_networks: Vec<IpNet>,
    max_redirects: usize,
}

impl Default for SandboxedHttpClientBuilder {
    fn default() -> Self {
        Self {
            allowed_networks: Vec::new(),
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }
}

impl SandboxedHttpClientBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allows destinations from an additional IP network.
    ///
    /// Public Internet destinations are always allowed. By default no
    /// non-public networks are allowed.
    pub fn allow_network(mut self, network: IpNet) -> Self {
        self.allowed_networks.push(network);
        self
    }

    pub fn allow_networks(mut self, networks: impl IntoIterator<Item = IpNet>) -> Self {
        self.allowed_networks.extend(networks);
        self
    }

    /// Sets the number of redirects allowed for requests without a streaming body.
    ///
    /// Each redirect target is resolved, validated and pinned independently.
    /// The default limit is five. Streaming requests never follow redirects
    /// because their bodies cannot be safely replayed.
    pub fn max_redirects(mut self, max_redirects: usize) -> Self {
        self.max_redirects = max_redirects;
        self
    }

    pub fn build(self) -> SandboxedHttpClient {
        SandboxedHttpClient {
            inner: awc::ClientBuilder::new().disable_redirects().finish(),
            resolver: Rc::new(OnceCell::new()),
            allowed_networks: self.allowed_networks.into(),
            max_redirects: self.max_redirects,
        }
    }

    fn from_env() -> Self {
        let mut builder = Self::new();

        if let Ok(networks) = env::var(ALLOWED_IPS_ENV) {
            for value in networks.split(',').map(str::trim).filter(|v| !v.is_empty()) {
                match value.parse() {
                    Ok(network) => builder.allowed_networks.push(network),
                    Err(error) => log::error!(
                        "Ignoring invalid network '{value}' in {ALLOWED_IPS_ENV}: {error}"
                    ),
                }
            }
        }

        if let Ok(value) = env::var(MAX_REDIRECTS_ENV) {
            match value.parse() {
                Ok(max_redirects) => builder.max_redirects = max_redirects,
                Err(error) => {
                    log::error!("Ignoring invalid value '{value}' in {MAX_REDIRECTS_ENV}: {error}")
                }
            }
        }

        builder
    }
}

impl SandboxedHttpClient {
    pub fn builder() -> SandboxedHttpClientBuilder {
        SandboxedHttpClientBuilder::new()
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Builds a client from `YA_TRANSFER_ALLOWED_IPS` and
    /// `YA_TRANSFER_MAX_REDIRECTS`.
    pub fn from_env() -> Self {
        SandboxedHttpClientBuilder::from_env().build()
    }

    pub fn request(&self, method: Method, url: Url) -> SandboxedHttpRequest {
        SandboxedHttpRequest {
            client: self.clone(),
            method,
            url,
            headers: Vec::new(),
        }
    }

    pub async fn get(&self, url: Url) -> Result<SandboxedHttpResponse, Error> {
        self.request(Method::GET, url).send().await
    }

    fn resolver(&self) -> Result<&TokioAsyncResolver, Error> {
        match self.resolver.get_or_init(|| {
            let mut options = ResolverOpts::default();
            options.use_hosts_file = false;
            TokioAsyncResolver::tokio(ResolverConfig::cloudflare_https(), options)
                .map_err(|error| error.to_string())
        }) {
            Ok(resolver) => Ok(resolver),
            Err(error) => Err(HttpError::Dns(error.clone()).into()),
        }
    }

    async fn resolve(&self, url: &Url) -> Result<SocketAddr, Error> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::UnsupportedSchemeError(url.scheme().to_string()));
        }

        let host = url
            .host()
            .ok_or_else(|| Error::InvalidUrlError(format!("missing host in URL: {url}")))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| Error::InvalidUrlError(format!("missing port for URL scheme: {url}")))?;

        let addresses = match host {
            url::Host::Ipv4(address) => vec![IpAddr::V4(address)],
            url::Host::Ipv6(address) => vec![IpAddr::V6(address)],
            url::Host::Domain(domain) => self
                .resolver()?
                .lookup_ip(domain)
                .await
                .map_err(|error| HttpError::Dns(error.to_string()))?
                .iter()
                .collect(),
        };

        select_address(addresses, port, &self.allowed_networks)
    }
}

pub struct SandboxedHttpRequest {
    client: SandboxedHttpClient,
    method: Method,
    url: Url,
    headers: Vec<(HeaderName, HeaderValue)>,
}

impl SandboxedHttpRequest {
    pub fn insert_header(mut self, header: impl TryIntoHeaderPair) -> Result<Self, Error> {
        let header = header
            .try_into_pair()
            .map_err(|error| HttpError::Client(error.into().to_string()))?;
        self.headers.push(header);
        Ok(self)
    }

    pub async fn send(self) -> Result<SandboxedHttpResponse, Error> {
        let mut method = self.method.clone();
        let mut url = self.url.clone();
        let mut redirects_left = self.client.max_redirects;
        let mut include_sensitive_headers = true;

        loop {
            let response = self
                .prepare(method.clone(), &url, include_sensitive_headers)
                .await?
                .send()
                .await?;
            let status = response.status();

            if !is_followable_redirect(status) {
                return validate_response(response);
            }
            if redirects_left == 0 {
                return Err(
                    HttpError::Client(format!("HTTP redirect limit exceeded ({status})")).into(),
                );
            }

            let location = response
                .headers()
                .get(header::LOCATION)
                .ok_or_else(|| {
                    HttpError::Client(format!("HTTP redirect missing Location ({status})"))
                })?
                .to_str()
                .map_err(|error| {
                    HttpError::Client(format!("invalid HTTP redirect Location: {error}"))
                })?;
            let next_url = url.join(location)?;

            include_sensitive_headers &= same_origin(&url, &next_url);
            method = redirect_method(status, method);
            url = next_url;
            redirects_left -= 1;
        }
    }

    pub async fn send_stream<S, E>(self, stream: S) -> Result<SandboxedHttpResponse, Error>
    where
        S: Stream<Item = Result<Bytes, E>> + 'static,
        E: Into<Box<dyn std::error::Error>> + 'static,
    {
        let request = self.prepare(self.method.clone(), &self.url, true).await?;
        validate_response(request.send_stream(stream).await?)
    }

    async fn prepare(
        &self,
        method: Method,
        url: &Url,
        include_sensitive_headers: bool,
    ) -> Result<awc::ClientRequest, Error> {
        let address = self.client.resolve(url).await?;
        let mut request = self
            .client
            .inner
            .request(method, url.as_str())
            .address(address);

        for (name, value) in &self.headers {
            if include_sensitive_headers || !is_sensitive_header(name) {
                request = request.insert_header((name.clone(), value.clone()));
            }
        }

        if !url.username().is_empty() {
            request = request.basic_auth(url.username(), url.password().unwrap_or_default());
        }

        Ok(request)
    }
}

fn same_origin(left: &Url, right: &Url) -> bool {
    left.scheme() == right.scheme()
        && left.host() == right.host()
        && left.port_or_known_default() == right.port_or_known_default()
}

fn is_sensitive_header(name: &HeaderName) -> bool {
    matches!(
        name.as_str(),
        "authorization" | "cookie" | "proxy-authorization"
    )
}

fn is_followable_redirect(status: StatusCode) -> bool {
    matches!(
        status,
        StatusCode::MOVED_PERMANENTLY
            | StatusCode::FOUND
            | StatusCode::SEE_OTHER
            | StatusCode::TEMPORARY_REDIRECT
            | StatusCode::PERMANENT_REDIRECT
    )
}

fn redirect_method(status: StatusCode, method: Method) -> Method {
    match status {
        StatusCode::MOVED_PERMANENTLY | StatusCode::FOUND | StatusCode::SEE_OTHER
            if !matches!(method, Method::GET | Method::HEAD) =>
        {
            Method::GET
        }
        _ => method,
    }
}

fn validate_response(response: SandboxedHttpResponse) -> Result<SandboxedHttpResponse, Error> {
    validate_status(response.status())?;
    Ok(response)
}

fn validate_status(status: StatusCode) -> Result<(), HttpError> {
    if status.is_informational() || status.is_success() {
        Ok(())
    } else if status.is_redirection() {
        Err(HttpError::Client(format!(
            "HTTP redirects are disabled ({status})"
        )))
    } else if status.is_client_error() {
        Err(HttpError::Client(status.to_string()))
    } else {
        Err(HttpError::Server(status.to_string()))
    }
}

fn select_address(
    addresses: impl IntoIterator<Item = IpAddr>,
    port: u16,
    allowed_networks: &[IpNet],
) -> Result<SocketAddr, Error> {
    let mut blocked = None;
    for address in addresses {
        if is_public_ip(address) || is_allowed(address, allowed_networks) {
            return Ok(SocketAddr::new(address, port));
        }
        blocked.get_or_insert(address);
    }

    match blocked {
        Some(address) => Err(HttpError::NonPublicAddress(address).into()),
        None => {
            Err(HttpError::Dns("DNS response did not contain any IP addresses".to_string()).into())
        }
    }
}

fn is_allowed(address: IpAddr, allowed_networks: &[IpNet]) -> bool {
    allowed_networks.iter().any(|network| {
        network.contains(&address)
            || match address {
                IpAddr::V6(address) => address
                    .to_ipv4_mapped()
                    .map(|address| network.contains(&IpAddr::V4(address)))
                    .unwrap_or(false),
                IpAddr::V4(_) => false,
            }
    })
}

fn is_public_ip(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => is_public_ipv4(address),
        IpAddr::V6(address) => match address.to_ipv4_mapped() {
            Some(mapped) => is_public_ipv4(mapped),
            None => {
                let [first, second, ..] = address.segments();
                first & 0xe000 == 0x2000
                    && !(first == 0x2001 && second <= 0x01ff)
                    && !(first == 0x2001 && second == 0x0db8)
                    && first != 0x2002
                    && !(first == 0x3fff && second & 0xf000 == 0)
            }
        },
    }
}

fn is_public_ipv4(address: Ipv4Addr) -> bool {
    let [first, second, third, _] = address.octets();
    first != 0
        && !address.is_private()
        && !address.is_loopback()
        && !address.is_link_local()
        && !address.is_broadcast()
        && !address.is_documentation()
        && !(first == 100 && second & 0xc0 == 0x40)
        && [first, second, third] != [192, 0, 0]
        && [first, second, third] != [192, 88, 99]
        && !(first == 198 && second & 0xfe == 18)
        && !address.is_multicast()
        && first < 240
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{web, App, HttpResponse, HttpServer};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn accepts_public_unicast_addresses() {
        for address in [
            "1.1.1.1",
            "8.8.8.8",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
        ] {
            let address: IpAddr = address.parse().unwrap();
            assert!(is_public_ip(address), "{} should be public", address);
        }
    }

    #[test]
    fn rejects_non_public_and_special_use_addresses() {
        for address in [
            "0.0.0.0",
            "10.0.0.1",
            "100.64.0.1",
            "127.0.0.1",
            "169.254.1.1",
            "172.16.0.1",
            "192.0.0.1",
            "192.0.0.9",
            "192.0.0.10",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.0.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:192.168.0.1",
            "2001:2::1",
            "2001:db8::1",
            "fc00::1",
            "fe80::1",
            "ff0e::1",
        ] {
            let address: IpAddr = address.parse().unwrap();
            assert!(!is_public_ip(address), "{} should not be public", address);
        }
    }

    #[test]
    fn selects_only_public_addresses_from_dns_response() {
        let selected = select_address(
            ["192.168.0.1", "1.1.1.1"]
                .iter()
                .copied()
                .map(|address| address.parse().unwrap()),
            443,
            &[],
        )
        .unwrap();

        assert_eq!(selected, SocketAddr::new("1.1.1.1".parse().unwrap(), 443));
    }

    #[test]
    fn rejects_dns_response_with_only_non_public_addresses() {
        let error = select_address(
            ["127.0.0.1", "192.168.0.1"]
                .iter()
                .copied()
                .map(|address| address.parse().unwrap()),
            80,
            &[],
        )
        .unwrap_err();

        assert!(matches!(
            error,
            Error::HttpError(HttpError::NonPublicAddress(_))
        ));
    }

    #[actix_rt::test]
    async fn rejects_private_ip_literal() {
        let error = SandboxedHttpClient::new()
            .resolve(&Url::parse("http://192.168.0.1/resource").unwrap())
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            Error::HttpError(HttpError::NonPublicAddress(address))
                if address == "192.168.0.1".parse::<IpAddr>().unwrap()
        ));
    }

    #[actix_rt::test]
    async fn allows_only_configured_private_networks() {
        let client = SandboxedHttpClient::builder()
            .allow_network("192.168.0.0/24".parse().unwrap())
            .build();

        let allowed = client
            .resolve(&Url::parse("http://192.168.0.42/resource").unwrap())
            .await
            .unwrap();
        assert_eq!(allowed.ip(), "192.168.0.42".parse::<IpAddr>().unwrap());

        let error = client
            .resolve(&Url::parse("http://192.168.1.42/resource").unwrap())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::HttpError(HttpError::NonPublicAddress(address))
                if address == "192.168.1.42".parse::<IpAddr>().unwrap()
        ));
    }

    #[actix_rt::test]
    async fn redirects_are_limited_and_each_target_is_validated() {
        let target_hits = Arc::new(AtomicUsize::new(0));
        let server_hits = target_hits.clone();
        let server = HttpServer::new(move || {
            let target_hits = server_hits.clone();
            App::new()
                .route(
                    "/redirect",
                    web::get().to(|| async {
                        HttpResponse::Found()
                            .insert_header(("Location", "/target"))
                            .finish()
                    }),
                )
                .route(
                    "/target",
                    web::get().to(move || {
                        let target_hits = target_hits.clone();
                        async move {
                            target_hits.fetch_add(1, Ordering::SeqCst);
                            HttpResponse::Ok().finish()
                        }
                    }),
                )
                .route(
                    "/private-redirect",
                    web::get().to(|| async {
                        HttpResponse::Found()
                            .insert_header(("Location", "http://192.168.0.1/target"))
                            .finish()
                    }),
                )
                .route(
                    "/chain/{remaining}",
                    web::get().to(|remaining: web::Path<usize>| async move {
                        let remaining = remaining.into_inner();
                        if remaining == 0 {
                            HttpResponse::Ok().finish()
                        } else {
                            HttpResponse::Found()
                                .insert_header(("Location", format!("/chain/{}", remaining - 1)))
                                .finish()
                        }
                    }),
                )
        })
        .bind(("127.0.0.1", 0))
        .unwrap();
        let address = server.addrs()[0];
        let server = server.run();
        let handle = server.handle();
        actix_rt::spawn(server);

        let loopback: IpNet = "127.0.0.0/8".parse().unwrap();
        let client = SandboxedHttpClient::builder()
            .allow_network(loopback)
            .build();
        let response = client
            .get(Url::parse(&format!("http://{address}/redirect")).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(target_hits.load(Ordering::SeqCst), 1);

        let response = client
            .get(Url::parse(&format!("http://{address}/chain/5")).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let error = client
            .get(Url::parse(&format!("http://{address}/chain/6")).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::HttpError(HttpError::Client(message))
                if message.contains("redirect limit exceeded")
        ));

        let error = client
            .get(Url::parse(&format!("http://{address}/private-redirect")).unwrap())
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            Error::HttpError(HttpError::NonPublicAddress(address))
                if address == "192.168.0.1".parse::<IpAddr>().unwrap()
        ));

        handle.stop(false).await;
    }
}
