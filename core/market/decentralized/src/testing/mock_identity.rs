use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::sync::Arc;

use crate::identity::{IdentityApi, IdentityError};

use ya_client::model::NodeId;
use ya_service_api_web::middleware::Identity;

pub struct MockIdentity {
    pub default: Identity,
}

#[async_trait::async_trait(?Send)]
impl IdentityApi for MockIdentity {
    async fn default_identity(&self) -> Result<NodeId, IdentityError> {
        Ok(self.default.identity.clone())
    }
}

impl MockIdentity {
    pub fn new(name: &str) -> Arc<MockIdentity> {
        let mock_identity = MockIdentity {
            default: generate_identity(name),
        };
        Arc::new(mock_identity)
    }
}

pub fn generate_identity(name: &str) -> Identity {
    let random_node_id: String = thread_rng().sample_iter(&Alphanumeric).take(20).collect();

    Identity {
        name: name.to_string(),
        role: "manager".to_string(),
        identity: NodeId::from(random_node_id.as_bytes()),
    }
}
