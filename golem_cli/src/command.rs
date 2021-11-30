//! Subcommand execution handling

use directories::UserDirs;
use std::path::Path;
use std::{env, fs};
use tokio::process::Command;

mod provider;
mod yagna;

pub use provider::*;
pub use yagna::*;

pub struct YaCommand {
    base_path: Box<Path>,
}

impl YaCommand {
    pub fn new() -> anyhow::Result<Self> {
        let mut me = env::current_exe()?;

        // find original binary path.
        for _ in 0..5 {
            if let Ok(base) = fs::read_link(&me) {
                me = base;
            } else {
                break;
            }
        }

        let base_path = me
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Unable to resolve yagna binaries location"))?;

        Ok(Self {
            base_path: base_path.into(),
        })
    }

    pub fn ya_provider(&self) -> anyhow::Result<YaProviderCommand> {
        let mut cmd = Command::new(self.base_path.join("ya-provider"));

        if let Some(user_dirs) = UserDirs::new() {
            let plugins_dir = user_dirs.home_dir().join(".local/lib/yagna/plugins");
            if plugins_dir.exists() {
                cmd.env("EXE_UNIT_PATH", plugins_dir.join("ya-*.json"));
            }
        }
        // YA_PAYMENT_NETWORK is used in different context in golemsp
        // and in ya-provider. golemsp always passes commandline
        // --payment-network arg, so it's safe to just remove it here.
        cmd.env_remove("YA_PAYMENT_NETWORK");

        Ok(YaProviderCommand { cmd })
    }

    pub fn yagna(&self) -> anyhow::Result<YagnaCommand> {
        let cmd = Command::new(self.base_path.join("yagna"));
        Ok(YagnaCommand { cmd })
    }
}
