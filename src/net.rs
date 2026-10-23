//! Stage E: the net diff of `base...head`, i.e. merge-base to head.

use crate::classify::Blobs;
use crate::codeowners::Owners;
use crate::git::Git;
use crate::glob::PathFilter;
use crate::rawdiff::{self, Entry, Stats};
use crate::report::{Classification, CommitReport, NetFile, NetState};
use crate::risk::RiskRules;
use anyhow::Result;
use std::collections::{HashMap, HashSet};

pub struct NetSettings<'a> {
    pub ignore_ws: bool,
    pub risk: &'a RiskRules,
    pub owners: Option<&'a Owners>,
}

pub struct NetRaw {
    pub entries: Vec<Entry>,
    pub stats: Stats,
}

pub fn diff_raw(git: &Git, merge_base: &str, head: &str, ignore_ws: bool) -> Result<NetRaw> {
    let mut args = vec!["diff-tree", "-r", "-M", "--raw", "--numstat", "-z"];
    if ignore_ws {
        args.push("-w");
    }
    args.extend([merge_base, head]);
    let out = git.run(&args)?;
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
    settings: &NetSettings,
) -> Vec<NetFile> {
    let ignore_ws = settings.ignore_ws;
    // path -> (commits touching it, distinct authors)
    let mut touch: HashMap<&str, (usize, HashSet<&str>)> = HashMap::new();
    for c in commits {
        for f in &c.files {
            let t = touch.entry(f.path.as_str()).or_default();
            t.0 += 1;
            t.1.insert(c.author.as_str());
        }
    }
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
            filter.matches_net(&e.path)
                || e.old_path.as_deref().is_some_and(|o| filter.matches_net(o))
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
            // with --ignore-whitespace a file whose PR changes are whitespace only has nothing left to ship
            let ws_only = ignore_ws && e.status == b'M' && ins == Some(0) && del == Some(0);
            NetFile {
                path: e.path.clone(),
                old_path: e.old_path.clone(),
                change: (e.status as char).to_string(),
                state: if same_blob || all_explained || ws_only {
                    NetState::AlreadyOnBase
                } else {
                    NetState::NewToBase
                },
                insertions: ins,
                deletions: del,
                binary: ins.is_none() && del.is_none() && !e.is_submodule(),
                submodule: e.is_submodule(),
                risk: {
                    let (n, authors) = touch
                        .get(e.path.as_str())
                        .map_or((0, 0), |t| (t.0, t.1.len()));
                    let lines = ins.zip(del).map(|(a, d)| a + d);
                    settings.risk.flags(&e.path, lines, n, authors)
                },
                owners: settings.owners.map(|o| o.of(&e.path)).unwrap_or_default(),
            }
        })
        .collect()
}
