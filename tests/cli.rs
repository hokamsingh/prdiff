mod common;
use common::*;
use std::io::Write;
use std::process::{Command, Stdio};

fn bin(r: &Repo, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(r.path())
        .args(args)
        .output()
        .unwrap()
}

fn setup() -> Repo {
    let r = Repo::with_branches(&[("c.txt", "base\n"), ("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: new thing", &[("n.txt", "n\n")]);
    r.commit("fix: conflict dev", &[("c.txt", "develop\n")]);
    r.checkout("staging");
    r.commit("fix: conflict base", &[("c.txt", "staging\n")]);
    r
}

#[test]
fn exit_codes() {
    let r = setup();
    assert_eq!(
        bin(&r, &["compare", "--base", "staging", "--head", "develop"])
            .status
            .code(),
        Some(0)
    );
    assert_eq!(
        bin(
            &r,
            &[
                "compare",
                "--base",
                "staging",
                "--head",
                "develop",
                "--fail-on-conflict"
            ]
        )
        .status
        .code(),
        Some(4)
    );
    assert_eq!(
        bin(
            &r,
            &[
                "conflicts",
                "--base",
                "staging",
                "--head",
                "develop",
                "--fail-on-conflict"
            ]
        )
        .status
        .code(),
        Some(4)
    );
    assert_eq!(
        bin(&r, &["compare", "--base", "nope", "--head", "develop"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(
        bin(&r, &["compare", "--base", "staging"]).status.code(),
        Some(2),
        "clap usage error"
    );
}

#[test]
fn json_describe_and_check() {
    let r = setup();
    let json = bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    );
    let v: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["totals"]["listed"], 2);
    assert_eq!(v["conflicts"][0], "c.txt");

    let md = bin(
        &r,
        &[
            "describe",
            "--base",
            "staging",
            "--head",
            "develop",
            "--title",
            "release: develop to staging",
        ],
    );
    let body = String::from_utf8(md.stdout).unwrap();
    assert!(body.starts_with("## release: develop to staging"));
    assert!(body.contains("Commits not yet on base: 2 (1 feat, 1 fix, 0 other)"));
    assert!(body.contains("Conflicted files: `c.txt`"));

    let path = r.path().join("body.md");
    std::fs::write(&path, &body).unwrap();
    let ok = bin(
        &r,
        &[
            "check",
            "--base",
            "staging",
            "--head",
            "develop",
            "--body",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(
        ok.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );

    let mut child = Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(r.path())
        .args([
            "check", "--base", "staging", "--head", "develop", "--body", "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"## Release\n- nothing listed\n")
        .unwrap();
    let bad = child.wait_with_output().unwrap();
    assert_eq!(bad.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("missing from description"));
}

#[test]
fn golden_json_for_a_squash_merge() {
    let r = Repo::with_branches(&[("README.md", "hi\n")]);
    r.checkout("develop");
    r.commit("feat: add a", &[("a.txt", "a\n")]);
    r.commit("feat: add b", &[("b.txt", "b\n")]);
    r.commit("fix: only on develop", &[("c.txt", "c\n")]);
    r.checkout("staging");
    r.commit("Merge feature (#12)", &[("a.txt", "a\n"), ("b.txt", "b\n")]);

    let out = bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    );
    let got = String::from_utf8(out.stdout).unwrap();
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/squash.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &got).unwrap();
    }
    let want = std::fs::read_to_string(&golden).expect("run with UPDATE_GOLDEN=1 once");
    assert_eq!(got, want);
}
