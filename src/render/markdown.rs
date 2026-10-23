//! `describe`: a PR body generated from the Report. Every count is derived from the same lists it prints.

use crate::commits::CommitKind;
use crate::report::{Classification, CommitReport, Report};
use std::fmt::Write;

fn strip_remote(name: &str) -> &str {
    name.strip_prefix("origin/").unwrap_or(name)
}

pub fn render(r: &Report, title: Option<&str>) -> String {
    let t = &r.totals;
    let mut s = String::new();
    let default_title = format!(
        "Release: {} → {}",
        strip_remote(&r.head.name),
        strip_remote(&r.base.name)
    );
    let _ = writeln!(s, "## {}\n", title.unwrap_or(&default_title));

    let feats = t.kind(CommitKind::Feat);
    let fixes = t.kind(CommitKind::Fix);
    let other = t.listed - feats - fixes;
    let _ = writeln!(s, "**Scope (checked by content, not subject)**");
    let _ = writeln!(
        s,
        "- Commits not yet on base: {} ({feats} feat, {fixes} fix, {other} other)",
        t.listed
    );
    let _ = writeln!(
        s,
        "- Excluded, identical on base: {}",
        t.count(Classification::Shipped)
    );
    let _ = writeln!(
        s,
        "- Excluded, patch-equivalent on base: {}",
        t.count(Classification::PatchEquivalent)
    );
    let _ = writeln!(
        s,
        "- Files changing vs base: {} (+{} / -{})",
        t.files_new_to_base, t.insertions, t.deletions
    );
    if r.conflicts.is_empty() {
        let _ = writeln!(s, "- Conflicted files: none");
    } else {
        let note = if r.conflicts_best_effort {
            " (best effort, old git)"
        } else {
            ""
        };
        let _ = writeln!(
            s,
            "- Conflicted files{note}: {}",
            r.conflicts
                .iter()
                .map(|p| format!("`{p}`"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for d in &r.conflict_details {
        let head: Vec<String> = d
            .head_commits
            .iter()
            .map(|c| format!("`{}`", &c[..7]))
            .collect();
        let base: Vec<String> = d
            .base_commits
            .iter()
            .map(|c| format!("`{}` {} ({})", &c.sha[..7], c.subject, c.author))
            .collect();
        let _ = writeln!(
            s,
            "  - `{}`: this PR {}; target {}",
            d.path,
            if head.is_empty() {
                "-".into()
            } else {
                head.join(", ")
            },
            if base.is_empty() {
                "-".into()
            } else {
                base.join("; ")
            }
        );
    }
    if t.risky_files > 0 {
        let _ = writeln!(s, "- Files that need a closer look: {}", t.risky_files);
    }
    s.push('\n');
    if !r.owners_summary.is_empty() {
        let who: Vec<String> = r
            .owners_summary
            .iter()
            .map(|o| {
                format!(
                    "{} ({} file{})",
                    o.owner,
                    o.files,
                    if o.files == 1 { "" } else { "s" }
                )
            })
            .collect();
        let _ = writeln!(s, "**Review needed from:** {}\n", who.join(", "));
    }
    let risky: Vec<_> = r
        .net_files
        .iter()
        .filter(|f| f.state == crate::report::NetState::NewToBase && !f.risk.is_empty())
        .collect();
    if !risky.is_empty() {
        let _ = writeln!(s, "**Look closely at**");
        for f in risky.iter().take(15) {
            let _ = writeln!(s, "- `{}`: {}", f.path, f.risk.join(", "));
        }
        if risky.len() > 15 {
            let _ = writeln!(s, "- … and {} more", risky.len() - 15);
        }
        s.push('\n');
    }

    let listed: Vec<&CommitReport> = r.commits.iter().filter(|c| c.class.is_listed()).collect();
    section(
        &mut s,
        "Features",
        listed
            .iter()
            .copied()
            .filter(|c| c.kind == CommitKind::Feat),
    );
    section(
        &mut s,
        "Fixes",
        listed.iter().copied().filter(|c| c.kind == CommitKind::Fix),
    );
    section(
        &mut s,
        "Other",
        listed
            .iter()
            .copied()
            .filter(|c| !matches!(c.kind, CommitKind::Feat | CommitKind::Fix)),
    );

    if !r.revert_pairs.is_empty() {
        let _ = writeln!(s, "**Revert pairs (net zero for these changes)**");
        for p in &r.revert_pairs {
            let _ = writeln!(s, "- `{}` reverts `{}`", &p.revert[..7], &p.reverted[..7]);
        }
        s.push('\n');
    }

    let excluded: Vec<&CommitReport> = r.commits.iter().filter(|c| !c.class.is_listed()).collect();
    if !excluded.is_empty() {
        let _ = writeln!(
            s,
            "<details><summary>Excluded: already on base ({})</summary>\n",
            excluded.len()
        );
        for c in excluded {
            let why = match c.class {
                Classification::PatchEquivalent => "patch-equivalent",
                _ => "identical on base",
            };
            let _ = writeln!(s, "- {} (`{}`) — {why}", c.subject, c.short);
        }
        let _ = writeln!(s, "\n</details>");
    }
    s
}

fn section<'a>(s: &mut String, name: &str, items: impl Iterator<Item = &'a CommitReport>) {
    let items: Vec<_> = items.collect();
    if items.is_empty() {
        return;
    }
    let _ = writeln!(s, "**{name}**");
    for c in items {
        let partial = if c.class == Classification::Partial {
            format!(
                " — partial: {}/{} hunks already on base",
                c.hunks_already_on_base, c.hunks_total
            )
        } else {
            String::new()
        };
        let _ = writeln!(s, "- {} (`{}`){partial}", c.subject, c.short);
    }
    s.push('\n');
}
