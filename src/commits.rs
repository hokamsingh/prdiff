//! Stage A: enumerate head-only commits with their changed files.

use crate::git::Git;
use crate::rawdiff::{self, Entry};
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommitKind {
    Feat,
    Fix,
    Refactor,
    Chore,
    Other,
}

impl CommitKind {
    pub fn parse(subject: &str) -> Self {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"^(?i)([a-z]+)(?:\([^)]*\))?!?:").unwrap());
        match re.captures(subject).map(|c| c[1].to_ascii_lowercase()) {
            Some(k) => match k.as_str() {
                "feat" | "feature" => CommitKind::Feat,
                "fix" | "bugfix" | "hotfix" => CommitKind::Fix,
                "refactor" => CommitKind::Refactor,
                "chore" => CommitKind::Chore,
                _ => CommitKind::Other,
            },
            None => CommitKind::Other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Commit {
    pub sha: String,
    pub author: String,
    pub author_time: i64,
    pub subject: String,
    pub kind: CommitKind,
    /// `This reverts commit <sha>` target, if any.
    pub reverts: Option<String>,
    /// `feat(api): ...` -> `api`.
    pub scope: Option<String>,
    /// `feat!:` or a `BREAKING CHANGE` footer.
    pub breaking: bool,
    /// `(#123)` or `Merge pull request #123`.
    pub pr: Option<u32>,
    /// `ABC-123` style ticket ids in the subject.
    pub tickets: Vec<String>,
    pub files: Vec<Entry>,
    /// Hash of the touched path set (both sides of renames), before any `--path` filtering.
    pub pathset: u64,
}

/// (scope, breaking, pr, tickets) from the subject and body.
pub fn parse_meta(subject: &str, body: &str) -> (Option<String>, bool, Option<u32>, Vec<String>) {
    static HEAD: OnceLock<Regex> = OnceLock::new();
    static PR: OnceLock<Regex> = OnceLock::new();
    static TICKET: OnceLock<Regex> = OnceLock::new();
    let head = HEAD.get_or_init(|| Regex::new(r"^(?i)[a-z]+(?:\(([^)]*)\))?(!)?:").unwrap());
    let pr = PR.get_or_init(|| Regex::new(r"\(#(\d+)\)|[Mm]erge pull request #(\d+)").unwrap());
    let ticket = TICKET.get_or_init(|| Regex::new(r"\b[A-Z][A-Z0-9]+-\d+\b").unwrap());
    let caps = head.captures(subject);
    let scope = caps
        .as_ref()
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty());
    let breaking = caps.as_ref().is_some_and(|c| c.get(2).is_some())
        || body
            .lines()
            .any(|l| l.starts_with("BREAKING CHANGE") || l.starts_with("BREAKING-CHANGE"));
    let pr_num = pr
        .captures(subject)
        .and_then(|c| c.get(1).or(c.get(2)))
        .and_then(|m| m.as_str().parse().ok());
    let mut tickets: Vec<String> = ticket
        .find_iter(subject)
        .map(|m| m.as_str().to_string())
        .collect();
    tickets.dedup();
    (scope, breaking, pr_num, tickets)
}

pub fn pathset_hash<'a>(paths: impl IntoIterator<Item = &'a str>) -> u64 {
    let mut v: Vec<&str> = paths.into_iter().collect();
    v.sort_unstable();
    v.dedup();
    // FNV-1a: stable across Rust versions, since the value is persisted in the cache.
    let mut h: u64 = 0xcbf29ce484222325;
    for p in v {
        for b in p.bytes().chain(std::iter::once(0)) {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
    }
    h
}

pub fn entry_paths(e: &Entry) -> impl Iterator<Item = &str> {
    std::iter::once(e.path.as_str()).chain(e.old_path.as_deref())
}

/// Head-only commits, oldest first. Merge commits are skipped: their changes appear in their parents.
pub fn load(git: &Git, base: &str, head: &str) -> Result<Vec<Commit>> {
    let range = format!("{base}..{head}");
    let meta = git.run(&[
        "log",
        "--no-merges",
        "--topo-order",
        "--reverse",
        "--format=%H%x1f%an%x1f%at%x1f%s%x1f%b%x1e",
        &range,
    ])?;
    let revert_re = Regex::new(r"This reverts commit ([0-9a-f]{40})").unwrap();
    let mut commits: Vec<Commit> = String::from_utf8_lossy(&meta)
        .split('\x1e')
        .filter_map(|rec| {
            let rec = rec.trim_start_matches('\n');
            let mut f = rec.splitn(5, '\x1f');
            let sha = f.next()?.trim();
            if sha.len() < 40 {
                return None;
            }
            let author = f.next()?.to_string();
            let author_time = f.next()?.parse().unwrap_or(0);
            let subject = f.next()?.to_string();
            let body = f.next().unwrap_or("");
            let (scope, breaking, pr, tickets) = parse_meta(&subject, body);
            Some(Commit {
                scope,
                breaking,
                pr,
                tickets,
                sha: sha.to_string(),
                author,
                author_time,
                kind: CommitKind::parse(&subject),
                reverts: revert_re.captures(body).map(|c| c[1].to_string()),
                subject,
                files: Vec::new(),
                pathset: 0,
            })
        })
        .collect();
    if commits.is_empty() {
        return Ok(commits);
    }

    let shas: Vec<String> = commits.iter().map(|c| c.sha.clone()).collect();
    let raw = git.run_stdin(
        &["diff-tree", "--stdin", "-r", "-M", "--raw", "-z", "--root"],
        Some(lines(&shas)),
    )?;
    let mut by_sha: HashMap<String, Vec<Entry>> =
        rawdiff::parse_stdin_raw(&raw).into_iter().collect();
    for c in &mut commits {
        c.files = by_sha.remove(&c.sha).unwrap_or_default();
        c.pathset = pathset_hash(c.files.iter().flat_map(entry_paths));
    }
    Ok(commits)
}

pub fn lines(items: &[String]) -> Vec<u8> {
    let mut v = items.join("\n").into_bytes();
    v.push(b'\n');
    v
}
