use crate::command::UsageDef;
use crate::terminal::clear_stdin;
use anyhow::Result;
use directories::ProjectDirs;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use std::fs;
use structopt::StructOpt;

const DEFAULT_SUBNET: &str = "community";

#[derive(StructOpt)]
pub struct RunConfig {
    #[structopt(long, env = "NODE_NAME")]
    pub node_name: Option<String>,
    #[structopt(long, env = "SUBNET")]
    pub subnet: Option<String>,
    #[structopt(long, env = "YA_CONF_PRICES", hidden = true)]
    pub prices_configured: bool,
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
        if self.prices_configured {
            vars.push("YA_CONF_PRICES=1".into())
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

pub fn init() -> Result<()> {
    dotenv::from_path(config_file()).ok();
    Ok(())
}

pub async fn setup(run_config: &mut RunConfig, force: bool) -> Result<i32> {
    if force {
        super::banner();
        eprintln!("Initial node setup");
        let _ = clear_stdin().await;
    }
    let cmd = crate::command::YaCommand::new()?;
    let mut config = cmd.ya_provider()?.get_config().await?;

    if config.node_name.is_none()
        || config
            .node_name
            .as_ref()
            .map(String::is_empty)
            .unwrap_or_default()
    {
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
        let subnet = promptly::prompt_default(
            "Subnet ",
            config.subnet.unwrap_or_else(|| DEFAULT_SUBNET.to_string()),
        )?;
        config.node_name = Some(node_name);
        config.subnet = Some(subnet);
        cmd.ya_provider()?.set_config(&config).await?;
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
        let ngnt_per_h = promptly::prompt_default("Price NGNT per hour", 5.0)?;

        let usage = UsageDef {
            cpu: ngnt_per_h / 3600.0,
            duration: ngnt_per_h / 3600.0 / 5.0,
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
        run_config.prices_configured = true;
        run_config.save()?;
    }

    Ok(0)
}
