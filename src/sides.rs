//! Both versions of each changed file, for the side-by-side viewer in `prdiff view`.
//!
//! Left = the file as it is on base's tip right now; right = the file as head has it. The line diff between
//! them is computed here (Patience) and shipped as compact ops, so the page never has to diff large files.

use crate::classify::Blobs;
use crate::git::Git;
use crate::rawdiff::{Entry, is_zero};
use crate::report::{NetFile, NetState};
use anyhow::Result;
use serde::Serialize;
use similar::{Algorithm, DiffOp, capture_diff_slices};
use std::collections::{HashMap, HashSet};

/// Per-side size cap: bigger files get a note and a `git diff` hint instead of an embedded copy.
pub const MAX_SIDE_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct FileSides {
    pub path: String,
    /// Where base has the file when head renamed it.
    pub old_path: Option<String>,
    /// `None` = the file does not exist on that side.
    pub base_text: Option<String>,
    pub head_text: Option<String>,
    /// `[tag, old_index, old_len, new_index, new_len]`; tag 0 equal, 1 delete, 2 insert, 3 replace.
    pub ops: Vec<[u32; 5]>,
    /// Line ranges `[start, len)` (0-based) of `base_text` that changed on the target since the branch point.
    pub target_changed: Vec<[u32; 2]>,
    /// Line ranges of `head_text` that this PR changed relative to the branch point.
    pub pr_changed: Vec<[u32; 2]>,
    /// False when the merge-base copy was unavailable, so there are no target-only/PR markers.
    pub three_way: bool,
    /// Why there is no viewer ("binary file", "too large", ...).
    pub note: Option<String>,
}

fn lines(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.strip_suffix('\n')
            .unwrap_or(text)
            .split('\n')
            .collect()
    }
}

fn ops(old: &str, new: &str) -> Vec<[u32; 5]> {
    let (a, b) = (lines(old), lines(new));
    capture_diff_slices(Algorithm::Patience, &a, &b)
        .into_iter()
        .map(|op| match op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => [
                0,
                old_index as u32,
                len as u32,
                new_index as u32,
                len as u32,
            ],
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => [1, old_index as u32, old_len as u32, new_index as u32, 0],
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => [2, old_index as u32, 0, new_index as u32, new_len as u32],
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => [
                3,
                old_index as u32,
                old_len as u32,
                new_index as u32,
                new_len as u32,
            ],
        })
        .collect()
}

/// Ranges of `new` that are not shared with `old` (inserted or replaced lines).
fn changed_ranges(old: &str, new: &str) -> Vec<[u32; 2]> {
    let (a, b) = (lines(old), lines(new));
    capture_diff_slices(Algorithm::Patience, &a, &b)
        .into_iter()
        .filter_map(|op| match op {
            DiffOp::Insert {
                new_index, new_len, ..
            }
            | DiffOp::Replace {
                new_index, new_len, ..
            } => Some([new_index as u32, new_len as u32]),
            _ => None,
        })
        .collect()
}

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// `(sha, size)` for each object, via one `cat-file --batch-check`.
fn sizes(git: &Git, shas: &HashSet<String>) -> Result<HashMap<String, usize>> {
    let mut input = String::new();
    for s in shas {
        input.push_str(s);
        input.push('\n');
    }
    let out = git.run_stdin(
        &["cat-file", "--batch-check=%(objectname) %(objectsize)"],
        Some(input.into_bytes()),
    )?;
    Ok(String::from_utf8_lossy(&out)
        .lines()
        .filter_map(|l| {
            let mut f = l.split(' ');
            Some((f.next()?.to_string(), f.next()?.parse().ok()?))
        })
        .collect())
}

/// Sides for every NEW_TO_BASE file, stopping once `budget` bytes have been embedded.
pub fn collect(
    git: &Git,
    entries: &[Entry],
    net: &[NetFile],
    blobs: &Blobs,
    mut budget: usize,
) -> Result<Vec<FileSides>> {
    let wanted: HashMap<&str, &NetFile> = net
        .iter()
        .filter(|f| f.state == NetState::NewToBase && !f.submodule)
        .map(|f| (f.path.as_str(), f))
        .collect();

    struct Job<'a> {
        entry: &'a Entry,
        base_sha: Option<String>,
        head_sha: Option<String>,
        /// The file at the merge-base, to tell target-only changes from this PR's.
        mb_sha: Option<String>,
    }
    let mut jobs: Vec<Job> = Vec::new();
    for e in entries {
        let Some(nf) = wanted.get(e.path.as_str()) else {
            continue;
        };
        if nf.binary {
            continue;
        }
        // For a rename, "the file on base" is the old path.
        let base_path = e.old_path.as_deref().unwrap_or(&e.path);
        let base_sha = match blobs.get(base_path) {
            Some(Some(sha)) => Some(sha.to_string()),
            _ => None,
        };
        let head_sha = (!e.is_delete()).then(|| e.new_sha.clone());
        let mb_sha = (!is_zero(&e.old_sha)).then(|| e.old_sha.clone());
        jobs.push(Job {
            entry: e,
            base_sha,
            head_sha,
            mb_sha,
        });
    }

    let mut all: HashSet<String> = HashSet::new();
    for j in &jobs {
        all.extend(j.base_sha.iter().cloned());
        all.extend(j.head_sha.iter().cloned());
        all.extend(j.mb_sha.iter().cloned());
    }
    if all.is_empty() {
        return Ok(Vec::new());
    }
    let size = sizes(git, &all)?;
    let small: HashSet<String> = all
        .iter()
        .filter(|s| size.get(*s).is_some_and(|n| *n <= MAX_SIDE_BYTES))
        .cloned()
        .collect();
    let contents = crate::hunks::read_blobs(git, &small)?;

    let mut out = Vec::new();
    for j in jobs {
        let e = j.entry;
        let mut fs = FileSides {
            path: e.path.clone(),
            old_path: e.old_path.clone(),
            base_text: None,
            head_text: None,
            ops: Vec::new(),
            target_changed: Vec::new(),
            pr_changed: Vec::new(),
            three_way: false,
            note: None,
        };
        let get = |sha: &Option<String>| -> Result<Option<Vec<u8>>, &'static str> {
            match sha {
                None => Ok(None),
                Some(s) => match contents.get(s) {
                    Some(d) if is_binary(d) => Err("binary file"),
                    Some(d) => Ok(Some(d.clone())),
                    None => Err("file too large to embed"),
                },
            }
        };
        match (get(&j.base_sha), get(&j.head_sha)) {
            (Ok(b), Ok(h)) => {
                let (b, h) = (
                    b.map(|d| String::from_utf8_lossy(&d).into_owned()),
                    h.map(|d| String::from_utf8_lossy(&d).into_owned()),
                );
                let cost = b.as_ref().map_or(0, String::len) + h.as_ref().map_or(0, String::len);
                if cost > budget {
                    fs.note =
                        Some("report size limit reached; use `git diff` for this file".into());
                } else {
                    budget -= cost;
                    fs.ops = ops(b.as_deref().unwrap_or(""), h.as_deref().unwrap_or(""));
                    // Third copy: the branch point, only used to classify differences.
                    let mb = match &j.mb_sha {
                        None => Some(String::new()), // added by the PR: nothing at the branch point
                        Some(sha) => contents
                            .get(sha)
                            .filter(|d| !is_binary(d))
                            .map(|d| String::from_utf8_lossy(d).into_owned()),
                    };
                    if let Some(mb) = mb {
                        fs.target_changed = changed_ranges(&mb, b.as_deref().unwrap_or(""));
                        fs.pr_changed = changed_ranges(&mb, h.as_deref().unwrap_or(""));
                        fs.three_way = true;
                    }
                    fs.base_text = b;
                    fs.head_text = h;
                }
            }
            (Err(n), _) | (_, Err(n)) => fs.note = Some(n.into()),
        }
        out.push(fs);
    }
    Ok(out)
}
