//! Rewritten branches: what changed between the old and the new version of a force-pushed head.
//! A thin parser over `git range-diff`, which pairs old commits with new ones by their patches.

use crate::error::bad;
use crate::git::Git;
use crate::refs::{self, RefInfo};
use anyhow::Result;
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PairStatus {
    /// Same patch and message.
    Same,
    /// Paired, but the patch or message changed.
    Changed,
    /// In the old version only.
    Dropped,
    /// In the new version only.
    Added,
}

#[derive(Debug, Clone, Serialize)]
pub struct Pair {
    pub status: PairStatus,
    /// Position in the old / new series (1-based).
    pub old_index: Option<u32>,
    pub new_index: Option<u32>,
    pub old_sha: Option<String>,
    pub new_sha: Option<String>,
    pub subject: String,
    /// What changed between the two versions of the commit (range-diff's diff of diffs).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub interdiff: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Rewrite {
    pub schema_version: u32,
    pub base: RefInfo,
    pub old: RefInfo,
    pub new: RefInfo,
    pub pairs: Vec<Pair>,
}

impl Rewrite {
    pub fn count(&self, s: PairStatus) -> usize {
        self.pairs.iter().filter(|p| p.status == s).count()
    }
}

pub fn parse_range_diff(text: &str) -> Vec<Pair> {
    let header =
        Regex::new(r"^(-|\d+):\s+([0-9a-f-]+) ([=!<>]) (-|\d+):\s+([0-9a-f-]+) (.*)$").unwrap();
    let mut pairs: Vec<Pair> = Vec::new();
    for line in text.lines() {
        if let Some(c) = header.captures(line) {
            let idx = |s: &str| s.parse::<u32>().ok();
            let sha = |s: &str| (!s.chars().all(|c| c == '-')).then(|| s.to_string());
            pairs.push(Pair {
                status: match &c[3] {
                    "=" => PairStatus::Same,
                    "!" => PairStatus::Changed,
                    "<" => PairStatus::Dropped,
                    _ => PairStatus::Added,
                },
                old_index: idx(&c[1]),
                new_index: idx(&c[4]),
                old_sha: sha(&c[2]),
                new_sha: sha(&c[5]),
                subject: c[6].trim().to_string(),
                interdiff: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix("    ")
            && let Some(p) = pairs.last_mut()
        {
            p.interdiff.push(rest.to_string());
        }
    }
    pairs
}

pub fn rewritten(git: &Git, base: &str, old: Option<&str>, new: &str) -> Result<Rewrite> {
    let base = refs::resolve(git, base)?;
    let new_info = refs::resolve(git, new)?;
    let old_spec = match old {
        Some(o) => o.to_string(),
        // the remote-tracking ref's previous value, i.e. what it pointed at before the last fetch
        None => format!("{new}@{{1}}"),
    };
    let old_info = refs::resolve(git, &old_spec).map_err(|_| {
        bad(format!(
            "cannot find the old version ({old_spec}): pass --old <ref or sha> (e.g. from `git reflog show {new}`)"
        ))
    })?;
    let out = git.run(&[
        "range-diff",
        "--no-color",
        &base.sha,
        &old_info.sha,
        &new_info.sha,
    ])?;
    Ok(Rewrite {
        schema_version: crate::report::SCHEMA_VERSION,
        base,
        old: RefInfo {
            name: old_spec,
            sha: old_info.sha,
        },
        new: new_info,
        pairs: parse_range_diff(&String::from_utf8_lossy(&out)),
    })
}

pub fn render_text(r: &Rewrite) -> String {
    let mut s = format!(
        "{} {} -> {}\n{} unchanged, {} amended, {} dropped, {} added\n\n",
        r.base.name,
        &r.old.sha[..10],
        &r.new.sha[..10],
        r.count(PairStatus::Same),
        r.count(PairStatus::Changed),
        r.count(PairStatus::Dropped),
        r.count(PairStatus::Added),
    );
    for p in &r.pairs {
        let (mark, label) = match p.status {
            PairStatus::Same => ("=", "unchanged"),
            PairStatus::Changed => ("!", "amended"),
            PairStatus::Dropped => ("<", "dropped"),
            PairStatus::Added => (">", "added"),
        };
        let sha = p.new_sha.as_deref().or(p.old_sha.as_deref()).unwrap_or("");
        s += &format!("  {mark} {label:<9} {sha:<9} {}\n", p.subject);
    }
    s
}
