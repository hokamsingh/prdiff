#![allow(dead_code)]
//! Fixture repos built with the real `git` binary, with fixed dates so SHAs are reproducible.

use prdiff::analyze::{Options, analyze};
use prdiff::report::{Classification, CommitReport, Report};
use std::cell::Cell;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

pub struct Repo {
    pub dir: TempDir,
    clock: Cell<u64>,
}

impl Repo {
    pub fn new() -> Repo {
        let r = Repo {
            dir: tempfile::tempdir().unwrap(),
            clock: Cell::new(1_700_000_000),
        };
        r.git(&["init", "-q", "-b", "main"]);
        r.git(&["config", "user.email", "t@example.com"]);
        r.git(&["config", "user.name", "Tester"]);
        r.git(&["config", "commit.gpgsign", "false"]);
        r
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn git(&self, args: &[&str]) -> String {
        self.try_git(args)
            .unwrap_or_else(|e| panic!("git {args:?} failed: {e}"))
    }

    pub fn try_git(&self, args: &[&str]) -> Result<String, String> {
        let t = self.clock.get() + 60;
        self.clock.set(t);
        let date = format!("{t} +0000");
        let out = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).to_string())
        }
    }

    pub fn write(&self, file: &str, content: &str) {
        let p = self.path().join(file);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    pub fn write_bytes(&self, file: &str, content: &[u8]) {
        std::fs::write(self.path().join(file), content).unwrap();
    }

    /// Write files and commit them; returns the new SHA.
    pub fn commit(&self, msg: &str, files: &[(&str, &str)]) -> String {
        for (f, c) in files {
            self.write(f, c);
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", msg]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn checkout(&self, branch: &str) {
        self.git(&["checkout", "-q", branch]);
    }

    pub fn branch_from(&self, name: &str, from: &str) {
        self.git(&["checkout", "-q", "-b", name, from]);
    }

    /// main + staging + develop, all at one initial commit touching `files`.
    pub fn with_branches(files: &[(&str, &str)]) -> Repo {
        let r = Repo::new();
        r.commit("chore: init", files);
        r.git(&["branch", "staging"]);
        r.git(&["branch", "develop"]);
        r
    }
}

pub fn run(repo: &Repo, base: &str, head: &str) -> Report {
    run_with(repo, base, head, |_| {})
}

pub fn run_with(repo: &Repo, base: &str, head: &str, tweak: impl FnOnce(&mut Options)) -> Report {
    let mut o = Options::new(repo.path(), base, head);
    tweak(&mut o);
    analyze(&o).unwrap()
}

pub fn by_subject<'a>(r: &'a Report, subject: &str) -> &'a CommitReport {
    r.commits
        .iter()
        .find(|c| c.subject == subject)
        .unwrap_or_else(|| {
            panic!(
                "no commit {subject:?} in {:?}",
                r.commits.iter().map(|c| &c.subject).collect::<Vec<_>>()
            )
        })
}

pub fn class_of(r: &Report, subject: &str) -> Classification {
    by_subject(r, subject).class
}

/// Totals must equal what the lists say, whatever the scenario.
pub fn assert_totals_consistent(r: &Report) {
    let t = &r.totals;
    assert_eq!(t.commits, r.commits.len());
    assert_eq!(t.by_class.values().sum::<usize>(), r.commits.len());
    for (class, n) in &t.by_class {
        assert_eq!(
            *n,
            r.commits.iter().filter(|c| c.class == *class).count(),
            "{class:?}"
        );
    }
    assert_eq!(
        t.listed,
        r.commits.iter().filter(|c| c.class.is_listed()).count()
    );
    assert_eq!(t.listed_by_kind.values().sum::<usize>(), t.listed);
    assert_eq!(
        t.files_new_to_base + t.files_already_on_base,
        r.net_files.len()
    );
    assert_eq!(t.conflicted_files, r.conflicts.len());
}
