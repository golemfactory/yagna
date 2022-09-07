use std::{
    collections::HashSet,
    convert::TryFrom,
    fs::{self, File},
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

use regex::RegexSetBuilder;
use serde::{Deserialize, Serialize};
use ya_utils_path::SwapSave;

use super::{CompositeMatcher, Matcher, RegexMatcher, StrictMatcher};
use crate::{util::str_to_short_hash, ArgMatch};

pub type DomainsMatcher = CompositeMatcher;
pub type SharedDomainPatterns = Arc<Mutex<DomainPatterns>>;
pub type SharedDomainMatchers = Arc<RwLock<DomainsMatcher>>;

#[derive(Clone, Default, Debug)]
pub struct DomainWhitelistState {
    pub patterns: SharedDomainPatterns,
    pub matchers: SharedDomainMatchers,
}

impl TryFrom<DomainPatterns> for DomainWhitelistState {
    type Error = anyhow::Error;

    fn try_from(patterns: DomainPatterns) -> Result<Self, Self::Error> {
        let matcher = DomainsMatcher::try_from(&patterns)?;
        let matcher = Arc::new(RwLock::new(matcher));
        let patterns = Arc::new(Mutex::new(patterns));
        Ok(Self {
            patterns,
            matchers: matcher,
        })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DomainPatterns {
    pub patterns: Vec<DomainPattern>,
}

impl TryFrom<&PathBuf> for DomainPatterns {
    type Error = anyhow::Error;

    fn try_from(path: &PathBuf) -> Result<Self, anyhow::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(serde_json::from_reader(reader)?)
    }
}

impl DomainPatterns {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            log::debug!("Loading domain patterns from: {}", path.display());
            Ok(serde_json::from_reader(io::BufReader::new(
                fs::OpenOptions::new().read(true).open(path)?,
            ))?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        if path.exists() {
            Self::load(path)
        } else {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::File::create(&path)?;
            let patterns = Self::default();
            patterns.save(path)?;
            Ok(patterns)
        }
    }

    pub fn update_and_save(&mut self, path: &Path, patterns: DomainPatterns) -> anyhow::Result<()> {
        self.patterns = patterns.patterns;
        self.save(path)
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        Ok(path.swap_save(serde_json::to_string_pretty(self)?)?)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DomainPattern {
    pub domain: String,
    #[serde(rename = "match", default = "DomainPattern::default_domain_match")]
    pub domain_match: ArgMatch,
}

impl DomainPattern {
    fn default_domain_match() -> ArgMatch {
        ArgMatch::Regex
    }
}

impl TryFrom<&DomainPatterns> for DomainsMatcher {
    type Error = anyhow::Error;

    fn try_from(domain_patterns: &DomainPatterns) -> Result<Self, Self::Error> {
        let mut strict_patterns = HashSet::new();
        let mut regex_patterns = HashSet::new();
        for domain_pattern in &domain_patterns.patterns {
            match domain_pattern.domain_match {
                ArgMatch::Strict => strict_patterns.insert(domain_pattern.domain.to_lowercase()),
                ArgMatch::Regex => regex_patterns.insert(domain_pattern.domain.to_lowercase()),
            };
        }
        let mut matchers: Vec<Box<dyn Matcher>> = Vec::new();
        if !strict_patterns.is_empty() {
            let matcher = StrictMatcher {
                values: strict_patterns,
            };
            matchers.push(Box::new(matcher));
        }
        if !regex_patterns.is_empty() {
            let regex_patterns = regex_patterns.into_iter().collect::<Vec<String>>();
            let regex_patterns = RegexSetBuilder::new(&regex_patterns)
                .case_insensitive(true)
                .ignore_whitespace(true)
                .build()?;
            let matcher = RegexMatcher {
                patterns: regex_patterns,
            };
            matchers.push(Box::new(matcher));
        }
        Ok(CompositeMatcher { matchers })
    }
}

pub fn pattern_to_id(pattern: &DomainPattern) -> String {
    let pattern = &pattern.domain;
    str_to_short_hash(pattern)
}
