# Architecture

`prdiff` is a Rust program that shells out to the `git` binary. It is one crate with a library (`src/lib.rs`) and a thin
command line (`src/main.rs`). This page describes how a run works, how fast it is, and how it is tested. The original design is
in [LLD](LLD.md); the section "Where it differs from the LLD" below says what changed.

## Why shell out to git

It does exactly what engineers run by hand, including `merge-tree`, `patch-id` and `range-diff`, which libraries cover poorly.
The cost is process start-up (a few milliseconds per call). A run uses roughly a dozen git processes, several of them in
parallel, which is where nearly all the time goes; the Rust side is well under a millisecond.
Every call goes through one runner (`git.rs`) with `LC_ALL=C`, a timeout, captured stderr, and the full argument list in every
error message.

## A run, step by step

`analyze.rs` orchestrates:

1. **Refs.** One `git rev-parse` resolves base and head to SHAs and checks the clone is not shallow; `git merge-base` finds the
   merge-base.
2. **Commits** (`commits.rs`). `git log base..head --no-merges` for metadata, and one `git diff-tree --stdin --raw -M` for every
   commit's files. The type, scope, breaking flag, PR number and ticket ids are parsed from the message.
3. **In parallel:**
   - **Patch equivalence** (`patchid.rs`): see below.
   - **Net diff** (`net.rs`): `git diff-tree -r -M --raw --numstat` from the merge-base to head.
   - **Conflicts** (`conflicts.rs`): a path-overlap check, then `git merge-tree --write-tree` if needed.
   - **Base blobs** (`classify.rs`): one `git cat-file --batch-check` for every touched path on base's tip.
4. **Classify** (`classify.rs`): compare blob ids per file, apply patch equivalence, assign a class.
5. **Hunks** (`hunks.rs`), in parallel with the base side of conflict attribution: in-memory hunk matching against base's blobs.
6. **Assemble:** net files with risk flags (`risk.rs`) and owners (`codeowners.rs`), conflict details, revert pairs, totals.
7. **Render** (`render/`): text, JSON, Markdown, Slack, notes, or the HTML report. `view` also gathers file contents
   (`sides.rs`), and optionally the release-train matrix (`train.rs`) and the rewritten-branch pairs (`rewrite.rs`).

`Totals::derive` is the only place counts are computed, so output cannot disagree with itself.

## Patch equivalence, fast

`git log -p base | git patch-id` over all of base's history is slow. Two ideas make it cheap:

- **Path-set prefilter.** An identical patch always touches an identical set of paths. A base commit is only patch-id'd when
  its path set equals the path set of some head-only commit. Path sets are cheap to list.
- **A per-commit cache** in `.git/prdiff/commits`: one line `<commit> <pathset-hex> <patch-id | - | ?>`, appended to and keyed
  by commit SHA, so a history rewrite simply misses it. The path-set hash is FNV-1a (stable across Rust versions because it is
  persisted).

The default window is base commits newer than the oldest head-only commit minus two days, which also finds backports that were
merged back into head.

## Conflicts without touching your repository

`git merge-tree --write-tree` writes tree objects into the object database. `prdiff` points it at a throwaway object directory
(on tmpfs on Linux, since writing a few hundred loose objects is slow on many disks) with your real objects as an alternate, so
nothing is added to your repository. Before running it, a cheap check proves the two sides cannot conflict (no path changed
by both, no file/directory clash, no add-versus-delete pair that could trigger rename handling) and skips it entirely.
A randomised test compares the shortcut with `merge-tree` on many generated histories.

## Hunk matching

A hunk is `ALREADY_ON_BASE` when base's version of the file contains the hunk's after-image (context plus added lines,
contiguous) and no longer contains its before-image. If only the context drifted, a replacement hunk is still recognised by
its own removed and added lines. Pure additions and deletions whose context changed are conservatively reported as not on base.
With `-w`, lines are compared with whitespace removed and whitespace-only hunks count as on base.

## Modules

| Module | Responsibility |
|---|---|
| `main.rs`, `lib.rs` | command line, exit codes; library root |
| `git.rs` | the single `git` runner, plus the patch-id pipeline |
| `refs.rs`, `config.rs` | ref resolution; `prdiff.toml` and ref inference |
| `commits.rs`, `rawdiff.rs` | head-only commits and their files; parsers for `diff-tree --raw -z` |
| `patchid.rs`, `cache.rs` | patch equivalence and its cache |
| `classify.rs`, `hunks.rs` | content identity and hunk analysis |
| `net.rs`, `conflicts.rs` | net diff; conflict prediction and attribution |
| `risk.rs`, `codeowners.rs`, `glob.rs` | risk flags, owners, glob matching |
| `report.rs` | the data model and `Totals` |
| `render/` | text, JSON, Markdown, Slack, release notes, and the HTML report (`view.rs`, `view.html`) |
| `sides.rs` | file contents and line diffs for the side-by-side viewer |
| `train.rs`, `rewrite.rs` | `where` and `matrix`; `rewritten` |
| `github.rs`, `cliff.rs`, `check.rs` | `gh` integration; git-cliff renderer; description check |

## Performance

Synthetic monorepo-shaped histories from `scripts/gen_bench_repo.py`: 150 head-only commits touching about 500 files, mixing squash
merges, cherry-picks, half-shipped commits and base-side edits, with a couple of real conflicts. 4 vCPU Xeon at 2.1 GHz,
git 2.43, medians:

| Base history | `git log base..head` | `git cherry` | `prdiff compare` cold | warm |
|---|---|---|---|---|
| 2,376 commits | 7 ms | 29 ms | 230 ms | 180 ms |
| 21,296 commits | 11 ms | 93 ms | 434 ms | 280 ms |
| 104,211 commits | 24 ms | 189 ms | 694 ms | 431 ms |

On all three the counts match what the generator built in (90 `NEW`, 10 `PARTIAL`, 30 `SHIPPED`, 20 `PATCH_EQUIVALENT`).
`--patch-id-scope all` on the largest repository takes about 5.7 s cold. These are synthetic repositories on one machine, not
yours.

```bash
python3 scripts/gen_bench_repo.py /tmp/bench 20000 1500
scripts/bench.sh /tmp/bench          # also checks the counts against what the generator built
```

## Testing

`cargo test` builds fixture repositories with the real `git` binary in temporary directories, so tests run the same program
users do. They cover squash, cherry-pick, backport merged back, revert, partial and hunk-level, renames, binary files,
submodules, conflicts, empty ranges, path filters, CODEOWNERS, risk flags, release trains, force-pushed branches, the HTML report
(including hostile commit text), the local server, the GitHub integration (against a fake `gh`), the git-cliff renderer (a fake
one, plus the real one when installed), `install.sh` against a local release, golden JSON, exit codes, and a randomised check
of the conflict shortcut. CI runs them with `cargo fmt --check` and `cargo clippy -D warnings` on Ubuntu and macOS.

## Where it differs from the LLD

- Hunk analysis also runs on `NEW` commits by default (`--shallow` restores the LLD behaviour).
- The cherry-pick search is path-set prefiltered and cached per commit instead of hashing all of base's history.
- Conflict detection first tries to prove disjointness and uses a throwaway object directory.
- The LLD says a hunk is on base when its reverse fails to apply; it is the opposite (the reverse applies cleanly when the change
  is present).
- Added: release notes, risk flags, CODEOWNERS, conflict attribution, the release train, force-push review, the HTML report,
  the GitHub integration, a config file, and git-cliff rendering.
