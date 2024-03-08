use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use ya_client_model::NodeId;

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RestrictConfig {
    pub enabled: bool,
    #[serde(default)]
    pub identity: HashSet<NodeId>,
    #[serde(default)]
    pub certified: HashSet<String>,
}
