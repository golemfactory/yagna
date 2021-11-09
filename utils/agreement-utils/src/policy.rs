use std::collections::{HashMap, HashSet};
use std::ops::Not;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use ethsign::PublicKey;
use structopt::StructOpt;
use strum::{IntoEnumIterator, VariantNames};
use strum_macros::{Display, EnumIter, EnumString, EnumVariantNames};

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
        let values: Vec<_> = s.split("|").map(|v| v.trim().to_string()).collect();
        Ok(if values.is_empty() {
            Match::All
        } else {
            Match::Values(values)
        })
    }
}

#[derive(Clone, Default)]
pub struct Keystore {
    inner: Arc<RwLock<HashMap<Box<[u8]>, String>>>,
}

impl Keystore {
    pub fn from_path(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let contents = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("cannot read the keystore file: {}", e))?;

        let mut counter = 0_u32;
        let mut map: HashMap<Box<[u8]>, String> = Default::default();

        // <scheme> <key> <alias>
        for line in contents.lines().map(|l| l.trim()) {
            if line.is_empty() || line.starts_with("#") {
                continue;
            }

            let parts = line.split_whitespace().collect::<Vec<_>>();
            let alias = match parts.len() {
                2 => format!("key_no_{}", counter + 1),
                3 => parts[2].to_string(),
                _ => anyhow::bail!("invalid key entry: {}", line),
            };
            let key = Self::parse_key(parts[0], parts[1])?;
            map.insert(key, alias);
            counter += 1;
        }

        Ok(Keystore {
            inner: Arc::new(RwLock::new(map)),
        })
    }

    pub fn replace(&self, other: Keystore) {
        let map = {
            let mut inner = other.inner.write().unwrap();
            std::mem::take(&mut (*inner))
        };
        let mut inner = self.inner.write().unwrap();
        *inner = map;
    }

    pub fn contains(&self, key: &[u8]) -> bool {
        let inner = self.inner.read().unwrap();
        inner.contains_key(key)
    }

    pub fn insert(&self, key: impl Into<Box<[u8]>>, name: impl ToString) {
        let mut inner = self.inner.write().unwrap();
        inner.insert(key.into(), name.to_string());
    }

    fn parse_key(scheme: &str, key: &str) -> anyhow::Result<Box<[u8]>> {
        match scheme.to_lowercase().as_str() {
            "secp256k1" => {
                let key_bytes = hex::decode(key)?;
                let key = PublicKey::from_slice(key_bytes.as_slice())
                    .map_err(|_| anyhow::anyhow!("invalid key"))?;
                Ok(key.bytes().to_vec().into())
            }
            _ => anyhow::bail!("invalid scheme: {}", scheme),
        }
    }
}

impl std::fmt::Debug for Keystore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keystore")
    }
}

fn parse_property_match(input: &str) -> anyhow::Result<(String, Match)> {
    let mut split = input.splitn(2, "=");
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
