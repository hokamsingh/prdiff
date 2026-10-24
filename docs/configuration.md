# Configuration

`prdiff` reads `prdiff.toml` from the repository root (the top level of the working tree) if it exists. Use `--config <file>`
to point at another file. Anything you pass on the command line wins over the file. An invalid file is a usage error
(exit code 2) and the message names the file.

```toml
# Defaults for --base and --head
base = "origin/staging"
head = "origin/develop"

# Only consider these paths (globs). --path on the command line replaces this list.
paths = ["src/**", "services/**"]

# Left out of file lists and stats. Commits are still classified on all of their files.
# --exclude on the command line adds to this list.
exclude = ["**/Cargo.lock", "**/package-lock.json", "**/*.snap"]

# Extra files to flag as risky, on top of the built-in rules (see below)
risky = ["infra/**", "billing/**"]

# Treat whitespace-only differences as no difference (same as -w)
ignore_whitespace = false

# The release train, furthest along first. Used by `matrix`, `where` and the Train tab of `view`.
[[environments]]
name = "production"
ref = "origin/production"

[[environments]]
name = "staging"
ref = "origin/staging"

# Used by `notes`
[notes]
repo_url = "https://github.com/acme/app"
ticket_url = "https://jira.example.com/browse/{id}"
```

## Keys

| Key | Type | Used by | Notes |
|---|---|---|---|
| `base`, `head` | string | every comparing command | refs; resolved to SHAs once per run |
| `paths` | list of globs | every comparing command | applied to commits and to the net diff |
| `exclude` | list of globs | net file list, stats, risk | not applied when classifying commits |
| `risky` | list of globs | risk flags | adds a `custom` flag |
| `ignore_whitespace` | bool | every comparing command | |
| `[[environments]]` | table list | `matrix`, `where`, `view` | `name` and `ref`; order matters |
| `notes.repo_url` | string | `notes` | base for `(#123)` links; default is derived from `origin` |
| `notes.ticket_url` | string | `notes` | must contain `{id}` |

## Glob syntax

Used by `--path`, `--exclude`, `paths`, `exclude` and `risky`.

| Pattern | Matches |
|---|---|
| `src/**` | everything under `src/` at any depth |
| `*.md` | any `.md` file at any depth (a pattern with no `/` matches at any depth) |
| `docs/` | everything under `docs/` (a trailing `/` means the directory) |
| `src/*/mod.rs` | one directory level only: `*` and `?` do not cross `/` |

## Built-in risk flags

Files that are new to base can carry flags, shown in `describe`, `compare` (text), and the Files tab of `view`.

| Flag | Fires on |
|---|---|
| `migration` | `migration/` or `migrations/` directories, `db/migrate/`, `alembic/versions/`, `*.sql`, `schema.rb`, `schema.prisma` |
| `lockfile` | `package-lock.json`, `yarn.lock`, `pnpm-lock.yaml`, `Cargo.lock`, `go.sum`, `poetry.lock`, `Gemfile.lock`, `composer.lock`, `Pipfile.lock` |
| `dependencies` | `package.json`, `Cargo.toml`, `go.mod`, `requirements*.txt`, `pyproject.toml`, `Gemfile`, `pom.xml`, `build.gradle` |
| `deploy-config` | `Dockerfile*`, `docker-compose*.yml`, `.github/workflows/*`, `.gitlab-ci.yml`, `Jenkinsfile`, `k8s/`, `kubernetes/`, `helm/`, `terraform/`, `deploy/`, `infra/`, `*.tf` |
| `env-config` | `.env*`, `config/production*`, `config/staging*` |
| `secrets` | `*.pem`, `*.key`, `*.p12`, `*.pfx`, `*.jks`, `id_rsa*`, `id_ed25519*`, and files or directories named `credentials`, `secret` or `secrets` |
| `large-diff` | 500 or more changed lines |
| `hot-file` | touched by 5 or more head-only commits |
| `many-authors` | touched by 3 or more distinct authors |
| `custom` | matches a glob in `risky` |

The thresholds are fixed in this version.

## CODEOWNERS

`prdiff` reads `.github/CODEOWNERS`, `CODEOWNERS` or `docs/CODEOWNERS` (the first that exists) **from the base branch**, so
the rules that apply are the ones on the branch you are merging into. The last matching rule wins. A leading `/` or an inner
`/` anchors the pattern to the repository root; a trailing `/` means everything under the directory. The owners of every
file still to ship appear in `describe` ("Review needed from"), on file rows in `view`, and in the owner filter. Turn it off
with `--no-owners`.

## Files `prdiff` writes

Only `.git/prdiff/commits`: an append-only cache of per-commit facts keyed by commit SHA. A history rewrite just misses
the cache. It is safe to delete. `--no-cache` neither reads nor writes it. Nothing else in your repository is modified;
conflict detection uses a throwaway object directory (tmpfs on Linux).
