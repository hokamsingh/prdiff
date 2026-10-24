# JSON output

`prdiff compare --format json` prints the full report. `prdiff view` embeds the same report, plus extra fields, in the HTML
page. Every count in `totals` is derived from `commits` and `net_files`, so consumers never need to recompute.

Compatibility: the report carries `"schema_version": 1`. New fields may be added within a schema version, so ignore fields you
do not know. Fields marked *(optional)* are left out when empty.

## Report

| Field | Type | Meaning |
|---|---|---|
| `schema_version` | number | `1` |
| `base`, `head` | `{name, sha}` | the refs as given and the commits they resolved to |
| `merge_base` | string | SHA of the common ancestor |
| `commits` | array | one entry per head-only commit, oldest first (see below) |
| `net_files` | array | one entry per file that differs between the merge-base and head |
| `revert_pairs` | array | `{revert, reverted}` SHAs where a listed commit reverts another listed commit |
| `conflicts` | array of strings | files that would conflict |
| `conflict_details` | array *(optional)* | per conflicted file: `path`, `head_commits` (SHAs), `base_commits` (`{sha, subject, author}`, newest first, at most 5) |
| `conflicts_best_effort` | bool | true when conflicts came from the pre-2.38 `git merge-tree` text format |
| `owners_summary` | array *(optional)* | `{owner, files}`: files still to ship per CODEOWNERS owner, most files first |
| `totals` | object | counts (see below) |

### `commits[]`

| Field | Type | Meaning |
|---|---|---|
| `sha`, `short` | string | full SHA and its first 7 characters |
| `subject`, `author` | string | |
| `kind` | string | `FEAT`, `FIX`, `REFACTOR`, `CHORE` or `OTHER`, parsed from a `type:` or `type(scope):` prefix |
| `class` | string | `NEW`, `PARTIAL`, `SHIPPED` or `PATCH_EQUIVALENT` |
| `matched_base_commit` | string or null | for `PATCH_EQUIVALENT`, the base commit carrying the same patch |
| `reverts` | string or null | SHA this commit reverts (from `This reverts commit <sha>`) |
| `scope` | string *(optional)* | `api` in `feat(api): ...` |
| `breaking` | true *(optional)* | `!` after the type, or a `BREAKING CHANGE` footer |
| `pr` | number *(optional)* | `123` from `(#123)` or `Merge pull request #123` |
| `tickets` | array *(optional)* | ids such as `PROJ-7` found in the subject |
| `files` | array | files the commit touches (below) |
| `hunks_total`, `hunks_already_on_base` | number | summed over its files |

### `commits[].files[]`

| Field | Type | Meaning |
|---|---|---|
| `path`, `old_path` | string, string or null | `old_path` is set for renames |
| `change` | string | `A` added, `M` modified, `D` deleted, `R` renamed, `T` type change |
| `status` | string | `IDENTICAL` (same blob on base), `DIFFERENT` (base has other content), `ABSENT` (base does not have the file) |
| `submodule`, `binary` | bool | |
| `hunks_total`, `hunks_already_on_base` | number | `0` for binary files and submodules |

### `net_files[]`

| Field | Type | Meaning |
|---|---|---|
| `path`, `old_path`, `change` | | as above |
| `state` | string | `NEW_TO_BASE` (still to ship) or `ALREADY_ON_BASE` |
| `insertions`, `deletions` | number or null | `null` for binary files |
| `binary`, `submodule` | bool | |
| `risk` | array *(optional)* | flags such as `migration`, `lockfile`, `large-diff` (see [Configuration](configuration.md)) |
| `owners` | array *(optional)* | CODEOWNERS owners |

### `totals`

| Field | Meaning |
|---|---|
| `commits` | head-only commits in the report |
| `by_class` | `{NEW, PARTIAL, SHIPPED, PATCH_EQUIVALENT}` counts; they sum to `commits` |
| `listed` | `NEW` + `PARTIAL`: the commits a description lists |
| `listed_by_kind` | `{FEAT, FIX, REFACTOR, CHORE, OTHER}` counts over the listed commits |
| `empty_skipped` | empty commits left out (unless `--include-empty`) |
| `files_new_to_base`, `files_already_on_base` | split of `net_files` by `state` |
| `insertions`, `deletions` | summed over `NEW_TO_BASE` files only |
| `conflicted_files` | `conflicts.length` |
| `risky_files` | `NEW_TO_BASE` files with at least one risk flag |

## Extra fields in `view`

Only the HTML report's embedded data carries these: `commits[].files[].hunk_details[]` (`header`, `on_base`, `lines`,
`truncated`), `file_sides[]` (the two versions of each changed file and the line diff between them), `train` (the matrix
below) when environments are given, and `rewritten` when `--old` is given.

## Other commands

**`matrix --format json`**: `{schema_version, head, envs: [{name, ref, sha, missing}], rows: [{sha, short, subject, kind, states}]}`.
`states` has one entry per environment, in `envs` order: `MERGED`, `PATCH_EQUIVALENT`, `SHIPPED`, `PARTIAL` or `NOT_PRESENT`.

**`where --format json`**: `{schema_version, commit, short, subject, envs: [{name, ref, sha, state}]}`, with the same states.

**`rewritten --format json`**: `{schema_version, base, old, new, pairs: [{status, old_index, new_index, old_sha, new_sha, subject, interdiff?}]}`
where `status` is `SAME`, `CHANGED`, `DROPPED` or `ADDED`.

**`notes --format json`**: `{schema_version, base, head, groups: [{name, entries: [{sha, scope, description, pr, tickets, partial}]}]}`.

**`conflicts --format json`**: `{schema_version, base, head, conflicts, conflicts_best_effort}`.

## Examples

```bash
# Commits still to ship
prdiff compare --format json | jq -r '.commits[] | select(.class=="NEW" or .class=="PARTIAL") | "\(.short) \(.subject)"'

# Risky files and why
prdiff compare --format json | jq '.net_files[] | select(.risk != null) | {path, risk}'

# Fail a script when something already shipped is about to be listed twice
test "$(prdiff compare --format json | jq '.totals.by_class.SHIPPED')" -eq 0
```
