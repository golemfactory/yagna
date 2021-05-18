use uuid::Uuid;

use futures::prelude::*;
use ya_core_model::appkey as model;
use ya_persistence::executor::DbExecutor;
use ya_service_bus::typed as bus;

use crate::dao::AppKeyDao;
use std::cell::{Ref, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

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

fn send_events(s: Ref<Subscription>, event: model::event::Event) -> impl Future<Output = ()> {
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

pub async fn activate(db: &DbExecutor) -> anyhow::Result<()> {
    let dbx = db.clone();
    let (tx, rx) = futures::channel::mpsc::unbounded();

    let subscription = Rc::new(RefCell::new(Subscription::default()));

    {
        let subscription = subscription.clone();
        tokio::task::spawn_local(async move {
            let _ = rx
                .for_each(|event| send_events(subscription.borrow(), event))
                .await;
        });
    }

    let _ = bus::bind(&model::BUS_ID, move |s: model::Subscribe| {
        let id = subscription.borrow_mut().subscribe(s.endpoint);
        future::ok(id)
    });

    let create_tx = tx.clone();
    // Create a new application key entry
    let _ = bus::bind(&model::BUS_ID, move |create: model::Create| {
        let key = Uuid::new_v4().to_simple().to_string();
        let db = dbx.clone();
        let mut create_tx = create_tx.clone();
        let identity = create.identity.clone();
        async move {
            let result = db
                .as_dao::<AppKeyDao>()
                .create(key.clone(), create.name, create.role, create.identity)
                .await
                .map_err(|e| model::Error::internal(e))
                .map(|_| key)?;
            let _ = create_tx
                .send(model::event::Event::NewKey { identity })
                .await;
            Ok(result)
        }
    });

    let dbx = db.clone();
    // Retrieve an application key entry based on the key itself
    let _ = bus::bind(&model::BUS_ID, move |get: model::Get| {
        let db = dbx.clone();
        async move {
            let (appkey, role) = db
                .as_dao::<AppKeyDao>()
                .get(get.key)
                .await
                .map_err(|e| model::Error::internal(e.to_string()))?;

            Ok(model::AppKey {
                name: appkey.name,
                key: appkey.key,
                role: role.name,
                identity: appkey.identity_id,
                created_date: appkey.created_date,
            })
        }
    });

    let dbx = db.clone();
    let _ = bus::bind(model::BUS_ID, move |list: model::List| {
        let db = dbx.clone();

        async move {
            let result = db
                .as_dao::<AppKeyDao>()
                .list(list.identity, list.page, list.per_page)
                .await
                .map_err(Into::into)?;

            let keys = result
                .0
                .into_iter()
                .map(|(app_key, role)| model::AppKey {
                    name: app_key.name,
                    key: app_key.key,
                    role: role.name,
                    identity: app_key.identity_id,
                    created_date: app_key.created_date,
                })
                .collect();

            Ok((keys, result.1))
        }
    });

    let dbx = db.clone();
    let _ = bus::bind(&model::BUS_ID, move |rm: model::Remove| {
        let db = dbx.clone();
        async move {
            db.as_dao::<AppKeyDao>()
                .remove(rm.name, rm.identity)
                .await
                .map_err(Into::into)?;
            Ok(())
        }
    });

    Ok(())
}
