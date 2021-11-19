use std::fs::OpenOptions;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

use ya_agreement_utils::policy::{KeyMeta, Keystore};

use crate::startup_config::ProviderConfig;

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub enum KeystoreConfig {
    /// List trusted keys
    List,
    /// Add a new trusted key
    Add(Add),
    /// Remove a trusted key
    Remove(Remove),
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Add {
    key: String,
    #[structopt(long, short)]
    name: Option<String>,
    #[structopt(long, short)]
    scheme: Option<String>,
}

#[derive(StructOpt, Clone, Debug)]
#[structopt(rename_all = "kebab-case")]
pub struct Remove {
    name: String,
}

impl KeystoreConfig {
    pub fn run(self, config: ProviderConfig) -> anyhow::Result<()> {
        match self {
            KeystoreConfig::List => list(config),
            KeystoreConfig::Add(add_) => add(config, add_),
            KeystoreConfig::Remove(remove_) => remove(config, remove_),
        }
    }
}

fn list(config: ProviderConfig) -> anyhow::Result<()> {
    let path = keystore_path(&config)?;
    let keystore = Keystore::load(path)?;
    let keys: Vec<_> = keystore
        .keys()
        .into_iter()
        .map(FormattedKey::from)
        .collect();

    if keys.is_empty() {
        return Ok(());
    }

    if config.json {
        println!("{}", serde_json::to_string_pretty(&keys)?);
    } else {
        println!("Name\tKey\tScheme");
        for key in keys {
            println!("\n{}\t{}\t{}", key.scheme, key.key, key.name);
        }
    }

    Ok(())
}

fn add(config: ProviderConfig, add: Add) -> anyhow::Result<()> {
    let path = keystore_path(&config)?;
    let key = hex::decode(add.key).context("key is not a hex string")?;
    let keystore = Keystore::load(&path)?;
    keystore.insert(key, add.scheme, add.name);
    keystore.save(path)?;
    Ok(())
}

fn remove(config: ProviderConfig, remove: Remove) -> anyhow::Result<()> {
    let path = keystore_path(&config)?;
    let keystore = Keystore::load(&path)?;
    keystore
        .remove_by_name(remove.name)
        .ok_or_else(|| anyhow::anyhow!("key does not exist"))
        .map(|_| ())?;
    keystore.save(path)
}

fn touch(path: impl AsRef<Path>) -> anyhow::Result<()> {
    let path = path.as_ref();
    OpenOptions::new()
        .create(true)
        .write(true)
        .open(path)
        .map(|_| ())
        .context(format!("unable to create file '{}'", path.display()))
}

fn keystore_path(config: &ProviderConfig) -> anyhow::Result<PathBuf> {
    let data_dir = config.data_dir.get_or_create()?;
    let path = data_dir.join(config.trusted_keys_file.as_path());
    touch(path.as_path())?;
    Ok(path)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct FormattedKey {
    name: String,
    key: String,
    scheme: String,
}

impl From<(Box<[u8]>, KeyMeta)> for FormattedKey {
    fn from(tup: (Box<[u8]>, KeyMeta)) -> Self {
        FormattedKey {
            name: tup.1.name,
            key: hex::encode(tup.0),
            scheme: tup.1.scheme,
        }
    }
}
