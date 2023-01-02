use actix_cors::Cors;
use actix_web::dev::RequestHead;
use actix_web::http::header::HeaderValue;
use actix_web_httpauth::headers::authorization::{Bearer, Scheme};

use actix_web::http::{header, Method};
use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use structopt::StructOpt;

use crate::middleware::auth::resolver::AppKeyResolver;

use ya_core_model::appkey as model;
use ya_service_api_cache::AutoResolveCache;
use ya_service_bus::RpcEndpoint;
use ya_service_bus::{actix_rpc, typed as bus};

pub const BUS_ID: &str = "/local/middleware/cors";

pub type Cache = AutoResolveCache<AppKeyResolver>;

#[derive(Clone, StructOpt, Debug)]
pub struct CorsConfig {
    #[structopt(long = "api-allow-origin")]
    allowed_origin: Option<String>,
    /// Set a maximum time (in seconds) for which this CORS request may be cached.
    #[structopt(long, default_value = "3600")]
    max_age: usize,
}

#[derive(Clone)]
pub struct AppKeyCors {
    /// Holds AppKey and Allowed Origins pairs.
    cors: Arc<RwLock<HashMap<String, String>>>,
    config: Arc<CorsConfig>,
}

impl AppKeyCors {
    pub async fn new(config: &CorsConfig) -> anyhow::Result<AppKeyCors> {
        let mut page = 1;
        let mut appkeys = vec![];

        loop {
            let (mut keys, pages) = actix_rpc::service(model::BUS_ID)
                .send(model::List {
                    identity: None,
                    page,
                    per_page: 20,
                })
                .await
                .map_err(|e| anyhow!("Failed to query app-keys: {e}"))??;
            appkeys.append(&mut keys);

            if page == pages {
                break;
            } else {
                page = page + 1;
            }
        }

        let mapping = appkeys
            .into_iter()
            .filter_map(|appkey| match appkey.allow_origin {
                Some(origin) => Some((appkey.key, origin)),
                None => None,
            })
            .collect::<HashMap<_, _>>();

        let appkey_cache = AppKeyCors {
            cors: Arc::new(RwLock::new(mapping)),
            config: Arc::new(config.clone()),
        };
        appkey_cache
            .listen_events()
            .await
            .map_err(|e| anyhow!("Can't build cors middleware: {e}"))?;
        Ok(appkey_cache)
    }

    pub fn cors(&self) -> Cors {
        let this = self.clone();
        let config = self.config.clone();

        let mut cors = Cors::default()
            .allowed_origin_fn(move |header, request| this.verify_origin(header, request))
            .allow_any_method()
            .allow_any_header()
            .block_on_origin_mismatch(false)
            .max_age(config.max_age);

        if let Some(allowed_origin) = config.allowed_origin.clone() {
            if allowed_origin == "*" {
                cors = cors.send_wildcard()
            } else {
                cors = cors.allowed_origin(&allowed_origin);
            }
        }
        cors
    }

    fn get(&self, key: &str) -> Option<String> {
        match self.cors.read() {
            Ok(cors) => cors.get(key).cloned(),
            Err(_) => None,
        }
    }

    fn update(&self, key: &str, origins: Option<String>) {
        if let Ok(mut cors) = self.cors.write() {
            match origins {
                None => cors.remove(key),
                Some(origins) => cors.insert(key.to_string(), origins.to_string()),
            };
        }
    }

    pub async fn listen_events(&self) -> anyhow::Result<()> {
        let this = self.clone();
        let endpoint = BUS_ID.to_string();

        let _ = bus::bind(&endpoint, move |event: model::event::Event| {
            let this = this.clone();

            async move {
                match event {
                    model::event::Event::NewKey(appkey) => {
                        log::debug!(
                            "Updating CORS for app-key: {}, origin: {:?}",
                            appkey.name,
                            appkey.allow_origin
                        );
                        this.update(&appkey.key, None)
                    }
                    model::event::Event::DroppedKey(appkey) => {
                        log::debug!("Removing CORS for app-key: {}", appkey.name);
                        this.update(&appkey.key, None)
                    }
                };
                Ok(())
            }
        });
        bus::service(model::BUS_ID)
            .send(model::Subscribe { endpoint })
            .await??;
        Ok(())
    }

    fn verify_origin(&self, origin: &HeaderValue, request: &RequestHead) -> bool {
        // Most browsers don't include authorization token in OPTIONS request.
        if request.method == Method::OPTIONS {
            // TODO: We should check if origin domain has chance of being allowed.
            //       That's why we check if origin exists in any of domains lists.
            //       Later calls will be checked against token.
            return true;
        }

        let key = request
            .headers()
            .get(header::AUTHORIZATION)
            .map(|header| Bearer::parse(header).ok())
            .flatten()
            .map(|bearer| bearer.token().to_string());

        log::debug!("Checking cors");
        match key {
            Some(key) => match self.get(&key) {
                None => {
                    log::debug!("Origin for appkey {key} not found");
                    false
                }
                Some(domain) => {
                    log::debug!("Checking cors policy for appkey: {key}");
                    if let Ok(origin) = origin.to_str() {
                        log::debug!("Cors: checking request origin ({origin}) with appkey allowed list: {domain}");
                        if origin == "*" {
                            return true;
                        }
                        if domain == origin {
                            return true;
                        }
                    }
                    false
                }
            },
            None => {
                log::debug!("App-key token not found in request");
                false
            }
        }
    }
}
