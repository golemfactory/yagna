use anyhow::{Context, Result};
use ethkey::EthAccount;
use std::{fs, path::PathBuf};
use structopt::*;

use futures::Future;
use ya_service_api::{CliCtx, Command, CommandOutput, ResponseTable};

const KEYS_SUBDIR: &str = "keys";
const DEFAULT_IDENTITY: &str = "primary";
const DEFAULT_PASSWORD: &str = "";

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
pub enum IdentityCommand {
    /// Show list of all identities
    List {
        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,
    },

    /// Display identity
    Show {
        /// Identity alias to show
        #[structopt(default_value = DEFAULT_IDENTITY)]
        alias: String,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,
    },

    /// Create identity
    Create {
        /// Identity alias to create
        #[structopt(default_value = DEFAULT_IDENTITY)]
        alias: String,

        /// Existing keystore to use
        #[structopt(long = "from-keystore")]
        from_keystore: Option<PathBuf>,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
        password: String,

        /// force recreation of existing identity
        #[structopt(short, long)]
        force: bool,
    },
    /// Update given identity
    Update {
        /// Identity alias to update
        #[structopt(default_value = DEFAULT_IDENTITY)]
        alias: String,

        /// password for keystore
        #[structopt(short, long)]
        #[structopt(default_value = DEFAULT_PASSWORD)]
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
        let keys_path = keys_path(&ctx.data_dir);
        match self {
            IdentityCommand::List { password } => {
                use ya_core_model::identity;
                use ya_service_bus::typed as bus;
                use ya_service_bus::RpcEndpoint;
                let a = async {
                    let identities: Vec<identity::IdentityInfo> = bus::service(identity::ID)
                        .send(identity::List::default())
                        .await
                        .map_err(|e| anyhow::Error::msg(e))?
                        .unwrap();
                    Ok(ResponseTable {
                        columns: vec!["alias".into(), "address".into()],
                        values: identities
                            .into_iter()
                            .map(|identity| {
                                serde_json::json! {
                                    [identity.alias, identity.node_id]
                                }
                            })
                            .collect(),
                    }
                    .into())
                };
                futures::pin_mut!(a);
                ctx.block_on(a)
            }

            IdentityCommand::Show { alias, password } => {
                let file_path = key_path(&keys_path, &alias);
                if let Err(e) = fs::File::open(&file_path) {
                    return CommandOutput::object(format!("identity '{}': {}", alias, e));
                }
                let account = EthAccount::load_or_generate(&file_path, password.as_str())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("reading keystore from {:?}", file_path))?;
                CommandOutput::object(format!("identity '{}': {:#?}", alias, account))
            }
            IdentityCommand::Update {
                alias,
                password,
                new_password,
            } => {
                let file_path = key_path(&keys_path, alias);
                let account = EthAccount::load_or_generate(&file_path, password.as_str())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("reading keystore from {:?}", file_path))?;

                account
                    .change_password(new_password.as_str())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("changing password for {:?}", file_path))?;

                CommandOutput::object(format!("password changed for identity '{}'", account))
            }
            IdentityCommand::Create {
                alias,
                from_keystore,
                password,
                force,
            } => {
                let dest_path = key_path(&keys_path, alias);
                let mut msg = format!("identity '{}' created", alias);
                if fs::File::open(&dest_path).is_ok() {
                    if !force {
                        return CommandOutput::object(format!(
                            "identity '{}' already exists. Use -f to override",
                            alias
                        ));
                    }
                    msg = format!("identity '{}' already existed. Recreated", alias);
                }

                if let Some(from_path) = from_keystore {
                    fs::copy(from_path, &dest_path).context(format!(
                        "copying keystore from {:?} to {:?}",
                        from_path, &dest_path
                    ))?;
                    let account = EthAccount::load_or_generate(&dest_path, password.as_str())
                        .map_err(|e| anyhow::Error::msg(e))
                        .context(format!("reading keystore from {:?}", from_path))?;

                    return CommandOutput::object(format!(
                        "{} from {:?}: {}",
                        msg, from_path, account
                    ));
                }

                let account = EthAccount::load_or_generate(&dest_path, password.as_str())
                    .map_err(|e| anyhow::Error::msg(e))
                    .context(format!("creating keystore at {:?}", dest_path))?;

                CommandOutput::object(format!("{}: {}", msg, account))
            }
            IdentityCommand::Drop { alias } => {
                let file_path = key_path(&keys_path, alias);
                fs::remove_file(&file_path)
                    .context(format!("Error removing file {:?}", &file_path))?;
                CommandOutput::object(format!("identity '{}' removed", alias))
            }
        }
    }
}

fn keys_path(path: &PathBuf) -> PathBuf {
    path.join(KEYS_SUBDIR)
}

fn key_path(keys_path: &PathBuf, alias: &String) -> PathBuf {
    keys_path.join(alias).with_extension("json")
}

fn files(dir_path: &PathBuf) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(dir_path)
        .context(format!("Error reading directory contents {:?}", dir_path))?
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
