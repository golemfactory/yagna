use anyhow::Result;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use structopt::StructOpt;

use ya_core_model::NodeId;
use ya_provider::ReceiverAccount;
use structopt::{clap};

use crate::command::UsageDef;
use crate::terminal::clear_stdin;

const OLD_DEFAULT_SUBNETS: &[&'static str] = &["community", "community.3", "community.4"];
const DEFAULT_SUBNET: &str = "public-beta";

#[derive(StructOpt, Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    #[structopt(env = "NODE_NAME", hidden = true)]
    pub node_name: Option<String>,
    #[structopt(long, env = "SUBNET")]
    pub subnet: Option<String>,

    #[structopt(flatten)]
    pub account: ReceiverAccount,

    /// changes log level from info to debug
    #[structopt(long)]    
    pub debug: bool,

    /// log dir for yagna service
    #[structopt(
        long,
        set = clap::ArgSettings::Global
    )]
    pub log_dir: Option<PathBuf>,    
}

impl RunConfig {
    fn save(&self) -> Result<()> {
        let env_path = config_file();
        if !env_path.exists() {
            fs::create_dir_all(env_path.parent().unwrap())?;
        }
        let mut vars = Vec::new();
        if let Some(node_name) = &self.node_name {
            vars.push(format!("NODE_NAME={}", node_name))
        }
        if let Some(subnet) = &self.subnet {
            vars.push(format!("SUBNET={}", subnet))
        }

        fs::write(env_path, vars.join("\n"))?;
        Ok(())
    }
}

fn project_dirs() -> ProjectDirs {
    ProjectDirs::from("", "GolemFactory", "yagna").unwrap()
}

fn config_file() -> PathBuf {
    let project_dirs = project_dirs();
    project_dirs.config_dir().join("provider.env")
}

pub fn init() -> Result<PathBuf> {
    let config_file = config_file();
    dotenv::from_path(&config_file).ok();
    Ok(config_file)
}

pub async fn setup(run_config: &RunConfig, force: bool) -> Result<i32> {
    if force {
        super::banner();
        eprintln!("Initial node setup");
        let _ = clear_stdin().await;
    }
    let cmd = crate::command::YaCommand::new()?;
    let mut config = cmd.ya_provider()?.get_config().await?;

    log::debug!("Got initial config: {:?}", config);

    if config.node_name.is_none()
        || config
            .node_name
            .as_ref()
            .map(String::is_empty)
            .unwrap_or_default()
    {
        log::debug!("Using node name: {:?}", run_config.node_name);
        config.node_name = run_config.node_name.clone();
    }

    if config.subnet.is_none() {
        config.subnet = run_config.subnet.clone();
    }

    if config.node_name.is_none() || force {
        let node_name = promptly::prompt_default(
            "Node name ",
            config
                .node_name
                .clone()
                .unwrap_or_else(|| names::Generator::default().next().unwrap_or_default()),
        )?;
        // Force subnet upgrade.
        if let Some(subn) = config.subnet.as_deref() {
            if OLD_DEFAULT_SUBNETS.iter().any(|n| n == &subn) {
                config.subnet = None;
            }
        }
        let subnet = promptly::prompt_default(
            "Subnet ",
            config.subnet.unwrap_or_else(|| DEFAULT_SUBNET.to_string()),
        )?;

        let account_msg = &config
            .account
            .map(|n| n.to_string())
            .unwrap_or("Internal Golem wallet".into());
        let message = format!(
            "Ethereum {} wallet address (default={})",
            run_config.account.network, account_msg
        );

        while let Some(account) = promptly::prompt_opt::<String, _>(&message)? {
            match account.parse::<NodeId>() {
                Err(e) => eprintln!("Invalid ethereum address, is should be 20-byte hex (example 0xB1974E1F44EAD2d22bB995167A709b89Fc466B6c): {}", e),
                Ok(account) => {
                    config.account = Some(account);
                    break;
                }
            }
        }

        config.node_name = Some(node_name);
        config.subnet = Some(subnet);
        cmd.ya_provider()?
            .set_config(&config, &run_config.account.network)
            .await?;
    }

    let is_configured = {
        let runtimes: HashSet<String> = cmd
            .ya_provider()?
            .list_runtimes()
            .await?
            .into_iter()
            .map(|r| r.name)
            .collect();
        let presets: HashMap<String, String> = cmd
            .ya_provider()?
            .list_presets()
            .await?
            .into_iter()
            .map(|p| (p.name, p.exeunit_name))
            .collect();
        runtimes.iter().all(|r| presets.get(r) == Some(r))
    };

    if !is_configured {
        let runtimes = cmd.ya_provider()?.list_runtimes().await?;
        let presets: HashSet<String> = cmd
            .ya_provider()?
            .list_presets()
            .await?
            .into_iter()
            .map(|p| p.name)
            .collect();

        let default_glm_per_h = 0.1;
        let glm_per_h = promptly::prompt_default("Price GLM per hour", default_glm_per_h)?;

        let usage = UsageDef {
            cpu: glm_per_h / 3600.0,
            duration: glm_per_h / 3600.0 / 5.0,
            initial: 0.0,
        };

        for runtime in &runtimes {
            eprintln!(
                "{:20} {:50}",
                runtime.name,
                runtime
                    .description
                    .as_ref()
                    .map(AsRef::as_ref)
                    .unwrap_or("")
            );
            if presets.contains(&runtime.name) {
                cmd.ya_provider()?
                    .update_preset(&runtime.name, &runtime.name, &usage)
                    .await?;
            } else {
                cmd.ya_provider()?
                    .create_preset(&runtime.name, &runtime.name, &usage)
                    .await?;
            }
            cmd.ya_provider()?
                .set_profile_activity(&runtime.name, true)
                .await?;
        }

        if cmd
            .ya_provider()?
            .active_presets()
            .await?
            .into_iter()
            .any(|p| p == "default")
        {
            cmd.ya_provider()?
                .set_profile_activity("default", false)
                .await?;
        }
        run_config.save()?;
    }

    Ok(0)
}
