# Troubleshooting

Messages are printed as `prdiff: <message>` on stderr. The exit code tells you the class of problem (see
[Command reference](commands.md#exit-codes)).

## Errors

| Message | Exit | Cause and fix |
|---|---|---|
| `shallow clone: run \`git fetch --unshallow\` first` | 3 | The clone has truncated history, so there is no reliable merge base. In CI set `fetch-depth: 0` on `actions/checkout` |
| `unknown ref: origin/staging` | 2 | The ref does not exist locally. `git fetch origin` first, and check the spelling (`git branch -a`) |
| `no merge base between A and B` | 2 | The two branches share no history, so there is nothing to compare |
| `cannot infer --head (detached HEAD)` | 2 | Nothing is checked out by name. Pass `--head` or set `head` in `prdiff.toml` |
| `cannot infer --base` | 2 | No remote default branch was found. Pass `--base` or set `base` in `prdiff.toml` |
| `prdiff.toml: ...` | 2 | The config file does not parse. The message includes the line and column |
| `--env expects name=ref` | 2 | Write environments as `production=origin/production` |
| `no environments: pass --env name=ref ...` | 2 | `where` and `matrix` need at least one environment, from `--env` or `[[environments]]` |
| `cannot run \`gh\`` | 2 | The GitHub CLI is not installed (https://cli.github.com), needed for `--pr`, `publish` and `check --pr` |
| `gh ... failed: ...` | 3 | `gh` ran and GitHub said no. Check `gh auth status` and that the PR number exists |
| `cannot run \`git-cliff\`` | 2 | `--renderer git-cliff` needs git-cliff installed (`cargo install git-cliff`) |
| `git-cliff failed (...)` | 3 | git-cliff rejected its config or arguments; its own message follows |
| `cannot find the old version (X@{1}): pass --old` | 2 | `rewritten` could not find the previous tip. Pass `--old <sha>` (from the PR timeline or `git reflog show`) |
| `no commit mentioning #123 in <ref>` | 2 | `where '#123'` searches commit messages of `--head`. Use the commit's SHA, or the right `--head` |
| `publish needs --pr <number>` | 2 | |
| `check needs --body <file\|-> or --pr <number>` | 2 | |
| `N conflicted file(s)` | 4 | Raised by `--fail-on-conflict` |
| `description disagrees with the report (N missing, M stale)` | 5 | From `check`; the lines above it name the commits |
| `cannot listen on 127.0.0.1:8765` | 2 | The port is taken. Use `--port 0` to pick a free one |
| `git ... timed out after ...` | 3 | A single git call exceeded `--timeout` (default 300 seconds) |

## Surprising results

**A commit is `NEW` but I know it is on the target.**
It may have been rewritten rather than copied. `prdiff` recognises cherry-picks by identical patches and squash merges by
identical content, so a change that was reworked while it was moved looks new. Also check you compared the right base:
The text summary of `prdiff compare` prints the resolved SHAs of both branches. If the file was edited on the target afterwards, make sure you did not pass
`--shallow`, which turns off the hunk analysis that catches that.

**A commit is `SHIPPED` but I know it is not live.**
Hunk analysis is a heuristic that checks the changed lines are present, not that the whole file behaves the same. Use
`--strict` to treat partly matching commits as new, `--shallow` to run hunk analysis only on commits that already have an
identical file, or look at the commit in `prdiff view` where on-base hunks are dimmed so you can judge.

**A backport I merged back is not detected.**
The default search window covers base commits newer than the oldest head-only commit (minus two days). For older cherry-picks
use `--patch-id-scope all` (slower the first time, then cached).

**Everything shows as changed after a reformat.**
Use `-w`. Without it, indentation changes are real changes.

**The conflict list says "best effort".**
Your git is older than 2.38, so exact prediction is unavailable and the legacy text output is parsed instead. Upgrade git
for the exact answer.

**`prdiff` says there are no conflicts but the merge conflicts.**
Conflicts are predicted for merging head into base *as they are now*. If either branch moved, run `git fetch` and again.
`--exact-conflicts` skips the fast path that proves two sides cannot conflict, in case you want to compare the two answers.

**Numbers differ from `git log base..head`.**
That is the point: `git log` counts commits that are not reachable from base, and `prdiff` counts commits whose content is
not on base. See [Concepts](concepts.md).

## Speed

- The first run on a large history is slower (it computes patch-ids and caches them in `.git/prdiff/commits`); later runs on
  the same repository are much faster. Use `-v` to see where the time goes.
- `--patch-id-scope all` walks all of base's history. Keep the default unless you need old cherry-picks.
- `--shallow` skips the hunk analysis for `NEW` commits.
- In CI, restoring `.git/prdiff` between runs is optional; it is only a cache.

## The report is large or slow to open

The side-by-side viewer embeds both versions of every changed file. Use `--no-sides` to leave file contents out, `--max-embed-mb`
to lower the budget, or `--max-lines` to show fewer diff lines per file.

## Resetting

The only thing `prdiff` writes in your repository is `.git/prdiff/commits`. Delete the `.git/prdiff` directory at any time;
nothing depends on it.

## Still stuck

Run with `-v` and open an issue with the command, the message, `git --version` and `prdiff --version`.
