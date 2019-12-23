//! Web utils
use awc::{
    http::{header, HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    ClientRequest, ClientResponse, SendClientRequest,
};
use bytes::Bytes;
use futures::compat::Future01CompatExt;
use serde::{de::DeserializeOwned, Serialize};
use std::{str::FromStr, time::Duration};
use url::form_urlencoded;

use crate::{configuration::ApiConfiguration, Error, Result};

#[derive(Clone, Debug)]
pub enum WebAuth {
    Bearer(String),
}

/// Convenient wrapper for the [`awc::Client`](
/// https://docs.rs/awc/0.2.8/awc/struct.Client.html) with builder.
pub struct WebClient {
    pub(crate) configuration: ApiConfiguration,
    pub(crate) awc: awc::Client,
}

pub struct WebRequest<T> {
    inner_request: T,
    url: String,
}

impl WebClient {
    pub fn builder() -> WebClientBuilder {
        WebClientBuilder::default()
    }

    fn url<T: Into<String>>(&self, suffix: T) -> url::Url {
        self.configuration.endpoint_url(suffix)
    }

    pub fn request(&self, method: Method, url: &str) -> WebRequest<ClientRequest> {
        let url = format!("{}", self.url(url));
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

fn handle_http_status<T>(response: ClientResponse<T>) -> Result<ClientResponse<T>> {
    match response.status() {
        StatusCode::OK | StatusCode::CREATED | StatusCode::ACCEPTED => Ok(response),
        status => Err(Error::HttpStatusCode(status)),
    }
}

impl WebRequest<SendClientRequest> {
    pub async fn json<T: DeserializeOwned>(self) -> Result<T> {
        let url = self.url.clone();
        let response = self
            .inner_request
            .compat()
            .await
            .map_err(|e| (e, url).into());

        // allow empty body and no content (201) to pass smoothly
        if let Ok(response) = &response {
            if StatusCode::NO_CONTENT == response.status() {
                return Ok(serde_json::from_str("\"[ NO CONTENT (http: 201) ]\"")?);
            }

            if let Some(Ok(Ok(0))) = response
                .headers()
                .get(header::CONTENT_LENGTH)
                .map(HeaderValue::to_str)
                .map(|r| r.map(usize::from_str))
            {
                return Ok(serde_json::from_str(&format!(
                    "\"[ EMPTY BODY (http: {}) ]\"",
                    response.status()
                ))?);
            }
        }

        response
            .and_then(handle_http_status)?
            .json()
            .compat()
            .await
            .map_err(From::from)
    }

    pub async fn body(self) -> Result<Bytes> {
        let url = self.url.clone();
        self.inner_request
            .compat()
            .await
            .map_err(|e| (e, url).into())
            .and_then(handle_http_status)?
            .body()
            .compat()
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
            configuration: ApiConfiguration::from(self.host_port, self.api_root)?,
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
