//! `prdiff.toml` and ref inference, so `prdiff compare` works with no arguments in most repos.

use crate::error::bad;
use crate::git::Git;
use anyhow::Result;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub base: Option<String>,
    pub head: Option<String>,
    /// Only consider these paths (globs).
    pub paths: Vec<String>,
    /// Left out of file lists and stats (lockfiles, generated code); commits are still classified on all files.
    pub exclude: Vec<String>,
    /// Extra globs to flag as risky, on top of the built-in rules.
    pub risky: Vec<String>,
    pub ignore_whitespace: bool,
    /// Release train, furthest along first: production, staging, develop.
    pub environments: Vec<EnvRef>,
    pub notes: NotesConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EnvRef {
    pub name: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NotesConfig {
    /// e.g. https://github.com/org/repo, used to link `(#123)`.
    pub repo_url: Option<String>,
    /// e.g. https://jira.example.com/browse/{id}
    pub ticket_url: Option<String>,
}

pub fn load(repo: &Path, explicit: Option<&Path>) -> Result<Config> {
    let path: Option<PathBuf> = match explicit {
        Some(p) => Some(p.to_path_buf()),
        None => {
            let git = Git::new(repo, std::time::Duration::from_secs(30));
            let top = git
                .text(&["rev-parse", "--show-toplevel"])
                .map(PathBuf::from)
                .unwrap_or_else(|_| repo.to_path_buf());
            let p = top.join("prdiff.toml");
            p.is_file().then_some(p)
        }
    };
    let Some(path) = path else {
        return Ok(Config::default());
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| bad(format!("cannot read {}: {e}", path.display())))?;
    toml::from_str(&text).map_err(|e| bad(format!("{}: {e}", path.display())))
}

/// The branch checked out right now.
pub fn infer_head(git: &Git) -> Result<String> {
    let out = git.exec(&["symbolic-ref", "--short", "-q", "HEAD"], None, &[])?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.code == 0 && !name.is_empty() {
        Ok(name)
    } else {
        Err(bad(
            "cannot infer --head (detached HEAD): pass --head or set `head` in prdiff.toml",
        ))
    }
}

/// The remote's default branch, falling back to the usual names.
pub fn infer_base(git: &Git) -> Result<String> {
    let out = git.exec(
        &["symbolic-ref", "--short", "-q", "refs/remotes/origin/HEAD"],
        None,
        &[],
    )?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if out.code == 0 && !name.is_empty() {
        return Ok(name);
    }
    for cand in ["origin/main", "origin/master", "main", "master"] {
        let spec = format!("{cand}^{{commit}}");
        if git
            .exec(&["rev-parse", "--verify", "--quiet", &spec], None, &[])?
            .code
            == 0
        {
            return Ok(cand.to_string());
        }
    }
    Err(bad(
        "cannot infer --base: pass --base or set `base` in prdiff.toml",
    ))
}
