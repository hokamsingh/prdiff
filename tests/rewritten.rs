mod common;
use common::*;
use prdiff::rewrite::parse_range_diff;
use std::process::Command;

fn bin(r: &Repo, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(r.path())
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn range_diff_output_is_parsed() {
    let text = "1:  aaa1111 = 1:  bbb2222 feat: same\n\
                2:  ccc3333 ! 2:  ddd4444 fix: amended\n    @@ Metadata\n    ## Commit message ##\n    -old\n    +new\n\
                3:  eee5555 < -:  ------- chore: dropped\n\
                -:  ------- > 3:  fff6666 feat: added\n";
    let p = parse_range_diff(text);
    assert_eq!(p.len(), 4);
    assert_eq!(p[0].subject, "feat: same");
    assert_eq!(
        p[1].interdiff,
        vec!["@@ Metadata", "## Commit message ##", "-old", "+new"]
    );
    assert_eq!(
        (p[2].old_sha.as_deref(), p[2].new_sha.as_deref()),
        (Some("eee5555"), None)
    );
    assert_eq!((p[3].old_index, p[3].new_index), (None, Some(3)));
}

#[test]
fn a_force_pushed_branch_shows_what_was_amended_dropped_and_added() {
    let body: String = (1..=10).map(|i| format!("l{i}\n")).collect();
    let r = Repo::with_branches(&[("lib.txt", &body)]);
    r.checkout("develop");
    let one: String = (1..=20).map(|i| format!("one {i}\n")).collect();
    r.commit("feat: one", &[("one.txt", &one)]);
    r.commit("feat: two", &[("two.txt", "2\n")]);
    r.commit("feat: three", &[("three.txt", "3\n")]);
    let old = r.git(&["rev-parse", "HEAD"]);
    let three = r.git(&["rev-parse", "HEAD"]);

    // rewrite: amend one, drop two, keep three as is, add four
    r.git(&["reset", "-q", "--hard", "HEAD~3"]);
    r.commit(
        "feat: one",
        &[("one.txt", &format!("{one}plus a review fix\n"))],
    );
    r.git(&["cherry-pick", &three]);
    r.commit("feat: four", &[("four.txt", "4\n")]);

    let out = bin(
        &r,
        &[
            "rewritten",
            "--base",
            "staging",
            "--head",
            "develop",
            "--old",
            &old,
            "--format",
            "json",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let status = |s: &str| {
        v["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["subject"] == s)
            .unwrap()["status"]
            .clone()
    };
    assert_eq!(status("feat: one"), "CHANGED");
    assert_eq!(status("feat: two"), "DROPPED");
    assert_eq!(status("feat: three"), "SAME");
    assert_eq!(status("feat: four"), "ADDED");
    let one = v["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["subject"] == "feat: one")
        .unwrap();
    assert!(
        one["interdiff"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l.as_str().unwrap().contains("plus a review fix"))
    );

    let text = String::from_utf8(
        bin(
            &r,
            &[
                "rewritten",
                "--base",
                "staging",
                "--head",
                "develop",
                "--old",
                &old,
            ],
        )
        .stdout,
    )
    .unwrap();
    assert!(
        text.contains("1 unchanged, 1 amended, 1 dropped, 1 added"),
        "{text}"
    );
}

#[test]
fn old_defaults_to_the_previous_reflog_entry_and_errors_are_bad_input() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: x", &[("x.txt", "x\n")]);
    // no previous version recorded for `staging@{1}`-style lookups on a fresh ref
    let out = bin(
        &r,
        &["rewritten", "--base", "staging", "--head", "nonexistent"],
    );
    assert_eq!(out.status.code(), Some(2));
    r.git(&["commit", "--amend", "-q", "-m", "feat: x again"]);
    let out = bin(&r, &["rewritten", "--base", "staging", "--head", "develop"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("amended"));
}

#[test]
fn view_gets_a_rewritten_tab_with_old() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: x", &[("x.txt", "x\n")]);
    let old = r.git(&["rev-parse", "HEAD"]);
    r.git(&["commit", "--amend", "-q", "-m", "feat: x (reworded)"]);
    let out = bin(
        &r,
        &[
            "view", "--base", "staging", "--head", "develop", "--old", &old, "--out", "-",
        ],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = String::from_utf8(out.stdout).unwrap();
    assert!(html.contains("\"rewritten\":{"));
}
