/// Identity management CLI parser and runner
use std::cmp::Reverse;
use std::convert::TryInto;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use ethsign::{KeyFile, Protected};
use rustc_hex::ToHex;
use sha2::Digest;
use structopt::*;
use tokio::io::{AsyncReadExt, BufReader};

use ya_client_model::NodeId;
use ya_core_model::identity::{self};
use ya_service_api::{CliCtx, CommandOutput, ResponseTable};
use ya_service_bus::typed as bus;
use ya_service_bus::RpcEndpoint;

mod drop_id;
mod list;

const FILE_CHUNK_SIZE: usize = 40960;

#[derive(Debug, Clone, Default)]
pub enum NodeOrAlias {
    Node(NodeId),
    Alias(String),
    #[default]
    DefaultNode,
}

impl std::str::FromStr for NodeOrAlias {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Ok(NodeOrAlias::DefaultNode);
        }
        Ok(if s.starts_with("0x") {
            match NodeId::from_str(s) {
                Ok(node_id) => NodeOrAlias::Node(node_id),
                Err(_e) => NodeOrAlias::Alias(s.to_owned()),
            }
        } else {
            NodeOrAlias::Alias(s.to_owned())
        })
    }
}

impl NodeOrAlias {
    async fn resolve(&self) -> anyhow::Result<NodeId> {
        match self {
            NodeOrAlias::Node(node_id) => Ok(*node_id),
            NodeOrAlias::Alias(alias) => {
                let id = bus::service(identity::BUS_ID)
                    .send(identity::Get::ByAlias(alias.to_owned()))
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                match id? {
                    Some(id) => Ok(id.node_id),
                    None => anyhow::bail!("node with alias {} not found", alias),
                }
            }
            NodeOrAlias::DefaultNode => {
                let id = bus::service(identity::BUS_ID)
                    .send(identity::Get::ByDefault)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))?;
                match id? {
                    Some(id) => Ok(id.node_id),
                    None => anyhow::bail!("default node not found"),
                }
            }
        }
    }
}

impl From<NodeOrAlias> for identity::Get {
    fn from(na: NodeOrAlias) -> Self {
        match na {
            NodeOrAlias::DefaultNode => identity::Get::ByDefault,
            NodeOrAlias::Alias(alias) => identity::Get::ByAlias(alias),
            NodeOrAlias::Node(node_id) => identity::Get::ByNodeId(node_id),
        }
    }
}

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(rename_all = "kebab-case")]
/// Identity management
pub enum IdentityCommand {
    /// Show list of all identities
    List {},

    /// Display identity
    Show {
        /// Identity alias to show
        node_or_alias: Option<NodeOrAlias>,
    },

    /// Print the public key
    PubKey {
        /// Identity alias
        node_or_alias: Option<NodeOrAlias>,
    },

    /// Sign file contents
    Sign(SignCommand),

    /// Locks identity
    Lock {
        /// NodeId or key
        node_or_alias: Option<NodeOrAlias>,
        #[structopt(long)]
        new_password: bool,
    },

    Unlock {
        node_or_alias: Option<NodeOrAlias>,
        /// Password from argument (unsafe) - do not pass this argument and you will be prompted for password in safe way.
        #[structopt(long = "password")]
        password: Option<String>,
    },

    /// Create identity
    Create {
        /// Identity alias to create
        alias: Option<String>,

        /// Existing keystore to use
        #[structopt(long = "from-keystore")]
        from_keystore: Option<PathBuf>,

        /// Existing private key to use (unsafe) - use keystore instead, it's much safer
        #[structopt(long = "from-private-key")]
        from_private_key: Option<String>,

        /// Password from argument (unsafe) - do not pass this argument and you will be prompted for password in safe way.
        #[structopt(long = "password")]
        password: Option<String>,

        /// password for keystore
        #[structopt(long = "no-password")]
        no_password: bool,
    },
    /// Update given identity
    Update {
        /// Identity to update
        #[structopt(default_value = "")]
        alias_or_id: NodeOrAlias,
        #[structopt(long)]
        alias: Option<String>,
        #[structopt(long = "set-default")]
        set_default: bool,
    },

    /// Drop given identity
    Drop {
        /// Identity alias to drop
        node_or_alias: NodeOrAlias,
        #[structopt(long)]
        force: bool,
    },

    /// Exports given identity to a file | stdout
    Export {
        /// Identity alias to export
        node_or_alias: Option<NodeOrAlias>,

        /// Export using unencrypted private key format,
        /// easier to use later, but less secure
        #[structopt(long = "plain")]
        plain: bool,

        /// File path where identity will be written. Defaults to `stdout`
        #[structopt(long = "file-path")]
        file_path: Option<PathBuf>,
    },
}

#[derive(StructOpt, Debug)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(rename_all = "kebab-case")]
pub struct SignCommand {
    /// Input file path
    file_path: PathBuf,

    /// NodeId or key
    node_or_alias: Option<NodeOrAlias>,
}

//local function to decrypt keystore (for export key command)
fn to_private_key(key_file_json: &str) -> Result<[u8; 32], anyhow::Error> {
    let key_file: KeyFile = serde_json::from_str(key_file_json)?;
    let empty_pass = Protected::new::<Vec<u8>>("".into());
    let secret = match key_file.to_secret_key(&empty_pass) {
        Ok(secret) => secret,
        Err(ethsign::Error::InvalidPassword) => {
            let password: Protected = rpassword::read_password_from_tty(Some("Password: "))
                .map_err(|e| anyhow!("Failed to read password: {}", e))?
                .into();
            match key_file.to_secret_key(&password) {
                Ok(secret) => secret,
                Err(ethsign::Error::InvalidPassword) => {
                    return Err(anyhow!("Invalid password"));
                }
                Err(e) => return Err(anyhow!(e)),
            }
        }
        Err(e) => return Err(anyhow!(e)),
    };

    // HACK, due to hidden secret key data we have to use this little hack to extract private key
    let pass = Protected::new::<Vec<u8>>("hack".into());

    secret
        .to_crypto(&pass, 1)
        .map_err(|err| anyhow!("Failed to encrypt private key: {}", err))?
        .decrypt(&pass)
        .map_err(|err| anyhow!("Failed to decrypt private key: {}", err))?
        .try_into()
        .map_err(|_| anyhow!("Wrong key length after decryption"))
}

impl IdentityCommand {
    pub async fn run_command(&self, _ctx: &CliCtx) -> Result<CommandOutput> {
        match self {
            IdentityCommand::List { .. } => list::list().await,
            IdentityCommand::Show { node_or_alias } => {
                let command: identity::Get = node_or_alias.clone().unwrap_or_default().into();
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(command)
                        .await
                        .map_err(anyhow::Error::msg)?,
                )
            }
            IdentityCommand::PubKey { node_or_alias } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::GetPubKey(node_id))
                        .await
                        .map_err(anyhow::Error::msg)?
                        .map(|v| {
                            let key = v.to_hex::<String>();
                            serde_json::json! {{ "pubKey": key }}
                        })?,
                )
            }
            IdentityCommand::Sign(SignCommand {
                node_or_alias,
                file_path,
            }) => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;

                let file = tokio::fs::File::open(file_path)
                    .await
                    .context("unable to read input path")?;
                let meta = file
                    .metadata()
                    .await
                    .context("unable to read input metadata")?;

                let mut reader = BufReader::with_capacity(FILE_CHUNK_SIZE, file);
                let mut buf: [u8; FILE_CHUNK_SIZE] = [0; FILE_CHUNK_SIZE];
                let mut remaining = meta.len() as usize;

                let mut sha256 = sha2::Sha256::default();

                loop {
                    let count = remaining.min(FILE_CHUNK_SIZE);
                    match reader.read_exact(&mut buf[..count]).await? {
                        0 => break,
                        count => {
                            sha256.update(&buf[..count]);
                            remaining -= count;
                        }
                    }
                }
                let payload = sha256.finalize().to_vec();

                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::Sign { node_id, payload })
                        .await
                        .map_err(anyhow::Error::msg)?
                        .map(|v| {
                            let sig = v.to_hex::<String>();
                            serde_json::json! {{ "sig": sig }}
                        })?,
                )
            }
            IdentityCommand::Update {
                alias_or_id,
                alias,
                set_default,
            } => {
                let node_id = alias_or_id.resolve().await?;
                let id = bus::service(identity::BUS_ID)
                    .send(
                        identity::Update::with_id(node_id)
                            .with_alias(alias.clone())
                            .with_default(*set_default),
                    )
                    .await
                    .map_err(anyhow::Error::msg)?;
                CommandOutput::object(id)
            }
            IdentityCommand::Create {
                alias,
                from_keystore,
                from_private_key,
                password,
                no_password,
            } => {
                if from_keystore.is_some() && from_private_key.is_some() {
                    anyhow::bail!("Only one of --from-keystore or --from-private-key can be used")
                }
                if from_private_key.is_some() {
                    log::warn!("Using private key directly is not recommended. Use keystore instead. Your key could leak in command history, check and clean logs.")
                }

                let from_private_key_slice: Option<[u8; 32]> = if let Some(from_private_key) =
                    from_private_key
                {
                    let v = hex::decode(from_private_key).map_err(|e| {
                        anyhow::anyhow!(
                            "Private key has to be plain hex string without 0x prefix - {e}"
                        )
                    })?;
                    let slice = v[0..32].try_into().map_err(|e|
                        anyhow::anyhow!("Ethereum key has to be 32 bytes long. Provide hex string of length 64 - {e}")
                    )?;
                    Some(slice)
                } else {
                    None
                };

                let key_file = if let Some(keystore) = from_keystore {
                    std::fs::read_to_string(keystore)?
                } else {
                    let password = if *no_password {
                        Protected::from("")
                    } else if let Some(password) = password {
                        Protected::from(password.as_str())
                    } else {
                        let password: Protected =
                            rpassword::read_password_from_tty(Some("Password: "))?.into();
                        let password2: Protected =
                            rpassword::read_password_from_tty(Some("Confirm password: "))?.into();
                        if password.as_ref() != password2.as_ref() {
                            anyhow::bail!("Password and confirmation do not match.")
                        }
                        password
                    };
                    crate::id_key::generate_new_keyfile(password, from_private_key_slice)?
                };

                let id = bus::service(identity::BUS_ID)
                    .send(identity::CreateGenerated {
                        alias: alias.clone(),
                        from_keystore: Some(key_file),
                    })
                    .await
                    .map_err(anyhow::Error::msg)?;
                CommandOutput::object(id)
            }
            IdentityCommand::Lock {
                node_or_alias,
                new_password,
            } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let password = if *new_password {
                    let password: String = rpassword::read_password_from_tty(Some("Password: "))?;
                    let password2: String =
                        rpassword::read_password_from_tty(Some("Confirm password: "))?;
                    if password != password2 {
                        anyhow::bail!("Password and confirmation do not match.")
                    }
                    Some(password)
                } else {
                    None
                };
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::Lock::with_id(node_id).with_set_password(password))
                        .await
                        .map_err(anyhow::Error::msg)?,
                )
            }
            IdentityCommand::Unlock {
                node_or_alias,
                password,
            } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let password = if let Some(password) = password {
                    password.to_string()
                } else {
                    rpassword::read_password_from_tty(Some("Password: "))?
                };
                CommandOutput::object(
                    bus::service(identity::BUS_ID)
                        .send(identity::Unlock::with_id(node_id, password))
                        .await
                        .map_err(anyhow::Error::msg)?,
                )
            }
            IdentityCommand::Drop {
                node_or_alias,
                force,
            } => drop_id::drop_id(node_or_alias, *force).await,
            IdentityCommand::Export {
                node_or_alias,
                file_path,
                plain,
            } => {
                let node_id = node_or_alias.clone().unwrap_or_default().resolve().await?;
                let mut key_file = bus::service(identity::BUS_ID)
                    .send(identity::GetKeyFile(node_id))
                    .await?
                    .map_err(anyhow::Error::msg)?;

                if *plain {
                    let private_key = to_private_key(&key_file);
                    let decrypted_key = match private_key {
                        Ok(key) => rustc_hex::ToHex::to_hex::<String>(key.as_slice()),
                        Err(e) => anyhow::bail!(e),
                    };
                    key_file = decrypted_key;
                }

                match file_path {
                    Some(file) => {
                        if file.exists() {
                            anyhow::bail!("File already exists")
                        }

                        std::fs::write(file, key_file)?;
                        CommandOutput::object(format!("Written to '{}'", file.display()))
                    }
                    None => CommandOutput::object(key_file),
                }
            }
        }
    }
}
