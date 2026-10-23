mod common;
use common::*;
use prdiff::render::notes::repo_url_from_remote;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Command, Stdio};

fn bin(r: &Repo, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(r.path())
        .args(args)
        .output()
        .unwrap()
}

fn stdout(out: std::process::Output) -> String {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn notes_repo() -> Repo {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit(
        "feat(api): add search endpoint (#101) PROJ-7",
        &[("api.txt", "1\n")],
    );
    r.commit("feat(ui): dark mode (#102)", &[("ui.txt", "1\n")]);
    r.commit("fix(api)!: drop v1 routes (#103)", &[("v1.txt", "1\n")]);
    r.commit("fix: typo", &[("typo.txt", "1\n")]);
    r.commit("chore: bump deps", &[("deps.txt", "1\n")]);
    r.git(&[
        "commit",
        "--allow-empty",
        "-q",
        "-m",
        "docs: x\n\nBREAKING CHANGE: nothing",
    ]);
    r
}

#[test]
fn notes_group_by_kind_with_breaking_first_and_links() {
    let r = notes_repo();
    let md = stdout(bin(
        &r,
        &[
            "notes",
            "--base",
            "staging",
            "--head",
            "develop",
            "--repo-url",
            "https://github.com/acme/app",
            "--ticket-url",
            "https://jira.example.com/browse/{id}",
        ],
    ));
    let breaking = md.find("### ⚠ Breaking changes").expect(&md);
    let feats = md.find("### Features").expect(&md);
    assert!(breaking < feats, "breaking changes come first:\n{md}");
    assert!(
        md.contains("- **api:** drop v1 routes ([#103](https://github.com/acme/app/pull/103))"),
        "{md}"
    );
    assert!(md.contains("- **api:** add search endpoint ([#101](https://github.com/acme/app/pull/101)) [PROJ-7](https://jira.example.com/browse/PROJ-7)"), "{md}");
    assert!(md.contains("### Fixes") && md.contains("- typo ("), "{md}");
    assert!(md.contains("### Chores"), "{md}");
    // the empty BREAKING CHANGE commit is skipped like any empty commit
    assert!(!md.contains("nothing"), "{md}");

    let json: serde_json::Value = serde_json::from_str(&stdout(bin(
        &r,
        &[
            "notes", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    )))
    .unwrap();
    assert_eq!(json["groups"][0]["name"], "Breaking changes");
}

#[test]
fn notes_say_so_when_nothing_is_new() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    let md = stdout(bin(
        &r,
        &["notes", "--base", "staging", "--head", "develop"],
    ));
    assert!(md.contains("Nothing new"), "{md}");
}

#[test]
fn remotes_become_repo_urls() {
    for (remote, want) in [
        ("git@github.com:acme/app.git", "https://github.com/acme/app"),
        (
            "https://github.com/acme/app.git",
            "https://github.com/acme/app",
        ),
        (
            "ssh://git@gitlab.com/acme/app.git",
            "https://gitlab.com/acme/app",
        ),
        (
            "https://github.com/acme/app/",
            "https://github.com/acme/app",
        ),
    ] {
        assert_eq!(
            repo_url_from_remote(remote).as_deref(),
            Some(want),
            "{remote}"
        );
    }
    assert_eq!(repo_url_from_remote("/local/path/repo"), None);
}

#[test]
fn slack_format_escapes_and_uses_mrkdwn() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: x <b> & y", &[("x.txt", "x\n")]);
    let out = stdout(bin(
        &r,
        &[
            "describe", "--base", "staging", "--head", "develop", "--format", "slack", "--title",
            "Rel 1",
        ],
    ));
    assert!(out.starts_with("*Rel 1*\n"), "{out}");
    assert!(out.contains("• Commits not yet on base: *1*"), "{out}");
    assert!(out.contains("feat: x &lt;b&gt; &amp; y"), "{out}");
    assert!(!out.contains("## "), "no Markdown headings in Slack output");
}

#[test]
fn serve_renders_on_request_and_reports_when_refs_move() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: first", &[("x.txt", "x\n")]);

    // never leave a server behind, even when an assertion fails
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Guard(
        Command::new(env!("CARGO_BIN_EXE_prdiff"))
            .arg("-C")
            .arg(r.path())
            .args([
                "view", "--base", "staging", "--head", "develop", "--serve", "--port", "0",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut err = BufReader::new(child.0.stderr.take().unwrap());
    let mut line = String::new();
    err.read_line(&mut line).unwrap();
    let addr = line
        .split("http://")
        .nth(1)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string();

    let get = |path: &str| -> String {
        let mut s = TcpStream::connect(&addr).unwrap();
        s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let page = get("/");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    assert!(page.contains("feat: first") && page.contains("fetch('/__sig'"));
    let sig1 = get("/__sig");
    assert_eq!(sig1, get("/__sig"), "stable while nothing moves");

    r.commit("feat: second", &[("y.txt", "y\n")]);
    assert_ne!(sig1, get("/__sig"), "the signature changes when head moves");
    assert!(
        get("/").contains("feat: second"),
        "the page is rebuilt on every request"
    );
    assert!(get("/nope").starts_with("HTTP/1.1 404"));
}
