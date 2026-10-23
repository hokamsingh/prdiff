//! Stage E: the net diff of `base...head`, i.e. merge-base to head.

use crate::classify::Blobs;
use crate::git::Git;
use crate::glob::PathFilter;
use crate::rawdiff::{self, Entry, Stats};
use crate::report::{Classification, CommitReport, NetFile, NetState};
use anyhow::Result;
use std::collections::HashMap;

pub struct NetRaw {
    pub entries: Vec<Entry>,
    pub stats: Stats,
}

pub fn diff_raw(git: &Git, merge_base: &str, head: &str) -> Result<NetRaw> {
    let out = git.run(&[
        "diff-tree",
        "-r",
        "-M",
        "--raw",
        "--numstat",
        "-z",
        merge_base,
        head,
    ])?;
    let (entries, stats) = rawdiff::parse_raw_numstat(&out);
    Ok(NetRaw { entries, stats })
}

/// A file is ALREADY_ON_BASE when base's blob equals head's, or when every head-only commit touching it
/// is itself SHIPPED / PATCH_EQUIVALENT. Otherwise it is NEW_TO_BASE.
pub fn build(
    raw: &NetRaw,
    blobs: &Blobs,
    filter: &PathFilter,
    commits: &[CommitReport],
) -> Vec<NetFile> {
    // path -> true while every commit touching it is already on base
    let mut explained: HashMap<&str, bool> = HashMap::new();
    for c in commits {
        let on_base = matches!(
            c.class,
            Classification::Shipped | Classification::PatchEquivalent
        );
        for f in &c.files {
            for p in std::iter::once(f.path.as_str()).chain(f.old_path.as_deref()) {
                let e = explained.entry(p).or_insert(true);
                *e &= on_base;
            }
        }
    }
    raw.entries
        .iter()
        .filter(|e| {
            filter.matches(&e.path) || e.old_path.as_deref().is_some_and(|o| filter.matches(o))
        })
        .map(|e| {
            let same_blob = match blobs.get(&e.path) {
                Some(None) => e.is_delete(),
                Some(Some(sha)) => !e.is_delete() && sha == e.new_sha,
                None => false,
            };
            let all_explained = explained.get(e.path.as_str()).copied().unwrap_or(false)
                && e.old_path
                    .as_deref()
                    .is_none_or(|o| explained.get(o).copied().unwrap_or(false));
            let (ins, del) = raw.stats.get(&e.path).copied().unwrap_or((None, None));
            NetFile {
                path: e.path.clone(),
                old_path: e.old_path.clone(),
                change: (e.status as char).to_string(),
                state: if same_blob || all_explained {
                    NetState::AlreadyOnBase
                } else {
                    NetState::NewToBase
                },
                insertions: ins,
                deletions: del,
                binary: ins.is_none() && del.is_none() && !e.is_submodule(),
                submodule: e.is_submodule(),
            }
        })
        .collect()
}
