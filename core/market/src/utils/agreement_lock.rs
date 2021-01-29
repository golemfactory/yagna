use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::db::model::AgreementId;

#[derive(Clone)]
pub struct AgreementLock {
    lock_map: Arc<RwLock<HashMap<AgreementId, Arc<Mutex<()>>>>>,
}

impl AgreementLock {
    pub fn new() -> AgreementLock {
        AgreementLock {
            lock_map: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_lock(&self, agreement_id: &AgreementId) -> Arc<Mutex<()>> {
        // Note how important are '{}' around this statement. Otherwise lock isn't freed
        // and we can't acquire write lock
        let potencial_lock = {
            self.lock_map
                .read()
                .await
                .get(agreement_id)
                .map(|lock| lock.clone())
        };
        match potencial_lock {
            Some(mutex) => mutex,
            None => {
                let mut lock_map = self.lock_map.write().await;
                lock_map
                    .entry(agreement_id.clone())
                    .or_insert(Arc::new(Mutex::new(())))
                    .clone()
            }
        }
        .clone()
    }

    pub async fn clear_locks(&self, agreement_id: &AgreementId) {
        self.lock_map.write().await.remove(agreement_id);
    }
}
