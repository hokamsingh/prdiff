# Recipes

## A release PR description that tells the truth

You merge `develop` into `staging` every week. Squash merges and hotfix cherry-picks make `git log` useless.

```bash
git fetch origin
prdiff describe --base origin/staging --head origin/develop --title "release: develop to staging" > body.md
```

Paste `body.md` into the PR, or let `prdiff` do it: `prdiff publish --pr 123`. Add a `prdiff check --pr 123` job so the
description cannot go stale when new commits land. See [CI and GitHub](ci-and-github.md).

Before you merge, open the report and look at the conflicts and the files marked risky:

```bash
prdiff view --base origin/staging --head origin/develop --open
```

## Is this fix live yet?

```bash
prdiff where '#4821' --head origin/develop --env production=origin/production --env staging=origin/staging
```

```
a1b2c3d fix: stop double-charging on retry (#4821)
  ✗ production     not there yet
  ✓ staging        merged
```

`#4821` finds the commit whose message mentions that pull request; a plain SHA works too. Put the environments in
`prdiff.toml` to drop the `--env` flags. A fix can also be there as a cherry-pick or with the same content (a squash merge),
and `where` says which.

## See the whole release train

```bash
prdiff matrix --head origin/develop --env production=origin/production --env staging=origin/staging
```

One row per commit, one column per environment, and a last line with how many commits each environment is missing.
`--format markdown` gives a table for a wiki or a ticket. In the HTML report the same data is the Train tab.

## Review a force-pushed pull request

The author rebased and force-pushed. What changed since you last looked?

```bash
git fetch origin
prdiff rewritten --base origin/main --head origin/fix-login          # old = origin/fix-login@{1}
prdiff rewritten --base origin/main --head origin/fix-login --old 3f2a9c1
```

Each commit is unchanged, amended, dropped or added, and amended ones show the diff between the two versions. `@{1}` only
works if you fetched before the force-push; otherwise pass the old SHA (it is in the PR's timeline, or `git reflog show`).

## Will it conflict, and who touched the file?

```bash
prdiff conflicts --base origin/staging --head origin/develop --fail-on-conflict
```

Use it as a gate before a release branch is cut: exit code 4 means conflicts. The report lists, for each conflicted file, the
commits on each side that touched it, which tells you who to ask.

## A monorepo

Limit the noise with a `prdiff.toml` at the root:

```toml
exclude = ["**/package-lock.json", "**/*.snap", "gen/**"]
risky = ["services/billing/**", "infra/**"]

[[environments]]
name = "production"
ref = "origin/production"
[[environments]]
name = "staging"
ref = "origin/staging"
```

And scope a single run to one service:

```bash
prdiff describe --path 'services/payments/**' --base origin/staging --head origin/develop
```

CODEOWNERS on the base branch tells you which teams have to review the release ("Review needed from" in the description, an
owner filter in the report).

## Release notes

```bash
prdiff notes --base origin/production --head origin/staging
```

Breaking changes first, then Features, Fixes and so on, with `(#123)` links and ticket links:

```bash
prdiff notes --base origin/production --head origin/staging \
  --repo-url https://github.com/acme/app --ticket-url 'https://jira.example.com/browse/{id}'
```

If your team already uses [git-cliff](https://git-cliff.org), keep its templates and let `prdiff` decide which commits
are new:

```bash
prdiff notes --base origin/production --head origin/staging --renderer git-cliff --cliff-config cliff.toml
```

## Post the summary to Slack

```bash
prdiff describe --format slack --base origin/staging --head origin/develop \
  | jq -Rs '{text: .}' \
  | curl -sS -X POST -H 'Content-type: application/json' --data @- "$SLACK_WEBHOOK_URL"
```

The `--format slack` output and the `jq` step are tested; posting to a live Slack webhook is not.

## A reformat should not count

A style-only commit (indentation, line wrapping) shows up as `NEW` and makes files look changed. Ask for whitespace to be
ignored:

```bash
prdiff compare -w --base origin/staging --head origin/develop
```

A commit whose only changes are whitespace becomes `SHIPPED`, and a file whose changes are whitespace only is not counted as
new to base.

## Scripting

```bash
prdiff compare --format json | jq -r '.commits[] | select(.class=="NEW" or .class=="PARTIAL") | "\(.short) \(.subject)"'
```

See [JSON output](json-output.md) for every field, and [Command reference](commands.md) for exit codes.
