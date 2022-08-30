pub mod domain;

use std::collections::HashSet;

use regex::RegexSet;

use crate::ArgMatch;

trait MatchPattern {
    fn value(&self) -> String;
    fn match_type(&self) -> ArgMatch;
}

pub trait Matcher {
    fn matches(&self, txt: &str) -> bool;
}

struct RegexMatcher {
    patterns: RegexSet,
}

impl Matcher for RegexMatcher {
    fn matches(&self, txt: &str) -> bool {
        self.patterns.matches(txt).matched_any()
    }
}

struct StrictMatcher {
    values: HashSet<String>,
}

impl Matcher for StrictMatcher {
    fn matches(&self, txt: &str) -> bool {
        self.values.contains(&txt.to_lowercase())
    }
}

#[derive(Default)]
pub struct CompositeMatcher {
    matchers: Vec<Box<dyn Matcher>>,
}

impl Matcher for CompositeMatcher {
    fn matches(&self, txt: &str) -> bool {
        self.matchers.iter().any(|matcher| matcher.matches(txt))
    }
}
