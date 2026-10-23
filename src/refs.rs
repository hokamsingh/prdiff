use crate::error::{bad, git_err};
use crate::git::Git;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RefInfo {
    pub name: String,
    pub sha: String,
}

/// Resolve a ref to a commit SHA once, up front.
pub fn resolve(git: &Git, name: &str) -> Result<RefInfo> {
    let spec = format!("{name}^{{commit}}");
    let out = git.exec(&["rev-parse", "--verify", "--quiet", &spec], None, &[])?;
    if out.code != 0 {
        return Err(bad(format!("unknown ref: {name}")));
    }
    Ok(RefInfo {
        name: name.to_string(),
        sha: String::from_utf8_lossy(&out.stdout).trim().to_string(),
    })
}

pub fn merge_base(git: &Git, base: &RefInfo, head: &RefInfo) -> Result<String> {
    let out = git.exec(&["merge-base", &base.sha, &head.sha], None, &[])?;
    match out.code {
        0 => Ok(String::from_utf8_lossy(&out.stdout).trim().to_string()),
        1 => Err(bad(format!(
            "no merge base between {} and {}",
            base.name, head.name
        ))),
        c => Err(git_err(format!(
            "git merge-base failed (exit {c}): {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

/// Shallow check plus both refs in a single `git rev-parse`. `None` means "use the slow path for an
/// accurate error" (a shallow repository or an unknown ref).
pub fn resolve_pair(git: &Git, base: &str, head: &str) -> Result<Option<(RefInfo, RefInfo)>> {
    let (b, h) = (format!("{base}^{{commit}}"), format!("{head}^{{commit}}"));
    let out = git.exec(&["rev-parse", "--is-shallow-repository", &b, &h], None, &[])?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    if out.code != 0 || lines.next() != Some("false") {
        return Ok(None);
    }
    match (lines.next(), lines.next()) {
        (Some(bs), Some(hs)) if bs.len() >= 40 && hs.len() >= 40 => Ok(Some((
            RefInfo {
                name: base.to_string(),
                sha: bs.to_string(),
            },
            RefInfo {
                name: head.to_string(),
                sha: hs.to_string(),
            },
        ))),
        _ => Ok(None),
    }
}

/// A shallow clone has no reliable merge base or history to compare.
pub fn ensure_not_shallow(git: &Git) -> Result<()> {
    if git.text(&["rev-parse", "--is-shallow-repository"])? == "true" {
        return Err(git_err("shallow clone: run `git fetch --unshallow` first"));
    }
    Ok(())
}
