use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr};
use std::ops::Not;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use anyhow::Context;
use futures::future::LocalBoxFuture;
use futures::{FutureExt, StreamExt, TryStreamExt};
use serde_json::{Map, Value};
use structopt::StructOpt;
use url::Url;

use ya_agreement_utils::AgreementView;
use ya_client_model::activity::ExeScriptCommand;
use ya_manifest_utils::{read_manifest, AppManifest, ArgMatch, Command, Feature, Script};
use ya_manifest_utils::{Policy, PolicyConfig};
use ya_utils_networking::resolver::resolve_domain_name;
use ya_utils_networking::vpn::Protocol;

type ValidatorMap = HashMap<Validator, Box<dyn Any>>;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("script validation error: {0}")]
    Script(String),
    #[error("URL validation error: {0}")]
    Url(String),
}

#[derive(Default, Clone)]
pub struct ManifestContext {
    pub manifest: Arc<Option<AppManifest>>,
    pub policy: Arc<PolicyConfig>,
    features: HashSet<Feature>,
    validators: Arc<RwLock<ValidatorMap>>,
}

impl ManifestContext {
    pub fn try_new(agreement: &AgreementView) -> anyhow::Result<Self> {
        let policy = PolicyConfig::from_args_safe().unwrap_or_default();
        let manifest = read_manifest(agreement).context("Unable to read manifest")?;
        let features = {
            let mut features = Self::build_default_features(agreement);
            if let Some(ref manifest) = manifest {
                features.extend(manifest.features().into_iter());
            }
            features
        };

        Ok(Self {
            manifest: Arc::new(manifest),
            policy: Arc::new(policy),
            features,
            validators: Arc::new(RwLock::new(Default::default())),
        })
    }

    pub fn features(&self) -> &HashSet<Feature> {
        &self.features
    }

    pub fn payload(&self) -> Option<String> {
        (*self.manifest)
            .as_ref()
            .and_then(|m| m.find_payload(std::env::consts::ARCH, std::env::consts::OS))
    }

    pub fn build_validators<'a>(&self) -> LocalBoxFuture<'a, anyhow::Result<ValidatorMap>> {
        if self.manifest.is_none()
            || self
                .policy
                .policy_set()
                .contains(&Policy::ManifestCompliance)
                .not()
        {
            return futures::future::ok(Default::default()).boxed_local();
        }

        let manifest = (*self.manifest).clone().unwrap();
        let policy = self.policy.clone();

        async move {
            let mut validators = ValidatorMap::default();

            if let Some(validator) = ScriptValidator::build(&manifest, &policy).await? {
                let validator: Box<dyn Any> = Box::new(validator);
                validators.insert(Validator::Script, validator);
            }

            if let Some(validator) = UrlValidator::build(&manifest, &policy).await? {
                let validator: Box<dyn Any> = Box::new(validator);
                validators.insert(Validator::Url, validator);
            }

            Ok(validators)
        }
        .boxed_local()
    }

    pub fn add_validators(&mut self, iter: impl IntoIterator<Item = (Validator, Box<dyn Any>)>) {
        self.validators.write().unwrap().extend(iter.into_iter());
    }

    pub fn validator<T: ManifestValidator + 'static>(&self) -> Option<T> {
        self.validators
            .read()
            .unwrap()
            .get(&<T as ManifestValidator>::VALIDATOR)
            .and_then(|c| {
                let validator_ref: &dyn Any = &**c;
                validator_ref.downcast_ref::<T>().cloned()
            })
    }

    fn build_default_features(agreement: &AgreementView) -> HashSet<Feature> {
        const CAPABILITIES: &str = "offer.properties.golem.runtime.capabilities";

        let mut features = HashSet::default();
        let cap_vpn = Feature::Vpn.to_string().to_lowercase();

        if let Ok(capabilities) = agreement.get_property::<Vec<String>>(CAPABILITIES) {
            if capabilities
                .iter()
                .any(|c| c.trim().to_lowercase() == cap_vpn)
            {
                features.insert(Feature::Vpn);
            }
        }

        features
    }
}

impl std::fmt::Debug for ManifestContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ManifestContext {{ manifest: {:?}, policy: {:?}, validators: {:?} }}",
            self.manifest,
            self.policy,
            self.validators.read().unwrap().keys().collect::<Vec<_>>()
        )
    }
}

pub trait ManifestValidator: Clone + Sized {
    const VALIDATOR: Validator;

    fn build<'a>(
        manifest: &AppManifest,
        policy: &PolicyConfig,
    ) -> LocalBoxFuture<'a, anyhow::Result<Option<Self>>>;
}

pub trait ManifestValidatorExt: Sized {
    type Inner;

    fn with<F, T, E>(&self, f: F) -> Result<T, E>
    where
        F: FnMut(&Self::Inner) -> Result<T, E>,
        T: Default;
}

impl<C: ManifestValidator> ManifestValidatorExt for Option<C> {
    type Inner = C;

    fn with<F, T, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut(&Self::Inner) -> Result<T, E>,
        T: Default,
    {
        match self.as_ref() {
            Some(c) => f(c),
            None => Ok(T::default()),
        }
    }
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Validator {
    Script,
    Url,
}

#[derive(Clone)]
pub struct ScriptValidator {
    inner: Arc<Script>,
}

impl ManifestValidator for ScriptValidator {
    const VALIDATOR: Validator = Validator::Script;

    fn build<'a>(
        manifest: &AppManifest,
        policy: &PolicyConfig,
    ) -> LocalBoxFuture<'a, anyhow::Result<Option<Self>>> {
        if policy
            .policy_set()
            .contains(&Policy::ManifestScriptCompliance)
            .not()
        {
            return futures::future::ok(None).boxed_local();
        }

        let validator = manifest
            .comp_manifest
            .as_ref()
            .and_then(|m| m.script.as_ref())
            .map(|s| Self {
                inner: Arc::new(s.clone()),
            });

        futures::future::ok(validator).boxed_local()
    }
}

impl ScriptValidator {
    pub fn validate<'a>(
        &self,
        iter: impl IntoIterator<Item = &'a ExeScriptCommand>,
    ) -> Result<(), ValidationError> {
        iter.into_iter()
            .try_for_each(|cmd| self.validate_command(&self.inner, cmd))
    }

    fn validate_command(
        &self,
        script: &Script,
        command: &ExeScriptCommand,
    ) -> Result<(), ValidationError> {
        match command {
            ExeScriptCommand::Transfer { from, to, .. } => {
                Self::validate_transfer(script, from, to)
            }
            ExeScriptCommand::Run {
                entry_point, args, ..
            } => Self::validate_run(script, entry_point, args),
            _ => Ok(()),
        }
    }

    fn validate_transfer(
        script: &Script,
        from: &String,
        to: &String,
    ) -> Result<(), ValidationError> {
        const NAME: &str = "transfer";

        let transfer = format!("{} {} {}", NAME, from, to);
        let mut valid = false;

        for command in script.commands.iter() {
            match command {
                Command::String(pattern) => {
                    if pattern.starts_with(NAME) {
                        valid =
                            valid || Self::match_str(transfer.as_str(), pattern, script.arg_match);
                    }
                }
                Command::Json(value) => {
                    let obj = match value {
                        Value::Object(map) => match map.get(NAME).and_then(|v| v.as_object()) {
                            Some(map) => map,
                            _ => continue,
                        },
                        _ => continue,
                    };
                    let from = match obj.get("from").and_then(|v| v.as_str()) {
                        Some(from) => from,
                        _ => continue,
                    };
                    let to = match obj.get("to").and_then(|v| v.as_str()) {
                        Some(to) => to,
                        _ => continue,
                    };

                    let arg_match = Self::extract_arg_match(obj, script.arg_match);
                    let pattern = format!("{} {} {}", NAME, from, to);

                    valid =
                        valid || Self::match_str(transfer.as_str(), pattern.as_str(), arg_match);
                }
            }

            if valid {
                return Ok(());
            }
        }

        Err(ValidationError::Script(format!(
            "no matching manifest entry found for '{}'",
            transfer
        )))
    }

    fn validate_run(
        script: &Script,
        entry_point: &String,
        args: &[String],
    ) -> Result<(), ValidationError> {
        const NAME: &str = "run";

        let run = format!("{} {} {}", NAME, entry_point, args.join(" "));
        let mut valid = false;

        for command in script.commands.iter() {
            match command {
                Command::String(pattern) => {
                    if pattern.starts_with(NAME) {
                        valid = valid
                            || Self::match_str(run.as_str(), pattern.as_str(), script.arg_match);
                    }
                }
                Command::Json(value) => {
                    let obj = match value {
                        Value::Object(map) => match map.get(NAME).and_then(|v| v.as_object()) {
                            Some(map) => map,
                            _ => continue,
                        },
                        _ => continue,
                    };
                    let args = match obj.get("args") {
                        Some(args) => match args {
                            Value::Array(arr) => arr
                                .iter()
                                .map(|e| e.to_string())
                                .collect::<Vec<_>>()
                                .join(" "),
                            Value::String(string) => string.clone(),
                            _ => continue,
                        },
                        _ => continue,
                    };

                    let arg_match = Self::extract_arg_match(obj, script.arg_match);
                    let pattern = format!("{} {}", NAME, args);

                    valid = valid || Self::match_str(run.as_str(), pattern.as_str(), arg_match);
                }
            }

            if valid {
                return Ok(());
            }
        }

        Err(ValidationError::Script(format!(
            "no matching manifest entry found for '{}'",
            run
        )))
    }

    fn match_str(source: &str, pattern: &str, method: ArgMatch) -> bool {
        match method {
            ArgMatch::Strict => source == pattern,
            ArgMatch::Regex => match regex::Regex::new(pattern) {
                Ok(re) => re.is_match(source),
                _ => false,
            },
        }
    }

    fn extract_arg_match(obj: &Map<String, Value>, fallback: ArgMatch) -> ArgMatch {
        match obj.get("match") {
            Some(val) => match serde_json::from_value::<ArgMatch>(val.clone()) {
                Ok(arg_match) => arg_match,
                _ => fallback,
            },
            _ => fallback,
        }
    }
}

impl FromStr for ScriptValidator {
    type Err = ValidationError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let script: Script = serde_json::from_str(s)
            .map_err(|e| ValidationError::Script(format!("invalid script: {}", e)))?;
        Ok(Self {
            inner: Arc::new(script),
        })
    }
}

#[derive(Clone)]
pub struct UrlValidator {
    inner: Arc<HashSet<(Protocol, IpAddr, u16)>>,
}

impl ManifestValidator for UrlValidator {
    const VALIDATOR: Validator = Validator::Url;

    fn build<'a>(
        manifest: &AppManifest,
        policy: &PolicyConfig,
    ) -> LocalBoxFuture<'a, anyhow::Result<Option<Self>>> {
        if policy
            .policy_set()
            .contains(&Policy::ManifestInetUrlCompliance)
            .not()
        {
            return futures::future::ok(None).boxed_local();
        }

        let urls = manifest
            .comp_manifest
            .as_ref()
            .and_then(|c| c.net.as_ref())
            .and_then(|net| net.inet.as_ref())
            .and_then(|inet| inet.out.as_ref())
            .and_then(|out| out.urls.clone());

        let mut set = Self::DEFAULT_ADDRESSES
            .iter()
            .map(|(proto, ip, port)| (*proto, IpAddr::from(*ip), *port))
            .collect::<HashSet<_, _>>();

        async move {
            let ips = match urls {
                Some(urls) => resolve_ips(urls.iter()).await?,
                None => return Ok(None),
            };

            set.extend(ips.into_iter());

            Ok(Some(Self {
                inner: Arc::new(set),
            }))
        }
        .boxed_local()
    }
}

impl UrlValidator {
    const DEFAULT_ADDRESSES: [(Protocol, Ipv4Addr, u16); 6] = [
        (Protocol::Udp, Ipv4Addr::new(1, 0, 0, 1), 53),
        (Protocol::Udp, Ipv4Addr::new(1, 1, 1, 1), 53),
        (Protocol::Udp, Ipv4Addr::new(8, 8, 4, 4), 53),
        (Protocol::Udp, Ipv4Addr::new(8, 8, 8, 8), 53),
        (Protocol::Udp, Ipv4Addr::new(9, 9, 9, 9), 53),
        (Protocol::Udp, Ipv4Addr::new(149, 112, 112, 112), 53),
    ];

    pub fn validate(&self, proto: Protocol, ip: IpAddr, port: u16) -> Result<(), ValidationError> {
        self.inner
            .contains(&(proto, ip, port))
            .then_some(())
            .ok_or_else(|| {
                ValidationError::Url(format!("address not allowed: {}:{} ({})", ip, port, proto))
            })
    }
}

async fn resolve_ips<'a>(
    urls: impl Iterator<Item = &'a Url>,
) -> anyhow::Result<HashSet<(Protocol, IpAddr, u16)>> {
    futures::stream::iter(urls)
        .map(Ok)
        .try_fold(HashSet::default(), |mut set, url| async move {
            let protocol = match url.scheme() {
                "udp" => Protocol::Udp,
                _ => Protocol::Tcp,
            };
            let port = url
                .port_or_known_default()
                .ok_or_else(|| anyhow::anyhow!("unknown port: {}", url))?;
            let host = url
                .host_str()
                .ok_or_else(|| anyhow::anyhow!("invalid url: {}", url))?;

            let ips: HashSet<IpAddr> = match IpAddr::from_str(host) {
                Ok(ip) => [ip].into(),
                Err(_) => {
                    log::debug!("Resolving IP addresses of '{}'", host);
                    resolve_domain_name(host).await?
                }
            };

            set.extend(ips.into_iter().map(|ip| (protocol, ip, port)));
            Ok(set)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_defaults() {
        let commands = vec![
            ExeScriptCommand::Sign {},
            ExeScriptCommand::Deploy {
                net: Default::default(),
                hosts: Default::default(),
            },
            ExeScriptCommand::Start {
                args: Default::default(),
            },
            ExeScriptCommand::Terminate {},
        ];

        let validator: ScriptValidator = r#"{
            "commands": [],
            "match": "strict"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();
    }

    #[test]
    fn script_run_single() {
        let commands = vec![ExeScriptCommand::Run {
            entry_point: "/bin/date".to_string(),
            args: vec!["-R".to_string()],
            capture: None,
        }];

        let validator: ScriptValidator = r#"{
            "commands": ["run /bin/date -R"]
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["run /bin/date -R"],
            "match": "strict"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["run /bin/date .*"],
            "match": "strict"
        }"#
        .parse()
        .unwrap();
        assert!(validator.validate(&commands).is_err());

        let validator: ScriptValidator = r#"{
            "commands": ["run /bin/date .*"],
            "match": "regex"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"run\": { \"args\": \"/bin/date -R\"} }"]
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"run\": { \"args\": \"/bin/date -R\"} }"],
            "match": "strict"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"run\": { \"args\": \"/bin/date .*\"} }"],
            "match": "strict"
        }"#
        .parse()
        .unwrap();
        assert!(validator.validate(&commands).is_err());

        let validator: ScriptValidator = r#"{
            "commands": ["{\"run\": { \"args\": \"/bin/date .*\"} }"],
            "match": "regex"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"run\": { \"args\": \"/bin/date .*\", \"match\": \"regex\" }}"],
            "match": "strict"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();
    }

    #[test]
    fn script_run_multiple() {
        let commands = vec![
            ExeScriptCommand::Run {
                entry_point: "/bin/date".to_string(),
                args: vec!["-R".to_string()],
                capture: None,
            },
            ExeScriptCommand::Run {
                entry_point: "/bin/cat".to_string(),
                args: vec!["/etc/motd".to_string()],
                capture: None,
            },
        ];

        let validator: ScriptValidator = r#"{
            "commands": [
                "run /bin/date",
                "run /bin/date -X",
                "run /bin/date -R",
                "run /bin/cat /tmp/file",
                "run /bin/cat /etc/motd"
            ]
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": [
                "run /bin/date",
                "run /bin/date -X",
                "run /bin/cat /tmp/file",
                "run /bin/cat /etc/motd"
            ]
        }"#
        .parse()
        .unwrap();
        assert!(validator.validate(&commands).is_err());

        let validator: ScriptValidator = r#"{
            "commands": [
                "run /bin/date",
                "run /bin/date -X",
                "run /bin/date -R",
                "run /bin/cat /tmp/file"
            ]
        }"#
        .parse()
        .unwrap();
        assert!(validator.validate(&commands).is_err());
    }

    #[test]
    fn script_transfer() {
        let commands = vec![ExeScriptCommand::Transfer {
            from: "/src/0x0add".to_string(),
            to: "/dst/0x0add".to_string(),
            args: Default::default(),
        }];

        let validator: ScriptValidator = r#"{
            "commands": [ "transfer /src/0x0add /dst/0x0add" ]
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": [ "transfer /src/.* /dst/0x0add" ],
            "match": "regex"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": [ "transfer /src/0x0add /dst/.*" ],
            "match": "regex"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"transfer\": { \"from\": \"/src/0x0add\", \"to\": \"/dst/0x0add\" } }"]
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"transfer\": { \"from\": \".*\", \"to\": \"/dst/0x0add\" } }"],
            "match": "regex"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();

        let validator: ScriptValidator = r#"{
            "commands": ["{\"transfer\": { \"from\": \"/src/0x0add\", \"to\": \".*\" } }"],
            "match": "regex"
        }"#
        .parse()
        .unwrap();
        validator.validate(&commands).unwrap();
    }
}
