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
use ya_service_bus::{typed as bus, RpcEndpoint, RpcMessage};

use ya_core_model::identity as model;
use ya_persistence::executor::DbExecutor;

use crate::dao::identity::Identity;
use crate::dao::{Error as DaoError, IdentityDao};
use crate::id_key::{default_password, generate_new, IdentityKey};

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
    sender: futures::channel::mpsc::UnboundedSender<model::event::Event>,
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
    }
}

fn send_event(s: Ref<Subscription>, event: model::event::Event) -> impl Future<Output = ()> {
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

impl IdentityService {
    pub async fn from_db(db: DbExecutor) -> anyhow::Result<Self> {
        crate::dao::init(&db).await?;

        let (sender, receiver) = futures::channel::mpsc::unbounded();
        let subscription = Rc::new(RefCell::new(Subscription::default()));
        {
            let subscription = subscription.clone();
            tokio::task::spawn_local(async move {
                let _ = receiver
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
                        let key: IdentityKey = generate_new(None, "".into()).into();

                        Ok(Identity {
                            identity_id: key.id(),
                            key_file_json: key.to_key_file().map_err(|e| DaoError::internal(e))?,
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
            sender,
            subscription,
            alias_to_id,
        })
    }

    fn sender(&self) -> &futures::channel::mpsc::UnboundedSender<model::event::Event> {
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
        let key = self.get_key_by_id(&node_id)?;
        if key.unlock(password).map_err(model::Error::new_err_msg)? {
            Ok(to_info(&default_key, key))
        } else {
            Err(model::Error::bad_request("Invalid password"))
        }
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

    pub async fn get_pub_key(
        &mut self,
        key_id: model::GetPubKey,
    ) -> Result<PublicKey, model::Error> {
        let key = self.get_key_by_id(&key_id.0)?;
        key.to_pub_key().map_err(|e| model::Error::new_err_msg(e))
    }

    pub async fn get_key_file(
        &mut self,
        key_id: model::GetKeyFile,
    ) -> Result<String, model::Error> {
        let key = self.get_key_by_id(&key_id.0)?;
        key.to_key_file().map_err(|e| model::Error::new_err_msg(e))
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
            async move {
                let mut lock_sender = this.lock().await.sender().clone();

                let result = this
                    .lock()
                    .await
                    .lock(lock.node_id, lock.set_password)
                    .await;

                if result.is_ok() {
                    let _ = lock_sender
                        .send(model::event::Event::AccountLocked {
                            identity: lock.node_id,
                        })
                        .await;
                }

                result
            }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |unlock: model::Unlock| {
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
                        .send(model::event::Event::AccountUnlocked {
                            identity: unlock.node_id,
                        })
                        .await;
                }
                result
            }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |sign: model::Sign| {
            let this = this.clone();
            async move { this.lock().await.sign(sign.node_id, sign.payload).await }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |subscribe: model::Subscribe| {
            let this = this.clone();
            async move { this.lock().await.subscribe(subscribe).await }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |unsubscribe: model::Unsubscribe| {
            let this = this.clone();
            async move { this.lock().await.unsubscribe(unsubscribe).await }
        });
        let this = me.clone();
        let _ = bus::bind(model::BUS_ID, move |node_id: model::GetPubKey| {
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
        let _ = bus::bind(model::BUS_ID, move |node_id: model::GetKeyFile| {
            let this = this.clone();
            async move { this.lock().await.get_key_file(node_id).await }
        });
    }
}

pub async fn wait_for_default_account_unlock(db: &DbExecutor) -> anyhow::Result<()> {
    let identity_key = get_default_identity_key(&db).await?;

    if identity_key.is_locked() {
        let locked_identity = identity_key.id();
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let endpoint = format!("{}/await_unlock", model::BUS_ID);

        let _ = bus::bind(&endpoint, move |e: model::event::Event| {
            let mut tx_clone = tx.clone();
            async move {
                match e {
                    model::event::Event::AccountLocked { .. } => {}
                    model::event::Event::AccountUnlocked { identity } => {
                        if locked_identity == identity {
                            log::debug!("Got unlocked event for default locked account with nodeId: {locked_identity}");
                            tx_clone.send(()).await.expect("Receiver is closed");
                        }
                    }
                };
                Ok(())
            }
        });
        subscribe(endpoint.clone()).await?;

        log::info!("{}", yansi::Color::RGB(0xFF, 0xA5, 0x00).paint(
            "Daemon cannot start because default account is locked. Unlock it by running 'yagna id unlock'"
        ));

        wait_for_unlock(rx).await?;

        unsubscribe(endpoint.clone()).await?;
        unbind(endpoint).await?;
    }

    Ok(())
}

async fn wait_for_unlock(
    mut rx: futures::channel::mpsc::UnboundedReceiver<()>,
) -> anyhow::Result<()> {
    tokio::select! {
        _ = rx.next() => {
            log::info!("Default account unlocked");
        }
        _ = tokio::signal::ctrl_c() => {
            bail!("Default account is locked");
        }
    };

    Ok(())
}

async fn subscribe(endpoint: String) -> anyhow::Result<()> {
    bus::service(model::BUS_ID)
        .send(model::Subscribe { endpoint })
        .await??;

    Ok(())
}

async fn unsubscribe(endpoint: String) -> anyhow::Result<()> {
    bus::service(model::BUS_ID)
        .send(model::Unsubscribe { endpoint })
        .await??;

    Ok(())
}

async fn unbind(endpoint: String) -> anyhow::Result<()> {
    bus::unbind(&format!("{}/{}", endpoint.clone(), model::event::Event::ID)).await?;

    Ok(())
}

async fn get_default_identity_key(db: &DbExecutor) -> anyhow::Result<IdentityKey> {
    Ok(db
        .as_dao::<IdentityDao>()
        .get_default_identity()
        .await?
        .try_into()?)
}
