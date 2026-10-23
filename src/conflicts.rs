//! Stage F: merge conflicts between base and head, without touching any worktree or index.

use crate::git::{self, Git};
use crate::rawdiff::{self, Entry};
use anyhow::Result;
use std::collections::HashSet;

pub struct Conflicts {
    pub paths: Vec<String>,
    /// The cheap path-overlap check proved there is nothing to merge-tree.
    pub shortcut: bool,
    /// Parsed from the pre-2.38 `git merge-tree` text format.
    pub best_effort: bool,
}

/// `merge_base` enables the shortcut (skip `git merge-tree` when the two sides cannot possibly conflict).
pub fn detect(git: &Git, base: &str, head: &str, merge_base: Option<&str>) -> Result<Conflicts> {
    if let Some(mb) = merge_base
        && disjoint(git, mb, base, head)?
    {
        return Ok(Conflicts {
            paths: Vec::new(),
            shortcut: true,
            best_effort: false,
        });
    }
    let (major, minor) = git::version(git)?;
    if (major, minor) >= (2, 38) {
        write_tree(git, base, head)
    } else {
        legacy(git, base, head)
    }
}

/// Prefer tmpfs: merge-tree writes a few hundred loose objects, which is slow on many disks.
fn scratch_dir() -> std::io::Result<tempfile::TempDir> {
    if cfg!(target_os = "linux")
        && std::path::Path::new("/dev/shm").is_dir()
        && let Ok(d) = tempfile::Builder::new()
            .prefix("prdiff-")
            .tempdir_in("/dev/shm")
    {
        return Ok(d);
    }
    tempfile::tempdir()
}

/// `git merge-tree --write-tree` writes the merged tree into the object database. Point it at a throwaway
/// object directory (with the real one as an alternate) so the repository itself stays untouched.
fn write_tree(git: &Git, base: &str, head: &str) -> Result<Conflicts> {
    let objects = git.text(&[
        "rev-parse",
        "--path-format=absolute",
        "--git-path",
        "objects",
    ])?;
    let tmp = scratch_dir()?;
    let tmp_path = tmp.path().to_string_lossy().into_owned();
    let out = git.exec(
        &[
            "-c",
            "core.looseCompression=0",
            "merge-tree",
            "--write-tree",
            "--name-only",
            "-z",
            base,
            head,
        ],
        None,
        &[
            ("GIT_OBJECT_DIRECTORY", &tmp_path),
            ("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects),
        ],
    )?;
    match out.code {
        0 | 1 => {}
        c => {
            return Err(crate::error::git_err(format!(
                "git merge-tree --write-tree failed (exit {c}): {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
    }
    // <tree>\0 then conflicted paths each NUL-terminated, then an empty field, then messages.
    let mut paths: Vec<String> = Vec::new();
    if out.code == 1 {
        let mut fields = out.stdout.split(|b| *b == 0);
        fields.next(); // tree id
        for f in fields {
            if f.is_empty() {
                break;
            }
            let p = String::from_utf8_lossy(f).into_owned();
            if !paths.contains(&p) {
                paths.push(p);
            }
        }
    }
    Ok(Conflicts {
        paths,
        shortcut: false,
        best_effort: false,
    })
}

/// `git merge-tree <merge-base> <base> <head>`: sections start with "changed in both" / "added in both" and a
/// section holding `<<<<<<<` markers is a conflict.
fn legacy(git: &Git, base: &str, head: &str) -> Result<Conflicts> {
    let mb = git.text(&["merge-base", base, head])?;
    let out = git.run(&["merge-tree", &mb, base, head])?;
    let text = String::from_utf8_lossy(&out);
    let mut paths: Vec<String> = Vec::new();
    let mut cur: Option<String> = None;
    let mut conflicted = false;
    let flush = |cur: &mut Option<String>, conflicted: &mut bool, paths: &mut Vec<String>| {
        if let Some(p) = cur.take()
            && *conflicted
            && !paths.contains(&p)
        {
            paths.push(p);
        }
        *conflicted = false;
    };
    for line in text.lines() {
        if line.starts_with("changed in both") || line.starts_with("added in both") {
            flush(&mut cur, &mut conflicted, &mut paths);
            cur = Some(String::new());
        } else if line.starts_with("removed in")
            || line.starts_with("added in")
            || line.starts_with("changed in")
        {
            flush(&mut cur, &mut conflicted, &mut paths);
        } else if let Some(p) = cur.as_mut() {
            if p.is_empty()
                && let Some(rest) = line.trim_start().strip_prefix("our ")
            {
                // our  100644 <sha> <path>
                if let Some(path) = rest.split_whitespace().nth(2) {
                    *p = path.to_string();
                }
            }
            if line.starts_with("+<<<<<<<") {
                conflicted = true;
            }
        }
    }
    flush(&mut cur, &mut conflicted, &mut paths);
    Ok(Conflicts {
        paths,
        shortcut: false,
        best_effort: true,
    })
}

struct Side {
    paths: HashSet<String>,
    adds: bool,
    deletes: bool,
}

fn side(git: &Git, mb: &str, tip: &str) -> Result<Side> {
    let out = git.run(&["diff-tree", "-r", "--no-renames", "--raw", "-z", mb, tip])?;
    let (entries, _) = rawdiff::parse_raw_numstat(&out);
    let mut s = Side {
        paths: HashSet::with_capacity(entries.len()),
        adds: false,
        deletes: false,
    };
    for e in entries {
        let Entry { status, path, .. } = e;
        s.adds |= status == b'A' || status == b'T';
        s.deletes |= status == b'D' || status == b'T';
        s.paths.insert(path);
    }
    Ok(s)
}

fn ancestors(path: &str) -> impl Iterator<Item = &str> {
    path.match_indices('/').map(move |(i, _)| &path[..i])
}

/// True when the two sides provably cannot conflict: no path changed by both, no file/directory clash,
/// and no add-vs-delete pair that could trigger rename or directory-rename handling.
fn disjoint(git: &Git, mb: &str, base: &str, head: &str) -> Result<bool> {
    let (b, h) = rayon::join(|| side(git, mb, base), || side(git, mb, head));
    let (b, h) = (b?, h?);
    if (b.deletes && h.adds) || (h.deletes && b.adds) {
        return Ok(false);
    }
    let (small, large) = if b.paths.len() <= h.paths.len() {
        (&b, &h)
    } else {
        (&h, &b)
    };
    if small.paths.iter().any(|p| large.paths.contains(p)) {
        return Ok(false);
    }
    // file vs directory: one side's file path is a parent directory of the other side's path
    let clash = |a: &Side, z: &Side| {
        a.paths
            .iter()
            .any(|p| ancestors(p).any(|d| z.paths.contains(d)))
    };
    Ok(!(clash(&b, &h) || clash(&h, &b)))
}

/// For each conflicted file: the head-only commits and the newest base commits (since the merge-base)
/// that touched it. Capped so a mass conflict stays cheap.
pub fn attribute(
    git: &Git,
    merge_base: &str,
    base: &str,
    paths: &[String],
    head_commits: &[crate::report::CommitReport],
) -> Vec<crate::report::ConflictDetail> {
    use crate::report::{BaseCommit, ConflictDetail};
    use rayon::prelude::*;
    let range = format!("{merge_base}..{base}");
    paths
        .par_iter()
        .take(50)
        .map(|path| {
            let log = git
                .run(&[
                    "log",
                    "--no-merges",
                    "-n",
                    "5",
                    "--format=%H%x1f%an%x1f%s",
                    &range,
                    "--",
                    path,
                ])
                .unwrap_or_default();
            let base_commits = String::from_utf8_lossy(&log)
                .lines()
                .filter_map(|l| {
                    let mut f = l.split('\x1f');
                    Some(BaseCommit {
                        sha: f.next()?.to_string(),
                        author: f.next()?.to_string(),
                        subject: f.next()?.to_string(),
                    })
                })
                .collect();
            let head_commits = head_commits
                .iter()
                .filter(|c| {
                    c.files
                        .iter()
                        .any(|f| f.path == *path || f.old_path.as_deref() == Some(path))
                })
                .map(|c| c.sha.clone())
                .collect();
            ConflictDetail {
                path: path.clone(),
                head_commits,
                base_commits,
            }
        })
        .collect()
}
