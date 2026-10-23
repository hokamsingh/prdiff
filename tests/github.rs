#![cfg(unix)]
//! GitHub integration against a fake `gh` and a local bare "origin".
mod common;
use common::*;
use prdiff::github::{BODY_END, BODY_START, COMMENT_MARK, merge_body};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

const FAKE_GH: &str = r#"#!/bin/bash
echo "$@" >> "$FAKE/calls.log"
case "$1 $2" in
  "pr view") cat "$FAKE/pr.json" ;;
  "pr edit") cat > "$FAKE/edited_body.txt" ;;
  "pr comment") cat > "$FAKE/new_comment.txt" ;;
  "api -X") cat > "$FAKE/patched_comment.txt"; echo "$4" > "$FAKE/patched_path" ;;
  api*) [ -f "$FAKE/comment_id" ] && cat "$FAKE/comment_id" ;;
esac
exit 0
"#;

struct Env {
    repo: Repo,
    fake: tempfile::TempDir,
    _origin: tempfile::TempDir,
    gh: PathBuf,
}

fn setup(pr_body: &str) -> Env {
    let repo = Repo::with_branches(&[("a.txt", "a\n")]);
    repo.checkout("develop");
    repo.commit("feat: from the pr", &[("x.txt", "x\n")]);
    let origin = tempfile::tempdir().unwrap();
    Command::new("git")
        .args(["init", "-q", "--bare"])
        .arg(origin.path())
        .status()
        .unwrap();
    repo.git(&["remote", "add", "origin", origin.path().to_str().unwrap()]);
    repo.git(&["push", "-q", "origin", "staging"]);
    repo.git(&["push", "-q", "origin", "develop:refs/pull/7/head"]);

    let fake = tempfile::tempdir().unwrap();
    let gh = fake.path().join("gh");
    fs::write(&gh, FAKE_GH).unwrap();
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o755)).unwrap();
    let pr =
        serde_json::json!({"baseRefName": "staging", "headRefName": "feature/x", "body": pr_body});
    fs::write(fake.path().join("pr.json"), pr.to_string()).unwrap();
    Env {
        repo,
        fake,
        _origin: origin,
        gh,
    }
}

impl Env {
    fn run(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_prdiff"))
            .arg("-C")
            .arg(self.repo.path())
            .args(args)
            .env("PRDIFF_GH", &self.gh)
            .env("FAKE", self.fake.path())
            .output()
            .unwrap()
    }
    fn read(&self, f: &str) -> String {
        fs::read_to_string(self.fake.path().join(f)).unwrap_or_default()
    }
}

#[test]
fn pr_resolves_to_its_base_and_head() {
    let e = setup("My description");
    let out = e.run(&["compare", "--pr", "7", "--format", "json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["base"]["name"], "origin/staging");
    assert_eq!(
        v["head"]["name"], "feature/x",
        "the PR's branch name, not refs/prdiff/pr/7"
    );
    assert_eq!(v["commits"][0]["subject"], "feat: from the pr");
}

#[test]
fn publish_body_appends_then_replaces_its_own_block() {
    let e = setup("My description");
    let dry = e.run(&["publish", "--pr", "7", "--dry-run"]);
    let text = String::from_utf8(dry.stdout).unwrap();
    assert!(
        text.starts_with("My description\n\n<!-- prdiff:start -->"),
        "{text}"
    );
    assert!(
        e.read("calls.log")
            .lines()
            .all(|l| l.starts_with("pr view")),
        "dry run only reads"
    );

    assert_eq!(e.run(&["publish", "--pr", "7"]).status.code(), Some(0));
    let first = e.read("edited_body.txt");
    assert!(
        first.starts_with("My description") && first.contains("feat: from the pr"),
        "{first}"
    );

    // the PR now has our block in its body; publishing again replaces it in place
    let pr =
        serde_json::json!({"baseRefName": "staging", "headRefName": "feature/x", "body": first});
    fs::write(e.fake.path().join("pr.json"), pr.to_string()).unwrap();
    e.repo.checkout("develop");
    e.repo.commit("feat: another", &[("y.txt", "y\n")]);
    e.repo
        .git(&["push", "-q", "-f", "origin", "develop:refs/pull/7/head"]);
    assert_eq!(e.run(&["publish", "--pr", "7"]).status.code(), Some(0));
    let second = e.read("edited_body.txt");
    assert_eq!(second.matches(BODY_START).count(), 1, "{second}");
    assert!(second.starts_with("My description"));
    assert!(second.contains("feat: another"), "{second}");
}

#[test]
fn publish_comment_creates_then_updates_one_comment() {
    let e = setup("body");
    assert_eq!(
        e.run(&["publish", "--pr", "7", "--mode", "comment"])
            .status
            .code(),
        Some(0)
    );
    let created = e.read("new_comment.txt");
    assert!(
        created.starts_with(COMMENT_MARK) && created.contains("feat: from the pr"),
        "{created}"
    );

    fs::write(e.fake.path().join("comment_id"), "424242\n").unwrap();
    assert_eq!(
        e.run(&["publish", "--pr", "7", "--mode", "comment"])
            .status
            .code(),
        Some(0)
    );
    assert!(
        e.read("patched_path").contains("issues/comments/424242"),
        "{}",
        e.read("patched_path")
    );
    assert!(e.read("patched_comment.txt").starts_with(COMMENT_MARK));
}

#[test]
fn check_reads_the_prs_own_body() {
    let e = setup("placeholder");
    // a body that lists nothing fails
    assert_eq!(e.run(&["check", "--pr", "7"]).status.code(), Some(5));
    // a body generated by describe passes
    let md = String::from_utf8(e.run(&["describe", "--pr", "7"]).stdout).unwrap();
    let pr = serde_json::json!({"baseRefName": "staging", "headRefName": "feature/x", "body": md});
    fs::write(e.fake.path().join("pr.json"), pr.to_string()).unwrap();
    assert_eq!(e.run(&["check", "--pr", "7"]).status.code(), Some(0));
}

#[test]
fn missing_gh_and_bad_usage_are_bad_input() {
    let e = setup("x");
    let out = Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(e.repo.path())
        .args(["compare", "--pr", "7"])
        .env("PRDIFF_GH", "/nonexistent/gh")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("GitHub CLI"));
    assert_eq!(
        e.run(&["publish", "--base", "staging", "--head", "develop"])
            .status
            .code(),
        Some(2),
        "publish needs --pr"
    );
    assert_eq!(
        e.run(&["check", "--base", "staging", "--head", "develop"])
            .status
            .code(),
        Some(2),
        "check needs a body"
    );
}

#[test]
fn merge_body_is_idempotent() {
    let once = merge_body("Intro", "## Release\n- a");
    assert_eq!(merge_body(&once, "## Release\n- a"), once);
    let updated = merge_body(&once, "## Release\n- b");
    assert!(updated.contains("- b") && !updated.contains("- a"));
    assert!(updated.starts_with("Intro\n\n") && updated.contains(BODY_END));
    assert_eq!(merge_body("", "x"), format!("{BODY_START}\nx\n{BODY_END}"));
    let tail = merge_body(
        "before\n<!-- prdiff:start -->\nold\n<!-- prdiff:end -->\nafter",
        "new",
    );
    assert_eq!(
        tail,
        "before\n<!-- prdiff:start -->\nnew\n<!-- prdiff:end -->\nafter"
    );
}
