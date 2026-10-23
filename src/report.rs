use crate::commits::CommitKind;
use crate::refs::RefInfo;
use serde::Serialize;
use std::collections::BTreeMap;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Classification {
    New,
    Partial,
    Shipped,
    PatchEquivalent,
}

impl Classification {
    /// Listed in a PR description: still has something to ship.
    pub fn is_listed(self) -> bool {
        matches!(self, Classification::New | Classification::Partial)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileStatus {
    /// Same blob on base as after the commit.
    Identical,
    /// Base has the file with other content.
    Different,
    /// Base does not have the file.
    Absent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NetState {
    NewToBase,
    AlreadyOnBase,
}

/// One displayed hunk (only populated for `prdiff view`).
#[derive(Debug, Clone, Serialize)]
pub struct HunkDetail {
    pub header: String,
    pub on_base: bool,
    /// Lines with their ` `/`+`/`-` prefix.
    pub lines: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    /// A, M, D, R or T.
    pub change: String,
    pub status: FileStatus,
    pub submodule: bool,
    pub binary: bool,
    pub hunks_total: u32,
    pub hunks_already_on_base: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hunk_details: Vec<HunkDetail>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CommitReport {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub author: String,
    pub kind: CommitKind,
    pub class: Classification,
    /// For PATCH_EQUIVALENT: the base commit carrying the same patch.
    pub matched_base_commit: Option<String>,
    pub reverts: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub breaking: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pr: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tickets: Vec<String>,
    pub files: Vec<FileChange>,
    pub hunks_total: u32,
    pub hunks_already_on_base: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct NetFile {
    pub path: String,
    pub old_path: Option<String>,
    pub change: String,
    pub state: NetState,
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    pub submodule: bool,
    /// Why this file deserves a second look: migration, lockfile, large-diff, hot-file, ...
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub risk: Vec<String>,
    /// CODEOWNERS owners for this path.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub owners: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OwnerSummary {
    pub owner: String,
    /// NEW_TO_BASE files this owner has to review.
    pub files: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct BaseCommit {
    pub sha: String,
    pub subject: String,
    pub author: String,
}

/// Who touched a conflicted file on each side.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictDetail {
    pub path: String,
    /// Head-only commits touching it.
    pub head_commits: Vec<String>,
    /// Newest commits on base since the merge-base touching it.
    pub base_commits: Vec<BaseCommit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RevertPair {
    pub revert: String,
    pub reverted: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Totals {
    /// Non-empty head-only commits (after `--path`).
    pub commits: usize,
    pub by_class: BTreeMap<Classification, usize>,
    /// New + Partial commits, i.e. what a description lists.
    pub listed: usize,
    pub listed_by_kind: BTreeMap<CommitKind, usize>,
    pub empty_skipped: usize,
    pub files_new_to_base: usize,
    pub files_already_on_base: usize,
    pub insertions: u64,
    pub deletions: u64,
    pub conflicted_files: usize,
    /// NEW_TO_BASE files with at least one risk flag.
    pub risky_files: usize,
}

impl Totals {
    /// The only place counts are computed: everything is derived from the lists.
    pub fn derive(
        commits: &[CommitReport],
        net: &[NetFile],
        conflicts: &[String],
        empty_skipped: usize,
    ) -> Totals {
        let mut t = Totals {
            commits: commits.len(),
            empty_skipped,
            conflicted_files: conflicts.len(),
            ..Default::default()
        };
        for class in [
            Classification::New,
            Classification::Partial,
            Classification::Shipped,
            Classification::PatchEquivalent,
        ] {
            t.by_class.insert(class, 0);
        }
        for kind in [
            CommitKind::Feat,
            CommitKind::Fix,
            CommitKind::Refactor,
            CommitKind::Chore,
            CommitKind::Other,
        ] {
            t.listed_by_kind.insert(kind, 0);
        }
        for c in commits {
            *t.by_class.entry(c.class).or_default() += 1;
            if c.class.is_listed() {
                t.listed += 1;
                *t.listed_by_kind.entry(c.kind).or_default() += 1;
            }
        }
        for f in net {
            match f.state {
                NetState::NewToBase => {
                    t.files_new_to_base += 1;
                    t.risky_files += !f.risk.is_empty() as usize;
                    t.insertions += f.insertions.unwrap_or(0);
                    t.deletions += f.deletions.unwrap_or(0);
                }
                NetState::AlreadyOnBase => t.files_already_on_base += 1,
            }
        }
        t
    }

    pub fn count(&self, c: Classification) -> usize {
        self.by_class.get(&c).copied().unwrap_or(0)
    }

    pub fn kind(&self, k: CommitKind) -> usize {
        self.listed_by_kind.get(&k).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub base: RefInfo,
    pub head: RefInfo,
    pub merge_base: String,
    pub commits: Vec<CommitReport>,
    pub net_files: Vec<NetFile>,
    pub revert_pairs: Vec<RevertPair>,
    pub conflicts: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflict_details: Vec<ConflictDetail>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub owners_summary: Vec<OwnerSummary>,
    /// Where each commit stands per environment (only with `--env` / `[[environments]]`, for `view`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub train: Option<crate::train::Matrix>,
    /// Both versions of each changed file (only populated for `prdiff view`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub file_sides: Vec<crate::sides::FileSides>,
    /// True when conflicts came from the pre-2.38 `git merge-tree` text parser.
    pub conflicts_best_effort: bool,
    pub totals: Totals,
}
