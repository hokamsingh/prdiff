# Getting started

## What you need

- **Git 2.38 or newer** for exact conflict detection. Older git works, but conflicts are then reported as best effort.
- A clone with **full history**. A shallow clone fails with exit code 3 (run `git fetch --unshallow`).
- Optional: the [GitHub CLI](https://cli.github.com) (`gh`) for `--pr` and `publish`, and [git-cliff](https://git-cliff.org)
  for `notes --renderer git-cliff`.

## Install

Pick one.

```bash
# Prebuilt binary into ~/.local/bin (checksum verified)
curl -fsSL https://raw.githubusercontent.com/hokamsingh/prdiff/main/install.sh | sh

# From source into ~/.cargo/bin (needs Rust)
cargo install --git https://github.com/hokamsingh/prdiff
```

Check it worked:

```bash
prdiff --version
```

The repository is currently private, which means anonymous downloads return 404. If that is still the case when you
read this, log in with `gh auth login` and install through the GitHub CLI instead:

```bash
gh api repos/hokamsingh/prdiff/contents/install.sh -H 'Accept: application/vnd.github.raw' | sh
```

`cargo install --git` works with whatever git credentials you already use for the repository.
Installer options: `PRDIFF_VERSION=v0.2.0` pins a release, `PRDIFF_BIN_DIR=/some/dir` changes the target directory.
Shell completions: `prdiff completions zsh > ~/.zfunc/_prdiff` (also bash, fish, powershell, elvish). Man page:
`prdiff man > prdiff.1 && man ./prdiff.1`.

## Your first run

Make sure the branches you compare are up to date (`git fetch origin`), then run it from inside the repository:

```bash
prdiff compare --base origin/staging --head origin/develop
```

`--base` is the branch the change would land on; `--head` is the branch carrying the change. If you leave them out,
`--head` is the branch you have checked out and `--base` is the remote's default branch.

You get a summary like this (example output from the benchmark repository described in [Architecture](architecture.md)):

```
base  origin/staging (f8c81d9a3a)
head  origin/develop (90ca5e2a11)
merge-base 27c51d5e00

commits: 150   NEW 90  PARTIAL 10  SHIPPED 30  PATCH_EQUIVALENT 20
  NEW      a01aeb5  feat: new work 140
  SHIPPED  951f107  feat: squashed part 22
  PATCH_EQ fdfb3fd  fix: backport candidate 47
  ...

files: 365 new to base (+365 -365), 136 already on base
  M pkg0/mod10/file180.txt  +1 -1
  ...

conflicts: none
```

Read it like this: `git log origin/staging..origin/develop` would list all 150 commits, but 50 of them are already on
`staging` (30 squash-merged, 20 cherry-picked). Only 100 are left to ship. [Concepts](concepts.md) explains each class.

## The five things you will do most

```bash
# 1. A PR description generated from the facts
prdiff describe --base origin/staging --head origin/develop --title "release: develop to staging" > body.md

# 2. Look at it in a browser: commits, files, side-by-side diffs, conflicts
prdiff view --base origin/staging --head origin/develop --open

# 3. Will it conflict?
prdiff conflicts --base origin/staging --head origin/develop

# 4. Release notes
prdiff notes --base origin/production --head origin/staging

# 5. Is this commit live yet?
prdiff where abc1234 --env production=origin/production --env staging=origin/staging
```

## Avoid typing the same flags

Create a `prdiff.toml` at the repository root:

```toml
base = "origin/staging"
head = "origin/develop"
exclude = ["**/Cargo.lock"]

[[environments]]
name = "production"
ref = "origin/production"
[[environments]]
name = "staging"
ref = "origin/staging"
```

Now `prdiff compare` and `prdiff matrix` need no flags. See [Configuration](configuration.md).

## Next

- [Concepts](concepts.md): what the four classes mean and where the edge cases are.
- [Recipes](recipes.md): release PRs, release trains, force-pushed branches.
- [CI and GitHub](ci-and-github.md): keep PR descriptions honest automatically.
