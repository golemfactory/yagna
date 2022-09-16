use crate::startup_config::NodeConfig;
use std::path::Path;
use ya_client::model::NodeId;

use serde::{Deserialize, Deserializer, Serialize};
use std::{fs, io};
use ya_utils_path::SwapSave;

pub(crate) const GLOBALS_JSON: &'static str = "globals.json";
pub(crate) const DEFAULT_SUBNET: &'static str = "public-beta";

fn default_subnet() -> Option<String> {
    Some(DEFAULT_SUBNET.into())
}

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
        } else {
            self.subnet = default_subnet();
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

#[cfg(test)]
mod test {
    use super::*;

    const GLOBALS_JSON_ALPHA_3: &str = r#"
{
  "node_name": "amusing-crate",
  "subnet": "community.3",
  "account": {
    "platform": null,
    "address": "0x979db95461652299c34e15df09441b8dfc4edf7a"
  }
}
"#;

    const GLOBALS_JSON_ALPHA_4: &str = r#"
{
  "node_name": "amusing-crate",
  "subnet": "community.4",
  "account": "0x979db95461652299c34e15df09441b8dfc4edf7a"
}
"#;

    #[test]
    fn deserialize_globals() {
        let mut g3: GlobalsState = serde_json::from_str(GLOBALS_JSON_ALPHA_3).unwrap();
        let g4: GlobalsState = serde_json::from_str(GLOBALS_JSON_ALPHA_4).unwrap();
        assert_eq!(g3.node_name, Some("amusing-crate".into()));
        assert_eq!(g3.node_name, g4.node_name);
        assert_eq!(g3.subnet, Some("community.3".into()));
        assert_eq!(g4.subnet, Some("community.4".into()));
        g3.subnet = Some("community.4".into());
        assert_eq!(
            serde_json::to_string(&g3).unwrap(),
            serde_json::to_string(&g4).unwrap()
        );
        assert_eq!(
            g3.account.unwrap().to_string(),
            g4.account.unwrap().to_string()
        );
    }

    #[test]
    fn deserialize_no_account() {
        let g: GlobalsState = serde_json::from_str(
            r#"
    {
      "node_name": "amusing-crate",
      "subnet": "community.3"
    }
    "#,
        )
        .unwrap();

        assert_eq!(g.node_name, Some("amusing-crate".into()));
        assert_eq!(g.subnet, Some("community.3".into()));
        assert!(g.account.is_none())
    }

    #[test]
    fn deserialize_null_account() {
        let g: GlobalsState = serde_json::from_str(
            r#"
    {
      "node_name": "amusing-crate",
      "subnet": "community.4",
      "account": null
    }
    "#,
        )
        .unwrap();

        assert_eq!(g.node_name, Some("amusing-crate".into()));
        assert_eq!(g.subnet, Some("community.4".into()));
        assert!(g.account.is_none())
    }
}
