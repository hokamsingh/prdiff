# prdiff

Tells you what **actually** changes when `head` lands on `base`, by content rather than by commit subject.

`git log base..head` over-reports in four common cases (squash merges, cherry-picks, reverts, already-shipped content), so
release PR descriptions list work that is already live. `prdiff` classifies every head-only commit instead:

| Class | Meaning | Case it fixes |
|---|---|---|
| `NEW` | still has something to ship | |
| `PARTIAL` | some of it is already on base | half-shipped commits |
| `SHIPPED` | its content is already on base (file blobs or hunks) | squash merges, later-edited code |
| `PATCH_EQUIVALENT` | the same patch is on base | cherry-picks, backports |

One static binary written in Rust. It shells out to `git` (2.38 or newer for exact conflict detection).

![the report](docs/view.png)

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/hokamsingh/prdiff/main/install.sh | sh   # prebuilt binary -> ~/.local/bin
cargo install --git https://github.com/hokamsingh/prdiff                               # from source -> ~/.cargo/bin
```

While the repository is private, anonymous downloads return 404: log in with `gh auth login` and see
[Getting started](docs/getting-started.md#install).

## Use

```bash
prdiff compare   --base origin/staging --head origin/develop                  # classify commits, net diff, conflicts
prdiff describe  --base origin/staging --head origin/develop > body.md        # a PR description from the facts
prdiff view      --base origin/staging --head origin/develop --open           # HTML report with side-by-side diffs
prdiff conflicts --base origin/staging --head origin/develop --fail-on-conflict
prdiff notes     --base origin/production --head origin/staging               # release notes
prdiff where '#4821' --env production=origin/production --env staging=origin/staging
```

`--base` and `--head` are optional: head defaults to the current branch, base to the remote's default branch.

| Command | What it does |
|---|---|
| `compare` | classify commits, net diff per file, conflicts (`text`, `json`, `markdown`, `slack`) |
| `describe` | PR description generated from the report |
| `notes` | release notes by kind, breaking changes first; `--renderer git-cliff` for your `cliff.toml` |
| `view` | one offline HTML report: commits, files, side-by-side diffs, risk badges, owners, conflicts, live `--serve` |
| `where`, `matrix` | which environments have a commit; every commit against every environment (the release train) |
| `rewritten` | what changed between the old and new version of a force-pushed branch |
| `conflicts` | files that would conflict, and who touched each one on both sides |
| `check`, `publish` | CI gate for stale PR descriptions; write the description into a PR body or a sticky comment |

## Documentation

| | |
|---|---|
| [Getting started](docs/getting-started.md) | install, first run, the commands you will use most |
| [Concepts](docs/concepts.md) | the four classes and how each is decided |
| [Command reference](docs/commands.md) | every command, flag and exit code |
| [Configuration](docs/configuration.md) | `prdiff.toml`, risk flags, CODEOWNERS |
| [The HTML report](docs/report-viewer.md) | tabs, side-by-side viewer, live mode |
| [JSON output](docs/json-output.md) | the report schema |
| [CI and GitHub](docs/ci-and-github.md) | the GitHub Action, `check`, `publish`, releasing |
| [Recipes](docs/recipes.md) | release PRs, release trains, force-pushes, monorepos |
| [Troubleshooting](docs/troubleshooting.md) | error messages and surprising results |
| [Architecture](docs/architecture.md) | how it works, performance, testing |
| [Contributing](CONTRIBUTING.md) | build, test, benchmark, release |

## Performance

Synthetic monorepo-shaped histories with 150 head-only commits touching about 500 files (squash merges, cherry-picks,
half-shipped commits, base-side edits), on a 4 vCPU machine, medians:

| Base history | `git log base..head` | `git cherry` | `prdiff compare` cold | warm |
|---|---|---|---|---|
| 2,376 commits | 7 ms | 29 ms | 230 ms | 180 ms |
| 21,296 commits | 11 ms | 93 ms | 434 ms | 280 ms |
| 104,211 commits | 24 ms | 189 ms | 694 ms | 431 ms |

The counts match what the generator built in. These are synthetic repositories on one machine, not yours; details in
[Architecture](docs/architecture.md#performance).

## Status

`cargo test` runs the test suite against fixture repositories built with the real `git` binary. CI runs it with
`cargo fmt --check` and `cargo clippy -D warnings` on Ubuntu and macOS, and it is green. CI has already caught real
platform differences (for example, `\b` does not work in POSIX regular expressions on macOS).

Not run against the real thing:

- The GitHub integration (`--pr`, `publish`, `check --pr`) was tested against a fake `gh` only, and `action.yml` has never run.
- `install.sh` was run against the real v0.2.0 release on Linux x86_64 through its `gh` fallback (the repository is private).
  The anonymous `curl | sh` path, macOS, arm64 and Homebrew installs are untested.
- The test against the real `git-cliff` runs only where it is installed, not in CI.

## Limits

- Equivalence is by patch and content, not behaviour: a rewritten function is `NEW`.
- Hunk matching is a contiguity check, not a full `git apply`; pure additions or deletions whose context changed are
  conservatively reported as not on base.
- Merge commits are skipped (their changes appear in their parents). Paths containing newlines are treated as "differs".
- `rewritten` relies on `git range-diff` pairing; heavily reworked commits can show as dropped plus added.

See [Concepts](docs/concepts.md#things-that-surprise-people) for the full list.
