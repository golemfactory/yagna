use futures::lock::Mutex;
use futures::prelude::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
/// Identity service
use ya_core_model::ethaddr::NodeId;
use ya_core_model::identity as model;
use ya_core_model::identity::IdentityInfo;
use ya_service_bus::actix_rpc::bind;
use ya_service_bus::typed as bus;

use crate::dao::identity::IdentityDao;
use crate::db::models::Identity;
use chrono::Utc;
use ethsign::KeyFile;
use std::convert::TryInto;
use ya_persistence::executor::DbExecutor;

const KEYS_SUBDIR: &str = "keys";

mod appkey;
mod id_key;

struct IdentityService {
    ids: HashMap<NodeId, id_key::IdentityKey>,
    alias_to_id: HashMap<String, NodeId>,
    db: DbExecutor,
}

impl Into<model::IdentityInfo> for &id_key::IdentityKey {
    fn into(self) -> IdentityInfo {
        model::IdentityInfo {
            alias: self.alias().map(ToOwned::to_owned),
            node_id: self.id(),
            is_locked: self.is_locked(),
        }
    }
}

impl IdentityService {
    pub async fn from_db(db: DbExecutor) -> anyhow::Result<Self> {
        crate::dao::init(&db);

        let mut ids: HashMap<NodeId, _> = Default::default();
        let mut alias_to_id: HashMap<String, _> = Default::default();

        for identity in db.as_dao::<IdentityDao>().list_identities().await? {
            let key: id_key::IdentityKey = identity.try_into()?;
            if let Some(alias) = key.alias() {
                let _ = alias_to_id.insert(alias.to_owned(), key.id());
            }
            let _ = ids.insert(key.id(), key);
        }

        Ok(IdentityService {
            db,
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
        let key = id_key::generate_new(alias.clone(), "".into());

        let new_identity = Identity {
            identity_id: key.id(),
            key_file_json: key
                .to_key_file()
                .map_err(|e| model::Error::InternalErr(e.to_string()))?,
            is_default: false,
            is_deleted: false,
            alias: key.alias().map(ToOwned::to_owned),
            note: None,
            created_date: Utc::now().naive_utc(),
        };

        self.db
            .as_dao::<IdentityDao>()
            .create_identity(new_identity)
            .await
            .map_err(|e| model::Error::InternalErr(e.to_string()))?;

        let output = (&key).into();

        if let Some(alias) = alias {
            let _ = self.alias_to_id.insert(alias, key.id());
        }
        let _ = self.ids.insert(key.id(), key);

        Ok(output)
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
        let _ = bus::bind(model::BUS_ID, move |create: model::CreateGenerated| {
            let this = this.clone();
            async move { this.lock().await.create_identity(create.alias).await }
        });
    }
}

pub async fn activate(db: &DbExecutor) -> anyhow::Result<()> {
    log::info!("activating identity service");
    let service = Arc::new(Mutex::new(IdentityService::from_db(db.clone()).await?));
    IdentityService::bind_service(service);
    log::info!("identity service activated");

    appkey::activate(db).await?;
    Ok(())
}

fn key_path(keys_path: &Path, alias: &str) -> PathBuf {
    keys_path.join(alias).with_extension("json")
}
