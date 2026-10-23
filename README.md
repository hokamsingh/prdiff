# prdiff

Tells you what **actually** changes when `head` lands on `base`, by content rather than by commit subject.
`git log base..head` over-reports in four common cases; `prdiff` classifies every head-only commit instead:

| Class | Meaning | Case it fixes |
|---|---|---|
| `NEW` | still has something to ship | |
| `PARTIAL` | some of it is already on base | half-shipped commits |
| `SHIPPED` | its content is already on base (file blobs or hunks) | squash merges, later-edited code |
| `PATCH_EQUIVALENT` | same patch-id exists on base | cherry-picks, backports |

Design: [docs/LLD.md](docs/LLD.md). Single static binary; shells out to `git` (2.38+ for exact conflict detection).

## Install

```bash
cargo install --git https://github.com/hokamsingh/prdiff      # installs to ~/.cargo/bin/prdiff
# or from a checkout:
git clone https://github.com/hokamsingh/prdiff && cd prdiff && cargo install --path .
```

## Use

```bash
prdiff compare   --base origin/staging --head origin/develop                 # human summary
prdiff compare   --base origin/staging --head origin/develop --format json   # stable schema (schema_version: 1)
prdiff describe  --base origin/staging --head origin/develop --title "release: develop to staging" > body.md
prdiff conflicts --base origin/staging --head origin/develop --fail-on-conflict
prdiff check     --base origin/staging --head origin/develop --body body.md  # CI gate: exit 5 if the PR body is stale
prdiff view      --base origin/staging --head origin/develop --open          # self-contained HTML report
```

### `prdiff view`

Writes one offline HTML file (`prdiff-report.html`, or `--out <file>`; `--out -` for stdout): class counts you can click
to filter, a commit list with an "already on base" bar per commit, a Files tab (new to base vs already on base, with the
commits behind each file), colour-coded hunks with the ones already on base dimmed, and a conflict banner. Light and
dark themes; commit text is embedded as JSON and rendered with `textContent`, so a hostile subject line cannot inject
markup. `--max-lines` caps the diff shown per file (default 400).

![prdiff view](docs/view.png)

**Side by side.** Every changed file has a *Side by side* button (inside an expanded commit, and on each row of the
Files tab). It opens a full-screen viewer: the file as it is **right now on the target branch** on the left, the file
as the **merging branch** has it on the right, lines aligned, removed lines red and added lines green. Unchanged runs
are collapsed (click to expand, or untick *Collapse unchanged*), *Prev/Next* jump between changes, `Esc` closes. A
renamed file is shown under its old name on the left; added and deleted files show an empty side. Binary files get no
viewer; files over 512 KB per side, or past the report-wide `--max-embed-mb` budget (default 8), get a note with the
`git diff` command instead. `--no-sides` leaves file contents out entirely.

The left side is the target's *current* tip, so a line that differs may also come from a commit that reached the target
after the branch was cut; the viewer says so.

![side by side](docs/side-by-side.png)

Useful flags: `--path 'src/**'` (repeatable), `--strict` (PARTIAL counts as NEW), `--shallow` (skip hunk analysis for
NEW commits), `--patch-id-scope auto|all|base-only`, `--no-cache`, `--exact-conflicts`, `-v` (stage timings), `-C <dir>`.

Exit codes: `0` ok, `2` bad input (unknown ref, no merge base, usage), `3` git failed (incl. shallow clone),
`4` conflicts with `--fail-on-conflict`, `5` `check` found a stale or incomplete description.

## How it differs from the LLD (all additive, all tested)

- **Hunk analysis runs on NEW commits too** (default). A change base later edited around has a different blob, so the
  blob check alone calls it NEW. `--shallow` restores the LLD behaviour. Hunks are matched in memory against the base
  blob; if only the surrounding context drifted, replacement hunks are still recognised by their own lines.
- **Cherry-pick search is path-set prefiltered and cached.** A base commit is only patch-id'd if it touches exactly the
  paths of some head-only commit. Per-commit results are cached by SHA in `.git/prdiff/commits`, so history rewrites
  just miss the cache. The default scope is base commits newer than the oldest head-only commit (minus 2 days of clock
  skew), which also catches backports that were merged back into head; `--patch-id-scope all` walks everything.
- **Conflict detection** first proves the two sides are disjoint (no shared path, no file/dir clash, no add-vs-delete
  pair) and skips `git merge-tree` if so; otherwise `merge-tree --write-tree` runs against a throwaway object
  directory (tmpfs on Linux) so the repository is never written to. A randomized test checks the shortcut against
  `merge-tree` on 120 histories. Pre-2.38 git falls back to text parsing, flagged `conflicts_best_effort`.
- **Revert pairs** are detected from `This reverts commit <sha>` and reported; **`check`** implements the CI gate from
  the LLD's open questions.

## Performance

Synthetic monorepo-shaped history (`scripts/gen_bench_repo.py`): 150 head-only commits touching ~500 files, mixing
squash merges, cherry-picks, half-shipped commits and base-side edits. 4 vCPU Xeon @ 2.1 GHz, git 2.43, medians:

| Base history | `git log base..head` | `git cherry` | `prdiff compare` cold | warm | cold, `--patch-id-scope all` |
|---|---|---|---|---|---|
| 2,376 commits | 7 ms | 28 ms | 218 ms | 133 ms | 198 ms |
| 21,296 commits | 11 ms | 82 ms | 359 ms | 227 ms | 1.1 s |
| 104,211 commits | 24 ms | 180 ms | 524 ms | 295 ms | 5.7 s |

On all three, the counts match what the generator built in: 90 NEW, 10 PARTIAL, 30 SHIPPED, 20 PATCH_EQUIVALENT.
Time is spent in `git` subprocesses (patch-id, merge-tree, diff-tree); the Rust side is under a millisecond.
Reproduce: `python3 scripts/gen_bench_repo.py /tmp/b 20000 1500 && scripts/bench.sh /tmp/b`.

## Develop

```bash
cargo test     # 24 tests: fixture repos built with the real git binary, a golden JSON, CLI exit codes, randomized conflict check
```
Golden file: `UPDATE_GOLDEN=1 cargo test --test cli golden`.

## Limits

- Equivalence is by patch and content, not behaviour: a rewritten function is NEW.
- Hunk matching is a contiguity check, not a full `git apply`; pure additions or deletions whose context changed are
  conservatively reported as not on base.
- Merge commits are skipped (their changes appear in their parents). Paths containing newlines are treated as "differs".
