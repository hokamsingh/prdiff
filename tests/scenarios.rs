mod common;
use common::*;
use prdiff::patchid::Scope;
use prdiff::report::{Classification::*, FileStatus, NetState};

#[test]
fn squash_merge_is_shipped_despite_new_hash_and_subject() {
    let r = Repo::with_branches(&[("README.md", "hi\n")]);
    r.checkout("develop");
    r.commit("feat: add a", &[("a.txt", "a\n")]);
    r.commit("feat: add b", &[("b.txt", "b\n")]);
    r.checkout("staging");
    r.commit("Merge feature (#12)", &[("a.txt", "a\n"), ("b.txt", "b\n")]);

    let rep = run(&r, "staging", "develop");
    assert_eq!(class_of(&rep, "feat: add a"), Shipped);
    assert_eq!(class_of(&rep, "feat: add b"), Shipped);
    assert_eq!(rep.totals.listed, 0);
    assert_eq!(rep.totals.files_new_to_base, 0);
    assert!(
        rep.net_files
            .iter()
            .all(|f| f.state == NetState::AlreadyOnBase)
    );
    assert_totals_consistent(&rep);
}

#[test]
fn cherry_pick_is_patch_equivalent_even_when_blobs_differ() {
    let r = Repo::with_branches(&[("lib.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")]);
    r.checkout("develop");
    r.commit(
        "fix: touch top",
        &[("lib.txt", "ONE\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")],
    );
    let c = r.git(&["rev-parse", "HEAD"]);
    r.checkout("staging");
    // base also changed the bottom of the same file, so after the pick the blobs differ from develop's
    r.commit(
        "chore: other work",
        &[("lib.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\nTEN\n")],
    );
    r.git(&["cherry-pick", &c]);

    let rep = run(&r, "staging", "develop");
    let fix = by_subject(&rep, "fix: touch top");
    assert_eq!(fix.class, PatchEquivalent);
    assert!(fix.matched_base_commit.is_some());
    assert_eq!(rep.totals.count(PatchEquivalent), 1);
    assert_eq!(rep.totals.listed, 0);
    assert_totals_consistent(&rep);
}

#[test]
fn backported_then_merged_back_needs_the_wider_scope() {
    let r = Repo::with_branches(&[("lib.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")]);
    r.checkout("develop");
    r.commit(
        "fix: hotfix",
        &[("lib.txt", "ONE\n2\n3\n4\n5\n6\n7\n8\n9\n10\n")],
    );
    let c = r.git(&["rev-parse", "HEAD"]);
    r.commit("feat: more", &[("m.txt", "m\n")]);
    r.checkout("staging");
    r.commit(
        "chore: base moves",
        &[("lib.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\nTEN\n")],
    );
    r.git(&["cherry-pick", &c]);
    // staging merged back into develop: the cherry-pick is now reachable from head too
    r.checkout("develop");
    r.git(&["merge", "-q", "--no-edit", "staging"]);

    let narrow = run_with(&r, "staging", "develop", |o| {
        o.scope = Scope::BaseOnly;
        o.deep = false; // hunk analysis would recognise the hotfix by content; isolate the patch-id scope
    });
    assert_eq!(
        class_of(&narrow, "fix: hotfix"),
        New,
        "base-only scope cannot see the backport"
    );

    let auto = run(&r, "staging", "develop");
    assert_eq!(class_of(&auto, "fix: hotfix"), PatchEquivalent);
    assert_eq!(class_of(&auto, "feat: more"), New);
    let all = run_with(&r, "staging", "develop", |o| o.scope = Scope::All);
    assert_eq!(class_of(&all, "fix: hotfix"), PatchEquivalent);
}

#[test]
fn revert_pair_is_reported_and_nets_to_zero() {
    let r = Repo::with_branches(&[("README.md", "hi\n")]);
    r.checkout("develop");
    r.commit("feat: risky", &[("risky.txt", "x\n")]);
    r.git(&["revert", "--no-edit", "HEAD"]);

    let rep = run(&r, "staging", "develop");
    assert_eq!(rep.commits.len(), 2);
    assert_eq!(rep.revert_pairs.len(), 1);
    assert_eq!(rep.net_files.len(), 0, "net diff for the pair is empty");
    assert_eq!(rep.totals.files_new_to_base, 0);
    assert_totals_consistent(&rep);
}

#[test]
fn partial_commit_has_one_identical_file() {
    let r = Repo::with_branches(&[("README.md", "hi\n")]);
    r.checkout("develop");
    r.commit("feat: two files", &[("one.txt", "1\n"), ("two.txt", "2\n")]);
    r.checkout("staging");
    r.commit("chore: ship one early", &[("one.txt", "1\n")]);

    let rep = run(&r, "staging", "develop");
    let c = by_subject(&rep, "feat: two files");
    assert_eq!(c.class, Partial);
    let one = c.files.iter().find(|f| f.path == "one.txt").unwrap();
    let two = c.files.iter().find(|f| f.path == "two.txt").unwrap();
    assert_eq!(one.status, FileStatus::Identical);
    assert_eq!(two.status, FileStatus::Absent);
    assert_eq!(rep.totals.listed, 1);

    let strict = run_with(&r, "staging", "develop", |o| o.strict = true);
    assert_eq!(class_of(&strict, "feat: two files"), New);
}

#[test]
fn hunk_level_detects_changes_already_on_base() {
    let body: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    let r = Repo::with_branches(&[("big.txt", &body), ("other.txt", "o\n")]);
    r.checkout("develop");
    // two far-apart hunks in big.txt, plus a change to other.txt
    let edited = body
        .replace("line 3\n", "line three\n")
        .replace("line 38\n", "line thirty-eight\n");
    r.commit(
        "fix: two hunks",
        &[("big.txt", &edited), ("other.txt", "o2\n")],
    );
    r.checkout("staging");
    // base took the first hunk and the other.txt change, but not the second hunk
    let first_only = body.replace("line 3\n", "line three\n");
    r.commit(
        "chore: hunk one",
        &[("big.txt", &first_only), ("other.txt", "o2\n")],
    );

    let rep = run(&r, "staging", "develop");
    let c = by_subject(&rep, "fix: two hunks");
    assert_eq!(c.class, Partial);
    let big = c.files.iter().find(|f| f.path == "big.txt").unwrap();
    assert_eq!((big.hunks_already_on_base, big.hunks_total), (1, 2));
    assert_eq!(c.hunks_total, 3);
    assert_eq!(
        c.hunks_already_on_base, 2,
        "one of big.txt's hunks, plus all of the identical other.txt"
    );
}

#[test]
fn deep_is_the_default_and_finds_changes_that_base_later_edited() {
    let r = Repo::with_branches(&[("f.txt", "a\nb\nc\nd\ne\nf\ng\n")]);
    r.checkout("develop");
    r.commit("fix: change c", &[("f.txt", "a\nb\nC\nd\ne\nf\ng\n")]);
    r.checkout("staging");
    // base has the same change to c, and then edited another line
    r.commit("chore: c and g", &[("f.txt", "a\nb\nC\nd\ne\nf\nG\n")]);

    let shallow = run_with(&r, "staging", "develop", |o| o.deep = false);
    assert_eq!(
        class_of(&shallow, "fix: change c"),
        New,
        "blob differs, so it looks new with --shallow"
    );
    let deep = run(&r, "staging", "develop");
    assert_eq!(class_of(&deep, "fix: change c"), Shipped);
}

#[test]
fn rename_is_tracked_and_binary_files_compare_by_blob() {
    let r = Repo::with_branches(&[
        ("old_name.txt", "line1\nline2\nline3\nline4\nline5\nline6\n"),
        ("img.bin", "x"),
    ]);
    r.write_bytes("img.bin", &[0, 1, 2, 3, 255, 0, 9]);
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "chore: add binary"]);
    r.git(&["branch", "-f", "staging", "HEAD"]);
    r.git(&["branch", "-f", "develop", "HEAD"]);
    r.checkout("develop");
    r.git(&["mv", "old_name.txt", "new_name.txt"]);
    r.write_bytes("img.bin", &[0, 1, 2, 3, 255, 0, 10]);
    r.git(&["add", "-A"]);
    r.git(&["commit", "-q", "-m", "refactor: rename and rebinary"]);

    let rep = run(&r, "staging", "develop");
    let c = by_subject(&rep, "refactor: rename and rebinary");
    assert_eq!(c.class, New);
    let ren = c.files.iter().find(|f| f.path == "new_name.txt").unwrap();
    assert_eq!(ren.change, "R");
    assert_eq!(ren.old_path.as_deref(), Some("old_name.txt"));
    let bin = c.files.iter().find(|f| f.path == "img.bin").unwrap();
    assert!(bin.binary);
    let nf = rep.net_files.iter().find(|f| f.path == "img.bin").unwrap();
    assert!(nf.binary);
    assert_totals_consistent(&rep);
}

#[test]
fn conflicts_are_found_without_touching_the_repo() {
    let r = Repo::with_branches(&[("c.txt", "base\n"), ("fine.txt", "x\n")]);
    r.checkout("develop");
    r.commit("fix: dev side", &[("c.txt", "develop\n")]);
    r.checkout("staging");
    r.commit("fix: base side", &[("c.txt", "staging\n")]);

    let before = r.git(&["count-objects", "-v"]);
    let rep = run(&r, "staging", "develop");
    assert_eq!(rep.conflicts, vec!["c.txt".to_string()]);
    assert!(!rep.conflicts_best_effort);
    assert_eq!(
        r.git(&["count-objects", "-v"]),
        before,
        "no objects leaked into the repo"
    );
    assert_eq!(r.git(&["status", "--porcelain"]), "");
    assert_totals_consistent(&rep);
}

#[test]
fn clean_merge_has_no_conflicts() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    r.commit("feat: b", &[("b.txt", "b\n")]);
    r.checkout("staging");
    r.commit("feat: c", &[("c.txt", "c\n")]);
    assert!(run(&r, "staging", "develop").conflicts.is_empty());
}

#[test]
fn empty_range_and_empty_commits() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    let rep = run(&r, "staging", "develop");
    assert!(rep.commits.is_empty());
    assert_eq!(rep.totals.listed, 0);

    r.checkout("develop");
    r.git(&["commit", "-q", "--allow-empty", "-m", "chore: nothing"]);
    r.commit("feat: real", &[("r.txt", "r\n")]);
    let rep = run(&r, "staging", "develop");
    assert_eq!(rep.commits.len(), 1);
    assert_eq!(rep.totals.empty_skipped, 1);
    let rep = run_with(&r, "staging", "develop", |o| o.include_empty = true);
    assert_eq!(rep.commits.len(), 2);
    assert!(by_subject(&rep, "chore: nothing").files.is_empty());
    assert_totals_consistent(&rep);
}

#[test]
fn root_commit_on_head_is_diffed_against_the_empty_tree() {
    let r = Repo::new();
    r.commit("feat: first", &[("a.txt", "a\n")]);
    r.git(&["branch", "staging"]);
    r.checkout("staging");
    r.git(&["checkout", "-q", "--orphan", "other"]);
    r.git(&["rm", "-rfq", "."]);
    r.commit("feat: unrelated root", &[("z.txt", "z\n")]);
    // no merge base between the histories
    let mut o = prdiff::analyze::Options::new(r.path(), "main", "other");
    o.use_cache = false;
    let err = prdiff::analyze::analyze(&o).unwrap_err();
    assert!(err.to_string().contains("no merge base"), "{err}");
}

#[test]
fn path_filter_applies_to_every_stage() {
    let r = Repo::with_branches(&[("README.md", "hi\n")]);
    r.checkout("develop");
    r.commit("feat: api", &[("src/api/x.rs", "x\n")]);
    r.commit("docs: readme", &[("README.md", "hello\n")]);
    r.commit(
        "feat: both",
        &[("src/api/y.rs", "y\n"), ("README.md", "hello again\n")],
    );

    let rep = run_with(&r, "staging", "develop", |o| {
        o.paths = vec!["src/**".into()]
    });
    let subjects: Vec<_> = rep.commits.iter().map(|c| c.subject.as_str()).collect();
    assert_eq!(subjects, vec!["feat: api", "feat: both"]);
    assert!(
        rep.commits
            .iter()
            .all(|c| c.files.iter().all(|f| f.path.starts_with("src/")))
    );
    assert!(rep.net_files.iter().all(|f| f.path.starts_with("src/")));
    assert_totals_consistent(&rep);
}

#[test]
fn submodule_entries_are_flagged() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    r.checkout("develop");
    // a gitlink without a real submodule: enough for the tree entry
    r.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        "160000,1111111111111111111111111111111111111111,vendor/lib",
    ]);
    r.git(&["commit", "-q", "-m", "chore: bump submodule"]);
    let rep = run(&r, "staging", "develop");
    let c = by_subject(&rep, "chore: bump submodule");
    assert!(c.files[0].submodule);
    assert!(rep.net_files[0].submodule);
}

#[test]
fn unknown_ref_is_bad_input_and_cache_is_reusable() {
    let r = Repo::with_branches(&[("a.txt", "a\n")]);
    let o = prdiff::analyze::Options::new(r.path(), "nope", "develop");
    let err = prdiff::analyze::analyze(&o).unwrap_err();
    assert!(err.to_string().contains("unknown ref"));

    r.checkout("develop");
    r.commit("feat: x", &[("x.txt", "x\n")]);
    let first = run(&r, "staging", "develop");
    let second = run(&r, "staging", "develop"); // warm cache
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    assert!(r.path().join(".git/prdiff/commits").exists());
}

#[test]
fn glob_semantics() {
    let f =
        prdiff::glob::PathFilter::new(&["src/**".into(), "*.md".into(), "docs/".into()]).unwrap();
    assert!(f.matches("src/a/b.rs"));
    assert!(f.matches("README.md"));
    assert!(f.matches("deep/dir/NOTES.md"));
    assert!(f.matches("docs/x/y.txt"));
    assert!(!f.matches("lib/a.rs"));
}
