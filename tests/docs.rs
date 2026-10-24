//! Keeps the documentation honest: every option is documented and every local link resolves.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_prdiff");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn help(args: &[&str]) -> String {
    let out = Command::new(BIN).args(args).arg("--help").output().unwrap();
    assert!(out.status.success(), "{args:?} --help");
    String::from_utf8(out.stdout).unwrap()
}

/// `--long-option` tokens at the start of an option line of clap's help output.
fn long_options(help: &str) -> BTreeSet<String> {
    help.lines()
        .filter_map(|l| {
            let t = l.trim_start();
            let rest = if let Some(r) = t.strip_prefix("--") {
                r
            } else {
                // `-C, --repo <REPO>`
                t.strip_prefix('-')?.split_once(", --")?.1
            };
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
                .collect();
            (!name.is_empty()).then(|| format!("--{name}"))
        })
        .collect()
}

#[test]
fn every_option_and_command_appears_in_the_command_reference() {
    let doc = fs::read_to_string(Path::new(ROOT).join("docs/commands.md")).unwrap();
    let top = help(&[]);
    let commands: Vec<String> = top
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .filter(|c| c != "help")
        .collect();
    assert!(commands.len() >= 12, "parsed commands: {commands:?}");

    let mut missing = Vec::new();
    for opt in long_options(&top) {
        if !doc.contains(&opt) {
            missing.push(format!("global {opt}"));
        }
    }
    for cmd in &commands {
        if !doc.contains(&format!("## `{cmd}`")) && !doc.contains(&format!("`{cmd}`")) {
            missing.push(format!("command {cmd}"));
        }
        for opt in long_options(&help(&[cmd])) {
            if opt != "--help" && !doc.contains(&opt) {
                missing.push(format!("{cmd} {opt}"));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "docs/commands.md does not mention: {missing:#?}"
    );
}

fn markdown_files() -> Vec<PathBuf> {
    let mut v = vec![
        Path::new(ROOT).join("README.md"),
        Path::new(ROOT).join("CONTRIBUTING.md"),
    ];
    for e in fs::read_dir(Path::new(ROOT).join("docs")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "md") {
            v.push(p);
        }
    }
    v
}

fn headings(path: &Path) -> BTreeSet<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| l.starts_with('#'))
        .map(|l| {
            l.trim_start_matches('#')
                .trim()
                .to_lowercase()
                .chars()
                .filter_map(|c| match c {
                    c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
                    ' ' => Some('-'),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

#[test]
fn every_local_link_in_the_docs_resolves() {
    let link = regex::Regex::new(r"\]\(([^)\s]+)\)").unwrap();
    let mut broken = Vec::new();
    for file in markdown_files() {
        let text = fs::read_to_string(&file).unwrap();
        for c in link.captures_iter(&text) {
            let target = &c[1];
            if target.starts_with("http") || target.starts_with("mailto:") {
                continue;
            }
            let (path, anchor) = target
                .split_once('#')
                .map_or((target, None), |(p, a)| (p, Some(a)));
            let resolved = if path.is_empty() {
                file.clone()
            } else {
                file.parent().unwrap().join(path)
            };
            if !resolved.exists() {
                broken.push(format!("{}: {target} (no such file)", file.display()));
            } else if let Some(a) = anchor
                && resolved.extension().is_some_and(|x| x == "md")
                && !headings(&resolved).contains(a)
            {
                broken.push(format!("{}: {target} (no such heading)", file.display()));
            }
        }
    }
    assert!(broken.is_empty(), "broken links: {broken:#?}");
}

#[test]
fn docs_index_lists_every_page() {
    let index = fs::read_to_string(Path::new(ROOT).join("docs/README.md")).unwrap();
    for e in fs::read_dir(Path::new(ROOT).join("docs")).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && name != "README.md" {
            assert!(
                index.contains(&format!("({name})")),
                "docs/README.md does not link {name}"
            );
        }
    }
}
