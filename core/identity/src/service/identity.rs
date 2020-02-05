use chrono::Utc;
use ethsign::{KeyFile, Protected};
use futures::lock::Mutex;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::sync::Arc;

use ya_core_model::ethaddr::NodeId;
use ya_core_model::identity as model;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

use crate::dao::identity::Identity;
use crate::dao::{Error as DaoError, IdentityDao};
use crate::id_key::{generate_new, IdentityKey};

pub struct IdentityService {
    default_key: NodeId,
    ids: HashMap<NodeId, IdentityKey>,
    alias_to_id: HashMap<String, NodeId>,
    db: DbExecutor,
}

fn to_info(default_key: &NodeId, key: &IdentityKey) -> model::IdentityInfo {
    let node_id = key.id();
    let is_default = *default_key == node_id;
    model::IdentityInfo {
        alias: key.alias().map(ToOwned::to_owned),
        node_id,
        is_locked: key.is_locked(),
        is_default,
    }
}

impl IdentityService {
    pub async fn from_db(db: DbExecutor) -> anyhow::Result<Self> {
        crate::dao::init(&db).await?;

        let default_key = db
            .as_dao::<IdentityDao>()
            .init_default_key(|| {
                let key: IdentityKey = generate_new(None, "".into()).into();
                let new_identity = Identity {
                    identity_id: key.id(),
                    key_file_json: key.to_key_file().map_err(|e| DaoError::internal(e))?,
                    is_default: true,
                    is_deleted: false,
                    alias: None,
                    note: None,
                    created_date: Utc::now().naive_utc(),
                };

                Ok(new_identity)
            })
            .await?
            .identity_id;

        let mut ids: HashMap<NodeId, _> = Default::default();
        let mut alias_to_id: HashMap<String, _> = Default::default();

        for identity in db.as_dao::<IdentityDao>().list_identities().await? {
            let key: IdentityKey = identity.try_into()?;
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
        Ok(Some(to_info(&self.default_key, &id)))
    }

    pub fn get_by_id(&self, node_id: &NodeId) -> Result<Option<model::IdentityInfo>, model::Error> {
        let id = match self.ids.get(node_id) {
            None => return Ok(None),
            Some(id) => id,
        };
        Ok(Some(to_info(&self.default_key, &id)))
    }

    pub fn get_default_id(&self) -> Result<Option<model::IdentityInfo>, model::Error> {
        let id = match self.ids.get(&self.default_key) {
            None => return Ok(None),
            Some(id) => id,
        };
        Ok(Some(to_info(&self.default_key, &id)))
    }

    pub fn list_ids(&self) -> Result<Vec<model::IdentityInfo>, model::Error> {
        Ok(self
            .ids
            .values()
            .map(|id_key| to_info(&self.default_key, id_key))
            .collect())
    }

    pub async fn create_identity(
        &mut self,
        alias: Option<String>,
    ) -> Result<model::IdentityInfo, model::Error> {
        let key = generate_new(alias.clone(), "".into());

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

        let output = to_info(&self.default_key, &key);

        if let Some(alias) = alias {
            let _ = self.alias_to_id.insert(alias, key.id());
        }
        let _ = self.ids.insert(key.id(), key);

        Ok(output)
    }

    pub async fn create_from_keystore(
        &mut self,
        alias: Option<String>,
        identity_id: NodeId,
        key_file: KeyFile,
    ) -> Result<model::IdentityInfo, model::Error> {
        let key_file_json = serde_json::to_string(&key_file).map_err(model::Error::new_err_msg)?;

        let new_identity = Identity {
            identity_id,
            key_file_json,
            is_default: false,
            is_deleted: false,
            alias: alias.clone(),
            note: None,
            created_date: Utc::now().naive_utc(),
        };

        self.db
            .as_dao::<IdentityDao>()
            .create_identity(new_identity.clone())
            .await
            .map_err(|e| model::Error::InternalErr(e.to_string()))?;

        let key = IdentityKey::try_from(new_identity).map_err(model::Error::new_err_msg)?;
        let output = to_info(&self.default_key, &key);

        if let Some(alias) = alias {
            let _ = self.alias_to_id.insert(alias, key.id());
        }
        let _ = self.ids.insert(key.id(), key);
        Ok(output)
    }

    fn get_key_by_id(&mut self, node_id: &NodeId) -> Result<&mut IdentityKey, model::Error> {
        Ok(match self.ids.get_mut(node_id) {
            Some(v) => v,
            None => return Err(model::Error::NodeNotFound(Box::new(node_id.clone()))),
        })
    }

    pub async fn lock(&mut self, node_id: NodeId) -> Result<model::IdentityInfo, model::Error> {
        let default_key = self.default_key;
        let key = self.get_key_by_id(&node_id)?;
        key.lock();
        let output = to_info(&default_key, key);
        Ok(output)
    }

    pub async fn unlock(
        &mut self,
        node_id: NodeId,
        password: Protected,
    ) -> Result<model::IdentityInfo, model::Error> {
        let default_key = self.default_key;
        let key = self.get_key_by_id(&node_id)?;
        key.unlock(password).map_err(model::Error::new_err_msg)?;
        let output = to_info(&default_key, key);
        Ok(output)
    }

    pub async fn sign(&mut self, node_id: NodeId, data: Vec<u8>) -> Result<Vec<u8>, model::Error> {
        let key = self.get_key_by_id(&node_id)?;
        if let Some(signature) = key.sign(data.as_slice()) {
            Ok(signature)
        } else {
            Err(model::Error::new_err_msg("sign error"))
        }
    }

    pub async fn update_identity(
        &mut self,
        update: model::Update,
    ) -> Result<model::IdentityInfo, model::Error> {
        let node_id = update.node_id;
        let key = match self.ids.get_mut(&node_id) {
            Some(v) => v,
            None => return Err(model::Error::NodeNotFound(Box::new(node_id.clone()))),
        };
        let update_alias = update.alias.clone();
        if let Some(new_alias) = update.alias {
            if self.alias_to_id.contains_key(&new_alias) {
                return Err(model::Error::AlreadyExists);
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

        self.db
            .with_transaction(move |conn| {
                use crate::db::schema::identity::dsl::*;
                use diesel::prelude::*;

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
            })
            .await
            .map_err(model::Error::new_err_msg)?;

        Ok(model::IdentityInfo {
            alias: key.alias().map(ToOwned::to_owned),
            node_id,
            is_locked: key.is_locked(),
            is_default: self.default_key == node_id,
        })
    }

    pub fn bind_service(me: Arc<Mutex<Self>>) {
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
                    model::Get::ByNodeId(node_id) => this.lock().await.get_by_id(&node_id),
                    model::Get::ByDefault => this.lock().await.get_default_id(),
                    _ => Err(model::Error::InternalErr("unsupported query".to_string())),
                }
            }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |create: model::CreateGenerated| {
            let this = this.clone();
            async move {
                if let Some(key_store) = create.from_keystore {
                    let key: KeyFile = serde_json::from_str(key_store.as_str())
                        .map_err(model::Error::keystore_format)?;
                    let addr_bytes = match &key.address {
                        Some(addr_bytes) => addr_bytes.0.as_slice(),
                        None => {
                            return Err(model::Error::BadKeyStoreFormat(
                                "missing address".to_string(),
                            ))
                        }
                    };
                    let node_id: NodeId = NodeId::from(addr_bytes);

                    this.lock()
                        .await
                        .create_from_keystore(create.alias, node_id, key)
                        .await
                } else {
                    this.lock().await.create_identity(create.alias).await
                }
            }
        });

        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |update: model::Update| {
            let this = this.clone();
            async move { this.lock().await.update_identity(update).await }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |lock: model::Lock| {
            let this = this.clone();
            async move { this.lock().await.lock(lock.node_id).await }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |unlock: model::Unlock| {
            let this = this.clone();
            async move {
                this.lock()
                    .await
                    .unlock(unlock.node_id, unlock.password.into())
                    .await
            }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |sign: model::Sign| {
            let this = this.clone();
            async move { this.lock().await.sign(sign.node_id, sign.payload).await }
        });
    }
}
