//! Orchestration: refs -> commits -> (patch-ids | net diff + conflicts + blobs) -> classify -> hunks -> report.

use crate::cache::Cache;
use crate::classify::{self, Blobs, Settings};
use crate::commits::{self, Commit};
use crate::conflicts;
use crate::git::Git;
use crate::glob::PathFilter;
use crate::hunks;
use crate::net;
use crate::patchid::{self, Scope};
use crate::refs;
use crate::report::*;
use anyhow::Result;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Options {
    pub repo: PathBuf,
    pub base: String,
    pub head: String,
    pub paths: Vec<String>,
    pub strict: bool,
    /// Hunk analysis for NEW commits too (default). Off = only PARTIAL commits, as in the LLD.
    pub deep: bool,
    pub include_empty: bool,
    pub scope: Scope,
    pub use_cache: bool,
    /// Collect displayable hunks (at most this many lines per file) for every commit, for `prdiff view`.
    pub detail: Option<usize>,
    /// With `detail`: bytes of file content to embed for the side-by-side viewer (0 = none).
    pub sides_budget: usize,
    /// Always run `git merge-tree` instead of first proving the sides are disjoint.
    pub exact_conflicts: bool,
    pub verbose: bool,
    pub timeout: Duration,
}

impl Options {
    pub fn new(repo: impl Into<PathBuf>, base: &str, head: &str) -> Self {
        Options {
            repo: repo.into(),
            base: base.into(),
            head: head.into(),
            paths: vec![],
            strict: false,
            deep: true,
            include_empty: false,
            scope: Scope::Auto,
            use_cache: true,
            detail: None,
            sides_budget: 0,
            exact_conflicts: false,
            verbose: false,
            timeout: Duration::from_secs(300),
        }
    }
}

struct Timings {
    verbose: bool,
    rows: Mutex<Vec<(&'static str, Duration)>>,
}

impl Timings {
    fn stage<T>(&self, name: &'static str, f: impl FnOnce() -> T) -> T {
        let t = Instant::now();
        let r = f();
        if self.verbose {
            self.rows.lock().unwrap().push((name, t.elapsed()));
        }
        r
    }
    fn print(&self, total: Duration) {
        if self.verbose {
            for (n, d) in self.rows.lock().unwrap().iter() {
                eprintln!("prdiff: {n:<12} {:>8.1} ms", d.as_secs_f64() * 1e3);
            }
            eprintln!(
                "prdiff: {:<12} {:>8.1} ms",
                "total",
                total.as_secs_f64() * 1e3
            );
        }
    }
}

pub fn analyze(o: &Options) -> Result<Report> {
    let started = Instant::now();
    let t = Timings {
        verbose: o.verbose,
        rows: Mutex::new(vec![]),
    };
    let git = Git::new(&o.repo, o.timeout);
    let filter = PathFilter::new(&o.paths)?;

    let (base, head, merge_base) = t.stage("refs", || -> Result<_> {
        refs::ensure_not_shallow(&git)?;
        let base = refs::resolve(&git, &o.base)?;
        let head = refs::resolve(&git, &o.head)?;
        let mb = refs::merge_base(&git, &base, &head)?;
        Ok((base, head, mb))
    })?;

    let all = t.stage("commits", || commits::load(&git, &base.sha, &head.sha))?;

    // `--path` filter and empty-commit handling; `all` keeps unfiltered files for the patch-id prefilter.
    let mut kept: Vec<Commit> = Vec::new();
    let mut empty_skipped = 0;
    for c in &all {
        if c.files.is_empty() {
            if o.include_empty && !filter.is_active() {
                kept.push(c.clone());
            } else {
                empty_skipped += 1;
            }
            continue;
        }
        let mut c = c.clone();
        c.files.retain(|e| {
            filter.matches(&e.path) || e.old_path.as_deref().is_some_and(|p| filter.matches(p))
        });
        if !c.files.is_empty() {
            kept.push(c);
        }
    }

    let mut cache = Cache::open(&git, o.use_cache);
    let kept_shas: HashSet<&str> = kept.iter().map(|c| c.sha.as_str()).collect();
    let patch_input: Vec<Commit> = all
        .iter()
        .filter(|c| kept_shas.contains(c.sha.as_str()))
        .cloned()
        .collect();

    let (equiv, (net_raw, conflict_res, blobs)) = rayon::join(
        || t.stage("patch-id", || patchid::equivalents(&git, &patch_input, &base.sha, &head.sha, o.scope, &mut cache)),
        || -> (Result<net::NetRaw>, Result<conflicts::Conflicts>, Result<Blobs>) {
            let (net_raw, conflict_res) = rayon::join(
                || t.stage("net-diff", || net::diff_raw(&git, &merge_base, &head.sha)),
                || t.stage("conflicts", || conflicts::detect(&git, &base.sha, &head.sha, (!o.exact_conflicts).then_some(merge_base.as_str()))),
            );
            let blobs = match &net_raw {
                Ok(raw) => t.stage("blobs", || {
                    let mut paths: HashSet<&str> = classify::touched_paths(&kept).collect();
                    for e in &raw.entries {
                        paths.extend(commits::entry_paths(e));
                    }
                    Blobs::lookup(&git, &base.sha, &paths)
                }),
                Err(_) => Blobs::lookup(&git, &base.sha, &HashSet::new()),
            };
            (net_raw, conflict_res, blobs)
        },
    );
    let (equiv, net_raw, conflict_res, blobs) = (equiv?, net_raw?, conflict_res?, blobs?);

    let mut reports = t.stage("classify", || {
        classify::classify(&kept, &blobs, &equiv, &Settings { strict: o.strict })
    });

    let targets: Vec<usize> = reports
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            (o.detail.is_some() && !r.files.is_empty())
                || r.class == Classification::Partial
                || (o.deep && r.class == Classification::New && !r.files.is_empty())
        })
        .map(|(i, _)| i)
        .collect();
    t.stage("hunks", || {
        hunks::analyze(
            &git,
            &kept,
            &mut reports,
            &targets,
            &blobs,
            !o.strict,
            o.detail,
        )
    })?;
    if o.detail.is_some() {
        // Already on base by definition, whatever the in-memory hunk matcher says.
        for r in &mut reports {
            if matches!(
                r.class,
                Classification::Shipped | Classification::PatchEquivalent
            ) {
                for f in &mut r.files {
                    f.hunks_already_on_base = f.hunks_total;
                    f.hunk_details.iter_mut().for_each(|h| h.on_base = true);
                }
                r.hunks_already_on_base = r.hunks_total;
            }
        }
    }

    let net_files = t.stage("net", || net::build(&net_raw, &blobs, &filter, &reports));

    let file_sides = if o.detail.is_some() && o.sides_budget > 0 {
        t.stage("sides", || {
            crate::sides::collect(&git, &net_raw.entries, &net_files, &blobs, o.sides_budget)
        })?
    } else {
        Vec::new()
    };

    let conflicts: Vec<String> = conflict_res
        .paths
        .into_iter()
        .filter(|p| filter.matches(p))
        .collect();

    let listed: HashSet<&str> = reports.iter().map(|r| r.sha.as_str()).collect();
    let revert_pairs: Vec<RevertPair> = reports
        .iter()
        .filter_map(|r| {
            let target = r.reverts.as_deref()?;
            listed.contains(target).then(|| RevertPair {
                revert: r.sha.clone(),
                reverted: target.to_string(),
            })
        })
        .collect();

    let totals = Totals::derive(&reports, &net_files, &conflicts, empty_skipped);
    t.print(started.elapsed());
    Ok(Report {
        schema_version: SCHEMA_VERSION,
        base,
        head,
        merge_base,
        commits: reports,
        net_files,
        revert_pairs,
        conflicts,
        file_sides,
        conflicts_best_effort: conflict_res.best_effort,
        totals,
    })
}

/// Just Stage F, for `prdiff conflicts`.
pub fn conflicts_only(o: &Options) -> Result<(refs::RefInfo, refs::RefInfo, Vec<String>, bool)> {
    let git = Git::new(&o.repo, o.timeout);
    let filter = PathFilter::new(&o.paths)?;
    refs::ensure_not_shallow(&git)?;
    let base = refs::resolve(&git, &o.base)?;
    let head = refs::resolve(&git, &o.head)?;
    let mb = refs::merge_base(&git, &base, &head)?;
    let c = conflicts::detect(
        &git,
        &base.sha,
        &head.sha,
        (!o.exact_conflicts).then_some(mb.as_str()),
    )?;
    let paths = c.paths.into_iter().filter(|p| filter.matches(p)).collect();
    Ok((base, head, paths, c.best_effort))
}
