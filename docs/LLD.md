# LLD: `prdiff`, a Git-based real-diff checker for PR descriptions

Status: Draft
Owner: Engineering
Target: Rust CLI, Linux and macOS, Git 2.38+

## 1. Problem

Branch-level `git log base..head` lists commits, not changes. It misreports release scope in four common cases:

1. **Squash merges.** A feature merged to `staging` as one commit has a new hash and subject. Its commits still appear as "not on staging" from `develop`'s side.
2. **Cherry-picks and backports.** Same patch, different hash. `git log` counts it twice.
3. **Reverts.** A change and its revert both appear in the log, and net to zero.
4. **Already-shipped content.** A commit's files can be byte-identical on the target branch under a different subject. Subject matching misses this.

The result: PR descriptions list work that is already live, and counts disagree with the diff. The goal is a tool that reports what actually changes when `head` lands on `base`, at file and hunk level, and produces a PR description from that.

## 2. Goals and non-goals

**Goals**
- Given `base` and `head`, classify every head-only commit as `NEW`, `PARTIAL`, `SHIPPED` (content identical on base), or `PATCH_EQUIVALENT` (same patch-id on base).
- Compute the net diff of `base...head` with per-file and per-hunk status (`NEW_TO_BASE`, `ALREADY_ON_BASE`).
- Detect merge conflicts between `base` and `head` without touching any worktree.
- Emit JSON (for tooling) and Markdown (for PR bodies), with counts that match the listed items by construction.
- Run on a large monorepo-sized history (about 150 commits, 500 files) in under 10 seconds on a laptop.

**Non-goals (v1)**
- Not a code review tool. It does not judge code quality.
- Does not write to remotes, create PRs, or modify refs.
- Does not understand semantic equivalence (a rewritten function with the same behavior is `NEW`).
- Not a replacement for `git range-diff`, which it complements.

## 3. Users and usage

```
prdiff compare --base origin/production --head origin/develop
prdiff compare --base origin/staging --head origin/develop --format json > report.json
prdiff describe --base origin/staging --head origin/develop --title "release: develop to staging" > body.md
prdiff conflicts --base origin/staging --head origin/develop
```

Exit codes: `0` ok, `2` bad input (unknown ref, no merge base), `3` git failed, `4` conflicts found (with `--fail-on-conflict`).

## 4. Inputs and resolution

- Refs are resolved to commit SHAs once, up front, with `git rev-parse --verify`.
- The merge base is `git merge-base base head`. If absent, exit `2`.
- Head-only commits: `git rev-list --no-merges --topo-order base..head`. Merge commits are skipped, since their changes appear in their parents.
- Optional `--path <glob>` filters files, applied to every stage.

## 5. Algorithm

### Stage A: commit enumeration
For each head-only commit `c`:
- Changed files: `git diff-tree --no-commit-id --name-status -r -M c^ c` (`-M` enables rename detection). For a root commit, use the empty tree.
- Subject and author from `git show -s --format=%H%x1f%s%x1f%an c`.

### Stage B: patch equivalence
- `git patch-id --stable` on `git diff-tree -p c^ c` gives a stable patch hash.
- Build the set of patch-ids for `base...`: `git log -p --no-merges base` piped through `git patch-id --stable`. Any `c` whose patch-id is in the set is `PATCH_EQUIVALENT`.
- Cost: this walks base history once. Cache the set in `.git/prdiff/<base-sha>.patchids`.

### Stage C: content identity (the step subject matching misses)
For each touched file `f` of `c`:
- Blob on base at HEAD of base: `git rev-parse base:f` (or absent).
- Blob on head after `c`: `git rev-parse c:f` (or absent).
- File status:
  - `IDENTICAL` if blob ids match on base and at `c`. The change is already present.
  - `DIFFERENT` otherwise.
- Commit classification:
  - `SHIPPED` if every touched file is `IDENTICAL`.
  - `PARTIAL` if some are `IDENTICAL`, some `DIFFERENT`. Hunk-level check (Stage D) decides the detail.
  - `NEW` if none are `IDENTICAL`.
  - `PATCH_EQUIVALENT` short-circuits all of the above when Stage B matches.

Blob ids are compared, never contents, so this is fast. Cache key: `(path, base_blob, head_blob)`.

### Stage D: hunk-level check (only for PARTIAL)
- For each `DIFFERENT` file, compute `git diff base c -- f` restricted to the commit's own hunks.
- A hunk is `ALREADY_ON_BASE` if applying its reverse to the base blob fails (the change is already present). Use the `patch` logic via `git apply --check -R` against a temporary index, not the worktree.
- Report hunk counts per commit.

### Stage E: net diff
- `git diff --name-status -M base...head` gives files that differ.
- For each, the file is `NEW_TO_BASE` if its base blob differs from head's blob and the diff is not explained by `SHIPPED` commits. Otherwise `ALREADY_ON_BASE`.
- Net stats: `git diff --numstat base...head`, excluding `ALREADY_ON_BASE` files.

### Stage F: conflicts
- `git merge-tree --write-tree --name-only base head` (Git 2.38+). Output: tree id, then conflicted paths until a blank line.
- Fallback for older Git: `git merge-tree base head` text parsing, marked as best-effort in the output.

## 6. Data model

```rust
pub enum Classification { New, Partial, Shipped, PatchEquivalent }
pub enum FileStatus { Identical, Different, Absent }

pub struct CommitReport {
    pub sha: String,
    pub subject: String,        // conventional-commit subject
    pub kind: CommitKind,       // Feat | Fix | Refactor | Chore | Other (parsed from subject)
    pub class: Classification,
    pub files: Vec<FileChange>, // path, status, hunks_already_on_base, hunks_total
}

pub struct Report {
    pub base: RefInfo,          // ref name, sha
    pub head: RefInfo,
    pub merge_base: String,
    pub commits: Vec<CommitReport>,
    pub net_files: Vec<NetFile>,
    pub conflicts: Vec<String>,
    pub totals: Totals,         // counts by Classification and CommitKind
}
```

`CommitKind` is parsed from the subject prefix (`feat:`, `fix:`, etc.). Anything else is `Other`.

## 7. Output

### JSON
Stable schema with a `schema_version` field. Every count in `totals` is derived from `commits` and `net_files`, so consumers never recompute.

### Markdown (`describe`)
Generated from the Report, not hand-written:

```
## Release: develop → staging

**Scope (checked by content, not subject)**
- Commits not yet on base: N (f feat, x fix, o other)
- Excluded, identical on base: S
- Excluded, patch-equivalent on base: P
- Files changing vs base: K
- Conflicted files: list or none

**Features** ...
**Fixes** ...
**Other** ...
```

Rule: a listed item is a commit with class `New` or `Partial`. Every count is computed from the same list, so the description cannot disagree with itself.

## 8. Rust design

Crates:
- `clap` (CLI), `serde`/`serde_json` (output), `anyhow` + `thiserror` (errors), `rayon` (parallel per-commit work), `tempfile` (tests), `regex` (subject parsing).
- Git access: shell out to the `git` binary. This matches exactly what engineers run, and avoids libgit2 gaps around `merge-tree` and `patch-id`. Wrap all calls in one `GitRunner` with `LC_ALL=C`, a timeout, and stderr capture.

Modules:
```
src/
  main.rs            entry, exit codes
  cli.rs             clap definitions
  git.rs             GitRunner: run, run_lines, timeouts, LC_ALL=C
  refs.rs            resolve refs, merge base
  commits.rs         enumerate head-only commits, subject/kind parsing
  patchid.rs         base patch-id set, per-commit patch-id, cache
  classify.rs        Stage C and D
  net.rs             Stage E
  conflicts.rs       Stage F
  report.rs          Report, totals
  render/json.rs     JSON output
  render/markdown.rs describe output
  cache.rs           .git/prdiff cache dir
tests/
  fixtures.rs        builds temp repos with git CLI
  squash.rs, cherry_pick.rs, revert.rs, partial.rs, conflict.rs
```

Concurrency: Stage C is independent per commit, run with `rayon`. Git processes are capped at `num_cpus` to avoid fork storms.

## 9. Performance

- Blob comparisons use `rev-parse`, not `show`. For 150 commits and 500 files, this is about 1,000 `rev-parse` calls. Batch them with `git cat-file --batch-check` to cut process count to about 2.
- Patch-id set is cached per base SHA. Reruns on the same base are near-instant.
- Expected budget: cold run under 10 s, warm under 1 s.

## 10. Edge cases

| Case | Handling |
|---|---|
| Squash merge on base | Files identical, so `SHIPPED`. Subject mismatch ignored. |
| Cherry-pick of head commit onto base | Patch-id match, so `PATCH_EQUIVALENT`. |
| Revert pair in head-only range | Both listed; net diff shows zero for those files. Report marks the pair. |
| Rename | `-M` on every diff. Old path counted as a delete of the same blob. |
| Binary file | Compare blob ids only. No hunk analysis. |
| Submodule | Compare gitlink ids. Report as `SUBMODULE`. |
| Empty commit | Listed as `Other` with zero files. Excluded from counts by default. |
| Root commit on head | Diff against empty tree. |
| Shallow clone | Fail with exit `3` and a message to `git fetch --unshallow`. |
| Ref is a branch that moves | Resolve once to SHA. Report includes both SHAs. |

## 11. Errors and observability

- Every failing git call logs the argv and exit code. Output is not swallowed.
- `--verbose` prints per-stage timings to stderr.
- No network calls. No writes except `.git/prdiff/`.

## 12. Testing

- Fixture repos built with the `git` CLI in `tempfile` directories, so tests run the same binary users do.
- Required scenarios: squash (identical files, new hash), cherry-pick (same patch-id), revert pair, partial commit (one file identical), rename, binary file, conflict, empty range.
- Golden tests: fixed JSON output for each fixture, diffed on CI.
- Property check: for any generated range, `totals` equals the sum over `commits` and `net_files`.
- Target: 90% line coverage on `classify.rs`, `net.rs`, `conflicts.rs`.

## 13. Milestones

1. **M1 (3 days):** `compare` with Stages A, B, C. JSON output. Squash and cherry-pick fixtures green.
2. **M2 (2 days):** Stage D hunk analysis, Stage E net diff, renames and binaries.
3. **M3 (2 days):** Stage F conflicts, `describe` Markdown renderer, `--path` filter.
4. **M4 (1 day):** Cache, parallelism, perf budget check on a real repo (user-backend).
5. **M5 (1 day):** Docs, `--help`, release binary for Linux and macOS.

## 14. Risks

- **`git patch-id` on very large histories** is slow. Mitigation: cache, and restrict to `base` commits touching the head's files.
- **Partial commits with identical files and new hunks** can be misread. Mitigation: Stage D, and a `--strict` flag that marks any `PARTIAL` as `NEW`.
- **Older Git without `merge-tree --write-tree`.** Mitigation: fallback with a warning in output.
- **Stale cache** after history rewrite. Mitigation: the cache key is the base SHA, so a rewrite misses the cache.

## 15. Open questions

1. Should `SHIPPED` require the file to be identical in every branch between `base` and the commit's original parent, or only at base's HEAD? Proposal: HEAD only, simpler and matches "is it live."
2. Do we want the PR description to name the commit SHA on the release branch, or only the subject? Proposal: subject plus short SHA.
3. Should `describe` be used in CI to fail a release PR whose description disagrees with the report?
