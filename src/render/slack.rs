//! Slack `mrkdwn` flavour of the PR description (Teams renders the Markdown one fine).

use crate::commits::CommitKind;
use crate::report::{Classification, Report};
use std::fmt::Write;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn render(r: &Report, title: Option<&str>) -> String {
    let t = &r.totals;
    let mut s = String::new();
    let default = format!(
        "Release: {} → {}",
        r.head.name.trim_start_matches("origin/"),
        r.base.name.trim_start_matches("origin/")
    );
    let _ = writeln!(s, "*{}*", esc(title.unwrap_or(&default)));
    let feats = t.kind(CommitKind::Feat);
    let fixes = t.kind(CommitKind::Fix);
    let _ = writeln!(
        s,
        "• Commits not yet on base: *{}* ({feats} feat, {fixes} fix, {} other)",
        t.listed,
        t.listed - feats - fixes
    );
    let _ = writeln!(
        s,
        "• Already there: {} identical, {} cherry-picked",
        t.count(Classification::Shipped),
        t.count(Classification::PatchEquivalent)
    );
    let _ = writeln!(
        s,
        "• Files changing: {} (+{} / -{})",
        t.files_new_to_base, t.insertions, t.deletions
    );
    if r.conflicts.is_empty() {
        let _ = writeln!(s, "• Conflicts: none");
    } else {
        let _ = writeln!(
            s,
            "• :warning: Conflicts: {}",
            r.conflicts
                .iter()
                .map(|p| format!("`{}`", esc(p)))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if t.risky_files > 0 {
        let _ = writeln!(s, "• Files needing a closer look: {}", t.risky_files);
    }
    for (name, pred) in [
        (
            "Features",
            (|k| k == CommitKind::Feat) as fn(CommitKind) -> bool,
        ),
        ("Fixes", |k| k == CommitKind::Fix),
        ("Other", |k| {
            !matches!(k, CommitKind::Feat | CommitKind::Fix)
        }),
    ] {
        let items: Vec<_> = r
            .commits
            .iter()
            .filter(|c| c.class.is_listed() && pred(c.kind))
            .collect();
        if items.is_empty() {
            continue;
        }
        let _ = writeln!(s, "\n*{name}*");
        for c in items {
            let _ = writeln!(s, "• {} (`{}`)", esc(&c.subject), c.short);
        }
    }
    s
}
