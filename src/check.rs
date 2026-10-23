//! `check`: does a PR description agree with the report? (Open question 3 in the LLD: usable as a CI gate.)

use crate::report::Report;
use regex::Regex;
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Mismatch {
    /// Commits that should be listed (NEW / PARTIAL) but the body never mentions.
    pub missing: Vec<String>,
    /// Commits the body mentions that are already on base.
    pub stale: Vec<String>,
}

impl Mismatch {
    pub fn is_empty(&self) -> bool {
        self.missing.is_empty() && self.stale.is_empty()
    }
}

pub fn check(report: &Report, body: &str) -> Mismatch {
    let re = Regex::new(r"\b[0-9a-f]{7,40}\b").unwrap();
    let mentioned: HashSet<&str> = re.find_iter(body).map(|m| m.as_str()).collect();
    let is_mentioned = |sha: &str| mentioned.iter().any(|m| sha.starts_with(m));
    let mut m = Mismatch::default();
    for c in &report.commits {
        let says = is_mentioned(&c.sha);
        if c.class.is_listed() && !says {
            m.missing.push(format!("{} {}", c.short, c.subject));
        } else if !c.class.is_listed() && says {
            m.stale.push(format!("{} {}", c.short, c.subject));
        }
    }
    m
}
