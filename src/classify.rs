//! Stage C: content identity. Blob ids are compared, never contents.

use crate::commits::Commit;
use crate::git::Git;
use crate::rawdiff::Entry;
use crate::report::{Classification, CommitReport, FileChange, FileStatus};
use anyhow::Result;
use std::collections::{HashMap, HashSet};

/// Blob id of each path at base's tip; `None` = absent.
pub struct Blobs {
    map: HashMap<String, Option<String>>,
}

impl Blobs {
    /// One `git cat-file --batch-check` process for every path.
    pub fn lookup(git: &Git, base: &str, paths: &HashSet<&str>) -> Result<Blobs> {
        // A path containing a newline cannot be sent to --batch-check; it is treated as "differs".
        let mut sorted: Vec<&str> = paths
            .iter()
            .copied()
            .filter(|p| !p.contains('\n'))
            .collect();
        sorted.sort_unstable();
        let mut input = String::new();
        for p in &sorted {
            input.push_str(base);
            input.push(':');
            input.push_str(p);
            input.push('\n');
        }
        let mut map = HashMap::with_capacity(sorted.len());
        if !sorted.is_empty() {
            let out = git.run_stdin(
                &["cat-file", "--batch-check=%(objectname) %(objecttype)"],
                Some(input.into_bytes()),
            )?;
            let text = String::from_utf8_lossy(&out);
            for (p, line) in sorted.iter().zip(text.lines()) {
                let sha = if line.ends_with(" missing") || line.ends_with(" ambiguous") {
                    None
                } else {
                    line.split(' ').next().map(str::to_string)
                };
                map.insert((*p).to_string(), sha);
            }
        }
        Ok(Blobs { map })
    }

    /// `Some(None)` = known absent, `Some(Some(sha))` = present, `None` = unknown.
    pub fn get(&self, path: &str) -> Option<Option<&str>> {
        self.map.get(path).map(|o| o.as_deref())
    }
}

pub fn file_status(e: &Entry, blobs: &Blobs) -> FileStatus {
    let new_here = blobs.get(&e.path);
    let old_gone = match &e.old_path {
        // A rename only counts as present on base if the old path is gone too.
        Some(old) => matches!(blobs.get(old), Some(None)),
        None => true,
    };
    match (e.is_delete(), new_here) {
        (_, None) => FileStatus::Different,
        (true, Some(None)) => FileStatus::Identical,
        (true, Some(Some(_))) => FileStatus::Different,
        (false, Some(None)) => FileStatus::Absent,
        (false, Some(Some(sha))) if sha == e.new_sha && old_gone => FileStatus::Identical,
        (false, Some(Some(_))) => FileStatus::Different,
    }
}

pub fn file_change(e: &Entry, blobs: &Blobs) -> FileChange {
    FileChange {
        path: e.path.clone(),
        old_path: e.old_path.clone(),
        change: (e.status as char).to_string(),
        status: file_status(e, blobs),
        submodule: e.is_submodule(),
        binary: false,
        hunks_total: 0,
        hunks_already_on_base: 0,
        hunk_details: Vec::new(),
    }
}

pub struct Settings {
    /// Treat every PARTIAL as NEW (no hunk analysis).
    pub strict: bool,
}

pub fn classify(
    commits: &[Commit],
    blobs: &Blobs,
    equivalent: &HashMap<String, String>,
    settings: &Settings,
) -> Vec<CommitReport> {
    commits
        .iter()
        .map(|c| {
            let files: Vec<FileChange> = c.files.iter().map(|e| file_change(e, blobs)).collect();
            let identical = files
                .iter()
                .filter(|f| f.status == FileStatus::Identical)
                .count();
            let matched = equivalent.get(&c.sha).cloned();
            let class = if matched.is_some() {
                Classification::PatchEquivalent
            } else if files.is_empty() || identical == 0 {
                Classification::New
            } else if identical == files.len() {
                Classification::Shipped
            } else if settings.strict {
                Classification::New
            } else {
                Classification::Partial
            };
            CommitReport {
                sha: c.sha.clone(),
                short: c.sha.chars().take(7).collect(),
                subject: c.subject.clone(),
                author: c.author.clone(),
                kind: if files.is_empty() {
                    crate::commits::CommitKind::Other
                } else {
                    c.kind
                },
                class,
                matched_base_commit: matched,
                reverts: c.reverts.clone(),
                files,
                hunks_total: 0,
                hunks_already_on_base: 0,
            }
        })
        .collect()
}

/// Every path a commit touches, for the blob lookup.
pub fn touched_paths(commits: &[Commit]) -> impl Iterator<Item = &str> {
    commits
        .iter()
        .flat_map(|c| c.files.iter().flat_map(crate::commits::entry_paths))
}
