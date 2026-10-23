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

#[test]
fn view_embeds_the_report_and_cannot_be_broken_by_commit_text() {
    let r = Repo::with_branches(&[("c.txt", "base\n")]);
    r.checkout("develop");
    r.commit(
        "feat: </script><img src=x onerror=alert(1)> & <!-- trick",
        &[("n.txt", "one\ntwo\n"), ("c.txt", "develop\n")],
    );
    r.checkout("staging");
    r.commit("fix: base side", &[("c.txt", "staging\n")]);

    let out = bin(
        &r,
        &[
            "view", "--base", "staging", "--head", "develop", "--out", "-",
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    let html = String::from_utf8(out.stdout).unwrap();
    let start = html
        .find("<script id=\"data\" type=\"application/json\">")
        .unwrap()
        + "<script id=\"data\" type=\"application/json\">".len();
    let end = start + html[start..].find("</script>").unwrap();
    let data = &html[start..end];
    assert!(!data.contains('<'), "no raw `<` inside the embedded JSON");
    let v: serde_json::Value = serde_json::from_str(data).unwrap();
    assert_eq!(
        v["commits"][0]["subject"],
        "feat: </script><img src=x onerror=alert(1)> & <!-- trick"
    );
    // hunks are included for display, and the conflict is reported
    let files = v["commits"][0]["files"].as_array().unwrap();
    assert!(
        files
            .iter()
            .all(|f| !f["hunk_details"].as_array().unwrap().is_empty())
    );
    assert_eq!(v["conflicts"][0], "c.txt");
}

#[test]
fn view_writes_a_file_and_leaves_compare_output_unchanged() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: x", &[("x.txt", "x\n")]);
    let before = bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    );
    let path = r.path().join("report.html");
    let out = bin(
        &r,
        &[
            "view",
            "--base",
            "staging",
            "--head",
            "develop",
            "--out",
            path.to_str().unwrap(),
        ],
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("<!doctype html>")
    );
    let after = bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    );
    assert_eq!(
        before.stdout, after.stdout,
        "hunk_details only appear in view mode"
    );
}

fn view_json(r: &Repo, extra: &[&str]) -> serde_json::Value {
    let mut args = vec![
        "view", "--base", "staging", "--head", "develop", "--out", "-",
    ];
    args.extend_from_slice(extra);
    let out = bin(r, &args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let html = String::from_utf8(out.stdout).unwrap();
    let open = "<script id=\"data\" type=\"application/json\">";
    let start = html.find(open).unwrap() + open.len();
    let end = start + html[start..].find("</script>").unwrap();
    serde_json::from_str(&html[start..end]).unwrap()
}

#[test]
fn side_by_side_has_the_targets_current_file_and_the_merging_branchs_file() {
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let r = Repo::with_branches(&[
        ("index.js", &body),
        ("old.js", "a\nb\nc\nd\ne\nf\ng\nh\n"),
        ("gone.js", "bye\n"),
        ("img.bin", "x"),
    ]);
    r.write_bytes("img.bin", &[0, 1, 2]);
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "chore: bin"]);
    r.git(&["branch", "-f", "staging", "HEAD"]);
    r.git(&["branch", "-f", "develop", "HEAD"]);

    r.checkout("develop");
    r.write("index.js", &body.replace("line 5\n", "line five\n"));
    r.write("new.ts", "export const x = 1;\n");
    r.write_bytes("img.bin", &[0, 1, 3]);
    r.git(&["mv", "old.js", "renamed.js"]);
    r.git(&["rm", "-q", "gone.js"]);
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "fix: pr work"]);
    // the target moved on after the branch was cut
    r.checkout("staging");
    r.commit(
        "chore: target moves",
        &[("index.js", &body.replace("line 15\n", "line fifteen\n"))],
    );

    let v = view_json(&r, &[]);
    let sides = v["file_sides"].as_array().unwrap();
    let by = |p: &str| {
        sides
            .iter()
            .find(|s| s["path"] == p)
            .unwrap_or_else(|| panic!("no sides for {p}"))
    };

    let idx = by("index.js");
    // left is the target's *current* content, so it has the target's own later edit
    assert!(idx["base_text"].as_str().unwrap().contains("line fifteen"));
    assert!(idx["head_text"].as_str().unwrap().contains("line five"));
    // ops cover exactly both files
    let (mut old_n, mut new_n) = (0, 0);
    for op in idx["ops"].as_array().unwrap() {
        let op: Vec<u64> = op
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        if op[0] != 2 {
            old_n += op[2];
        }
        if op[0] != 1 {
            new_n += op[4];
        }
    }
    assert_eq!((old_n, new_n), (20, 20));

    assert!(
        by("new.ts")["base_text"].is_null(),
        "added file: nothing on the left"
    );
    assert!(
        by("gone.js")["head_text"].is_null(),
        "deleted file: nothing on the right"
    );
    let ren = by("renamed.js");
    assert_eq!(ren["old_path"], "old.js");
    assert!(
        ren["base_text"].as_str().unwrap().starts_with("a\nb"),
        "left is the file under its old name"
    );
    assert!(
        sides.iter().all(|s| s["path"] != "img.bin"),
        "binary files get no viewer"
    );

    // the compare JSON is unaffected, and --no-sides keeps the report lean
    assert!(view_json(&r, &["--no-sides"]).get("file_sides").is_none());
    let cmp = bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    );
    assert!(!String::from_utf8_lossy(&cmp.stdout).contains("file_sides"));
}

#[test]
fn oversized_files_get_a_note_instead_of_a_viewer() {
    let r = Repo::with_branches(&[("big.txt", "x\n")]);
    r.checkout("develop");
    let big: String = "0123456789abcdef\n".repeat(40_000); // ~680 KB
    r.commit("feat: big", &[("big.txt", &big)]);
    let v = view_json(&r, &[]);
    let s = &v["file_sides"][0];
    assert_eq!(s["path"], "big.txt");
    assert!(s["note"].as_str().unwrap().contains("too large"));
    assert!(s["head_text"].is_null());
}

fn covers(ranges: &serde_json::Value, line: u64) -> bool {
    ranges.as_array().unwrap().iter().any(|r| {
        let (s, n) = (r[0].as_u64().unwrap(), r[1].as_u64().unwrap());
        (s..s + n).contains(&line)
    })
}

#[test]
fn side_by_side_markers_tell_target_only_changes_from_the_prs_own() {
    let body: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let r = Repo::with_branches(&[("index.js", &body), ("also.js", &body)]);
    r.checkout("develop");
    r.write("index.js", &body.replace("line 5\n", "line five\n"));
    r.write("also.js", &body.replace("line 8\n", "line PR\n"));
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "fix: pr work"]);
    r.checkout("staging");
    r.write("index.js", &body.replace("line 15\n", "line fifteen\n"));
    r.write("also.js", &body.replace("line 8\n", "line TARGET\n"));
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "chore: target moves"]);

    let v = view_json(&r, &[]);
    let sides = v["file_sides"].as_array().unwrap();
    let by = |p: &str| sides.iter().find(|s| s["path"] == p).unwrap();

    let idx = by("index.js");
    assert_eq!(idx["three_way"], true);
    assert!(
        covers(&idx["target_changed"], 14),
        "target edited line 15 (index 14)"
    );
    assert!(!covers(&idx["target_changed"], 4));
    assert!(
        covers(&idx["pr_changed"], 4),
        "the PR edited line 5 (index 4)"
    );
    assert!(!covers(&idx["pr_changed"], 14));

    // both edited line 8: it is flagged on both sides
    let also = by("also.js");
    assert!(covers(&also["target_changed"], 7) && covers(&also["pr_changed"], 7));
}

#[test]
fn a_file_added_by_the_pr_is_all_pr_change() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: add", &[("new.js", "one\ntwo\n")]);
    let v = view_json(&r, &[]);
    let s = &v["file_sides"][0];
    assert_eq!(s["three_way"], true);
    assert!(covers(&s["pr_changed"], 0) && covers(&s["pr_changed"], 1));
    assert!(s["target_changed"].as_array().unwrap().is_empty());
}
