use structopt::StructOpt;
use crate::startup_config::{NodeConfig, ProviderConfig};
use crate::config::globals::GlobalsState;

#[derive(StructOpt, Clone, Debug)]
pub enum ConfigConfig {
    Get {
        /// 'node_name' or 'subnet'. If unspecified all config is printed.
        name: Option<String>,
    },
    Set(NodeConfig),
}

impl ConfigConfig {

    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        match self {
            ConfigConfig::Get { name } => config_get(config, name),
            ConfigConfig::Set(node_config) => {
                let mut state = GlobalsState::load_or_create(&config.globals_file)?;
                state.update_and_save(node_config, &config.globals_file)?;
                Ok(())
            }
        }
    }
}

pub fn config_get(config: ProviderConfig, name: Option<String>) -> anyhow::Result<()> {
    let globals_state = GlobalsState::load(&config.globals_file)?;
    match name {
        None => {
            if config.json {
                println!("{}", serde_json::to_string_pretty(&globals_state)?);
            } else {
                println!("{}", &globals_state)
            }
        }
        Some(name) => {
            let state = serde_json::to_value(globals_state)?;
            let value = state
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("Invalid name global state property: {}", name))?;
            if config.json {
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                println!("{}: {}", name, serde_json::to_string_pretty(value)?);
            }
        }
    }
    Ok(())
}





