use futures::lock::Mutex;
use futures::prelude::*;
use std::collections::HashMap;
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
use std::convert::{TryInto, identity};
use ya_persistence::executor::DbExecutor;
use crate::dao::appkey::DaoError;
use crate::service::id_key::IdentityKey;

mod appkey;
mod id_key;

struct IdentityService {
    default_key : NodeId,
    ids: HashMap<NodeId, id_key::IdentityKey>,
    alias_to_id: HashMap<String, NodeId>,
    db: DbExecutor,
}

impl IdentityService {

    fn to_info(&self, key : &id_key::IdentityKey) -> model::IdentityInfo {
        let node_id = key.id();
        let is_default = self.default_key == node_id;
        model::IdentityInfo {
            alias: key.alias().map(ToOwned::to_owned),
            node_id,
            is_locked: key.is_locked(),
            is_default
        }
    }

    pub async fn from_db(db: DbExecutor) -> anyhow::Result<Self> {
        crate::dao::init(&db)?;

        let default_key = db.as_dao::<IdentityDao>().init_default_key(|| {
            let key : IdentityKey = id_key::generate_new(None, "".into()).into();
            let new_identity = Identity {
                identity_id: key.id(),
                key_file_json: key
                    .to_key_file()
                    .map_err(|e| DaoError::internal(e))?,
                is_default: true,
                is_deleted: false,
                alias:None,
                note: None,
                created_date: Utc::now().naive_utc(),
            };

            Ok(new_identity)
        }).await?.identity_id;

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
            default_key,
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
        Ok(Some(self.to_info(id)))
    }

    pub fn list_ids(&self) -> Result<Vec<model::IdentityInfo>, model::Error> {
        Ok(self.ids.values()
            .map(|id_key| {
                self.to_info(id_key)
            })
            .collect())
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

        let output = self.to_info(&key);

        if let Some(alias) = alias {
            let _ = self.alias_to_id.insert(alias, key.id());
        }
        let _ = self.ids.insert(key.id(), key);

        Ok(output)
    }

    pub async fn update_identity(&mut self, update : model::Update) -> Result<model::IdentityInfo, model::Error> {
        let node_id = update.node_id;
        let key = match self.ids.get_mut(&node_id) {
            Some(v) => v,
            None => return Err(model::Error::NodeNotFound(Box::new(node_id.clone())))
        };
        let update_alias = update.alias.clone();
        if let Some(new_alias) = update.alias {
            if self.alias_to_id.contains_key(&new_alias) {
                return Err(model::Error::AlreadyExists)
            }
            if let Some(old_alias) = key.replace_alias(Some(new_alias.clone())) {
                let _ = self.alias_to_id.remove(&old_alias);
            }
            self.alias_to_id.insert(new_alias, key.id());
        }
        let prev_default = self.default_key;
        let set_default = update.set_default;
        if update.set_default {
            self.default_key = key.id();
        }

        self.db.with_transaction(move |conn| {
            use diesel::prelude::*;
            use crate::db::schema::identity::dsl::*;

            if update_alias.is_some() {
                let _ = diesel::update(identity.filter(identity_id.eq(&node_id)))
                    .set(alias.eq(&update_alias.unwrap()))
                    .execute(conn)?;
            }
            if set_default && prev_default != node_id {
                diesel::update(identity.filter(identity_id.eq(&prev_default)))
                    .set(is_default.eq(false))
                    .execute(conn)?;
                diesel::update(identity.filter(identity_id.eq(&node_id)))
                    .set(is_default.eq(true))
                    .execute(conn)?;
            }
            Ok::<_, DaoError>(())
        }).await.map_err(model::Error::new_err_msg)?;

        Ok(model::IdentityInfo {
            alias: key.alias().map(ToOwned::to_owned),
            node_id,
            is_locked: key.is_locked(),
            is_default: self.default_key == node_id
        })
    }

    fn bind_service(me: Arc<Mutex<Self>>) {
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |_list: model::List| {
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

        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |update : model::Update| {
            let this = this.clone();
            async move {
                this.lock().await.update_identity(update).await
            }
        });
    }
}

pub async fn activate(db: &DbExecutor) -> anyhow::Result<()> {
    log::info!("activating identity service");
    log::debug!("loading default identity");

    let service = Arc::new(Mutex::new(IdentityService::from_db(db.clone()).await?));
    IdentityService::bind_service(service);
    log::info!("identity service activated");

    appkey::activate(db).await?;
    Ok(())
}
