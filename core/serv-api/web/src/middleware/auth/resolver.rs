use anyhow::{anyhow, bail};
use std::collections::HashMap;
use std::convert::TryFrom;
use std::sync::{Arc, RwLock};

use ya_core_model::appkey as model;
use ya_core_model::appkey::event::AppKeyEvent;
use ya_core_model::appkey::AppKey;
use ya_service_bus::{typed as bus, RpcEndpoint};

use super::ident::Identity;
use ya_client::model::NodeId;

pub const BUS_ID: &str = "/local/middleware/auth";

pub struct AdminCredential {
    token: String,
    principal: Identity,
}

impl AdminCredential {
    pub fn new(token: impl Into<String>, identity: NodeId) -> anyhow::Result<Self> {
        let token = token.into();
        if token.is_empty() {
            bail!("Administrator token cannot be empty");
        }
        Ok(Self {
            token,
            principal: Identity::admin(identity),
        })
    }

    fn token(&self) -> &str {
        &self.token
    }

    fn principal(&self) -> Identity {
        self.principal.clone()
    }
}

#[derive(Clone)]
pub struct AppKeyCache {
    appkeys: Arc<RwLock<HashMap<String, AppKey>>>,
    admin: Option<Arc<AdminCredential>>,
}

impl AppKeyCache {
    pub async fn new() -> anyhow::Result<AppKeyCache> {
        Self::new_with_admin(None).await
    }

    pub async fn new_with_admin(admin: Option<AdminCredential>) -> anyhow::Result<AppKeyCache> {
        let mut page = 1;
        let mut appkeys = vec![];

        log::trace!("AppKeyCache: asking Identity service for app-keys.");

        loop {
            let (mut keys, pages) = bus::service(model::BUS_ID)
                .send(model::List {
                    identity: None,
                    page,
                    per_page: 20,
                })
                .await
                .map_err(|e| anyhow!("Failed to query app-keys: {e}"))??;
            appkeys.append(&mut keys);

            if page >= pages {
                break;
            } else {
                page += 1;
            }
        }

        let appkey_cache = Self::from_appkeys(appkeys, admin)?;
        appkey_cache
            .listen_events()
            .await
            .map_err(|e| anyhow!("Can't build cors middleware: {e}"))?;
        Ok(appkey_cache)
    }

    fn from_appkeys(
        appkeys: Vec<AppKey>,
        admin: Option<AdminCredential>,
    ) -> anyhow::Result<AppKeyCache> {
        for appkey in &appkeys {
            Identity::try_from(appkey).map_err(|e| {
                anyhow!(
                    "Invalid persisted application key role for '{}': {e}",
                    appkey.name
                )
            })?;
        }

        let mapping = appkeys
            .into_iter()
            .map(|appkey| (appkey.key.clone(), appkey))
            .collect::<HashMap<_, _>>();

        if admin
            .as_ref()
            .map(|admin| mapping.contains_key(admin.token()))
            .unwrap_or(false)
        {
            bail!("Administrator token collides with an application key");
        }

        Ok(AppKeyCache {
            appkeys: Arc::new(RwLock::new(mapping)),
            admin: admin.map(Arc::new),
        })
    }

    pub fn resolve_bearer(&self, token: &str) -> Option<Identity> {
        if let Some(admin) = &self.admin {
            if admin.token() == token {
                return Some(admin.principal());
            }
        }
        self.resolve_manager(token)
    }

    pub fn resolve_query(&self, token: &str) -> Option<Identity> {
        self.resolve_manager(token)
    }

    fn resolve_manager(&self, token: &str) -> Option<Identity> {
        self.get_appkey(token)
            .and_then(|appkey| Identity::try_from(appkey).ok())
    }

    pub fn get_appkey(&self, key: &str) -> Option<AppKey> {
        match self.appkeys.read() {
            Ok(keymap) => keymap.get(key).cloned(),
            Err(_) => None,
        }
    }

    pub fn get_allowed_origins(&self, key: &str) -> Vec<String> {
        match self.appkeys.read() {
            Ok(keymap) => keymap
                .get(key)
                .map(|appkey| appkey.allow_origins.clone())
                .unwrap_or_default(),
            Err(_) => vec![],
        }
    }

    pub fn list_all_potential_origins(&self) -> Vec<String> {
        self.appkeys
            .read()
            .unwrap()
            .values()
            .flat_map(|appkey| appkey.allow_origins.iter().cloned())
            .collect()
    }

    fn update(&self, key: &str, appkey: Option<AppKey>) {
        if let Some(appkey) = &appkey {
            if Identity::try_from(appkey).is_err() {
                log::error!(
                    "Ignoring application-key cache update for '{}' with unsupported role",
                    appkey.name
                );
                return;
            }
            if self
                .admin
                .as_ref()
                .map(|admin| admin.token() == key)
                .unwrap_or(false)
            {
                log::error!(
                    "Ignoring application-key cache update because it collides with the administrator credential"
                );
                return;
            }
        }

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

        log::trace!("AppKeyCache: binding endpoints listening to events.");

        let _ = bus::bind(&endpoint, move |event: AppKeyEvent| {
            let this = this.clone();

            async move {
                match event {
                    AppKeyEvent::NewKey(appkey) => {
                        log::debug!(
                            "Updating CORS for app-key: {}, origin: {:?}",
                            appkey.name,
                            appkey.allow_origins
                        );
                        this.update(&appkey.key.clone(), Some(appkey))
                    }
                    AppKeyEvent::DroppedKey(appkey) => {
                        log::debug!("Removing CORS for app-key: {}", appkey.name);
                        this.update(&appkey.key, None)
                    }
                };
                Ok(())
            }
        });

        log::trace!("AppKeyCache: subscribing to events notifications.");
        bus::service(model::BUS_ID)
            .send(model::Subscribe { endpoint })
            .await??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::super::ident::Role;
    use super::*;

    fn appkey(token: &str, role: &str) -> AppKey {
        AppKey {
            name: "manager-key".to_string(),
            key: token.to_string(),
            role: role.to_string(),
            identity: NodeId::default(),
            created_date: Default::default(),
            allow_origins: vec![],
        }
    }

    #[test]
    fn admin_resolves_only_as_bearer() {
        let admin = AdminCredential::new("admin-token", NodeId::default()).unwrap();
        let cache =
            AppKeyCache::from_appkeys(vec![appkey("manager-token", "manager")], Some(admin))
                .unwrap();

        assert_eq!(
            cache.resolve_bearer("admin-token").unwrap().role,
            Role::Admin
        );
        assert!(cache.resolve_query("admin-token").is_none());
        assert_eq!(
            cache.resolve_bearer("manager-token").unwrap().role,
            Role::Manager
        );
        assert_eq!(
            cache.resolve_query("manager-token").unwrap().role,
            Role::Manager
        );
    }

    #[test]
    fn admin_token_must_not_be_empty() {
        let error = AdminCredential::new("", NodeId::default())
            .err()
            .expect("empty token must fail");
        assert!(!error.to_string().contains("token:"));
    }

    #[test]
    fn admin_token_collision_is_rejected_without_echoing_token() {
        let token = "same-token";
        let admin = AdminCredential::new(token, NodeId::default()).unwrap();
        let error = AppKeyCache::from_appkeys(vec![appkey(token, "manager")], Some(admin))
            .err()
            .expect("collision must fail");

        assert!(!error.to_string().contains(token));
    }

    #[test]
    fn persisted_admin_and_unknown_roles_fail_closed() {
        assert!(AppKeyCache::from_appkeys(vec![appkey("admin", "admin")], None).is_err());
        assert!(AppKeyCache::from_appkeys(vec![appkey("unknown", "owner")], None).is_err());
    }

    #[test]
    fn manager_subject_is_non_secret_name() {
        let cache =
            AppKeyCache::from_appkeys(vec![appkey("manager-token", "manager")], None).unwrap();
        let principal = cache.resolve_bearer("manager-token").unwrap();

        assert_eq!(principal.subject, "manager-key");
        assert_ne!(principal.subject, "manager-token");
    }
}
