//! Render release notes with [git-cliff](https://git-cliff.org) so teams keep their own `cliff.toml` templates.
//!
//! prdiff decides which commits are really new; git-cliff only formats them. Every other commit in
//! `base..head` (squashed, cherry-picked, merges, empty, filtered out by `--path`) is passed as `--skip-commit`.

use crate::error::{bad, git_err};
use crate::git::Git;
use crate::report::Report;
use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};

fn bin() -> String {
    std::env::var("PRDIFF_GIT_CLIFF").unwrap_or_else(|_| "git-cliff".into())
}

/// Commits of `base..head` that must not appear in the notes.
pub fn commits_to_skip(git: &Git, r: &Report) -> Result<Vec<String>> {
    let range = format!("{}..{}", r.base.sha, r.head.sha);
    let all = git.run(&["rev-list", &range])?;
    let listed: HashSet<&str> = r
        .commits
        .iter()
        .filter(|c| c.class.is_listed())
        .map(|c| c.sha.as_str())
        .collect();
    Ok(String::from_utf8_lossy(&all)
        .lines()
        .filter(|sha| !listed.contains(sha))
        .map(str::to_string)
        .collect())
}

/// The git-cliff argument list (everything after the program name).
pub fn args(
    r: &Report,
    skip: &[String],
    config: Option<&Path>,
    title: Option<&str>,
) -> Vec<String> {
    let mut a = vec![
        // a cliff.toml from the repository may define commands; never run them from here
        "--no-exec".to_string(),
        format!("{}..{}", r.base.sha, r.head.sha),
    ];
    if let Some(c) = config {
        a.push("--config".into());
        a.push(c.display().to_string());
    }
    if let Some(t) = title {
        a.push("--tag".into());
        a.push(t.to_string());
    }
    for sha in skip {
        a.push("--skip-commit".into());
        a.push(sha.clone());
    }
    a
}

pub fn render(
    repo: &Path,
    git: &Git,
    r: &Report,
    config: Option<&Path>,
    title: Option<&str>,
) -> Result<String> {
    let skip = commits_to_skip(git, r)?;
    let args = args(r, &skip, config, title);
    let out = Command::new(bin())
        .current_dir(repo)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| {
            bad(format!(
                "cannot run `{}`: {e} (install git-cliff: https://git-cliff.org, or `cargo install git-cliff`)",
                bin()
            ))
        })?;
    if !out.status.success() {
        return Err(git_err(format!(
            "git-cliff failed ({}): {}",
            out.status.code().map_or("signal".into(), |c| c.to_string()),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    if !out.stderr.is_empty() {
        // warnings such as "cliff.toml not found, using the default configuration"
        eprint!("{}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
