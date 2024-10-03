use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use ya_core_model::activity;
use ya_service_bus::typed as bus;

#[derive(Clone)]
pub struct FakeActivity {
    name: String,
    _testdir: PathBuf,

    inner: Arc<RwLock<FakeActivityInner>>,
}

#[derive(Clone, Debug)]
pub struct Activity {
    id: String,
    agreement_id: String,
}

pub struct FakeActivityInner {
    agreement_mapping: HashMap<String, Vec<String>>,
    activities: HashMap<String, Activity>,
}

impl FakeActivity {
    pub fn new(name: &str, testdir: &Path) -> Self {
        FakeActivity {
            name: name.to_string(),
            _testdir: testdir.to_path_buf(),
            inner: Arc::new(RwLock::new(FakeActivityInner {
                agreement_mapping: Default::default(),
                activities: Default::default(),
            })),
        }
    }

    pub async fn bind_gsb(&self) -> anyhow::Result<()> {
        log::info!("Activity ({}) - binding GSB", self.name);

        let self_ = self.clone();
        bus::bind(
            activity::local::BUS_ID,
            move |msg: activity::local::GetAgreementId| {
                let self_ = self_.clone();
                async move {
                    log::info!(
                        "[FakeActivity] - responding to GetAgreementId for activity: {}",
                        msg.activity_id
                    );
                    self_.get_agreement_id(&msg.activity_id).await.ok_or(
                        activity::RpcMessageError::NotFound(format!(
                            "Activity id: {}",
                            msg.activity_id
                        )),
                    )
                }
            },
        );
        Ok(())
    }

    pub async fn create_activity(&self, agreement_id: &str) -> String {
        let id = Uuid::new_v4().to_simple().to_string();
        let activity = Activity {
            id: id.clone(),
            agreement_id: agreement_id.to_string(),
        };

        let mut lock = self.inner.write().await;
        lock.activities.insert(id.clone(), activity.clone());
        lock.agreement_mapping
            .entry(agreement_id.to_string())
            .or_default()
            .push(id);
        activity.id
    }

    pub async fn get_agreement_id(&self, activity_id: &str) -> Option<String> {
        let lock = self.inner.read().await;
        lock.activities
            .get(activity_id)
            .map(|a| a.agreement_id.clone())
    }
}
