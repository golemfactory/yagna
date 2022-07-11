use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::OpenOptions;
use std::io::Write;
use std::ops::Not;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use ethsign::PublicKey;
use structopt::StructOpt;
use strum::{Display, EnumIter, EnumString, EnumVariantNames, IntoEnumIterator, VariantNames};

const SCHEME_SECP256K1: &str = "secp256k1";

/// Policy configuration
#[derive(StructOpt, Clone, Debug, Default)]
pub struct PolicyConfig {
    /// Disable policy components
    #[structopt(
        long,
        env,
        parse(try_from_str),
        possible_values = Policy::VARIANTS,
        case_insensitive = true,
    )]
    pub policy_disable_component: Vec<Policy>,
    /// Whitelist property names (optionally filtered by value)
    // e.g.
    //  POLICY_TRUST_PROPERTY="prop1=1|2,prop2=3|4|5,prop3"
    //  POLICY_TRUST_PROPERTY=prop4
    #[structopt(
        long,
        env,
        parse(try_from_str = parse_property_match),
    )]
    pub policy_trust_property: Vec<(String, Match)>,
    #[structopt(skip)]
    pub trusted_keys: Option<Keystore>,
}

impl PolicyConfig {
    pub fn policy_set(&self) -> HashSet<Policy> {
        if self.policy_disable_component.contains(&Policy::All) {
            Default::default()
        } else {
            let mut components: HashSet<_> = Policy::iter().collect();
            components.retain(|c| self.policy_disable_component.contains(c).not());
            components
        }
    }

    #[inline]
    pub fn trusted_property_map(&self) -> HashMap<String, Match> {
        self.policy_trust_property.iter().cloned().collect()
    }
}

#[non_exhaustive]
#[derive(
    Clone, Copy, Debug, Hash, Eq, PartialEq, EnumIter, EnumVariantNames, EnumString, Display,
)]
#[strum(serialize_all = "snake_case")]
pub enum Policy {
    All,
    ManifestSignatureValidation,
    ManifestCompliance,
    ManifestInetUrlCompliance,
    ManifestScriptCompliance,
}

#[non_exhaustive]
#[derive(Clone, Debug, Eq, PartialEq, Display)]
pub enum Match {
    All,
    Values(Vec<String>),
}

impl FromStr for Match {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // use '|' instead of ',' to support multi-value
        // environment variables
        let values: Vec<_> = s.split('|').map(|v| v.trim().to_string()).collect();
        Ok(if values.is_empty() {
            Match::All
        } else {
            Match::Values(values)
        })
    }
}

#[derive(Clone, Default)]
pub struct Keystore {
    inner: Arc<RwLock<BTreeMap<Box<[u8]>, KeyMeta>>>,
}

#[derive(Clone, Debug)]
pub struct KeyMeta {
    pub scheme: String,
    pub name: String,
}

impl Default for KeyMeta {
    fn default() -> Self {
        KeyMeta::new(None, None)
    }
}

impl KeyMeta {
    pub fn new(scheme: Option<String>, name: Option<String>) -> Self {
        KeyMeta {
            scheme: scheme.unwrap_or_else(|| SCHEME_SECP256K1.to_string()),
            name: name.unwrap_or_else(|| petname::petname(3, "-")),
        }
    }
}

impl Keystore {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read the keystore file: {}", e))?;

        let map = contents
            .lines()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty() && !s.starts_with('#'))
            .map(parse_key_entry)
            .collect::<Result<_, _>>()?;

        Ok(Keystore {
            inner: Arc::new(RwLock::new(map)),
        })
    }

    pub fn save(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let lines: Vec<String> = {
            let inner = self.inner.read().unwrap();
            inner.iter().map(key_entry_to_string).collect()
        };

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(path)?;
        lines
            .into_iter()
            .try_for_each(|line| file.write_all(line.as_bytes()))?;

        file.flush()?;
        file.sync_all()?;

        Ok(())
    }

    pub fn replace(&self, other: Keystore) {
        let map = {
            let mut inner = other.inner.write().unwrap();
            std::mem::take(&mut (*inner))
        };
        let mut inner = self.inner.write().unwrap();
        *inner = map;
    }
}

impl Keystore {
    pub fn contains(&self, key: &[u8]) -> bool {
        let inner = self.inner.read().unwrap();
        inner.contains_key(key)
    }

    pub fn insert(
        &self,
        key: impl Into<Box<[u8]>>,
        scheme: Option<String>,
        name: Option<String>,
    ) -> anyhow::Result<()> {
        let mut inner = self.inner.write().unwrap();
        let meta = KeyMeta::new(scheme, name);
        let boxed_key = key.into();
        let verification_key = boxed_key.clone();
        // Verify key
        PublicKey::from_slice(&*verification_key)
            .map_err(|e| anyhow::anyhow!(format!("invalid key provided: {:?}", e)))?;

        inner.insert(boxed_key, meta);
        Ok(())
    }

    pub fn remove_by_name(&self, name: impl AsRef<str>) -> Option<Box<[u8]>> {
        let name = name.as_ref();
        let mut inner = self.inner.write().unwrap();
        let key = inner
            .iter()
            .find(|(_, meta)| meta.name.as_str() == name)
            .map(|(key, _)| key.clone())?;
        inner.remove(&key);
        Some(key)
    }

    pub fn keys(&self) -> BTreeMap<Box<[u8]>, KeyMeta> {
        let inner = self.inner.read().unwrap();
        (*inner).clone()
    }
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keystore")
    }
}

fn parse_property_match(input: &str) -> anyhow::Result<(String, Match)> {
    let mut split = input.splitn(2, '=');
    let property = split
        .next()
        .ok_or_else(|| anyhow::anyhow!("missing property name"))?
        .to_string();
    let values = match split.next() {
        Some(s) => Match::from_str(s)?,
        None => Match::All,
    };
    Ok((property, values))
}

fn parse_key(scheme: &str, key: &str) -> anyhow::Result<Box<[u8]>> {
    match scheme.to_lowercase().as_str() {
        SCHEME_SECP256K1 => {
            let key_bytes = hex::decode(key)?;
            let key = PublicKey::from_slice(key_bytes.as_slice())
                .map_err(|_| anyhow::anyhow!("invalid key"))?;
            Ok(key.bytes().to_vec().into())
        }
        _ => anyhow::bail!("invalid scheme: {}", scheme),
    }
}

fn parse_key_entry(line: &str) -> anyhow::Result<(Box<[u8]>, KeyMeta)> {
    let mut split = line.trim().split_whitespace();
    let scheme = match split.next() {
        Some(scheme) => scheme.to_string(),
        None => anyhow::bail!("scheme missing"),
    };
    let key = match split.next() {
        Some(key_hex) => parse_key(scheme.as_str(), key_hex)?,
        None => anyhow::bail!("key missing"),
    };
    let name = split.next().map(|s| s.to_string());

    Ok((key, KeyMeta::new(Some(scheme), name)))
}

fn key_entry_to_string((key, meta): (&Box<[u8]>, &KeyMeta)) -> String {
    format!("{}\t{}\t{}\n", meta.scheme, hex::encode(key), meta.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_config<S: AsRef<str>>(args: S) -> PolicyConfig {
        let arguments = shlex::split(args.as_ref()).expect("failed to parse arguments");
        PolicyConfig::from_iter(arguments)
    }

    #[test]
    fn policy_config() {
        let config = build_config("TEST");
        assert_eq!(config.policy_disable_component, Vec::default());
        assert_eq!(config.policy_trust_property, Vec::default());

        let config = build_config(
            "TEST \
            --policy-trust-property property",
        );
        assert_eq!(config.policy_disable_component, Vec::default());
        assert_eq!(
            config.policy_trust_property,
            vec![("property".to_string(), Match::All)]
        );

        let config = build_config(
            "TEST \
            --policy-disable-component all \
            --policy-trust-property property=value1|value2",
        );
        assert_eq!(config.policy_disable_component, vec![Policy::All]);
        assert_eq!(
            config.policy_trust_property,
            vec![(
                "property".to_string(),
                Match::Values(vec!["value1".to_string(), "value2".to_string()])
            )]
        );
    }
}
