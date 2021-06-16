use ya_client::model::NodeId;
use std::path::{Path, PathBuf};
use crate::startup_config::{NodeConfig, FileMonitor};
use std::sync::{Arc, Mutex};
use serde::{Deserialize, Deserializer, Serialize};
use ya_utils_path::SwapSave;
use std::{io, fs};

pub(crate) const GLOBALS_JSON: &'static str = "globals.json";

#[derive(Clone, Debug, Default, Serialize, derive_more::Display)]
#[display(
fmt = "{}{}{}",
"node_name.as_ref().map(|nn| format!(\"Node name: {}\", nn)).unwrap_or(\"\".into())",
"subnet.as_ref().map(|s| format!(\"\nSubnet: {}\", s)).unwrap_or(\"\".into())",
"account.as_ref().map(|a| format!(\"\nAccount: {}\", a)).unwrap_or(\"\".into())"
)]
pub struct GlobalsState {
    pub node_name: Option<String>,
    pub subnet: Option<String>,
    pub account: Option<NodeId>,
}

impl<'de> Deserialize<'de> for GlobalsState {
    fn deserialize<D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Self, <D as Deserializer<'de>>::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        pub enum Account {
            NodeId(NodeId),
            Deprecated {
                platform: Option<String>,
                address: NodeId,
            },
        }

        impl Account {
            pub fn address(self) -> NodeId {
                match self {
                    Account::NodeId(address) => address,
                    Account::Deprecated { address, .. } => address,
                }
            }
        }

        #[derive(Deserialize)]
        pub struct GenericGlobalsState {
            pub node_name: Option<String>,
            pub subnet: Option<String>,
            pub account: Option<Account>,
        }

        let s = GenericGlobalsState::deserialize(deserializer)?;
        Ok(GlobalsState {
            node_name: s.node_name,
            subnet: s.subnet,
            account: s.account.map(|a| a.address()),
        })
    }
}

impl GlobalsState {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            log::debug!("Loading global state from: {}", path.display());
            Ok(serde_json::from_reader(io::BufReader::new(
                fs::OpenOptions::new().read(true).open(path)?,
            ))?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(&path)?;
            let state = Self::default();
            state.save(path)?;
            Ok(state)
        }
    }

    pub fn update_and_save(&mut self, node_config: NodeConfig, path: &Path) -> anyhow::Result<()> {
        if node_config.node_name.is_some() {
            self.node_name = node_config.node_name;
        }
        if node_config.subnet.is_some() {
            self.subnet = node_config.subnet;
        }
        if node_config.account.account.is_some() {
            self.account = node_config.account.account;
        }
        self.save(path)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }
}

