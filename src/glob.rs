//! `--path` filter: `**` crosses directories, `*` and `?` do not; a pattern without `/` matches at any depth.

use anyhow::Result;
use regex::Regex;

pub struct PathFilter {
    res: Vec<Regex>,
    excludes: Vec<Regex>,
}

impl PathFilter {
    pub fn new(globs: &[String]) -> Result<Self> {
        Self::with_excludes(globs, &[])
    }

    pub fn with_excludes(globs: &[String], excludes: &[String]) -> Result<Self> {
        let compile = |gs: &[String], what: &str| {
            gs.iter()
                .map(|g| {
                    Regex::new(&to_regex(g))
                        .map_err(|e| crate::error::bad(format!("bad {what} glob {g:?}: {e}")))
                })
                .collect::<Result<Vec<_>>>()
        };
        Ok(PathFilter {
            res: compile(globs, "--path")?,
            excludes: compile(excludes, "--exclude")?,
        })
    }

    /// For file lists and stats: inside `--path` and not excluded.
    pub fn matches_net(&self, path: &str) -> bool {
        self.matches(path) && !self.excludes.iter().any(|r| r.is_match(path))
    }

    pub fn is_active(&self) -> bool {
        !self.res.is_empty()
    }

    pub fn matches(&self, path: &str) -> bool {
        self.res.is_empty() || self.res.iter().any(|r| r.is_match(path))
    }
}

fn to_regex(glob: &str) -> String {
    let mut g = glob.trim_start_matches("./").to_string();
    if g.ends_with('/') {
        g.push_str("**");
    }
    let mut re = String::from("^");
    if !g.contains('/') {
        re.push_str("(?:.*/)?");
    }
    let chars: Vec<char> = g.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 1;
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                    re.push_str("(?:.*/)?");
                } else {
                    re.push_str(".*");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    re
}
