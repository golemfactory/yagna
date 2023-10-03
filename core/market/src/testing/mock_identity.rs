use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::sync::{Arc, Mutex};

use crate::identity::{IdentityApi, IdentityError};

use std::collections::HashMap;
use ya_client::model::NodeId;
use ya_service_api_web::middleware::Identity;

pub struct MockIdentity {
    inner: Arc<Mutex<MockIdentityInner>>,
}

struct MockIdentityInner {
    pub default: Identity,
    pub identities: HashMap<String, Identity>,
}

#[async_trait::async_trait(?Send)]
impl IdentityApi for MockIdentity {
    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(self.get_default_id().identity)
    }

    async fn list(&self) -> Result<Vec<NodeId>, IdentityError> {
        Ok(self
            .list_ids()
            .into_values()
            .map(|id| id.identity)
            .collect())
    }
}

impl MockIdentity {
    pub fn new(name: &str) -> Arc<MockIdentity> {
        let default = generate_identity(name);
        let mut identities = HashMap::new();
        identities
            .entry(name.to_string())
            .or_insert_with(|| default.clone());

        let mock_identity = MockIdentityInner {
            default,
            identities,
        };

        Arc::new(MockIdentity {
            inner: Arc::new(Mutex::new(mock_identity)),
        })
    }
    pub fn new_identity(&self, name: &str) -> Identity {
        let new_id = generate_identity(name);
        self.inner
            .lock()
            .unwrap()
            .identities
            .entry(name.to_string())
            .or_insert(new_id)
            .clone()
    }

    pub fn get_default_id(&self) -> Identity {
        self.inner.lock().unwrap().default.clone()
    }

    pub fn list_ids(&self) -> HashMap<String, Identity> {
        self.inner.lock().unwrap().identities.clone()
    }
}

pub fn generate_identity(name: &str) -> Identity {
    let random_node_id: String = thread_rng()
        .sample_iter(&Alphanumeric)
        .map(char::from)
        .take(20)
        .collect();

    Identity {
        name: name.to_string(),
        role: "manager".to_string(),
        identity: NodeId::from(random_node_id.as_bytes()),
    }
}
