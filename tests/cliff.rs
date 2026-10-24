#![cfg(unix)]
//! `notes --renderer git-cliff`: a fake git-cliff checks the arguments; the real one (if installed) the output.
mod common;
use common::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

const FAKE: &str = r###"#!/bin/bash
echo "cwd=$(pwd)" > "$FAKE/call.txt"
for a in "$@"; do echo "arg=$a" >> "$FAKE/call.txt"; done
echo "## fake changelog"
[ -n "${FAKE_FAIL:-}" ] && { echo "boom: bad config" >&2; exit 1; }
exit 0
"###;

struct Fixture {
    repo: Repo,
    fake: tempfile::TempDir,
    shipped: String,
    picked: String,
    merge: String,
    empty: String,
    new: String,
}

fn setup() -> Fixture {
    let lib: String = (1..=10).map(|i| format!("l{i}\n")).collect();
    let repo = Repo::with_branches(&[("lib.txt", &lib), ("a.txt", "a\n")]);
    repo.checkout("develop");
    let new = repo.commit("feat: still new", &[("new.txt", "n\n")]);
    let shipped = repo.commit("feat: squashed on staging", &[("s.txt", "s\n")]);
    let picked = repo.commit(
        "fix: backported",
        &[("lib.txt", &lib.replace("l10\n", "L10\n"))],
    );
    repo.git(&["commit", "--allow-empty", "-q", "-m", "chore: empty"]);
    let empty = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "side", "develop~4"]);
    repo.commit("feat: side work", &[("side.txt", "x\n")]);
    repo.checkout("develop");
    repo.git(&["merge", "-q", "--no-ff", "--no-edit", "side"]);
    let merge = repo.git(&["rev-parse", "HEAD"]);
    repo.checkout("staging");
    repo.commit("Merge feature (#9)", &[("s.txt", "s\n"), ("z.txt", "z\n")]);
    repo.commit(
        "chore: staging top edit",
        &[("lib.txt", &lib.replace("l1\n", "L1\n"))],
    );
    repo.git(&["cherry-pick", &picked]);
    let fake = tempfile::tempdir().unwrap();
    let bin = fake.path().join("git-cliff");
    fs::write(&bin, FAKE).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    Fixture {
        repo,
        fake,
        shipped,
        picked,
        merge,
        empty,
        new,
    }
}

impl Fixture {
    fn run(&self, args: &[&str], fail: bool) -> std::process::Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_prdiff"));
        cmd.arg("-C")
            .arg(self.repo.path())
            .args(["notes", "--base", "staging", "--head", "develop"])
            .args(args)
            .env("PRDIFF_GIT_CLIFF", self.fake.path().join("git-cliff"))
            .env("FAKE", self.fake.path());
        if fail {
            cmd.env("FAKE_FAIL", "1");
        }
        cmd.output().unwrap()
    }
    fn call(&self) -> Vec<String> {
        fs::read_to_string(self.fake.path().join("call.txt"))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }
}

#[test]
fn git_cliff_gets_the_range_and_skips_everything_that_is_not_new() {
    let f = setup();
    let out = f.run(&["--renderer", "git-cliff"], false);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "## fake changelog"
    );

    let call = f.call();
    assert!(
        call.contains(&"arg=--no-exec".to_string()),
        "never run commands from a repo's cliff.toml: {call:?}"
    );
    let range = call
        .iter()
        .find(|l| l.starts_with("arg=") && l.contains(".."))
        .expect("a range argument");
    let (base, head) = (
        f.repo.git(&["rev-parse", "staging"]),
        f.repo.git(&["rev-parse", "develop"]),
    );
    assert_eq!(range, &format!("arg={base}..{head}"));
    assert!(
        call[0].ends_with(f.repo.path().file_name().unwrap().to_str().unwrap())
            || call[0].starts_with("cwd="),
        "{call:?}"
    );

    let skipped: Vec<&str> = call
        .windows(2)
        .filter(|w| w[0] == "arg=--skip-commit")
        .map(|w| w[1].trim_start_matches("arg="))
        .collect();
    for (what, sha) in [
        ("shipped", &f.shipped),
        ("cherry-picked", &f.picked),
        ("merge", &f.merge),
        ("empty", &f.empty),
    ] {
        assert!(
            skipped.contains(&sha.as_str()),
            "{what} commit must be skipped: {skipped:?}"
        );
    }
    assert!(
        !skipped.contains(&f.new.as_str()),
        "a new commit must not be skipped"
    );
}

#[test]
fn config_and_title_are_passed_through() {
    let f = setup();
    let cfg = f.repo.path().join("my-cliff.toml");
    fs::write(&cfg, "# empty\n").unwrap();
    let out = f.run(
        &[
            "--renderer",
            "git-cliff",
            "--cliff-config",
            cfg.to_str().unwrap(),
            "--title",
            "v1.2.3",
        ],
        false,
    );
    assert_eq!(out.status.code(), Some(0));
    let call = f.call();
    let pair = |flag: &str| {
        call.windows(2)
            .find(|w| w[0] == format!("arg={flag}"))
            .map(|w| w[1].clone())
    };
    assert_eq!(pair("--config"), Some(format!("arg={}", cfg.display())));
    assert_eq!(pair("--tag"), Some("arg=v1.2.3".to_string()));
}

#[test]
fn failures_are_reported_with_the_right_exit_codes() {
    let f = setup();
    let bad = f.run(&["--renderer", "git-cliff"], true);
    assert_eq!(bad.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("boom: bad config"));

    let missing = Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(f.repo.path())
        .args([
            "notes",
            "--base",
            "staging",
            "--head",
            "develop",
            "--renderer",
            "git-cliff",
        ])
        .env("PRDIFF_GIT_CLIFF", "/nonexistent/git-cliff")
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("install git-cliff"));

    let json = f.run(&["--renderer", "git-cliff", "--format", "json"], false);
    assert_eq!(json.status.code(), Some(2));
}

#[test]
fn builtin_renderer_is_still_the_default() {
    let f = setup();
    let out = f.run(&[], false);
    assert_eq!(out.status.code(), Some(0));
    let md = String::from_utf8_lossy(&out.stdout);
    assert!(
        md.contains("Release notes") && md.contains("still new"),
        "{md}"
    );
    assert!(
        !f.fake.path().join("call.txt").exists(),
        "git-cliff is not run unless asked for"
    );
}

/// Only runs where the real tool is installed (not on CI): the notes contain exactly the new commits.
#[test]
fn real_git_cliff_formats_only_the_new_commits() {
    let Ok(path) = std::env::var("PATH") else {
        return;
    };
    let Some(real) = path
        .split(':')
        .map(|d| std::path::Path::new(d).join("git-cliff"))
        .find(|p| p.is_file())
    else {
        eprintln!("git-cliff not installed; skipping");
        return;
    };
    let f = setup();
    let out = Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(f.repo.path())
        .args([
            "notes",
            "--base",
            "staging",
            "--head",
            "develop",
            "--renderer",
            "git-cliff",
        ])
        .env("PRDIFF_GIT_CLIFF", real)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let md = String::from_utf8_lossy(&out.stdout);
    assert!(md.contains("Still new"), "{md}");
    assert!(
        md.contains("Side work"),
        "the commit behind the merge is new: {md}"
    );
    assert!(!md.contains("Squashed on staging"), "{md}");
    assert!(!md.contains("Backported"), "{md}");
    assert!(!md.to_lowercase().contains("merge branch"), "{md}");
}
