# Command reference

```
prdiff [GLOBAL OPTIONS] <COMMAND> [OPTIONS]
```

Run `prdiff <command> --help` for the same information from the binary.

## Global options

| Option | Meaning |
|---|---|
| `-C, --repo <dir>` | Repository directory (default `.`) |
| `--config <file>` | Config file (default: `prdiff.toml` at the repository root, if present). See [Configuration](configuration.md) |
| `-v, --verbose` | Print per-stage timings to stderr |
| `--timeout <seconds>` | Timeout for each git call (default 300) |
| `-V, --version`, `-h, --help` | Version and help |

## Common options

These are accepted by `compare`, `describe`, `notes`, `conflicts`, `view`, `rewritten`, `where`, `matrix`, `publish` and
`check`. Commands that compare against environments (`where`, `matrix`) do not use `--base`.

| Option | Meaning |
|---|---|
| `--base <ref>` | The branch the change would land on. Default: `base` in `prdiff.toml`, else the remote's default branch (`origin/HEAD`, then `origin/main`, `origin/master`, `main`, `master`) |
| `--head <ref>` | The branch carrying the change. Default: `head` in `prdiff.toml`, else the current branch |
| `--pr <number>` | Compare a GitHub pull request instead (needs `gh`). Fetches `refs/pull/<n>/head` into `refs/prdiff/pr/<n>` and the base branch from `origin`; overrides `--base` and `--head` |
| `--path <glob>` | Only consider files matching this glob. Repeatable. `**` crosses directories, `*` and `?` do not, a pattern without `/` matches at any depth |
| `--exclude <glob>` | Leave files matching this glob out of file lists and stats (lockfiles, generated code). Repeatable. Commits are still classified on all of their files |
| `-w, --ignore-whitespace` | Treat whitespace-only differences as no difference |
| `--strict` | Treat every `PARTIAL` commit as `NEW` |
| `--shallow` | Skip hunk analysis for `NEW` commits (only `PARTIAL` ones get it). Faster, but a change that base later edited around looks new |
| `--include-empty` | Keep commits with no file changes |
| `--patch-id-scope auto\|all\|base-only` | How much of base's history is searched for cherry-picks (see [Concepts](concepts.md)) |
| `--no-cache` | Do not read or write `.git/prdiff` |
| `--exact-conflicts` | Always run `git merge-tree` instead of first proving the two sides cannot conflict |
| `--fail-on-conflict` | Exit 4 when base and head conflict |
| `--no-owners` | Do not read CODEOWNERS |

Refs are resolved to commit SHAs once, up front, so a branch that moves during a run does not change the answer.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | OK |
| 2 | Bad input: unknown ref, no merge base, bad config, missing `gh` or `git-cliff`, usage error |
| 3 | `git` or `gh` failed, including a shallow clone (`git fetch --unshallow`) |
| 4 | Conflicts found, with `--fail-on-conflict` |
| 5 | `check` found a stale or incomplete description |

## `compare`

Classify every head-only commit and compute the net diff and conflicts.

```bash
prdiff compare [--base B] [--head H] [--format text|json|markdown|slack]
```

`text` (default) is a human summary. `json` is the full report ([JSON output](json-output.md)). `markdown` is the same as
`describe` without a title. `slack` is Slack mrkdwn.

## `describe`

A PR description generated from the report. Every count comes from the same lists it prints.

```bash
prdiff describe --title "release: develop to staging" > body.md
prdiff describe --format slack
```

| Option | Meaning |
|---|---|
| `--title <text>` | Heading (default `Release: <head> → <base>`) |
| `--format markdown\|slack\|json` | Output flavour (default markdown) |

The description contains: scope counts (commits not yet on base, excluded as identical, excluded as patch-equivalent, files
changing, conflicted files with who touched them on each side, files that need a closer look), who has to review (from
CODEOWNERS), a *Look closely at* list of risky files, the commits grouped into Features, Fixes and Other (a partial commit
shows how many hunks are already on base), revert pairs, and a collapsed list of the excluded commits.

## `notes`

Release notes grouped by kind, with breaking changes first.

```bash
prdiff notes --base origin/production --head origin/staging
prdiff notes --repo-url https://github.com/acme/app --ticket-url 'https://jira.example.com/browse/{id}'
prdiff notes --renderer git-cliff --cliff-config cliff.toml
```

| Option | Meaning |
|---|---|
| `--title <text>` | Heading (with git-cliff: passed as `--tag`) |
| `--repo-url <url>` | Base URL for `(#123)` links. Default: `notes.repo_url` in the config, else derived from `origin` |
| `--ticket-url <template>` | Ticket link template containing `{id}` |
| `--format markdown\|json` | `json` is not available with `--renderer git-cliff` |
| `--renderer builtin\|git-cliff` | Who formats the notes |
| `--cliff-config <file>` | git-cliff config file (default: git-cliff's own discovery) |

The built-in renderer groups commits into Breaking changes (a `!` after the type, or a `BREAKING CHANGE` footer), Features,
Fixes, Refactoring, Chores and Other changes, strips the `type(scope):` prefix, shows the scope in bold, links `(#123)`
pull requests and ticket ids such as `PROJ-7`, and marks partly shipped commits.

With `--renderer git-cliff`, prdiff works out which commits are still new and git-cliff only formats them: the range
`base..head` is passed with a `--skip-commit` for every other commit (squashed, cherry-picked, merges, empty, filtered by
`--path`). prdiff runs it with `--no-exec`, so commands in a repository's `cliff.toml` are never executed.

## `view`

Write a self-contained HTML report. See [The HTML report](report-viewer.md) for what is in it.

```bash
prdiff view --open
prdiff view --out report.html --env production=origin/production --env staging=origin/staging
prdiff view --serve --port 8765
```

| Option | Meaning |
|---|---|
| `--out <file>` | Output file, or `-` for stdout (default `prdiff-report.html`) |
| `--open` | Open the report in the default browser |
| `--serve` | Serve on localhost instead of writing a file; the page reloads when base or head moves |
| `--port <n>` | Port for `--serve` (default 8765, 0 picks a free one) |
| `--max-lines <n>` | Diff lines shown per file (default 400) |
| `--no-sides` | Do not embed file contents (no side-by-side viewer); keeps the report small |
| `--max-embed-mb <n>` | Most file content to embed, in MB (default 8) |
| `--env <name=ref>` | Adds a Train tab. Repeatable. Default: `[[environments]]` in the config |
| `--old <ref>` | Adds a Rewritten tab: the previous version of the head |

## `where`

Which environments already have a commit.

```bash
prdiff where abc1234 --env production=origin/production --env staging=origin/staging
prdiff where '#4821' --head origin/develop --env production=origin/production
```

The argument is a revision, or `#123` (or `PR123`) for the commit whose message mentions that pull request, searched in
`--head`. For each environment the answer is one of: merged (an ancestor), there as a cherry-pick, there with the same
content, partly there, or not there yet.

| Option | Meaning |
|---|---|
| `--env <name=ref>` | An environment, repeatable, furthest along first. Default: `[[environments]]` in the config |
| `--format text\|json` | Output |

## `matrix`

Every head-only commit against every environment: the release train.

```bash
prdiff matrix --head origin/develop --env production=origin/production --env staging=origin/staging
prdiff matrix --format markdown
```

Rows are the head-only commits relative to the least advanced environment. The last line counts, per environment, how many
commits it is still missing. `--format text|json|markdown`.

## `conflicts`

List the files that would conflict, without touching the worktree, index, or object database of your repository.

```bash
prdiff conflicts --base origin/staging --head origin/develop --fail-on-conflict
```

`--format text|json`. With git older than 2.38 the answer comes from parsing the legacy `git merge-tree` output and is
marked best effort (`conflicts_best_effort` in JSON).

## `rewritten`

What changed between the old and new version of a force-pushed branch, using `git range-diff`.

```bash
prdiff rewritten --base origin/main --head origin/fix --old <old-sha>
prdiff rewritten --base origin/main --head origin/fix        # old defaults to origin/fix@{1}
```

| Option | Meaning |
|---|---|
| `--old <ref>` | The previous version. Default: `<head>@{1}` (the remote-tracking ref before your last fetch) |
| `--format text\|json` | Output |

Each pair is `SAME`, `CHANGED` (amended), `DROPPED` or `ADDED`; changed pairs carry the diff between the two versions.

## `check`

A CI gate: fail when a PR description lists commits that are already on base, or leaves out commits that still have to ship.

```bash
prdiff check --base origin/staging --head origin/develop --body body.md
prdiff check --pr 123
```

| Option | Meaning |
|---|---|
| `--body <file\|->` | The description to check. With `--pr` this defaults to the pull request's own body |

Commits are matched by SHA mentioned in the text (7 or more hex characters). Exit 5 prints which commits are missing and
which are stale.

## `publish`

Post the generated description to a pull request. Needs `gh`. Requires `--pr`.

```bash
prdiff publish --pr 123                    # into the PR body, between markers
prdiff publish --pr 123 --mode comment     # one sticky comment, updated in place
prdiff publish --pr 123 --dry-run          # print what would be posted
```

| Option | Meaning |
|---|---|
| `--mode body\|comment` | `body` replaces only the block between `<!-- prdiff:start -->` and `<!-- prdiff:end -->` (appending one if absent); `comment` finds its own comment by a marker and edits it, or creates it |
| `--title <text>` | Heading of the generated description |
| `--dry-run` | Print the result and change nothing |

## `completions` and `man`

```bash
prdiff completions bash|zsh|fish|powershell|elvish
prdiff man
```
