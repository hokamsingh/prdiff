//! Stage D: hunk-level check for commits that are only partly on base.
//!
//! A hunk is `ALREADY_ON_BASE` when base's file already contains the hunk's after-image (context + added
//! lines, contiguous) and no longer contains its before-image (context + removed lines). That is the same
//! question `git apply -R --check` answers, asked in memory against the base blob so no index is needed.

use crate::classify::Blobs;
use crate::commits::Commit;
use crate::git::Git;
use crate::report::{Classification, CommitReport, FileStatus, HunkDetail};
use anyhow::Result;
use rayon::prelude::*;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};

/// A line as compared: verbatim, or with all ASCII whitespace removed under `--ignore-whitespace`.
type Line<'a> = Cow<'a, [u8]>;

fn line(l: &[u8], ignore_ws: bool) -> Line<'_> {
    if ignore_ws {
        Cow::Owned(
            l.iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect(),
        )
    } else {
        Cow::Borrowed(l)
    }
}

struct Hunk<'a> {
    /// The `@@ -a,b +c,d @@ ...` line.
    header: &'a [u8],
    /// Every line of the hunk with its ` `/`+`/`-` prefix, for display.
    raw: Vec<&'a [u8]>,
    /// Context + removed lines.
    before: Vec<Line<'a>>,
    /// Context + added lines.
    after: Vec<Line<'a>>,
    removed: Vec<Line<'a>>,
    added: Vec<Line<'a>>,
}

struct FileDiff<'a> {
    binary: bool,
    hunks: Vec<Hunk<'a>>,
}

/// Split `git diff-tree -p` output into per-file blocks, in the same order as the raw listing.
fn parse_diff(diff: &[u8], ws: bool) -> Vec<FileDiff<'_>> {
    let mut files: Vec<FileDiff> = Vec::new();
    for line in diff.split(|b| *b == b'\n') {
        if line.starts_with(b"diff --git ") {
            files.push(FileDiff {
                binary: false,
                hunks: Vec::new(),
            });
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if line.starts_with(b"@@ ") {
            file.hunks.push(Hunk {
                header: line,
                raw: Vec::new(),
                before: Vec::new(),
                after: Vec::new(),
                removed: Vec::new(),
                added: Vec::new(),
            });
        } else if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
            file.binary = true;
        } else if let Some(h) = file.hunks.last_mut() {
            if matches!(line.first(), Some(b' ' | b'-' | b'+' | b'\\')) {
                h.raw.push(line);
            }
            match line.first() {
                Some(b' ') => {
                    h.before.push(self::line(&line[1..], ws));
                    h.after.push(self::line(&line[1..], ws));
                }
                Some(b'-') => {
                    h.before.push(self::line(&line[1..], ws));
                    h.removed.push(self::line(&line[1..], ws));
                }
                Some(b'+') => {
                    h.after.push(self::line(&line[1..], ws));
                    h.added.push(self::line(&line[1..], ws));
                }
                _ => {}
            }
        }
    }
    files
}

fn contains(haystack: &[Line], needle: &[Line]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }
    let first = &needle[0];
    (0..=haystack.len() - needle.len())
        .any(|i| haystack[i] == *first && haystack[i..i + needle.len()] == *needle)
}

fn hunk_on_base(h: &Hunk, base: &[Line], ignore_ws: bool) -> bool {
    // A hunk that only changes whitespace is no change at all when whitespace is ignored.
    if ignore_ws && h.before == h.after {
        return true;
    }
    let applied = contains(base, &h.after);
    let reverted = h.removed.is_empty() || !contains(base, &h.before);
    if applied && reverted {
        return true;
    }
    // The surrounding context may have drifted since (a later edit next to the hunk). A replacement hunk is
    // still recognisable by its own lines: the new ones are present and the old ones are gone. Pure additions
    // and pure deletions are too ambiguous to recognise without their context, so they are not matched.
    !h.removed.is_empty()
        && h.added
            .iter()
            .any(|l| !l.is_empty() && !l.iter().all(u8::is_ascii_whitespace))
        && contains(base, &h.added)
        && !contains(base, &h.removed)
}

/// Split `diff-tree --stdin` output into one slice per commit: each starts with a line holding just its SHA.
fn split_by_commit<'a>(buf: &'a [u8], shas: impl Iterator<Item = &'a str>) -> Vec<&'a [u8]> {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut current: Option<usize> = None; // start of the current commit's diff
    for sha in shas {
        // find "\n<sha>\n" (or at the very start) at or after pos
        let needle = sha.as_bytes();
        let mut found = None;
        let mut at = pos;
        while at < buf.len() {
            let end = buf[at..]
                .iter()
                .position(|b| *b == b'\n')
                .map_or(buf.len(), |n| at + n);
            if &buf[at..end] == needle {
                found = Some((at, end));
                break;
            }
            at = end + 1;
        }
        match found {
            Some((line_start, line_end)) => {
                if let Some(start) = current.take() {
                    out.push(&buf[start..line_start]);
                }
                current = Some((line_end + 1).min(buf.len()));
                pos = line_end + 1;
            }
            None => {
                if let Some(start) = current.take() {
                    out.push(&buf[start..pos.min(buf.len())]);
                }
                out.push(&buf[0..0]);
            }
        }
    }
    if let Some(start) = current {
        out.push(&buf[start.min(buf.len())..]);
    }
    out
}

/// `git cat-file --batch` for the given blob ids.
pub(crate) fn read_blobs(git: &Git, shas: &HashSet<String>) -> Result<HashMap<String, Vec<u8>>> {
    let mut out = HashMap::new();
    if shas.is_empty() {
        return Ok(out);
    }
    let mut input = String::new();
    for s in shas {
        input.push_str(s);
        input.push('\n');
    }
    let buf = git.run_stdin(&["cat-file", "--batch"], Some(input.into_bytes()))?;
    let mut pos = 0;
    while pos < buf.len() {
        let Some(nl) = buf[pos..].iter().position(|b| *b == b'\n') else {
            break;
        };
        let header = String::from_utf8_lossy(&buf[pos..pos + nl]).into_owned();
        pos += nl + 1;
        let mut f = header.split(' ');
        let (Some(sha), Some(kind)) = (f.next(), f.next()) else {
            break;
        };
        if kind == "missing" {
            continue;
        }
        let size: usize = f.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let end = (pos + size).min(buf.len());
        out.insert(sha.to_string(), buf[pos..end].to_vec());
        pos = end + 1;
    }
    Ok(out)
}

fn hunk_details(hunks: &[Hunk], on_base: &[bool], max_lines: usize) -> Vec<HunkDetail> {
    let mut budget = max_lines;
    hunks
        .iter()
        .zip(on_base)
        .map(|(h, &on_base)| {
            let take = h.raw.len().min(budget);
            budget -= take;
            HunkDetail {
                header: String::from_utf8_lossy(h.header).into_owned(),
                on_base,
                lines: h.raw[..take]
                    .iter()
                    .map(|l| String::from_utf8_lossy(l).into_owned())
                    .collect(),
                truncated: take < h.raw.len(),
            }
        })
        .collect()
}

#[derive(Clone, Copy)]
pub struct HunkSettings {
    /// A NEW commit with some hunks on base becomes PARTIAL.
    pub promote_partial: bool,
    /// Keep displayable hunks (at most this many lines per file).
    pub detail: Option<usize>,
    pub ignore_ws: bool,
}

/// Fill in hunk counts for `targets` (indexes into `commits`/`reports`) and promote a commit to SHIPPED
/// when every hunk of every non-identical file is already on base.
pub fn analyze(
    git: &Git,
    commits: &[Commit],
    reports: &mut [CommitReport],
    targets: &[usize],
    blobs: &Blobs,
    settings: &HunkSettings,
) -> Result<()> {
    let HunkSettings {
        promote_partial,
        detail,
        ignore_ws,
    } = *settings;
    if targets.is_empty() {
        return Ok(());
    }
    // One `diff-tree --stdin` per chunk of commits instead of one process per commit.
    let chunk = targets
        .len()
        .div_ceil(rayon::current_num_threads().max(1))
        .max(8);
    let chunks: Vec<Result<Vec<u8>>> = targets
        .par_chunks(chunk)
        .map(|ts| {
            let shas: Vec<String> = ts.iter().map(|&i| commits[i].sha.clone()).collect();
            git.run_stdin(
                &[
                    "diff-tree",
                    "--stdin",
                    "-p",
                    "-r",
                    "-M",
                    "--root",
                    "-U3",
                    "--no-ext-diff",
                    "--no-textconv",
                ],
                Some(crate::commits::lines(&shas)),
            )
        })
        .collect();
    let chunks: Vec<Vec<u8>> = chunks.into_iter().collect::<Result<_>>()?;
    let mut diffs: Vec<&[u8]> = Vec::with_capacity(targets.len());
    for (buf, ts) in chunks.iter().zip(targets.chunks(chunk)) {
        diffs.extend(split_by_commit(
            buf,
            ts.iter().map(|&i| commits[i].sha.as_str()),
        ));
    }

    let mut needed: HashSet<String> = HashSet::new();
    for &i in targets {
        for (e, f) in commits[i].files.iter().zip(&reports[i].files) {
            if f.status == FileStatus::Different
                && let Some(Some(sha)) = blobs.get(&e.path)
            {
                needed.insert(sha.to_string());
            }
        }
    }
    let contents = read_blobs(git, &needed)?;
    let lines: HashMap<&str, Vec<Line>> = contents
        .iter()
        .map(|(sha, data)| {
            (
                sha.as_str(),
                data.split(|b| *b == b'\n')
                    .map(|l| line(l, ignore_ws))
                    .collect(),
            )
        })
        .collect();

    for (t, &i) in targets.iter().enumerate() {
        let parsed = parse_diff(diffs[t], ignore_ws);
        let mut explained = true;
        let mut any_hunk_file = false;
        for (k, (e, f)) in commits[i]
            .files
            .iter()
            .zip(reports[i].files.iter_mut())
            .enumerate()
        {
            let Some(fd) = parsed.get(k) else {
                if f.status != FileStatus::Identical {
                    explained = false;
                }
                continue;
            };
            f.binary = fd.binary;
            f.hunks_total = fd.hunks.len() as u32;
            let base_lines = match blobs.get(&e.path) {
                Some(Some(sha)) if f.status == FileStatus::Different && !e.is_delete() => {
                    lines.get(sha)
                }
                _ => None,
            };
            let on_base: Vec<bool> = if f.status == FileStatus::Identical {
                // The whole file matches base, so every hunk is on base.
                vec![true; fd.hunks.len()]
            } else if let Some(base) = base_lines {
                fd.hunks
                    .iter()
                    .map(|h| hunk_on_base(h, base, ignore_ws))
                    .collect()
            } else {
                vec![false; fd.hunks.len()]
            };
            f.hunks_already_on_base = on_base.iter().filter(|b| **b).count() as u32;
            if let Some(max_lines) = detail {
                f.hunk_details = hunk_details(&fd.hunks, &on_base, max_lines);
            }
            if f.status == FileStatus::Identical {
                continue;
            }
            if fd.binary
                || f.submodule
                || f.hunks_total == 0
                || f.hunks_already_on_base != f.hunks_total
            {
                explained = false;
            } else {
                any_hunk_file = true;
            }
        }
        let r = &mut reports[i];
        r.hunks_total = r.files.iter().map(|f| f.hunks_total).sum();
        r.hunks_already_on_base = r.files.iter().map(|f| f.hunks_already_on_base).sum();
        if !matches!(r.class, Classification::New | Classification::Partial) {
            // SHIPPED / PATCH_EQUIVALENT were decided earlier; this pass only fills in hunk detail.
            continue;
        }
        if explained && (any_hunk_file || r.files.iter().all(|f| f.status == FileStatus::Identical))
        {
            r.class = Classification::Shipped;
        } else if promote_partial && r.class == Classification::New && r.hunks_already_on_base > 0 {
            // Some of it is on base even though no file is byte-identical.
            r.class = Classification::Partial;
        }
    }
    Ok(())
}
