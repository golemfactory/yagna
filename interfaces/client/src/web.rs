//! Web utils
use awc::{
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    ClientRequest, ClientResponse, SendClientRequest,
};
use bytes::Bytes;
use serde::{de::DeserializeOwned, Serialize};
use std::{rc::Rc, str::FromStr, time::Duration};
use url::{form_urlencoded, Url};

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub enum WebAuth {
    Bearer(String),
}

/// Convenient wrapper for the [`awc::Client`](
/// https://docs.rs/awc/0.2.8/awc/struct.Client.html) with builder.
#[derive(Clone)]
pub struct WebClient {
    base_url: Rc<Url>,
    awc: awc::Client,
}

pub trait WebInterface {
    const API_URL_ENV_VAR: &'static str;
    const API_SUFFIX: &'static str;

    fn rebase_service_url(base_url: Rc<Url>) -> Result<Rc<Url>> {
        if let Some(url) = std::env::var(Self::API_URL_ENV_VAR).ok() {
            return Ok(Url::from_str(&url)?.into());
        }
        Ok(base_url.join(Self::API_SUFFIX)?.into())
    }

    fn from(client: WebClient) -> Self;
}

pub struct WebRequest<T> {
    inner_request: T,
    url: String,
}

impl WebClient {
    pub fn builder() -> WebClientBuilder {
        WebClientBuilder::default()
    }

    pub fn with_token(token: &str) -> Result<WebClient> {
        WebClientBuilder::default()
            .auth(WebAuth::Bearer(token.to_string()))
            .build()
    }

    fn url<T: AsRef<str>>(&self, suffix: T) -> Result<url::Url> {
        Ok(self.base_url.join(suffix.as_ref())?)
    }

    pub fn request(&self, method: Method, url: &str) -> WebRequest<ClientRequest> {
        let url = self.url(url).unwrap().to_string();
        log::info!("doing {} on {}", method, url);
        WebRequest {
            inner_request: self.awc.request(method, &url),
            url,
        }
    }

    pub fn get(&self, url: &str) -> WebRequest<ClientRequest> {
        self.request(Method::GET, url)
    }

    pub fn post(&self, url: &str) -> WebRequest<ClientRequest> {
        self.request(Method::POST, url)
    }

    pub fn put(&self, url: &str) -> WebRequest<ClientRequest> {
        self.request(Method::PUT, url)
    }

    pub fn delete(&self, url: &str) -> WebRequest<ClientRequest> {
        self.request(Method::DELETE, url)
    }

    pub fn interface<T: WebInterface>(&self) -> Result<T> {
        let base_url = T::rebase_service_url(self.base_url.clone())?;
        let awc = self.awc.clone();
        Ok(T::from(WebClient { base_url, awc }))
    }

    pub fn interface_at<T: WebInterface>(&self, base_url: Url) -> T {
        let awc = self.awc.clone();
        T::from(WebClient {
            base_url: base_url.into(),
            awc,
        })
    }
}

impl WebRequest<ClientRequest> {
    pub fn send_json<T: Serialize>(self, value: &T) -> WebRequest<SendClientRequest> {
        WebRequest {
            inner_request: self.inner_request.send_json(value),
            url: self.url,
        }
    }

    pub fn send(self) -> WebRequest<SendClientRequest> {
        WebRequest {
            inner_request: self.inner_request.send(),
            url: self.url,
        }
    }
}

fn filter_http_status<T>(response: ClientResponse<T>) -> Result<ClientResponse<T>> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(Error::HttpStatusCode(response.status()))
    }
}

impl WebRequest<SendClientRequest> {
    pub async fn json<T: DeserializeOwned>(self) -> Result<T> {
        let url = self.url.clone();
        let mut response = self
            .inner_request
            .await
            .map_err(|e| (e, url).into())
            .and_then(filter_http_status)?;

        log::debug!("{:?}", response.headers());
        // allow empty body and no content (204) to pass smoothly
        if StatusCode::NO_CONTENT == response.status()
            || Some("0")
                == response
                    .headers()
                    .get(header::CONTENT_LENGTH)
                    .and_then(|h| h.to_str().ok())
        {
            return Ok(serde_json::from_str(&format!(
                "\"[ EMPTY BODY (http: {}) ]\"",
                response.status()
            ))?);
        }

        response.json().await.map_err(From::from)
    }

    pub async fn body(self) -> Result<Bytes> {
        let url = self.url.clone();
        self.inner_request
            .await
            .map_err(|e| (e, url).into())
            .and_then(filter_http_status)?
            .body()
            .await
            .map_err(From::from)
    }
}

#[derive(Clone, Debug)]
pub struct WebClientBuilder {
    pub(crate) host_port: Option<String>,
    pub(crate) api_root: Option<String>,
    pub(crate) auth: Option<WebAuth>,
    pub(crate) headers: HeaderMap,
    pub(crate) timeout: Option<Duration>,
}

impl WebClientBuilder {
    pub fn auth(mut self, auth: WebAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn host_port<T: Into<String>>(mut self, host_port: T) -> Self {
        self.host_port = Some(host_port.into());
        self
    }

    pub fn api_root<T: Into<String>>(mut self, api_root: T) -> Self {
        self.api_root = Some(api_root.into());
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn header(mut self, name: String, value: String) -> Result<Self> {
        let name = HeaderName::from_str(name.as_str())?;
        let value = HeaderValue::from_str(value.as_str())?;

        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn build(self) -> Result<WebClient> {
        let mut builder = awc::Client::build();

        if let Some(timeout) = self.timeout {
            builder = builder.timeout(timeout);
        }
        if let Some(auth) = &self.auth {
            builder = match auth {
                WebAuth::Bearer(token) => builder.bearer_auth(token),
            }
        }
        for (key, value) in self.headers.iter() {
            builder = builder.header(key.clone(), value.clone());
        }

        Ok(WebClient {
            base_url: Rc::new(Url::parse(&format!(
                "http://{}",
                self.host_port.unwrap_or_else(|| "127.0.0.1:5001".into())
            ))?),
            awc: builder.finish(),
        })
    }
}

impl Default for WebClientBuilder {
    fn default() -> Self {
        WebClientBuilder {
            host_port: None,
            api_root: None,
            auth: None,
            headers: HeaderMap::new(),
            timeout: None,
        }
    }
}

/// Builder for the query part of the URLs.
pub struct QueryParamsBuilder<'a> {
    serializer: form_urlencoded::Serializer<'a, String>,
}

impl<'a> QueryParamsBuilder<'a> {
    pub fn new() -> Self {
        let serializer = form_urlencoded::Serializer::new("".into());
        QueryParamsBuilder { serializer }
    }

    pub fn put<N: ToString, V: ToString>(mut self, name: N, value: Option<V>) -> Self {
        let value = match value {
            Some(v) => v.to_string(),
            None => String::new(),
        };

        self.serializer
            .append_pair(name.to_string().as_str(), value.as_str());
        self
    }

    pub fn build(mut self) -> String {
        self.serializer.finish()
    }
}
