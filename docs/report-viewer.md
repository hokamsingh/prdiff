# The HTML report (`prdiff view`)

`prdiff view` writes one self-contained HTML file. It needs no network, server or build step, works offline, and follows
your system's light or dark theme. Open it in any browser, attach it to a ticket, or keep it as a CI artifact.

```bash
prdiff view --base origin/staging --head origin/develop --open
```

By default it is written to `prdiff-report.html` in the current directory (`--out <file>`, or `--out -` for stdout).

![the report](view.png)

## Tabs

### Commits

One row per head-only commit, with its class (New, Partial, Shipped, Patch-equivalent), short SHA, subject, kind, number of
files, and a bar showing how much of it is already on base. The four class chips at the top filter the list. A row expands
to its files and hunks; hunks that are already on base are dimmed and labelled. Rows also show *reverts / reverted by* links
and, for a cherry-pick, the base commit carrying the same patch.

Controls: a text filter (press `/`), a kind filter, *Hide already on base*, and *Expand all*.

### Files

The net diff: every file that changes between the merge-base and head, marked *New to base* or *Already on base*, with
insertions and deletions, the commits behind it, and:

- **Risk badges** (migration, lockfile, large-diff and so on; see [Configuration](configuration.md)).
- **Owners** from CODEOWNERS, with an owner filter.

### Train (with `--env` or `[[environments]]`)

A table of every commit against every environment: merged, cherry-pick, same content, partial, or not yet, with a count of
what each environment is missing.

### Rewritten (with `--old <ref>`)

The previous and the current version of a force-pushed head, paired by `git range-diff`: unchanged, amended, dropped or
added, with the diff between the two versions of an amended commit.

## Side by side

Every changed file has a **Side by side** button (inside an expanded commit, and on each row of the Files tab). It opens a
full-screen viewer:

- **Left:** the file as it is *right now on the target branch*. **Right:** the file as the merging branch has it. A file
  that does not exist on one side shows an empty side; a renamed file is shown under its old name on the left.
- Unchanged runs are collapsed (click one to expand, or untick *Collapse unchanged*). *Prev* and *Next* jump between
  changes. `Esc` closes the viewer.
- The **merge-base** is used as a third copy to tell apart three kinds of difference:

| Colour | Meaning |
|---|---|
| red and green | changed by this PR |
| amber | changed on the target branch since the PR branch was cut; not part of this PR |
| amber with a red edge | changed on both sides; a manual merge is likely |

The header counts each kind. One approximation: if the target changed a line and the PR deleted that same line, it is shown
as target-only.

Binary files get no viewer. Files larger than 512 KB per side, or beyond `--max-embed-mb` (default 8) in total, show a note
with the `git diff` command to run instead. `--no-sides` leaves file contents out entirely, which makes the report much
smaller.

![side by side](side-by-side.png)

## Conflicts

A banner at the top says whether base and head conflict. If they do, each conflicted file lists the commits on this PR and
the newest commits on the target that touched it.

## Live mode

```bash
prdiff view --serve --port 8765 --open
```

Serves the report on `127.0.0.1` instead of writing a file. The page is rebuilt on each request and reloads itself when base
or head moves (for instance after you `git fetch`). Stop it with Ctrl-C.

## Security notes

Commit subjects, file names and file contents are embedded as JSON and rendered as text, never as HTML, so a hostile commit
message cannot inject markup. The `<` character is escaped inside the embedded data. The serve mode listens on localhost only.
The report contains the contents of your changed files; treat it like source code when sharing it.
