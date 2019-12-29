use anyhow::{Context, Result};
use std::path::PathBuf;
use structopt::*;

use ethkey::EthAccount;
use std::fs;
use ya_service_api::{CliCtx, Command, CommandOutput, ResponseTable};

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum IdentityCommand {
    /// Show list of all identities
    List,

    /// Display identity
    Show {
        /// Identity alias to show
        alias: String,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = "")]
        password: String,
    },

    /// Create identity
    Create {
        /// Identity alias to create
        #[structopt(long)]
        alias: Option<String>,

        /// Existing keystore to use
        #[structopt(long = "from-keystore")]
        from_keystore: Option<PathBuf>,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = "")]
        password: String,
    },
    /// Update given identity
    Update {
        /// Identity alias to update
        alias: String,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = "")]
        password: String,

        /// password for keystore
        #[structopt(short, long)]
        new_password: String,
    },

    /// Drop given identity
    Drop {
        /// Identity alias to drop
        alias: String,
    },
}

impl Command for IdentityCommand {
    fn run_command(&self, ctx: &CliCtx) -> Result<CommandOutput> {
        let keys_dir = ctx.data_dir.join("keys");
        match self {
            IdentityCommand::List => Ok(ResponseTable {
                columns: vec!["alias".into(), "address".into()],
                values: files(&keys_dir)?
                    .iter()
                    .filter_map(|path| {
                        EthAccount::load_or_generate(path, "")
                            .map_err(|e| {
                                log::info!("{} reading keystore from {:?}", e, path);
                                e
                            })
                            .map(|account| {
                                serde_json::json!([
                                    path.as_path()
                                        .file_name()
                                        .map(|n| n.to_string_lossy())
                                        .unwrap_or("none".into()),
                                    format!("{}", account.address())
                                ])
                            })
                            .ok()
                    })
                    .collect(),
            }
            .into()),
            IdentityCommand::Show { alias, password } => {
                let file_path = keys_dir.join(alias);
                let account = EthAccount::load_or_generate(&file_path, password.clone())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("reading keystore from {:?}", &file_path))?;
                CommandOutput::object(format!("{:#?}", account))
            }
            IdentityCommand::Update {
                alias,
                password,
                new_password,
            } => {
                let file_path = keys_dir.join(alias);
                let account = EthAccount::load_or_generate(&file_path, password.clone())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("reading keystore from {:?}", &file_path))?;

                account
                    .change_password(new_password.clone())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("changing password for {:?}", &file_path))?;

                CommandOutput::object(format!("password changed for {}", account))
            }
            IdentityCommand::Create {
                alias,
                from_keystore,
                password,
            } => {
                let dest_file = keys_dir.join(alias.as_ref().unwrap_or(&"primary".into()));
                if let Some(file_path) = from_keystore {
                    let account = EthAccount::load_or_generate(file_path, password.clone())
                        .map_err(|e| anyhow::Error::msg(e))
                        .context(format!("reading keystore from {:?}", file_path))?;
                    fs::copy(file_path, dest_file)?;

                    return CommandOutput::object(format!("{} read from {:?}", account, file_path));
                }

                let account = EthAccount::load_or_generate(&dest_file, password.clone())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("creating keystore at {:?}", dest_file))?;

                CommandOutput::object(format!("{} created", account))
            }
            _ => anyhow::bail!("command id {:?} is not implemented yet", self),
        }
    }
}

fn files(path: &PathBuf) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(path)
        .context(format!("Error reading directory contents {:?}", path))?
        .flat_map(Result::ok)
        .filter(|entry| {
            let metadata = entry.metadata().ok();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            metadata.map_or(false, |m| !m.is_dir())
                && !name.starts_with(".")
                && !["thumbs.db"].contains(&&*name)
        })
        .map(|entry| entry.path())
        .collect::<Vec<PathBuf>>())
}

#[cfg(test)]
mod tests {
    use ethkey::prelude::*;

    #[test]
    fn test_ethkey() {
        let key = EthAccount::load_or_generate("/tmp/path/to/keystore", "passwd")
            .expect("should load or generate new eth key");

        println!("{:?}", key.address());

        let message = [7_u8; 32];

        // sign the message
        let signature = key.sign(&message).unwrap();

        // verify the signature
        let result = key.verify(&signature, &message).unwrap();
        println!(
            "{}",
            if result {
                "verification ok"
            } else {
                "wrong signature"
            }
        );
    }
}
