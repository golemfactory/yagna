use actix_cors::Cors;
use actix_web::dev::RequestHead;
use actix_web::http::header::HeaderValue;
use actix_web_httpauth::headers::authorization::{Bearer, Scheme};

use actix_web::http::{header, Method};
use std::sync::Arc;
use structopt::StructOpt;

use crate::middleware::auth::resolver::AppKeyCache;

#[derive(Default, Clone, StructOpt, Debug)]
pub struct CorsConfig {
    #[structopt(long = "api-allow-origin", env = "YAGNA_API_ALLOW_ORIGIN")]
    allowed_origins: Vec<String>,
    /// Set a maximum time (in seconds) for which this CORS request may be cached.
    #[structopt(long, default_value = "3600")]
    max_age: usize,
}

#[derive(Clone)]
pub struct AppKeyCors {
    /// Holds AppKey and Allowed Origins list.
    cache: AppKeyCache,
    config: Arc<CorsConfig>,
}

impl AppKeyCors {
    pub async fn new(config: &CorsConfig) -> anyhow::Result<AppKeyCors> {
        let cache = AppKeyCache::new().await?;
        Ok(AppKeyCors {
            cache,
            config: Arc::new(config.clone()),
        })
    }

    pub fn cache(&self) -> AppKeyCache {
        self.cache.clone()
    }

    pub fn cors(&self) -> Cors {
        let this = self.clone();
        let config = self.config.clone();

        Cors::default()
            .allowed_origin_fn(move |header, request| this.verify_origin(header, request))
            .allow_any_method()
            .allow_any_header()
            .block_on_origin_mismatch(false)
            .max_age(config.max_age)
    }

    fn list_all_potential_origins(&self) -> Vec<String> {
        self.config
            .allowed_origins
            .iter()
            .cloned()
            .chain(self.cache.list_all_potential_origins().iter().cloned())
            .collect()
    }

    fn verify_origin(&self, origin: &HeaderValue, request: &RequestHead) -> bool {
        // Most browsers don't include authorization token in OPTIONS request.
        if request.method == Method::OPTIONS {
            // We should check if origin domain has chance of being allowed.
            // That's why we check if origin exists in any of domains lists.
            // Later calls will be checked against token.
            if origin_match_list(origin, &self.list_all_potential_origins()) {
                return true;
            }
        }

        // First check default origins
        if origin_match_list(origin, &self.config.allowed_origins) {
            return true;
        }

        // If non of default origins matches, we try with per app-key origins.
        let key = request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|header| Bearer::parse(header).ok())
            .map(|bearer| bearer.token().to_string());

        match key {
            Some(key) => self
                .cache
                .get_allowed_origins(&key)
                .into_iter()
                .any(|allowed| origin_match(origin, &allowed)),
            None => {
                log::debug!("App-key token not found in request");
                false
            }
        }
    }
}

fn origin_match(origin: &HeaderValue, allowed: &str) -> bool {
    if let Ok(origin) = origin.to_str() {
        if allowed == "*" {
            return true;
        }
        if origin == allowed {
            return true;
        }
    }
    false
}

fn origin_match_list(origin: &HeaderValue, allowed_list: &Vec<String>) -> bool {
    for allowed_origin in allowed_list {
        if origin_match(origin, allowed_origin) {
            return true;
        }
    }
    false
}
