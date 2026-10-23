//! Stage B: patch equivalence (cherry-picks and backports).
//!
//! Instead of hashing every commit in base's history, a base commit is only patch-id'd when its touched
//! path set equals the path set of some head-only commit: identical patches always touch identical paths.
//! Both the path set and the patch-id are cached per commit SHA.

use crate::cache::{Cache, Cached};
use crate::commits::{Commit, lines, pathset_hash};
use crate::git::Git;
use crate::rawdiff;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Base commits newer than (oldest head-only author date - 2 days). Catches backports that were merged back.
    Auto,
    /// All of base's history.
    All,
    /// Only commits reachable from base but not from head.
    BaseOnly,
}

const SLACK_SECS: i64 = 2 * 24 * 3600;

/// Map of head-only commit SHA -> the base commit carrying the same patch.
pub fn equivalents(
    git: &Git,
    commits: &[Commit],
    base: &str,
    head: &str,
    scope: Scope,
    cache: &mut Cache,
) -> Result<HashMap<String, String>> {
    if commits.is_empty() {
        return Ok(HashMap::new());
    }

    // 1. base commits in scope
    let mut args: Vec<String> = vec!["rev-list".into(), "--no-merges".into(), base.into()];
    match scope {
        Scope::All => {}
        Scope::BaseOnly => args.push(format!("^{head}")),
        Scope::Auto => {
            let oldest = commits.iter().map(|c| c.author_time).min().unwrap_or(0);
            args.push(format!("--since={}", (oldest - SLACK_SECS).max(0)));
        }
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let base_commits: Vec<String> = String::from_utf8_lossy(&git.run(&arg_refs)?)
        .lines()
        .map(str::to_string)
        .collect();

    // 2. path sets of base commits we have not seen before (one diff-tree process)
    let missing: Vec<String> = base_commits
        .iter()
        .filter(|c| !cache.map.contains_key(*c))
        .cloned()
        .collect();
    let mut touched: Vec<String> = Vec::new();
    if !missing.is_empty() {
        let raw = git.run_stdin(
            &[
                "diff-tree",
                "--stdin",
                "-r",
                "--no-renames",
                "--raw",
                "-z",
                "--root",
            ],
            Some(lines(&missing)),
        )?;
        let mut seen: HashMap<String, u64> = rawdiff::parse_stdin_raw(&raw)
            .into_iter()
            .map(|(sha, es)| {
                let h = pathset_hash(es.iter().map(|e| e.path.as_str()));
                (sha, h)
            })
            .collect();
        for c in &missing {
            // commits absent from the output have an empty diff
            let pathset = seen
                .remove(c)
                .unwrap_or_else(|| pathset_hash(std::iter::empty()));
            cache.insert(
                c.clone(),
                Cached {
                    pathset,
                    patch_id: None,
                },
            );
            touched.push(c.clone());
        }
    }

    // 3. which patch-ids do we need?
    let head_sets: HashSet<u64> = commits.iter().map(|c| c.pathset).collect();
    let empty_set = pathset_hash(std::iter::empty());
    let mut need: Vec<String> = Vec::new();
    for c in commits {
        let known = cache.map.entry(c.sha.clone()).or_insert(Cached {
            pathset: c.pathset,
            patch_id: None,
        });
        if !c.files.is_empty() && known.patch_id.is_none() {
            need.push(c.sha.clone());
        }
    }
    for b in &base_commits {
        let Some(v) = cache.map.get(b) else { continue };
        if v.patch_id.is_none() && v.pathset != empty_set && head_sets.contains(&v.pathset) {
            need.push(b.clone());
        }
    }
    need.sort();
    need.dedup();

    // 4. patch-ids, in parallel chunks (each chunk is one diff-tree | patch-id pipeline)
    if !need.is_empty() {
        let threads = rayon::current_num_threads().max(1);
        let chunk = need.len().div_ceil(threads).clamp(16, 2048);
        let results: Vec<Result<Vec<u8>>> =
            need.par_chunks(chunk).map(|c| git.patch_ids(c)).collect();
        let mut found: HashMap<String, String> = HashMap::new();
        for r in results {
            for line in String::from_utf8_lossy(&r?).lines() {
                let mut f = line.split_whitespace();
                if let (Some(id), Some(commit)) = (f.next(), f.next()) {
                    found.insert(commit.to_string(), id.to_string());
                }
            }
        }
        for c in &need {
            // no output means an empty diff
            let id = found.remove(c).unwrap_or_default();
            let entry = cache.map.entry(c.clone()).or_default();
            entry.patch_id = Some(id);
            touched.push(c.clone());
        }
    }
    touched.sort();
    touched.dedup();
    cache.append(&touched);

    // 5. match
    let mut by_id: HashMap<&str, &str> = HashMap::new();
    for b in &base_commits {
        if let Some(Cached {
            patch_id: Some(id), ..
        }) = cache.map.get(b)
            && !id.is_empty()
        {
            by_id.entry(id.as_str()).or_insert(b.as_str());
        }
    }
    let mut out = HashMap::new();
    for c in commits {
        if let Some(Cached {
            patch_id: Some(id), ..
        }) = cache.map.get(&c.sha)
            && let Some(b) = by_id.get(id.as_str())
        {
            out.insert(c.sha.clone(), (*b).to_string());
        }
    }
    Ok(out)
}
