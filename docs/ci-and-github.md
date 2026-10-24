# CI and GitHub

Three jobs `prdiff` can do for a pull request: **check** that its description is honest, **publish** a generated description, or
**summarise** it in the job log. All of them are available as plain commands and as a GitHub Action.

Everything on this page that talks to GitHub goes through the [GitHub CLI](https://cli.github.com) (`gh`), which brings its own
authentication (locally from `gh auth login`, in Actions from `GH_TOKEN`).

> **Status:** the GitHub integration is tested against a fake `gh` and against a local bare repository, not against a live pull
> request. `action.yml` has not been run on GitHub yet. Try it on a throwaway PR first.

## Compare a pull request

`--pr N` works with every command: it asks `gh` for the PR's base branch, fetches the base branch and `refs/pull/N/head`
from `origin`, and compares them. The head is shown under the PR's own branch name.

```bash
prdiff compare --pr 123
prdiff view --pr 123 --open
```

## Check that a description is honest

```bash
prdiff check --pr 123                          # uses the PR's own body
prdiff check --base origin/staging --head origin/develop --body body.md
```

`check` looks for commit SHAs (7 or more hex characters) in the text. It fails with exit code 5 when:

- a commit that still has to ship (`NEW` or `PARTIAL`) is not mentioned, or
- a commit that is already on base (`SHIPPED` or `PATCH_EQUIVALENT`) is mentioned.

It prints which commits are missing and which are stale. The description that `describe` generates always passes.

## Publish a generated description

```bash
prdiff publish --pr 123                    # into the PR body, between markers
prdiff publish --pr 123 --mode comment     # one sticky comment, updated in place
prdiff publish --pr 123 --dry-run          # show what would be posted
```

- **`--mode body`** replaces only the block between `<!-- prdiff:start -->` and `<!-- prdiff:end -->`, so text you wrote around
  it is kept. If the markers are not there yet, the block is appended. Running it again replaces the block, not duplicates it.
- **`--mode comment`** finds its own comment by a hidden marker and edits it, or creates it on the first run.

## The GitHub Action

```yaml
# .github/workflows/prdiff.yml
name: prdiff
on: pull_request
permissions:
  contents: read
  pull-requests: write      # needed for body and comment modes
jobs:
  prdiff:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0     # prdiff needs full history; a shallow clone fails
      - uses: hokamsingh/prdiff@main
        with:
          mode: comment
```

| Input | Default | Meaning |
|---|---|---|
| `mode` | `check` | `check` fails the job when the description is stale; `body` writes the description into the PR body; `comment` posts or updates one comment; `summary` writes the description to the job summary only |
| `pr` | the triggering PR | pull request number |
| `version` | `latest` | which release of `prdiff` to install |
| `args` | empty | extra arguments, e.g. `--exclude Cargo.lock` |

The action installs a prebuilt binary with `install.sh` and then runs `prdiff check`, `prdiff publish` or `prdiff describe`.
If the repository is private, other repositories can only use the action when the repository's Actions access settings allow it.

A common setup is two jobs: `mode: summary` on every PR so reviewers see the real scope, and `mode: check` on release PRs.

### Pull requests from forks

A workflow triggered by `pull_request` from a fork has a read-only token, so `body` and `comment` modes cannot write. Use
`check` or `summary` there. `prdiff` runs `git-cliff` with `--no-exec`, so a pull request cannot make it run commands through
a `cliff.toml`.

## Releasing `prdiff` itself

Binaries are built by `.github/workflows/release.yml` for Linux (x86_64 and arm64, static musl builds) and macOS (Intel and
Apple silicon), each with a `.sha256` file, and attached to a GitHub release. The workflow refuses to publish when the tag does
not match the version in `Cargo.toml`.

1. Set `version` in `Cargo.toml`, commit, and push to `main`.
2. Either push a tag, or run the workflow by hand and let it create the tag:

```bash
git tag v0.3.0 && git push origin v0.3.0
# or
gh workflow run release.yml -f version=v0.3.0
```

The installer (`install.sh`) downloads the right archive for the machine, verifies the checksum, and installs to
`~/.local/bin`. For a private repository it reads the release through `gh api` when the GitHub CLI is logged in.

## Rendering a Homebrew formula

`packaging/homebrew/render.sh v0.3.0` prints a formula from the checksums published with a release. It needs a separate
`homebrew-tap` repository to be useful; that repository does not exist yet.
