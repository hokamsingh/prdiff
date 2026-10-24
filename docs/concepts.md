# Concepts

## The problem

`git log base..head` lists the commits on `head` that are not **reachable** from `base`. That is not the same as the changes
that are not on `base`. It over-reports in four common cases:

1. **Squash merges.** A feature merged to `staging` as one commit has a new hash and subject, so its original commits still
   look "not on staging".
2. **Cherry-picks and backports.** The same patch under a different hash is counted twice.
3. **Reverts.** A change and its revert both show up and net to zero.
4. **Already-shipped content.** A commit's files can be byte-identical on the target under another subject.

The result is a release PR that lists work which is already live, and counts that disagree with the diff.
`prdiff` answers the question that matters: *what really changes when `head` lands on `base`?*

## Vocabulary

- **base** is the branch the change would land on (the target), for example `origin/staging`.
- **head** is the branch carrying the change, for example `origin/develop` or a PR branch.
- **merge-base** is the common ancestor of the two.
- A **head-only commit** is a non-merge commit reachable from head but not from base. Merge commits are skipped because
  their changes appear in their parents.

## The four classes

Every head-only commit gets exactly one class.

| Class | Plain meaning | How it is decided |
|---|---|---|
| `PATCH_EQUIVALENT` | The same change is already on base as a different commit (a cherry-pick) | its patch-id matches a commit on base |
| `SHIPPED` | Everything the commit changed is already on base | every touched file has the same blob on base, or every hunk is present there |
| `PARTIAL` | Some of the commit is already on base, some is not | some files or hunks match, others do not |
| `NEW` | Nothing of it is on base yet | none match |

A commit **needs to be listed in a PR description** if it is `NEW` or `PARTIAL`. `SHIPPED` and `PATCH_EQUIVALENT` commits are
excluded, and the description says how many were excluded and why. Every count is derived from the same lists, so a
description cannot disagree with itself.

### Examples

**Squash merge.** `develop` has commits `feat: add a` and `feat: add b`. Someone squash-merged that work onto `staging` as
`Merge feature (#12)` with identical files. Both commits are `SHIPPED`, whatever the subjects say.

**Cherry-pick.** A hotfix is committed on `develop` and cherry-picked onto `staging` after other edits to the same file, so
the files differ but the patch is the same. The commit is `PATCH_EQUIVALENT`.

**Half shipped.** A commit touched `one.txt` and `two.txt`. `staging` already has `one.txt` from an earlier release.
The commit is `PARTIAL`, and `prdiff` reports how many of its hunks are already there.

**Edited since.** `develop` fixed line 3 of a file. `staging` got the same fix and then someone edited line 7.
The file blobs differ, so a blob-only check would call the commit `NEW`. Hunk analysis sees that the line 3 change is
already present and calls it `SHIPPED`.

## How a commit is judged

The stages run in this order. Details of the implementation are in [Architecture](architecture.md).

1. **Patch equivalence.** A stable patch-id is computed for each head-only commit and compared with patch-ids of commits on
   base. To keep it fast, a base commit is only patch-id'd if it touches exactly the same set of paths as some head-only
   commit. Results are cached per commit.
2. **Content identity.** For each file a commit touches, `prdiff` compares the blob after the commit with the blob on base's
   tip. Same blob means the file is `IDENTICAL`. Blob ids are compared, never contents, so this is fast.
3. **Hunk analysis.** For commits that are not fully decided, each hunk is checked in memory against base's version of the
   file: a hunk is *already on base* when its after-image is there and its before-image is gone. If only the surrounding
   context changed, a replacement hunk (removed and added lines) is still recognised by its own lines.
4. **Net diff.** The diff from the merge-base to head gives the files that change. A file is `ALREADY_ON_BASE` if base's
   blob equals head's, or if every head-only commit touching it is itself `SHIPPED` or `PATCH_EQUIVALENT`; otherwise it is
   `NEW_TO_BASE`.
5. **Conflicts.** `git merge-tree --write-tree` predicts conflicting files without touching the worktree or index.

## Things that surprise people

- **Hunk analysis is a heuristic.** It checks that the lines are present, not that a full `git apply` would succeed.
  It errs towards "not on base": pure additions or deletions whose context changed are reported as not on base.
  `--shallow` turns off hunk analysis for `NEW` commits; `--strict` turns every `PARTIAL` into `NEW`.
- **Behaviour is not compared.** A rewritten function with the same behaviour is `NEW`.
- **Content is judged against base's tip.** If base had a change and later removed it, the file no longer matches, so the
  commit is not `SHIPPED`. **Patch equivalence looks at commits instead**, so a cherry-pick that base later reverted is
  still `PATCH_EQUIVALENT`.
- **Renames** are followed (`-M`). A renamed file counts as present on base only if the old path is gone too.
- **Binary files** are compared by blob id only; submodules by gitlink id.
- **Empty commits** are skipped by default (`--include-empty` keeps them).
- **Revert pairs** are detected from `This reverts commit <sha>` and reported together; the net diff shows zero for them.
- **Whitespace.** `-w` treats whitespace-only differences as no difference. Older and newer git differ in whether a
  whitespace-only file appears in the raw diff at all; `prdiff` treats both the same way.
- **The cherry-pick search window.** By default only base commits newer than the oldest head-only commit (minus two days of
  clock skew) are searched. That catches backports that were merged back into head, and is fast. `--patch-id-scope all`
  searches all of base's history; `base-only` searches only commits that are on base but not on head.

## Files, not just commits

The **net diff** is reported per file: `NEW_TO_BASE` (still to ship) or `ALREADY_ON_BASE`, with insertions and deletions
counted only for files still to ship. Files can also carry **risk flags** and **owners**, and `--path` and `--exclude` limit
what is listed. See [Command reference](commands.md) and [The HTML report](report-viewer.md).
