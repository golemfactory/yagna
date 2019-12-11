use crate::error::Error;
use awc::http::{HeaderMap, HeaderName, HeaderValue};
use std::str::FromStr;
use std::time::Duration;
use url::form_urlencoded;

const API_HOST: &str = "http://localhost:5001";

#[derive(Clone, Debug)]
pub enum WebAuth {
    Bearer(String),
}

pub struct WebClient {
    pub(crate) endpoint: String,
    pub(crate) awc: awc::Client,
}

impl WebClient {
    pub fn builder() -> WebClientBuilder {
        WebClientBuilder::default()
    }
}

#[derive(Clone, Debug)]
pub struct WebClientBuilder {
    pub(crate) endpoint: Option<String>,
    pub(crate) auth: Option<WebAuth>,
    pub(crate) headers: HeaderMap,
    pub(crate) timeout: Option<Duration>,
}

impl WebClientBuilder {
    pub fn auth(mut self, auth: WebAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    pub fn endpoint(mut self, endpoint: String) -> Self {
        self.endpoint = Some(endpoint);
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn header(mut self, name: String, value: String) -> Result<Self, Error> {
        let name = match HeaderName::from_str(name.as_str()) {
            Ok(name) => name,
            Err(e) => return Err(Error::HeaderError(format!("{:?}", e))),
        };
        let value = match HeaderValue::from_str(value.as_str()) {
            Ok(value) => value,
            Err(e) => return Err(Error::HeaderError(format!("{:?}", e))),
        };

        self.headers.insert(name, value);
        Ok(self)
    }

    pub fn build(self) -> WebClient {
        let mut builder = awc::Client::build();
        let endpoint = match self.endpoint {
            Some(endpoint) => endpoint,
            None => API_HOST.to_string(),
        };

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

        WebClient {
            endpoint,
            awc: builder.finish(),
        }
    }
}

impl Default for WebClientBuilder {
    fn default() -> Self {
        WebClientBuilder {
            endpoint: None,
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
        let serializer = form_urlencoded::Serializer::new(String::new());
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
