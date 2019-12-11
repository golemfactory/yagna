use crate::{configuration::ApiConfiguration, Result};
use awc::http::{HeaderMap, HeaderName, HeaderValue};
use std::str::FromStr;
use std::time::Duration;
use url::form_urlencoded;

#[derive(Clone, Debug)]
pub enum WebAuth {
    Bearer(String),
}

pub struct WebClient {
    pub(crate) configuration: ApiConfiguration,
    pub(crate) awc: awc::Client,
}

impl WebClient {
    pub fn builder() -> WebClientBuilder {
        WebClientBuilder::default()
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
