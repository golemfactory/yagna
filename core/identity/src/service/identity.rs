use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::bail;
use chrono::Utc;
use ethsign::{KeyFile, Protected, PublicKey};
use futures::lock::Mutex;
use futures::prelude::*;

use ya_client_model::NodeId;
use ya_core_model::bus::GsbBindPoints;
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use ya_core_model::identity as model;
use ya_core_model::identity::event::IdentityEvent;
use ya_persistence::executor::DbExecutor;

use crate::dao::identity::Identity;
use crate::dao::{Error as DaoError, IdentityDao};
use crate::id_key::{default_password, generate_identity_key, IdentityKey, UnlockOutcome};

#[derive(Default)]
struct Subscription {
    subscriptions: Vec<String>,
}

impl Subscription {
    fn subscribe(&mut self, endpoint: String) {
        self.subscriptions.push(endpoint);
    }

    fn unsubscribe(&mut self, endpoint: String) {
        self.subscriptions.retain(|s| s != &endpoint);
    }
}

pub struct IdentityService {
    default_key: NodeId,
    ids: HashMap<NodeId, IdentityKey>,
    alias_to_id: HashMap<String, NodeId>,
    sender: futures::channel::mpsc::UnboundedSender<IdentityEvent>,
    subscription: Rc<RefCell<Subscription>>,
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
        deleted: key.is_deleted(),
    }
}

fn send_event(s: Ref<Subscription>, event: IdentityEvent) -> impl Future<Output = ()> {
    let subscriptions: Vec<String> = s.subscriptions.clone();
    log::debug!("sending event: {:?} to {:?}", event, subscriptions);

    async move {
        for endpoint in subscriptions {
            let msg = event.clone();
            tokio::task::spawn_local(async move {
                log::debug!("Sending event: {:?}", msg);
                match bus::service(&endpoint).call(msg).await {
                    Err(e) => log::error!("Failed to send event: {:?}", e),
                    Ok(Err(e)) => log::error!("Failed to send event: {:?}", e),
                    Ok(Ok(_)) => log::debug!("Event sent to {:?}", endpoint),
                }
            });
        }
    }
}

async fn persist_upgraded_key_file(db: &DbExecutor, key: &IdentityKey) {
    let identity_id = key.id().to_string();
    let key_file = match key.to_key_file() {
        Ok(key_file) => key_file,
        Err(error) => {
            log::warn!("Failed to serialize upgraded keyfile for {identity_id}: {error}");
            return;
        }
    };
    if let Err(error) = db
        .as_dao::<IdentityDao>()
        .update_keyfile(identity_id.clone(), key_file)
        .await
    {
        log::warn!("Failed to persist upgraded keyfile for {identity_id}: {error}");
    }
}

async fn upgrade_key_file_with_default_password(db: &DbExecutor, key: &mut IdentityKey) {
    match key.upgrade_key_file_with_default_password() {
        Ok(true) => persist_upgraded_key_file(db, key).await,
        Ok(false) => {}
        Err(error) => log::warn!(
            "Failed to upgrade PBKDF2 parameters for identity {}: {error}",
            key.id()
        ),
    }
}

impl IdentityService {
    pub async fn from_db(db: DbExecutor) -> anyhow::Result<Self> {
        crate::dao::init(&db).await?;

        let (sender, receiver) = futures::channel::mpsc::unbounded();
        let subscription = Rc::new(RefCell::new(Subscription::default()));
        {
            let subscription = subscription.clone();
            tokio::task::spawn_local(async move {
                receiver
                    .for_each(|event| send_event(subscription.borrow(), event))
                    .await;
            });
        }

        let default_key =
            if let Some(key) = crate::autoconf::preconfigured_identity(default_password())? {
                db.as_dao::<IdentityDao>()
                    .init_preconfigured(Identity {
                        identity_id: key.id(),
                        key_file_json: key.to_key_file()?,
                        is_default: true,
                        is_deleted: false,
                        alias: None,
                        note: None,
                        created_date: Utc::now().naive_utc(),
                    })
                    .await?
                    .identity_id
            } else {
                db.as_dao::<IdentityDao>()
                    .init_default_key(|| {
                        log::info!("generating new default identity");
                        let key: IdentityKey = generate_identity_key(None, "".into(), None);

                        Ok(Identity {
                            identity_id: key.id(),
                            key_file_json: key.to_key_file().map_err(DaoError::internal)?,
                            is_default: true,
                            is_deleted: false,
                            alias: None,
                            note: None,
                            created_date: Utc::now().naive_utc(),
                        })
                    })
                    .await?
                    .identity_id
            };

        log::info!("using default identity: {:?}", default_key);

        let mut ids: HashMap<NodeId, _> = Default::default();
        let mut alias_to_id: HashMap<String, _> = Default::default();

        for identity in db.as_dao::<IdentityDao>().list_identities().await? {
            let mut key: IdentityKey = identity.try_into()?;
            upgrade_key_file_with_default_password(&db, &mut key).await;
            if let Some(alias) = key.alias() {
                let _ = alias_to_id.insert(alias.to_owned(), key.id());
            }
            let _ = ids.insert(key.id(), key);
        }

        Ok(IdentityService {
            default_key,
            db,
            ids,
            sender,
            subscription,
            alias_to_id,
        })
    }

    fn sender(&self) -> &futures::channel::mpsc::UnboundedSender<IdentityEvent> {
        &self.sender
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
        Ok(Some(to_info(&self.default_key, id)))
    }

    pub fn get_by_id(&self, node_id: &NodeId) -> Result<Option<model::IdentityInfo>, model::Error> {
        let id = match self.ids.get(node_id) {
            None => return Ok(None),
            Some(id) => id,
        };
        Ok(Some(to_info(&self.default_key, id)))
    }

    pub fn get_default_id(&self) -> Result<Option<model::IdentityInfo>, model::Error> {
        let id = match self.ids.get(&self.default_key) {
            None => return Ok(None),
            Some(id) => id,
        };
        Ok(Some(to_info(&self.default_key, id)))
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
        private_key: Option<[u8; 32]>,
    ) -> Result<model::IdentityInfo, model::Error> {
        let key = generate_identity_key(alias.clone(), "".into(), private_key);

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
        let is_locked = key.is_locked();
        let identity = key.id();

        if let Some(alias) = alias {
            let _ = self.alias_to_id.insert(alias, key.id());
        }
        let _ = self.ids.insert(key.id(), key);

        if !is_locked {
            self.sender()
                .send(IdentityEvent::AccountUnlocked { identity })
                .await
                .ok();
        }

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

        let mut key = IdentityKey::try_from(new_identity).map_err(model::Error::new_err_msg)?;
        upgrade_key_file_with_default_password(&self.db, &mut key).await;
        let output = to_info(&self.default_key, &key);

        if let Some(alias) = alias {
            let _ = self.alias_to_id.insert(alias, key.id());
        }
        let is_locked = key.is_locked();
        let identity = key.id();

        let _ = self.ids.insert(key.id(), key);

        if !is_locked {
            self.sender()
                .send(IdentityEvent::AccountUnlocked { identity })
                .await
                .ok();
        }

        Ok(output)
    }

    fn get_key_by_id(&mut self, node_id: &NodeId) -> Result<&mut IdentityKey, model::Error> {
        Ok(match self.ids.get_mut(node_id) {
            Some(v) => v,
            None => return Err(model::Error::NodeNotFound(Box::new(*node_id))),
        })
    }

    pub async fn lock(
        &mut self,
        node_id: NodeId,
        new_password: Option<String>,
    ) -> Result<model::IdentityInfo, model::Error> {
        let default_key = self.default_key;
        let key = self.get_key_by_id(&node_id)?;
        let new_key = new_password.is_some();
        key.lock(new_password)
            .map_err(|e| model::Error::InternalErr(e.to_string()))?;
        let output = to_info(&default_key, key);
        if new_key {
            let key_file = key
                .to_key_file()
                .map_err(|e| model::Error::InternalErr(e.to_string()))?;
            let identity_id = output.node_id.to_string();
            self.db
                .as_dao::<IdentityDao>()
                .update_keyfile(identity_id, key_file)
                .await
                .map_err(|e| model::Error::InternalErr(e.to_string()))?;
        }

        Ok(output)
    }

    pub async fn unlock(
        &mut self,
        node_id: NodeId,
        password: Protected,
    ) -> Result<model::IdentityInfo, model::Error> {
        let default_key = self.default_key;
        let (output, key_file_updated) = {
            let key = self.get_key_by_id(&node_id)?;
            match key.unlock(password).map_err(model::Error::new_err_msg)? {
                UnlockOutcome::InvalidPassword => return Err(model::Error::InvalidPassword),
                UnlockOutcome::Unlocked { key_file_updated } => {
                    (to_info(&default_key, key), key_file_updated)
                }
            }
        };
        if key_file_updated {
            let key = self
                .ids
                .get(&node_id)
                .expect("unlocked identity must remain registered");
            persist_upgraded_key_file(&self.db, key).await;
        }
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
            None => return Err(model::Error::NodeNotFound(Box::new(node_id))),
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
            .with_transaction("identity_service_update_identity", move |conn| {
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
            deleted: false,
        })
    }

    pub async fn subscribe(
        &mut self,
        subscribe: model::Subscribe,
    ) -> Result<model::Ack, model::Error> {
        self.subscription.borrow_mut().subscribe(subscribe.endpoint);
        Ok(model::Ack {})
    }

    pub async fn unsubscribe(
        &mut self,
        unsubscribe: model::Unsubscribe,
    ) -> Result<model::Ack, model::Error> {
        self.subscription
            .borrow_mut()
            .unsubscribe(unsubscribe.endpoint);
        Ok(model::Ack {})
    }

    pub async fn drop_id(
        &mut self,
        drop_id: model::DropId,
    ) -> Result<model::IdentityInfo, model::DropError> {
        let mut sender = self.sender.clone();

        match self.ids.get_mut(&drop_id.node_id) {
            None => Err(model::DropError::NodeNotFound(Box::new(drop_id.node_id))),
            Some(id) => {
                if id.is_deleted() {
                    return Err(model::DropError::AlreadyDeleted);
                }
                let was_locked = id.is_locked();

                match self
                    .db
                    .as_dao::<IdentityDao>()
                    .mark_deleted(drop_id.node_id.to_string())
                    .await
                {
                    Ok(_) => {
                        id.mark_deleted();
                        let removed = to_info(&self.default_key, id);

                        if !was_locked {
                            sender
                                .send(IdentityEvent::AccountLocked { identity: id.id() })
                                .await
                                .ok();
                        }

                        Ok(removed)
                    }
                    Err(_) => Err(model::DropError::InternalErr(
                        "Failed to mark identity as deleted".into(),
                    )),
                }
            }
        }
    }

    pub async fn get_pub_key(
        &mut self,
        key_id: model::GetPubKey,
    ) -> Result<PublicKey, model::Error> {
        let key = self.get_key_by_id(&key_id.0)?;
        key.to_pub_key().map_err(model::Error::new_err_msg)
    }

    pub async fn get_key_file(
        &mut self,
        key_id: model::GetKeyFile,
    ) -> Result<String, model::Error> {
        let key = self.get_key_by_id(&key_id.0)?;
        key.to_key_file().map_err(model::Error::new_err_msg)
    }

    pub fn bind_service(me: Arc<Mutex<Self>>, gsb: Arc<GsbBindPoints>) {
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |_list: model::List| {
            let this = this.clone();
            async move { this.lock().await.list_ids() }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |get: model::Get| {
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
        let _ = bus::bind(gsb.local_addr(), move |create: model::CreateGenerated| {
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
                    this.lock().await.create_identity(create.alias, None).await
                }
            }
        });

        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |update: model::Update| {
            let this = this.clone();
            async move { this.lock().await.update_identity(update).await }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |lock: model::Lock| {
            let this = this.clone();
            async move {
                let mut lock_sender = this.lock().await.sender().clone();

                let result = this
                    .lock()
                    .await
                    .lock(lock.node_id, lock.set_password)
                    .await;

                if result.is_ok() {
                    let _ = lock_sender
                        .send(IdentityEvent::AccountLocked {
                            identity: lock.node_id,
                        })
                        .await;
                }

                result
            }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |unlock: model::Unlock| {
            let this = this.clone();
            async move {
                let mut unlock_sender = this.lock().await.sender().clone();
                let result = this
                    .lock()
                    .await
                    .unlock(unlock.node_id, unlock.password.into())
                    .await;
                if result.is_ok() {
                    let _ = unlock_sender
                        .send(IdentityEvent::AccountUnlocked {
                            identity: unlock.node_id,
                        })
                        .await;
                }
                result
            }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |sign: model::Sign| {
            let this = this.clone();
            async move { this.lock().await.sign(sign.node_id, sign.payload).await }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |subscribe: model::Subscribe| {
            let this = this.clone();
            async move { this.lock().await.subscribe(subscribe).await }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |unsubscribe: model::Unsubscribe| {
            let this = this.clone();
            async move { this.lock().await.unsubscribe(unsubscribe).await }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |node_id: model::GetPubKey| {
            let this = this.clone();
            async move {
                this.lock()
                    .await
                    .get_pub_key(node_id)
                    .await
                    .map(|key| key.bytes().to_vec())
            }
        });
        let this = me.clone();
        let _ = bus::bind(gsb.local_addr(), move |node_id: model::GetKeyFile| {
            let this = this.clone();
            async move { this.lock().await.get_key_file(node_id).await }
        });
        let this = me;
        let _ = bus::bind(gsb.local_addr(), move |drop_cmd: model::DropId| {
            let this = this.clone();
            async move {
                log::trace!("Dropping identity: {:?}", drop_cmd);
                let mut guard = this.lock().await;
                match guard.drop_id(drop_cmd).await {
                    Ok(id) => Ok(id),
                    Err(err) => Err(err),
                }
            }
        });
    }
}

pub async fn wait_for_default_account_unlock(gsb: Arc<GsbBindPoints>) -> anyhow::Result<()> {
    let identity_key = get_default_identity_key(gsb.clone()).await?;

    if identity_key.is_locked {
        let locked_identity = identity_key.node_id;
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let endpoint = gsb.endpoint("await_unlock").addr().to_string();

        let _ = bus::bind(&endpoint, move |e: IdentityEvent| {
            let mut tx_clone = tx.clone();
            async move {
                match e {
                    IdentityEvent::AccountLocked { .. } => {}
                    IdentityEvent::AccountUnlocked { identity } => {
                        if locked_identity == identity {
                            log::debug!("Got unlocked event for default locked account with nodeId: {locked_identity}");
                            tx_clone.send(()).await.expect("Receiver is closed");
                        }
                    }
                };
                Ok(())
            }
        });
        subscribe(gsb.clone(), endpoint.clone()).await?;

        log::info!("{}", yansi::Color::RGB(0xFF, 0xA5, 0x00).paint(
            "Daemon cannot start because default account is locked. Unlock it by running 'yagna id unlock'"
        ));

        wait_for_unlock(gsb.clone(), rx).await?;

        unsubscribe(gsb.clone(), endpoint.clone()).await?;
        unbind(endpoint).await?;
    }

    Ok(())
}

async fn wait_for_unlock(
    gsb: Arc<GsbBindPoints>,
    mut rx: futures::channel::mpsc::UnboundedReceiver<()>,
) -> anyhow::Result<()> {
    // Check lock second time because user could unlock database before subscription
    if get_default_identity_key(gsb).await?.is_locked {
        tokio::select! {
            _ = rx.next() => {
                log::info!("Default account unlocked");
            }
            _ = tokio::signal::ctrl_c() => {
                bail!("Default account is locked");
            }
        };
    }

    Ok(())
}

async fn subscribe(gsb: Arc<GsbBindPoints>, endpoint: String) -> anyhow::Result<()> {
    gsb.local().send(model::Subscribe { endpoint }).await??;

    Ok(())
}

async fn unsubscribe(gsb: Arc<GsbBindPoints>, endpoint: String) -> anyhow::Result<()> {
    gsb.local().send(model::Unsubscribe { endpoint }).await??;

    Ok(())
}

async fn unbind(endpoint: String) -> anyhow::Result<()> {
    bus::unbind(&format!("{}/{}", endpoint.clone(), IdentityEvent::ID)).await?;

    Ok(())
}

async fn get_default_identity_key(gsb: Arc<GsbBindPoints>) -> anyhow::Result<model::IdentityInfo> {
    gsb.local()
        .send(model::Get::ByDefault {})
        .await??
        .ok_or_else(|| anyhow::anyhow!("No default Identity found"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ethsign::keyfile::{Bytes, Kdf};
    use ethsign::SecretKey;

    const LEGACY_ITERATIONS: u32 = 10_240;
    const CURRENT_ITERATIONS: u32 = 600_000;

    fn legacy_identity(
        secret_byte: u8,
        password: &str,
        is_default: bool,
    ) -> anyhow::Result<(Identity, String)> {
        let secret = SecretKey::from_raw(&[secret_byte; 32])?;
        let identity_id = NodeId::from(secret.public().address().as_ref());
        let key_file = KeyFile {
            id: uuid::Uuid::new_v4().to_string(),
            version: 3,
            crypto: secret.to_crypto(&Protected::new(password), LEGACY_ITERATIONS)?,
            address: Some(Bytes(secret.public().address().to_vec())),
        };
        let key_file_json = serde_json::to_string(&key_file)?;

        Ok((
            Identity {
                identity_id,
                key_file_json: key_file_json.clone(),
                is_default,
                is_deleted: false,
                alias: None,
                note: None,
                created_date: Utc::now().naive_utc(),
            },
            key_file_json,
        ))
    }

    async fn stored_key_file(db: &DbExecutor, node_id: &NodeId) -> anyhow::Result<String> {
        db.as_dao::<IdentityDao>()
            .list_identities()
            .await?
            .into_iter()
            .find(|identity| &identity.identity_id == node_id)
            .map(|identity| identity.key_file_json)
            .ok_or_else(|| anyhow::anyhow!("identity {node_id} not found"))
    }

    fn pbkdf2_iterations(key_file_json: &str) -> anyhow::Result<u32> {
        let key_file: KeyFile = serde_json::from_str(key_file_json)?;
        match key_file.crypto.kdf {
            Kdf::Pbkdf2(params) => Ok(params.c),
            Kdf::Scrypt(_) => anyhow::bail!("expected PBKDF2 keyfile"),
        }
    }

    #[actix_rt::test]
    async fn legacy_pbkdf2_is_upgraded_when_password_becomes_available() -> anyhow::Result<()> {
        let db =
            DbExecutor::in_memory(&format!("identity-pbkdf2-upgrade-{}", uuid::Uuid::new_v4()))?;
        crate::dao::init(&db).await?;

        let (empty_password, empty_before) = legacy_identity(1, "", true)?;
        let empty_id = empty_password.identity_id;
        db.as_dao::<IdentityDao>()
            .create_identity(empty_password)
            .await?;

        let password = "correct password";
        let (protected, protected_before) = legacy_identity(2, password, false)?;
        let protected_id = protected.identity_id;
        db.as_dao::<IdentityDao>()
            .create_identity(protected)
            .await?;

        let mut service = IdentityService::from_db(db.clone()).await?;

        let empty_info = service
            .get_by_id(&empty_id)?
            .expect("empty-password identity should be loaded");
        assert!(!empty_info.is_locked);
        let empty_after = stored_key_file(&db, &empty_id).await?;
        assert_ne!(empty_after, empty_before);
        assert_eq!(pbkdf2_iterations(&empty_after)?, CURRENT_ITERATIONS);

        assert_eq!(stored_key_file(&db, &protected_id).await?, protected_before);

        let result = service
            .unlock(protected_id, Protected::new("wrong password"))
            .await;
        assert!(matches!(result, Err(model::Error::InvalidPassword)));
        assert_eq!(stored_key_file(&db, &protected_id).await?, protected_before);

        let unlocked = service
            .unlock(protected_id, Protected::new(password))
            .await?;
        assert!(!unlocked.is_locked);
        assert_eq!(unlocked.node_id, protected_id);

        let protected_after = stored_key_file(&db, &protected_id).await?;
        assert_ne!(protected_after, protected_before);
        assert_eq!(pbkdf2_iterations(&protected_after)?, CURRENT_ITERATIONS);

        Ok(())
    }
}
