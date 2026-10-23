mod common;
use common::*;
use prdiff::commits::parse_meta;
use std::process::Command;

fn bin(r: &Repo, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_prdiff"))
        .arg("-C")
        .arg(r.path())
        .args(args)
        .output()
        .unwrap()
}

fn json(out: std::process::Output) -> serde_json::Value {
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn refs_default_to_the_current_branch_and_the_default_branch() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: x", &[("x.txt", "x\n")]);
    let out = bin(&r, &["compare", "--format", "json"]);
    assert!(String::from_utf8_lossy(&out.stderr).contains("--base not given, using main"));
    let v = json(out);
    assert_eq!(v["base"]["name"], "main");
    assert_eq!(v["head"]["name"], "develop");
    assert_eq!(v["commits"][0]["subject"], "feat: x");
}

#[test]
fn prdiff_toml_supplies_refs_and_excludes() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit(
        "feat: x",
        &[
            ("x.txt", "x\n"),
            ("Cargo.lock", "lock\n"),
            ("gen/out.snap", "s\n"),
        ],
    );
    r.write(
        "prdiff.toml",
        "base = \"staging\"\nhead = \"develop\"\nexclude = [\"Cargo.lock\", \"**/*.snap\"]\n",
    );
    let v = json(bin(&r, &["compare", "--format", "json"]));
    assert_eq!(v["base"]["name"], "staging");
    let files: Vec<&str> = v["net_files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        files,
        vec!["x.txt"],
        "lockfile and snapshot are left out of the net diff"
    );
    // the commit itself is still classified on all of its files
    assert_eq!(v["commits"][0]["files"].as_array().unwrap().len(), 3);
    // the command line adds to the config's excludes
    let v = json(bin(
        &r,
        &["compare", "--format", "json", "--exclude", "x.txt"],
    ));
    assert!(v["net_files"].as_array().unwrap().is_empty());
}

#[test]
fn bad_config_is_bad_input() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.write("prdiff.toml", "base = [not valid");
    assert_eq!(bin(&r, &["compare"]).status.code(), Some(2));
}

#[test]
fn ignore_whitespace_makes_a_reformat_a_non_event() {
    let r = Repo::with_branches(&[("a.js", "x=1;\ny=2;\n")]);
    r.checkout("develop");
    r.commit("style: format", &[("a.js", "x = 1;\ny = 2;\n")]);
    r.commit("feat: real", &[("b.js", "b\n")]);

    let plain = json(bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json",
        ],
    ));
    assert_eq!(plain["totals"]["by_class"]["NEW"], 2);

    let ws = json(bin(
        &r,
        &[
            "compare", "--base", "staging", "--head", "develop", "--format", "json", "-w",
        ],
    ));
    assert_eq!(ws["totals"]["by_class"]["SHIPPED"], 1, "{ws:#}");
    assert_eq!(ws["totals"]["by_class"]["NEW"], 1);
    // newer git leaves a whitespace-only file out of `diff --raw -w` altogether, older git lists it with
    // 0/0 counts; either way it must not count as something left to ship
    let a = ws["net_files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == "a.js");
    assert!(a.is_none_or(|f| f["state"] == "ALREADY_ON_BASE"), "{a:?}");
    assert_eq!(
        ws["totals"]["files_new_to_base"], 1,
        "only b.js is left to ship"
    );
}

#[test]
fn commit_metadata_is_parsed() {
    assert_eq!(
        parse_meta("feat(api): add x (#123)", ""),
        (Some("api".into()), false, Some(123), vec![])
    );
    assert!(parse_meta("fix!: drop y", "").1);
    assert!(parse_meta("chore: z", "BREAKING CHANGE: gone\n").1);
    assert_eq!(
        parse_meta("Merge pull request #45 from a/b", "").2,
        Some(45)
    );
    assert_eq!(
        parse_meta("fix(web): PROJ-12 and ABC-9 done", "").3,
        vec!["PROJ-12", "ABC-9"]
    );
    assert_eq!(parse_meta("plain subject", ""), (None, false, None, vec![]));
}
