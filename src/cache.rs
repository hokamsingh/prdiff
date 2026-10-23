//! Append-only cache of per-commit facts in `.git/prdiff/commits`.
//!
//! Keyed by commit SHA, so a history rewrite simply misses the cache; no invalidation is needed.
//! Line format: `<commit> <pathset-hex> <patch-id | - | ?>`
//! (`-` = empty diff, `?` = not computed yet). A later line for a commit overrides an earlier one.

use crate::git::Git;
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct Cached {
    pub pathset: u64,
    /// `Some("")` = the commit has an empty diff; `None` = not computed yet.
    pub patch_id: Option<String>,
}

pub struct Cache {
    file: Option<PathBuf>,
    pub map: HashMap<String, Cached>,
}

impl Cache {
    pub fn open(git: &Git, enabled: bool) -> Cache {
        let mut cache = Cache {
            file: None,
            map: HashMap::new(),
        };
        if !enabled {
            return cache;
        }
        let Ok(dir) = git.text(&["rev-parse", "--path-format=absolute", "--git-common-dir"]) else {
            return cache;
        };
        let dir = PathBuf::from(dir).join("prdiff");
        let file = dir.join("commits");
        if let Ok(text) = fs::read_to_string(&file) {
            for line in text.lines() {
                let mut f = line.split(' ');
                if let (Some(c), Some(p), Some(id)) = (f.next(), f.next(), f.next())
                    && let Ok(pathset) = u64::from_str_radix(p, 16)
                {
                    cache.map.insert(
                        c.to_string(),
                        Cached {
                            pathset,
                            patch_id: match id {
                                "?" => None,
                                "-" => Some(String::new()),
                                id => Some(id.to_string()),
                            },
                        },
                    );
                }
            }
        }
        if fs::create_dir_all(&dir).is_ok() {
            cache.file = Some(file);
        }
        cache
    }

    pub fn insert(&mut self, commit: String, c: Cached) {
        self.map.insert(commit, c);
    }

    /// Persist the given commits. Failures are ignored: the cache is an optimisation only.
    pub fn append(&self, commits: &[String]) {
        let Some(path) = &self.file else { return };
        let mut buf = String::new();
        for c in commits {
            if let Some(v) = self.map.get(c) {
                let id = match v.patch_id.as_deref() {
                    None => "?",
                    Some("") => "-",
                    Some(id) => id,
                };
                buf.push_str(&format!("{c} {:x} {id}\n", v.pathset));
            }
        }
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = f.write_all(buf.as_bytes());
        }
    }
}
