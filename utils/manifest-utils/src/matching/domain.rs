use std::{collections::HashSet, convert::TryFrom, fs::File, io::BufReader, path::PathBuf};

use regex::RegexSetBuilder;
use serde::{Deserialize, Serialize};

use crate::ArgMatch;

use super::{CompositeMatcher, Matcher, RegexMatcher, StrictMatcher};

pub type DomainsMatcher = CompositeMatcher;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DomainPatterns {
    pub domains: Vec<DomainPattern>,
}

impl TryFrom<&PathBuf> for DomainPatterns {
    type Error = anyhow::Error;

    fn try_from(path: &PathBuf) -> Result<Self, anyhow::Error> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        Ok(serde_json::from_reader(reader)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

impl TryFrom<DomainPatterns> for DomainsMatcher {
    type Error = anyhow::Error;

    fn try_from(domain_patterns: DomainPatterns) -> Result<Self, Self::Error> {
        let mut strict_patterns = HashSet::new();
        let mut regex_patterns = HashSet::new();
        for domain_pattern in domain_patterns.domains {
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
