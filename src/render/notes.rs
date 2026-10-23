//! `notes`: release notes grouped by kind, with breaking changes first and PR / ticket links.

use crate::commits::CommitKind;
use crate::report::{Classification, CommitReport, Report};
use regex::Regex;
use serde::Serialize;
use std::fmt::Write;
use std::sync::OnceLock;

#[derive(Debug, Clone, Default)]
pub struct NotesOptions {
    pub title: Option<String>,
    /// e.g. https://github.com/org/repo
    pub repo_url: Option<String>,
    /// e.g. https://jira.example.com/browse/{id}
    pub ticket_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub sha: String,
    pub scope: Option<String>,
    pub description: String,
    pub pr: Option<u32>,
    pub tickets: Vec<String>,
    pub partial: bool,
}

#[derive(Debug, Serialize)]
pub struct Group {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// `https://github.com/org/repo` from an origin URL (`git@github.com:org/repo.git`, https, ssh://).
pub fn repo_url_from_remote(remote: &str) -> Option<String> {
    let r = remote.trim().trim_end_matches('/').trim_end_matches(".git");
    if let Some(rest) = r.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        return Some(format!("https://{host}/{path}"));
    }
    for p in ["ssh://git@", "ssh://", "git://"] {
        if let Some(rest) = r.strip_prefix(p) {
            let (host, path) = rest.split_once('/')?;
            return Some(format!("https://{host}/{path}"));
        }
    }
    r.starts_with("http").then(|| r.to_string())
}

/// The subject without `type(scope):`, `(#123)` and leading or trailing ticket ids, which are linked separately.
fn description(subject: &str) -> String {
    static HEAD: OnceLock<Regex> = OnceLock::new();
    static PR: OnceLock<Regex> = OnceLock::new();
    static T_START: OnceLock<Regex> = OnceLock::new();
    static T_END: OnceLock<Regex> = OnceLock::new();
    let head = HEAD.get_or_init(|| Regex::new(r"^[A-Za-z]+(?:\([^)]*\))?!?:\s*").unwrap());
    let pr = PR.get_or_init(|| Regex::new(r"\s*\(#\d+\)").unwrap());
    let t_start = T_START.get_or_init(|| Regex::new(r"^\[?[A-Z][A-Z0-9]+-\d+\]?[:\s-]*").unwrap());
    let t_end = T_END.get_or_init(|| Regex::new(r"[\s(\[-]*[A-Z][A-Z0-9]+-\d+[\])]?\s*$").unwrap());
    let mut s = head.replace(subject, "").into_owned();
    s = pr.replace_all(&s, "").into_owned();
    loop {
        let before = s.len();
        s = t_start.replace(s.trim(), "").into_owned();
        s = t_end.replace(s.trim(), "").into_owned();
        if s.len() == before {
            break;
        }
    }
    s.trim().to_string()
}

fn entry(c: &CommitReport) -> Entry {
    Entry {
        sha: c.short.clone(),
        scope: c.scope.clone(),
        description: description(&c.subject),
        pr: c.pr,
        tickets: c.tickets.clone(),
        partial: c.class == Classification::Partial,
    }
}

pub fn groups(r: &Report) -> Vec<Group> {
    let listed: Vec<&CommitReport> = r.commits.iter().filter(|c| c.class.is_listed()).collect();
    let mut out = Vec::new();
    let mut push = |name: &str, pred: &dyn Fn(&CommitReport) -> bool| {
        let mut entries: Vec<Entry> = listed
            .iter()
            .copied()
            .filter(|c| pred(c))
            .map(entry)
            .collect();
        entries.sort_by(|a, b| a.scope.cmp(&b.scope)); // stable: keeps commit order within a scope
        if !entries.is_empty() {
            out.push(Group {
                name: name.to_string(),
                entries,
            });
        }
    };
    push("Breaking changes", &|c| c.breaking);
    push("Features", &|c| !c.breaking && c.kind == CommitKind::Feat);
    push("Fixes", &|c| !c.breaking && c.kind == CommitKind::Fix);
    push("Refactoring", &|c| {
        !c.breaking && c.kind == CommitKind::Refactor
    });
    push("Chores", &|c| !c.breaking && c.kind == CommitKind::Chore);
    push("Other changes", &|c| {
        !c.breaking && c.kind == CommitKind::Other
    });
    out
}

pub fn render(r: &Report, o: &NotesOptions) -> String {
    let mut s = String::new();
    let title = o.title.clone().unwrap_or_else(|| {
        format!(
            "Release notes: {} → {}",
            r.head.name.trim_start_matches("origin/"),
            r.base.name.trim_start_matches("origin/")
        )
    });
    let _ = writeln!(s, "## {title}\n");
    let gs = groups(r);
    if gs.is_empty() {
        let _ = writeln!(
            s,
            "Nothing new: everything in `{}` is already on `{}`.",
            r.head.name, r.base.name
        );
        return s;
    }
    for g in gs {
        let _ = writeln!(
            s,
            "### {}{}\n",
            if g.name == "Breaking changes" {
                "⚠ "
            } else {
                ""
            },
            g.name
        );
        for e in g.entries {
            let scope = e
                .scope
                .as_ref()
                .map(|x| format!("**{x}:** "))
                .unwrap_or_default();
            let pr = match (e.pr, &o.repo_url) {
                (Some(n), Some(url)) => format!(" ([#{n}]({url}/pull/{n}))"),
                (Some(n), None) => format!(" (#{n})"),
                _ => String::new(),
            };
            let tickets: String = e
                .tickets
                .iter()
                .map(|t| match &o.ticket_url {
                    Some(tpl) => format!(" [{t}]({})", tpl.replace("{id}", t)),
                    None => format!(" {t}"),
                })
                .collect();
            let partial = if e.partial {
                " _(partly on the target already)_"
            } else {
                ""
            };
            let _ = writeln!(
                s,
                "- {scope}{}{pr}{tickets}{partial} (`{}`)",
                e.description, e.sha
            );
        }
        s.push('\n');
    }
    s
}

pub fn render_json(r: &Report) -> String {
    serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": crate::report::SCHEMA_VERSION,
        "base": r.base, "head": r.head, "groups": groups(r),
    }))
    .expect("serialises")
        + "\n"
}
