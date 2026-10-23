//! Human summary for `compare`.

use crate::report::{Classification, NetState, Report};
use std::fmt::Write;

pub fn render(r: &Report) -> String {
    let t = &r.totals;
    let mut s = String::new();
    let _ = writeln!(s, "base  {} ({})", r.base.name, &r.base.sha[..10]);
    let _ = writeln!(s, "head  {} ({})", r.head.name, &r.head.sha[..10]);
    let _ = writeln!(s, "merge-base {}\n", &r.merge_base[..10]);
    let _ = writeln!(
        s,
        "commits: {}   NEW {}  PARTIAL {}  SHIPPED {}  PATCH_EQUIVALENT {}",
        t.commits,
        t.count(Classification::New),
        t.count(Classification::Partial),
        t.count(Classification::Shipped),
        t.count(Classification::PatchEquivalent)
    );
    for c in &r.commits {
        let label = match c.class {
            Classification::New => "NEW",
            Classification::Partial => "PARTIAL",
            Classification::Shipped => "SHIPPED",
            Classification::PatchEquivalent => "PATCH_EQ",
        };
        let extra = if c.class == Classification::Partial {
            format!(
                "  [{}/{} hunks on base]",
                c.hunks_already_on_base, c.hunks_total
            )
        } else {
            String::new()
        };
        let _ = writeln!(s, "  {label:<8} {}  {}{extra}", c.short, c.subject);
    }
    let _ = writeln!(
        s,
        "\nfiles: {} new to base (+{} -{}), {} already on base",
        t.files_new_to_base, t.insertions, t.deletions, t.files_already_on_base
    );
    for f in r
        .net_files
        .iter()
        .filter(|f| f.state == NetState::NewToBase)
    {
        let stat = match (f.insertions, f.deletions) {
            (Some(a), Some(d)) => format!("+{a} -{d}"),
            _ => "binary".to_string(),
        };
        let _ = writeln!(s, "  {} {}  {stat}", f.change, f.path);
    }
    if r.conflicts.is_empty() {
        let _ = writeln!(s, "\nconflicts: none");
    } else {
        let _ = writeln!(
            s,
            "\nconflicts: {}{}",
            r.conflicts.len(),
            if r.conflicts_best_effort {
                " (best effort)"
            } else {
                ""
            }
        );
        for p in &r.conflicts {
            let _ = writeln!(s, "  {p}");
        }
    }
    s
}
