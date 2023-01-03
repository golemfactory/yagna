use anyhow::anyhow;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use ya_core_model::appkey as model;
use ya_core_model::appkey::AppKey;
use ya_service_bus::RpcEndpoint;
use ya_service_bus::{actix_rpc, typed as bus};

pub const BUS_ID: &str = "/local/middleware/auth";

#[derive(Clone)]
pub struct AppKeyCache {
    appkeys: Arc<RwLock<HashMap<String, AppKey>>>,
}

impl AppKeyCache {
    pub async fn new() -> anyhow::Result<AppKeyCache> {
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
            .map(|appkey| (appkey.key.clone(), appkey))
            .collect::<HashMap<_, _>>();

        let appkey_cache = AppKeyCache {
            appkeys: Arc::new(RwLock::new(mapping)),
        };
        appkey_cache
            .listen_events()
            .await
            .map_err(|e| anyhow!("Can't build cors middleware: {e}"))?;
        Ok(appkey_cache)
    }

    pub fn get_appkey(&self, key: &str) -> Option<AppKey> {
        match self.appkeys.read() {
            Ok(keymap) => keymap.get(key).map(|appkey| appkey.clone()),
            Err(_) => None,
        }
    }

    pub fn get_allowed_origins(&self, key: &str) -> Vec<String> {
        match self.appkeys.read() {
            Ok(keymap) => keymap
                .get(key)
                .map(|appkey| appkey.allow_origins.clone())
                .unwrap_or(vec![]),
            Err(_) => vec![],
        }
    }

    fn update(&self, key: &str, appkey: Option<AppKey>) {
        if let Ok(mut keymap) = self.appkeys.write() {
            match appkey {
                Some(appkey) => keymap.insert(key.to_string(), appkey),
                None => keymap.remove(key),
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
                        this.update(&appkey.key.clone(), Some(appkey))
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
}
