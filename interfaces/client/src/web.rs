//! Web utils
use actix_http::{encoding::Decoder, Payload};
use awc::{
    http::{HeaderMap, HeaderName, HeaderValue, HttpTryFrom, Uri},
    ClientRequest, ClientResponse, SendClientRequest,
};
use bytes::Bytes;
use futures::compat::Future01CompatExt;
use serde::{Deserialize, Serialize};
use std::{str::FromStr, time::Duration};
use url::form_urlencoded;

use crate::{configuration::ApiConfiguration, Result};
use serde::de::DeserializeOwned;

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

impl WebClient {
    pub fn builder() -> WebClientBuilder {
        WebClientBuilder::default()
    }

    pub fn get<U>(&self, url: U) -> WebRequest
    where
        Uri: HttpTryFrom<U>,
    {
        WebRequest(self.awc.get(url))
    }
    pub fn post<U>(&self, url: U) -> WebRequest
    where
        Uri: HttpTryFrom<U>,
    {
        WebRequest(self.awc.post(url))
    }

    pub fn put<U>(&self, url: U) -> WebRequest
    where
        Uri: HttpTryFrom<U>,
    {
        WebRequest(self.awc.put(url))
    }

    pub fn delete<U>(&self, url: U) -> WebRequest
    where
        Uri: HttpTryFrom<U>,
    {
        WebRequest(self.awc.delete(url))
    }
}

pub struct WebRequest(ClientRequest);

impl WebRequest {
    pub async fn send_json<T: Serialize>(self, value: &T) -> crate::Result<WebResponse> {
        self.0
            .send_json(value)
            .compat()
            .await
            .map_err(crate::Error::from)
            .map(|r| WebResponse(r))
    }

    pub async fn send(self) -> Result<WebResponse> {
        self.0
            .send()
            .compat()
            .await
            .map_err(crate::Error::from)
            .map(|r| WebResponse(r))
    }
}

pub struct WebResponse(ClientResponse<Decoder<Payload>>);

impl WebResponse {
    pub async fn json<T: DeserializeOwned>(&mut self) -> crate::Result<T> {
        self.0.json()
            .compat()
            .await
            .map_err(crate::Error::from)
    }

    pub async fn body<T>(&mut self) -> crate::Result<Bytes> {
        self.0.body()
            .compat()
            .await
            .map_err(crate::Error::from)
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
        let serializer = form_urlencoded::Serializer::new("?".into());
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
