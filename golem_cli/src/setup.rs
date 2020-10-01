use crate::settings_show::get_runtimes;
use anyhow::Result;
use directories::{BaseDirs, ProjectDirs};
use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};
use structopt::StructOpt;

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
    if env::var("EXE_UNIT_PATH").is_err() {
        let exe_unit_path = BaseDirs::new()
            .unwrap()
            .home_dir()
            .join(".local/lib/yagna/plugins/ya-runtime-*.json");
        eprintln!("path={}", exe_unit_path.display());
        env::set_var("EXE_UNIT_PATH", exe_unit_path);
    }
    Ok(())
}

pub async fn setup(config: &mut RunConfig, force: bool) -> Result<i32> {
    if force {
        super::banner();
        eprintln!("Initial node setup");
    }
    if config.node_name.is_none() || force {
        let node_name = promptly::prompt_default(
            "Node name",
            config
                .node_name
                .clone()
                .unwrap_or_else(|| names::Generator::default().next().unwrap_or_default()),
        )?;
        let subnet = promptly::prompt_default("Subnet", "u-testnet".to_string())?;
        config.node_name = Some(node_name);
        config.subnet = Some(subnet);
        config.save()?;
    }
    if force && !config.prices_configured {
        let runtimes = get_runtimes().await?;
        let ngnt_per_h = promptly::prompt_default("Price NGNT per hour", 5.0)?;

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
            let _ = Command::new("ya-provider")
                .arg("preset")
                .arg("create")
                .arg("--preset-name")
                .arg(&runtime.name)
                .arg("--exe-unit")
                .arg(&runtime.name)
                .arg("--no-interactive")
                .arg("--pricing")
                .arg("linear")
                .arg("--price")
                .arg(format!("CPU={}", ngnt_per_h / 3600.0))
                .arg("--price")
                .arg(format!("Duration={}", ngnt_per_h / 3600.0 / 5.0))
                .arg("--price")
                .arg(format!("Init price=0"))
                .status()?;
            let _ = Command::new("ya-provider")
                .arg("preset")
                .arg("activate")
                .arg(&runtime.name)
                .status()?;
        }
        let _ = Command::new("ya-provider")
            .arg("preset")
            .arg("deactivate")
            .arg("default")
            .status()?;
        config.prices_configured = true;
        config.save()?;
    }

    Ok(0)
}
