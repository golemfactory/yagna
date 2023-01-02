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
    allowed_origins: Vec<String>,
    /// Set a maximum time (in seconds) for which this CORS request may be cached.
    #[structopt(long, default_value = "3600")]
    max_age: usize,
}

#[derive(Clone)]
pub struct AppKeyCors {
    /// Holds AppKey and Allowed Origins pairs.
    cors: Arc<RwLock<HashMap<String, Vec<String>>>>,
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
            .map(|appkey| {
                let key = appkey.key.clone();
                let origins = appkey
                    .allow_origins
                    .into_iter()
                    .map(move |origin| origin)
                    .collect::<Vec<_>>();
                (key, origins)
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

        for allowed_origin in &config.allowed_origins {
            if allowed_origin == "*" {
                cors = cors.send_wildcard()
            } else {
                cors = cors.allowed_origin(&allowed_origin);
            }
        }

        cors
    }

    fn get(&self, key: &str) -> Vec<String> {
        match self.cors.read() {
            Ok(cors) => cors.get(key).cloned().unwrap_or(vec![]),
            Err(_) => vec![],
        }
    }

    fn update(&self, key: &str, origins: Vec<String>) {
        if let Ok(mut cors) = self.cors.write() {
            match origins.is_empty() {
                true => cors.remove(key),
                false => cors.insert(key.to_string(), origins),
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
                            appkey.allow_origins
                        );
                        this.update(&appkey.key, appkey.allow_origins)
                    }
                    model::event::Event::DroppedKey(appkey) => {
                        log::debug!("Removing CORS for app-key: {}", appkey.name);
                        this.update(&appkey.key, vec![])
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

        match key {
            Some(key) => self.get(&key).into_iter().any(|allowed| {
                if let Ok(origin) = origin.to_str() {
                    if origin == "*" {
                        return true;
                    }
                    if origin == allowed {
                        return true;
                    }
                }
                return false;
            }),
            None => {
                log::debug!("App-key token not found in request");
                false
            }
        }
    }
}
