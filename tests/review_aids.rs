mod common;
use common::*;
use prdiff::codeowners::Owners;
use prdiff::risk::RiskRules;

fn file<'a>(r: &'a prdiff::report::Report, p: &str) -> &'a prdiff::report::NetFile {
    r.net_files
        .iter()
        .find(|f| f.path == p)
        .unwrap_or_else(|| panic!("no {p}"))
}

#[test]
fn risk_flags_fire_on_the_usual_suspects() {
    let rules = RiskRules::new(&["secret-stuff/**".into()]).unwrap();
    let f = |p: &str| rules.flags(p, Some(10), 1, 1);
    assert_eq!(f("db/migrations/001_init.sql"), vec!["migration"]);
    assert_eq!(f("web/package-lock.json"), vec!["lockfile"]);
    assert_eq!(f("package.json"), vec!["dependencies"]);
    assert_eq!(f(".github/workflows/ci.yml"), vec!["deploy-config"]);
    assert_eq!(f("deploy/prod.env"), vec!["deploy-config"]);
    assert_eq!(f("server.key"), vec!["secrets"]);
    assert_eq!(f("secret-stuff/a.txt"), vec!["custom"]);
    assert!(f("src/app.rs").is_empty());
    assert_eq!(
        rules.flags("src/a.rs", Some(900), 6, 3),
        vec!["large-diff", "hot-file", "many-authors"]
    );
    assert!(
        rules.flags("img.png", None, 1, 1).is_empty(),
        "binary files have no line count"
    );
}

#[test]
fn risk_flags_appear_on_net_files_and_in_totals() {
    let r = Repo::with_branches(&[("src/hot.rs", "0\n"), ("README.md", "hi\n")]);
    r.checkout("develop");
    for i in 1..=5 {
        r.commit(
            &format!("fix: hot {i}"),
            &[("src/hot.rs", &format!("{i}\n"))],
        );
    }
    r.commit(
        "feat: schema",
        &[("db/migrations/001.sql", "create table t();\n")],
    );
    let big: String = (0..600).map(|i| format!("line {i}\n")).collect();
    r.commit("feat: big", &[("src/big.rs", &big)]);

    let rep = run(&r, "staging", "develop");
    assert_eq!(file(&rep, "src/hot.rs").risk, vec!["hot-file"]);
    assert_eq!(file(&rep, "db/migrations/001.sql").risk, vec!["migration"]);
    assert_eq!(file(&rep, "src/big.rs").risk, vec!["large-diff"]);
    assert_eq!(rep.totals.risky_files, 3);
    assert_totals_consistent(&rep);
}

#[test]
fn many_authors_flag_counts_distinct_authors() {
    let r = Repo::with_branches(&[("shared.rs", "0\n")]);
    r.checkout("develop");
    for (i, who) in ["Ann", "Bob", "Cy"].iter().enumerate() {
        r.write("shared.rs", &format!("v{i}\n"));
        r.git(&["add", "-A"]);
        r.git(&[
            "commit",
            "-q",
            "-m",
            &format!("fix: by {who}"),
            "--author",
            &format!("{who} <{who}@x.io>"),
        ]);
    }
    let rep = run(&r, "staging", "develop");
    assert_eq!(file(&rep, "shared.rs").risk, vec!["many-authors"]);
}

#[test]
fn codeowners_semantics() {
    let o = Owners::parse(
        "# comment\n* @everyone\n*.js @frontend\n/docs/ @docs-team @writers\napps/web/ @web\n/Makefile @build # trailing comment\n",
    );
    assert_eq!(o.of("README.md"), vec!["@everyone"]);
    assert_eq!(o.of("src/deep/app.js"), vec!["@frontend"]);
    assert_eq!(o.of("docs/guide/a.md"), vec!["@docs-team", "@writers"]);
    assert_eq!(o.of("apps/web/src/x.ts"), vec!["@web"]);
    assert_eq!(
        o.of("other/apps/web/x.ts"),
        vec!["@everyone"],
        "an inner slash anchors the pattern"
    );
    assert_eq!(o.of("Makefile"), vec!["@build"]);
    assert_eq!(
        o.of("sub/Makefile"),
        vec!["@everyone"],
        "a leading slash anchors to the root"
    );
}

#[test]
fn owners_come_from_the_target_branch() {
    let r = Repo::with_branches(&[("README.md", "hi\n")]);
    r.checkout("staging");
    r.commit(
        "chore: owners",
        &[(".github/CODEOWNERS", "*.rs @core\n/web/ @web-team\n")],
    );
    r.checkout("develop");
    r.commit(
        "feat: stuff",
        &[
            ("src/a.rs", "a\n"),
            ("src/b.rs", "b\n"),
            ("web/index.js", "i\n"),
            ("notes.txt", "n\n"),
        ],
    );
    let rep = run(&r, "staging", "develop");
    assert_eq!(file(&rep, "src/a.rs").owners, vec!["@core"]);
    assert_eq!(file(&rep, "web/index.js").owners, vec!["@web-team"]);
    assert!(file(&rep, "notes.txt").owners.is_empty());
    let sum: Vec<_> = rep
        .owners_summary
        .iter()
        .map(|o| (o.owner.as_str(), o.files))
        .collect();
    assert_eq!(sum, vec![("@core", 2), ("@web-team", 1)]);

    let off = run_with(&r, "staging", "develop", |o| o.owners = false);
    assert!(off.owners_summary.is_empty());
}

#[test]
fn conflicts_say_who_touched_the_file_on_each_side() {
    let r = Repo::with_branches(&[("c.txt", "base\n")]);
    r.checkout("develop");
    let mine = r.commit("fix: my side", &[("c.txt", "develop\n")]);
    r.checkout("staging");
    r.git(&["commit", "--allow-empty", "-q", "-m", "chore: unrelated"]);
    r.write("c.txt", "staging\n");
    r.git(&["add", "-A"]);
    r.git(&[
        "commit",
        "-q",
        "-m",
        "fix: their side",
        "--author",
        "Dana <dana@x.io>",
    ]);

    let rep = run(&r, "staging", "develop");
    assert_eq!(rep.conflicts, vec!["c.txt"]);
    let d = &rep.conflict_details[0];
    assert_eq!(d.path, "c.txt");
    assert_eq!(d.head_commits, vec![mine]);
    assert_eq!(
        d.base_commits.len(),
        1,
        "only commits that touched the file"
    );
    assert_eq!(d.base_commits[0].subject, "fix: their side");
    assert_eq!(d.base_commits[0].author, "Dana");
}
