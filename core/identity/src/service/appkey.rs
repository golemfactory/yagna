use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use chrono::{NaiveDateTime, Utc};
use futures::prelude::*;
use uuid::Uuid;
use ya_service_bus::{typed as bus, RpcEndpoint};

use ya_core_model::appkey as model;
use ya_core_model::appkey::event::AppKeyEvent;
use ya_core_model::identity as idm;
use ya_persistence::executor::DbExecutor;

use crate::dao::AppKeyDao;

#[derive(Default)]
struct Subscription {
    subscriptions: HashMap<u64, String>,
    last_id: u64,
}

impl Subscription {
    fn subscribe(&mut self, endpoint: String) -> u64 {
        let id = self.last_id;
        self.last_id += 1;
        let r = self.subscriptions.insert(id, endpoint);
        assert!(r.is_none());
        id
    }
}

fn send_events(s: Ref<Subscription>, event: AppKeyEvent) -> impl Future<Output = ()> {
    let destinations: Vec<String> = s.subscriptions.values().cloned().collect();

    // TODO: Remove on no destination.
    async move {
        for endpoint in destinations {
            match bus::service(&endpoint).call(event.clone()).await {
                Err(e) => log::error!("fail to send event: {}", e),
                Ok(Err(e)) => log::error!("fail to send event: {}", e),
                Ok(Ok(_)) => log::debug!("send event: {:?} to {}", event, endpoint),
            }
        }
    }
}

pub async fn preconfigured_to_appkey_model(
    preconfigured_node_id: Option<ya_client_model::NodeId>,
    preconfigured_appkey: String,
    created_date: NaiveDateTime,
) -> Result<model::AppKey, ya_core_model::appkey::Error> {
    let node_id = match preconfigured_node_id {
        Some(node_id) => node_id,
        None => {
            let default_identity = bus::service(idm::BUS_ID)
                .send(idm::Get::ByDefault)
                .await
                .map_err(model::Error::internal)?
                .map_err(model::Error::internal)?
                .ok_or_else(|| model::Error::internal("Default identity not found"))?;
            default_identity.node_id
        }
    };
    Ok(model::AppKey {
        name: model::AUTOCONFIGURED_KEY_NAME.to_string(),
        key: preconfigured_appkey.clone(),
        role: model::DEFAULT_ROLE.to_string(),
        identity: node_id,
        created_date,
        allow_origins: vec![],
    })
}

pub async fn activate(db: &DbExecutor) -> anyhow::Result<()> {
    let (tx, rx) = futures::channel::mpsc::unbounded();

    let subscription = Rc::new(RefCell::new(Subscription::default()));

    {
        let subscription = subscription.clone();
        tokio::task::spawn_local(async move {
            rx.for_each(|event| send_events(subscription.borrow(), event))
                .await;
        });
    }

    let _ = bus::bind(model::BUS_ID, move |s: model::Subscribe| {
        let id = subscription.borrow_mut().subscribe(s.endpoint);
        future::ok(id)
    });

    let create_tx = tx.clone();
    let preconfigured_appkey = crate::autoconf::preconfigured_appkey();
    let preconfigured_node_id = crate::autoconf::preconfigured_node_id()?;
    let start_datetime = Utc::now().naive_utc();

    {
        // Create a new application key entry
        let db = db.clone();
        let preconfigured_appkey = preconfigured_appkey.clone();
        let _ = bus::bind(model::BUS_ID, move |create: model::Create| {
            let key = Uuid::new_v4().to_simple().to_string();
            let db = db.clone();
            let preconfigured_appkey = preconfigured_appkey.clone();
            let mut create_tx = create_tx.clone();
            async move {
                let dao = db.as_dao::<AppKeyDao>();

                if let Some(_preconfigured_appkey) = preconfigured_appkey {
                    if create.name == model::AUTOCONFIGURED_KEY_NAME {
                        return Err(model::Error::internal(
                            "Preconfigured appkey already exists",
                        ));
                    }
                }

                let result = match dao.get_for_name(create.name.clone()).await {
                    Ok((app_key, _)) => {
                        if app_key.identity_id == create.identity {
                            Ok(app_key.key)
                        } else {
                            Err(model::Error::bad_request(format!(
                                "app-key with name {} already defined with identity {}",
                                app_key.name, app_key.identity_id
                            )))
                        }
                    }
                    Err(crate::dao::Error::Dao(diesel::result::Error::NotFound)) => dao
                        .create(
                            key.clone(),
                            create.name,
                            create.role,
                            create.identity,
                            create.allow_origins,
                        )
                        .await
                        .map_err(model::Error::internal)
                        .map(|_| key),
                    Err(e) => Err(model::Error::internal(e)),
                }?;

                let (appkey, role) = db
                    .as_dao::<AppKeyDao>()
                    .get(result.clone())
                    .await
                    .map_err(|e| model::Error::internal(e.to_string()))?;

                let _ = create_tx
                    .send(AppKeyEvent::NewKey(appkey.to_core_model(role)))
                    .await;
                Ok(result)
            }
        });
    }

    {
        let db = db.clone();
        let preconfigured_appkey = preconfigured_appkey.clone();
        let _ = bus::bind(model::BUS_ID, move |get: model::Get| {
            let db = db.clone();
            let preconfigured_appkey = preconfigured_appkey.clone();
            async move {
                if let Some(preconfigured_appkey) = preconfigured_appkey {
                    if preconfigured_appkey == get.key {
                        return preconfigured_to_appkey_model(
                            preconfigured_node_id,
                            preconfigured_appkey,
                            start_datetime,
                        )
                        .await;
                    }
                }
                let (appkey, role) = db
                    .as_dao::<AppKeyDao>()
                    .get(get.key)
                    .await
                    .map_err(|e| model::Error::internal(e.to_string()))?;

                Ok(appkey.to_core_model(role))
            }
        });
    }

    {
        let db = db.clone();
        let preconfigured_appkey = preconfigured_appkey.clone();
        let _ = bus::bind(model::BUS_ID, move |get: model::GetByName| {
            let db = db.clone();
            let preconfigured_appkey = preconfigured_appkey.clone();
            async move {
                if let Some(preconfigured_appkey) = preconfigured_appkey {
                    if model::AUTOCONFIGURED_KEY_NAME == get.name {
                        return preconfigured_to_appkey_model(
                            preconfigured_node_id,
                            preconfigured_appkey,
                            start_datetime,
                        )
                        .await;
                    }
                }

                let (appkey, role) = db
                    .as_dao::<AppKeyDao>()
                    .get_for_name(get.name)
                    .await
                    .map_err(|e| model::Error::internal(e.to_string()))?;

                Ok(appkey.to_core_model(role))
            }
        });
    }

    {
        let db = db.clone();
        let preconfigured_appkey = preconfigured_appkey.clone();
        let _ = bus::bind(model::BUS_ID, move |list: model::List| {
            let db = db.clone();
            let preconfigured_appkey = preconfigured_appkey.clone();
            async move {
                let result = db
                    .as_dao::<AppKeyDao>()
                    .list(list.identity, list.page, list.per_page)
                    .await
                    .map_err(Into::<model::Error>::into)?;

                let mut keys: Vec<ya_core_model::appkey::AppKey> = result
                    .0
                    .into_iter()
                    .map(|(app_key, role)| app_key.to_core_model(role))
                    .collect();

                if let Some(preconfigured_appkey) = preconfigured_appkey {
                    keys.push(
                        preconfigured_to_appkey_model(
                            preconfigured_node_id,
                            preconfigured_appkey,
                            start_datetime,
                        )
                        .await?,
                    );
                }

                Ok((keys.clone(), result.1))
            }
        });
    }

    {
        let create_tx = tx;
        let db = db.clone();
        let _ = bus::bind(model::BUS_ID, move |rm: model::Remove| {
            let db = db.clone();
            let preconfigured_appkey = preconfigured_appkey.clone();
            let mut create_tx = create_tx.clone();
            async move {
                if let Some(_preconfigured_appkey) = preconfigured_appkey {
                    if model::AUTOCONFIGURED_KEY_NAME == rm.name {
                        return Err(model::Error::internal("Cannot remove autoconfigured key"));
                    }
                }

                let (appkey, role) = db
                    .as_dao::<AppKeyDao>()
                    .get_for_name(rm.name.clone())
                    .await
                    .map_err(|e| model::Error::internal(e.to_string()))?;

                db.as_dao::<AppKeyDao>()
                    .remove(rm.name, rm.identity)
                    .await
                    .map_err(Into::<model::Error>::into)?;

                let _ = create_tx
                    .send(AppKeyEvent::DroppedKey(appkey.to_core_model(role)))
                    .await;
                Ok(())
            }
        });
    }

    Ok(())
}
