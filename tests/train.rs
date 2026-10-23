mod common;
use common::*;
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

/// production = init; staging has A merged, D squashed, E cherry-picked; develop has A..E.
fn train_repo() -> Repo {
    let lib: String = (1..=12).map(|i| format!("l{i}\n")).collect();
    let r = Repo::new();
    r.commit("chore: init", &[("lib.txt", &lib)]);
    r.git(&["branch", "production"]);
    r.git(&["checkout", "-q", "-b", "develop"]);
    r.commit("feat: a", &[("a.txt", "a\n")]);
    r.git(&["branch", "staging"]);
    r.commit("feat: b", &[("b.txt", "b\n")]);
    r.commit("feat: c (#12)", &[("c.txt", "c\n")]);
    r.commit("feat: d", &[("d.txt", "d\n")]);
    r.commit("fix: e", &[("lib.txt", &lib.replace("l12\n", "L12\n"))]);
    let e = r.git(&["rev-parse", "HEAD"]);
    r.checkout("staging");
    r.commit("Squashed: d", &[("d.txt", "d\n"), ("z.txt", "z\n")]);
    r.commit(
        "chore: staging touches the top",
        &[("lib.txt", &lib.replace("l1\n", "L1\n"))],
    );
    r.git(&["cherry-pick", &e]);
    r
}

const ENVS: [&str; 4] = ["--env", "production=production", "--env", "staging=staging"];

#[test]
fn matrix_shows_each_commit_per_environment() {
    let r = train_repo();
    let mut args = vec!["matrix", "--head", "develop", "--format", "json"];
    args.extend(ENVS);
    let v = json(bin(&r, &args));
    let envs: Vec<_> = v["envs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| (e["name"].as_str().unwrap(), e["missing"].as_u64().unwrap()))
        .collect();
    assert_eq!(envs, vec![("production", 5), ("staging", 2)]);
    let state = |subject: &str| -> Vec<String> {
        let row = v["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["subject"] == subject)
            .unwrap();
        row["states"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(state("feat: a"), vec!["NOT_PRESENT", "MERGED"]);
    assert_eq!(state("feat: b"), vec!["NOT_PRESENT", "NOT_PRESENT"]);
    assert_eq!(state("feat: d"), vec!["NOT_PRESENT", "SHIPPED"]);
    assert_eq!(state("fix: e"), vec!["NOT_PRESENT", "PATCH_EQUIVALENT"]);

    let mut args = vec!["matrix", "--head", "develop"];
    args.extend(ENVS);
    let text = String::from_utf8(bin(&r, &args).stdout).unwrap();
    assert!(text.contains("missing: production 5 staging 2"), "{text}");
}

#[test]
fn where_locates_a_commit_by_sha_or_pr_number() {
    let r = train_repo();
    for rev in ["#12", "PR12"] {
        let mut args = vec!["where", rev, "--head", "develop", "--format", "json"];
        args.extend(ENVS);
        let v = json(bin(&r, &args));
        assert_eq!(v["subject"], "feat: c (#12)");
        assert_eq!(v["envs"][0]["state"], "NOT_PRESENT");
        assert_eq!(v["envs"][1]["state"], "NOT_PRESENT");
    }
    let a = r.git(&["rev-parse", "develop~4"]);
    let mut args = vec!["where", &a, "--head", "develop"];
    args.extend(ENVS);
    let text = String::from_utf8(bin(&r, &args).stdout).unwrap();
    assert!(text.contains("✗ production"), "{text}");
    assert!(
        text.contains("✓ staging") && text.contains("merged"),
        "{text}"
    );
}

#[test]
fn environments_come_from_the_config_and_errors_are_bad_input() {
    let r = train_repo();
    r.write(
        "prdiff.toml",
        "[[environments]]\nname = \"production\"\nref = \"production\"\n[[environments]]\nname = \"staging\"\nref = \"staging\"\n",
    );
    let v = json(bin(
        &r,
        &["matrix", "--head", "develop", "--format", "json"],
    ));
    assert_eq!(v["envs"].as_array().unwrap().len(), 2);

    let bare = Repo::with_branches(&[("a.txt", "a\n")]);
    assert_eq!(
        bin(&bare, &["matrix", "--head", "develop"]).status.code(),
        Some(2)
    );
    assert_eq!(
        bin(&bare, &["matrix", "--head", "develop", "--env", "nonsense"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn view_gets_a_train_tab_when_environments_are_given() {
    let r = train_repo();
    let mut args = vec![
        "view",
        "--head",
        "develop",
        "--base",
        "production",
        "--out",
        "-",
    ];
    args.extend(ENVS);
    let out = bin(&r, &args);
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
    let v: serde_json::Value = serde_json::from_str(&html[start..end]).unwrap();
    assert_eq!(v["train"]["rows"].as_array().unwrap().len(), 5);
}
