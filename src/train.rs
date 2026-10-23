//! Release train: how far has a change travelled (develop -> staging -> production)?

use crate::analyze::{Options, analyze};
use crate::commits::CommitKind;
use crate::config::EnvRef;
use crate::error::bad;
use crate::git::Git;
use crate::refs::{self, RefInfo};
use crate::report::Classification;
use anyhow::Result;
use rayon::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Presence {
    /// An ancestor of the environment: it was merged there.
    Merged,
    /// Same patch is there (cherry-pick).
    PatchEquivalent,
    /// Same content is there (squash merge).
    Shipped,
    Partial,
    NotPresent,
}

impl Presence {
    pub fn is_present(self) -> bool {
        matches!(
            self,
            Presence::Merged | Presence::PatchEquivalent | Presence::Shipped
        )
    }
}

impl From<Classification> for Presence {
    fn from(c: Classification) -> Self {
        match c {
            Classification::New => Presence::NotPresent,
            Classification::Partial => Presence::Partial,
            Classification::Shipped => Presence::Shipped,
            Classification::PatchEquivalent => Presence::PatchEquivalent,
        }
    }
}

/// `name=ref` as given on the command line.
pub fn parse_env(spec: &str) -> Result<EnvRef> {
    match spec.split_once('=') {
        Some((n, r)) if !n.is_empty() && !r.is_empty() => Ok(EnvRef {
            name: n.to_string(),
            git_ref: r.to_string(),
        }),
        _ => Err(bad(format!("--env expects name=ref, got {spec:?}"))),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvStatus {
    pub name: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub sha: String,
    pub state: Presence,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhereReport {
    pub schema_version: u32,
    pub commit: String,
    pub short: String,
    pub subject: String,
    pub envs: Vec<EnvStatus>,
}

fn with_base(o: &Options, git_ref: &str, head: &str) -> Options {
    let mut x = o.clone();
    x.base = git_ref.to_string();
    x.head = head.to_string();
    x.detail = None;
    x.sides_budget = 0;
    x
}

/// Resolve a revision, or `#123` / `PR123` to the commit that mentions it, searching `search_in`.
pub fn resolve_commit(git: &Git, rev: &str, search_in: &str) -> Result<String> {
    let pr = rev
        .strip_prefix('#')
        .or_else(|| rev.strip_prefix("PR"))
        .filter(|n| n.chars().all(|c| c.is_ascii_digit()) && !n.is_empty());
    if let Some(n) = pr {
        let pat = format!("#{n}\\b");
        let out = git.run(&[
            "log",
            "-n",
            "1",
            "--format=%H",
            "-E",
            "--grep",
            &pat,
            search_in,
        ])?;
        let sha = String::from_utf8_lossy(&out).trim().to_string();
        if sha.is_empty() {
            return Err(bad(format!("no commit mentioning #{n} in {search_in}")));
        }
        return Ok(sha);
    }
    Ok(refs::resolve(git, rev)?.sha)
}

pub fn locate(o: &Options, envs: &[EnvRef], commit: &str) -> Result<WhereReport> {
    if envs.is_empty() {
        return Err(bad(
            "no environments: pass --env name=ref (repeatable) or set [[environments]] in prdiff.toml",
        ));
    }
    let git = Git::new(&o.repo, o.timeout);
    let subject = git.text(&["log", "-n", "1", "--format=%s", commit])?;
    let statuses: Vec<Result<EnvStatus>> = envs
        .par_iter()
        .map(|e| {
            let info = refs::resolve(&git, &e.git_ref)?;
            let anc = git.exec(
                &["merge-base", "--is-ancestor", commit, &info.sha],
                None,
                &[],
            )?;
            let state = if anc.code == 0 {
                Presence::Merged
            } else {
                match analyze(&with_base(o, &info.sha, commit)) {
                    Ok(r) => r
                        .commits
                        .iter()
                        .find(|c| c.sha == commit)
                        .map_or(Presence::NotPresent, |c| c.class.into()),
                    // unrelated histories: nothing of it can be there
                    Err(_) => Presence::NotPresent,
                }
            };
            Ok(EnvStatus {
                name: e.name.clone(),
                git_ref: e.git_ref.clone(),
                sha: info.sha,
                state,
            })
        })
        .collect();
    Ok(WhereReport {
        schema_version: crate::report::SCHEMA_VERSION,
        commit: commit.to_string(),
        short: commit.chars().take(7).collect(),
        subject,
        envs: statuses.into_iter().collect::<Result<_>>()?,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EnvInfo {
    pub name: String,
    #[serde(rename = "ref")]
    pub git_ref: String,
    pub sha: String,
    /// Commits of `head` this environment does not have yet.
    pub missing: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatrixRow {
    pub sha: String,
    pub short: String,
    pub subject: String,
    pub kind: CommitKind,
    /// One entry per environment, in `envs` order.
    pub states: Vec<Presence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Matrix {
    pub schema_version: u32,
    pub head: RefInfo,
    pub envs: Vec<EnvInfo>,
    pub rows: Vec<MatrixRow>,
}

/// Every head-only commit (relative to the least advanced environment) against every environment.
pub fn matrix(o: &Options, envs: &[EnvRef]) -> Result<Matrix> {
    if envs.is_empty() {
        return Err(bad(
            "no environments: pass --env name=ref (repeatable) or set [[environments]] in prdiff.toml",
        ));
    }
    let reports: Vec<Result<crate::report::Report>> = envs
        .par_iter()
        .map(|e| analyze(&with_base(o, &e.git_ref, &o.head)))
        .collect();
    let reports: Vec<crate::report::Report> = reports.into_iter().collect::<Result<_>>()?;

    // rows: the largest report first (the least advanced environment), then anything extra
    let mut order: Vec<usize> = (0..reports.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(reports[i].commits.len()));
    let mut rows: Vec<MatrixRow> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for &i in &order {
        for c in &reports[i].commits {
            if seen.insert(c.sha.clone()) {
                rows.push(MatrixRow {
                    sha: c.sha.clone(),
                    short: c.short.clone(),
                    subject: c.subject.clone(),
                    kind: c.kind,
                    states: Vec::new(),
                });
            }
        }
    }
    for row in &mut rows {
        row.states = reports
            .iter()
            .map(|r| {
                r.commits
                    .iter()
                    .find(|c| c.sha == row.sha)
                    .map_or(Presence::Merged, |c| c.class.into())
            })
            .collect();
    }
    let infos = envs
        .iter()
        .zip(&reports)
        .enumerate()
        .map(|(i, (e, r))| EnvInfo {
            name: e.name.clone(),
            git_ref: e.git_ref.clone(),
            sha: r.base.sha.clone(),
            missing: rows
                .iter()
                .filter(|row| !row.states[i].is_present())
                .count(),
        })
        .collect();
    Ok(Matrix {
        schema_version: crate::report::SCHEMA_VERSION,
        head: reports[0].head.clone(),
        envs: infos,
        rows,
    })
}

pub fn render_where(w: &WhereReport) -> String {
    let mut s = format!("{} {}\n", w.short, w.subject);
    for e in &w.envs {
        let label = match e.state {
            Presence::Merged => "merged",
            Presence::PatchEquivalent => "there as a cherry-pick",
            Presence::Shipped => "there (same content)",
            Presence::Partial => "partly there",
            Presence::NotPresent => "not there yet",
        };
        s += &format!(
            "  {} {:<14} {}\n",
            if e.state.is_present() { "✓" } else { "✗" },
            e.name,
            label
        );
    }
    s
}

fn cell(p: Presence) -> &'static str {
    match p {
        Presence::Merged => "merged",
        Presence::PatchEquivalent => "cherry-pick",
        Presence::Shipped => "same content",
        Presence::Partial => "partial",
        Presence::NotPresent => "-",
    }
}

pub fn render_matrix(m: &Matrix) -> String {
    let mut s = format!("{}\n\n", m.head.name);
    let w = m
        .envs
        .iter()
        .map(|e| e.name.len().max(12))
        .max()
        .unwrap_or(12);
    s += &format!("{:<9}", "commit");
    for e in &m.envs {
        s += &format!("{:<w$}  ", e.name);
    }
    s += "subject\n";
    for r in &m.rows {
        s += &format!("{:<9}", r.short);
        for st in &r.states {
            s += &format!("{:<w$}  ", cell(*st));
        }
        s += &format!("{}\n", r.subject);
    }
    s += "\nmissing:";
    for e in &m.envs {
        s += &format!(" {} {}", e.name, e.missing);
    }
    s.push('\n');
    s
}

pub fn render_matrix_markdown(m: &Matrix) -> String {
    let mut s = String::from("| Commit | Subject |");
    for e in &m.envs {
        s += &format!(" {} |", e.name);
    }
    s += "\n|---|---|";
    for _ in &m.envs {
        s += "---|";
    }
    s.push('\n');
    for r in &m.rows {
        s += &format!("| `{}` | {} |", r.short, r.subject.replace('|', "\\|"));
        for st in &r.states {
            s += &format!(
                " {} |",
                if st.is_present() {
                    format!("✅ {}", cell(*st))
                } else if *st == Presence::Partial {
                    "🟡 partial".into()
                } else {
                    "❌".into()
                }
            );
        }
        s.push('\n');
    }
    s
}
