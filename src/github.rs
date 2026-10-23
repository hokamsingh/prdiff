//! GitHub integration through the `gh` CLI: resolve a PR to refs, and publish the description back.
//! Nothing here talks to GitHub directly; `gh` brings its own auth (locally, or `GH_TOKEN` in Actions).

use crate::error::{bad, git_err};
use crate::git::Git;
use anyhow::Result;
use serde::Deserialize;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const BODY_START: &str = "<!-- prdiff:start -->";
pub const BODY_END: &str = "<!-- prdiff:end -->";
pub const COMMENT_MARK: &str = "<!-- prdiff:comment -->";

#[derive(Debug, Deserialize)]
pub struct PrInfo {
    #[serde(rename = "baseRefName")]
    pub base_ref: String,
    #[serde(rename = "headRefName")]
    pub head_ref: String,
    #[serde(default)]
    pub body: String,
}

fn gh_bin() -> String {
    std::env::var("PRDIFF_GH").unwrap_or_else(|_| "gh".into())
}

/// Run `gh` in the repository and return stdout.
fn gh(repo: &Path, args: &[&str], stdin: Option<&str>) -> Result<String> {
    let mut cmd = Command::new(gh_bin());
    cmd.current_dir(repo)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        bad(format!(
            "cannot run `{}`: {e} (install the GitHub CLI: https://cli.github.com)",
            gh_bin()
        ))
    })?;
    if let Some(input) = stdin {
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(input.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(git_err(format!(
            "gh {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn pr_info(repo: &Path, number: u32) -> Result<PrInfo> {
    let n = number.to_string();
    let json = gh(
        repo,
        &["pr", "view", &n, "--json", "baseRefName,headRefName,body"],
        None,
    )?;
    serde_json::from_str(&json).map_err(|e| git_err(format!("unexpected `gh pr view` output: {e}")))
}

/// Make the PR's base branch and head available locally; returns `(base_ref, head_ref)` to compare.
pub fn fetch_pr(git: &Git, number: u32, info: &PrInfo) -> Result<(String, String)> {
    let head_ref = format!("refs/prdiff/pr/{number}");
    let head_spec = format!("+refs/pull/{number}/head:{head_ref}");
    let base_spec = format!("+refs/heads/{0}:refs/remotes/origin/{0}", info.base_ref);
    git.run(&["fetch", "--quiet", "origin", &base_spec, &head_spec])?;
    Ok((format!("origin/{}", info.base_ref), head_ref))
}

/// Replace the marked block in `existing`, or append one.
pub fn merge_body(existing: &str, md: &str) -> String {
    let block = format!("{BODY_START}\n{}\n{BODY_END}", md.trim_end());
    if let (Some(a), Some(b)) = (existing.find(BODY_START), existing.find(BODY_END))
        && a < b
    {
        return format!(
            "{}{block}{}",
            &existing[..a],
            &existing[b + BODY_END.len()..]
        );
    }
    if existing.trim().is_empty() {
        block
    } else {
        format!("{}\n\n{block}", existing.trim_end())
    }
}

pub fn comment_body(md: &str) -> String {
    format!("{COMMENT_MARK}\n{}", md.trim_end())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Published {
    BodyUpdated,
    CommentCreated,
    CommentUpdated(String),
}

pub fn publish_body(repo: &Path, number: u32, current: &str, md: &str) -> Result<Published> {
    let n = number.to_string();
    gh(
        repo,
        &["pr", "edit", &n, "--body-file", "-"],
        Some(&merge_body(current, md)),
    )?;
    Ok(Published::BodyUpdated)
}

pub fn publish_comment(repo: &Path, number: u32, md: &str) -> Result<Published> {
    let n = number.to_string();
    let body = comment_body(md);
    let path = format!("repos/{{owner}}/{{repo}}/issues/{n}/comments");
    let jq = format!(r#".[] | select(.body | startswith("{COMMENT_MARK}")) | .id"#);
    let found = gh(repo, &["api", &path, "--paginate", "-q", &jq], None)?;
    if let Some(id) = found
        .lines()
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let upd = format!("repos/{{owner}}/{{repo}}/issues/comments/{id}");
        gh(
            repo,
            &["api", "-X", "PATCH", &upd, "-F", "body=@-"],
            Some(&body),
        )?;
        Ok(Published::CommentUpdated(id.to_string()))
    } else {
        gh(
            repo,
            &["pr", "comment", &n, "--body-file", "-"],
            Some(&body),
        )?;
        Ok(Published::CommentCreated)
    }
}
