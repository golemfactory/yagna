use futures::lock::Mutex;
use futures::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
/// Identity service
use ya_core_model::ethaddr::NodeId;
use ya_core_model::identity as model;
use ya_core_model::identity::IdentityInfo;
use ya_service_bus::actix_rpc::bind;
use ya_service_bus::typed as bus;

use ethsign::KeyFile;
use std::convert::TryInto;

const KEYS_SUBDIR: &str = "keys";

struct Identity {
    node_id : NodeId,
    alias : Option<String>,
    key: KeyFile
}

struct IdentityService {
    keys_dir: Arc<Path>,
    ids: HashMap<NodeId, Identity>,
    alias_to_id: HashMap<String, NodeId>,
}

impl Into<model::IdentityInfo> for &Identity {
    fn into(self) -> IdentityInfo {
        model::IdentityInfo {
            alias: self.alias.clone().unwrap_or_default(),
            node_id: NodeId::from(self.key.address.as_ref().unwrap().0.as_ref()),
            is_locked: false,
        }
    }
}

impl IdentityService {
    pub async fn from_appdir(keys_dir: Arc<Path>) -> anyhow::Result<Self> {
        let mut de = fs::read_dir(&keys_dir).await?;
        let mut ids: HashMap<NodeId, _> = Default::default();
        let mut alias_to_id: HashMap<String, _> = Default::default();

        while let Some(entry) = de.next_entry().await? {
            let path: PathBuf = entry.path();
            if path.extension() != Some("json".as_ref()) {
                continue;
            }
            let name = match path.file_name() {
                None => continue,
                Some(v) => match v.to_str() {
                    Some(v) => v,
                    None => continue,
                },
            };
            let name = if name.ends_with(".json") {
                &name[..name.len() - 5]
            } else {
                name
            };
            let json : KeyFile = serde_json::from_slice(fs::read(&path).await?.as_ref())?;

            /*let node_id : NodeId = json.address.unwrap().into();

            let _ = alias_to_id.insert(name.into(), node_id);
            let _ = ids.insert(node_id, Identity {
                    key: json,
                    alias: Some(name.to_string()),
                },
            );*/
            todo!()
        }

        Ok(IdentityService {
            keys_dir,
            ids,
            alias_to_id,
        })
    }

    pub fn get_by_alias(&self, alias: &str) -> Result<Option<model::IdentityInfo>, model::Error> {
        let addr = match self.alias_to_id.get(alias) {
            None => return Ok(None),
            Some(s) => s,
        };
        let id = match self.ids.get(addr) {
            None => return Ok(None),
            Some(id) => id,
        };
        Ok(Some(id.into()))
    }

    pub fn list_ids(&self) -> Result<Vec<model::IdentityInfo>, model::Error> {
        Ok(self.ids.values().map(Into::into).collect())
    }

    pub async fn create_identity(
        &mut self,
        alias: Option<String>,
    ) -> Result<model::IdentityInfo, model::Error> {
        let name = alias.unwrap_or_else(|| "todo".to_string());
        let dest_path = key_path(&self.keys_dir, &name);

        if dest_path.exists() || self.alias_to_id.contains_key(&name) {
            return Err(model::Error::AlreadyExists);
        }

        unimplemented!()
    }

    fn bind_service(me: Arc<Mutex<Self>>) {
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |list: model::List| {
            let this = this.clone();
            async move { this.lock().await.list_ids() }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |get: model::Get| {
            let this = this.clone();
            async move {
                match get {
                    model::Get::ByAlias(alias) => this.lock().await.get_by_alias(&alias),
                    _ => unimplemented!(),
                }
            }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |create: model::CreateGenerated| async move {
            todo!()
        });
    }
}

pub async fn activate(data_dir: &Path) -> anyhow::Result<()> {
    log::info!("activating identity service");
    let key_dir: Arc<Path> = data_dir.join(KEYS_SUBDIR).into();
    let service = Arc::new(Mutex::new(
        IdentityService::from_appdir(key_dir.clone()).await?,
    ));
    IdentityService::bind_service(service);
    log::info!("identity service activated");
    Ok(())
}

fn key_path(keys_path: &Path, alias: &str) -> PathBuf {
    keys_path.join(alias).with_extension("json")
}
