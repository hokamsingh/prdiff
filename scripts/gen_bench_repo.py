#!/usr/bin/env python3
"""Generate a synthetic monorepo-shaped history for benchmarking prdiff, via `git fast-import`.

  staging  = base: N_COMMON shared commits, then more commits after develop branched off
  develop  = head: 150 commits touching ~500 files, of which (by construction):
               SQUASH   commits are squash-merged onto staging (same blobs, different commits) -> SHIPPED
               PICKED   commits are cherry-picked onto staging, while staging edited other lines  -> PATCH_EQUIVALENT
               PARTIAL  commits touch 2 files, one of which staging already has                    -> PARTIAL
               the rest are only on develop                                                        -> NEW
Writes <dir>/expected.json with the counts the tool should report.
"""
import json, os, random, subprocess, sys

out = sys.argv[1]
n_common = int(sys.argv[2]) if len(sys.argv) > 2 else 20000
n_after = int(sys.argv[3]) if len(sys.argv) > 3 else 1500
N_FILES = 3000
LINES = 40
rnd = random.Random(42)

if os.path.exists(out):
    sys.exit(f"{out} exists")
os.makedirs(out)
subprocess.check_call(["git", "init", "-q", "-b", "staging", out])
fi = subprocess.Popen(["git", "-C", out, "fast-import", "--quiet", "--max-pack-size=1g"], stdin=subprocess.PIPE)
w = fi.stdin.write

def data(s):
    b = s.encode()
    w(b"data %d\n" % len(b)); w(b); w(b"\n")

mark = [0]
t = [1_650_000_000]

def commit(ref, msg, files, frm=None):
    mark[0] += 1
    t[0] += 60
    w(f"commit refs/heads/{ref}\nmark :{mark[0]}\n".encode())
    w(f"author Dev <dev@example.com> {t[0]} +0000\ncommitter Dev <dev@example.com> {t[0]} +0000\n".encode())
    data(msg)
    if frm:
        w(f"from :{frm}\n".encode())
    for path, lines in files.items():
        w(f"M 100644 inline {path}\n".encode()); data("".join(lines))
    return mark[0]

def base_file(i):
    return [f"file {i} line {j}\n" for j in range(LINES)]

state = {}   # staging content
def f_path(i):
    return f"pkg{i % 60}/mod{i % 17}/file{i}.txt"

# commit 0: every file
for i in range(N_FILES):
    state[f_path(i)] = base_file(i)
last = commit("staging", "chore: init monorepo", dict(state))

TOP = range(0, LINES // 2)           # staging edits the top half of shared files
BOT = range(LINES // 2, LINES)       # develop edits the bottom half

def edit(content, line, text):
    c = list(content); c[line] = text + "\n"; return c

n = 0
def staging_noise(k):
    global last, n
    for _ in range(k):
        n += 1
        files = {}
        for _ in range(rnd.randint(1, 3)):
            p = f_path(rnd.randrange(N_FILES))
            state[p] = edit(state[p], rnd.choice(TOP), f"staging edit {n}")
            files[p] = state[p]
        last = commit("staging", f"fix: staging change {n}", files)

staging_noise(n_common)
branch_point = last

# --- develop plan: 150 commits, ~500 file touches
plan = []
free = list(range(N_FILES))
rnd.shuffle(free)
def take(k):
    return [f_path(free.pop()) for _ in range(k)]

squash_groups = [[take(rnd.randint(3, 4)) for _ in range(6)] for _ in range(5)]   # 30 commits in 5 squashes
picked = [take(1)[0] for _ in range(20)]                                              # 20 cherry-picks
partial = [take(2) for _ in range(10)]                                                # 10 partial
rest_n = 150 - 30 - 20 - 10
rest = [take(rnd.randint(3, 5)) for _ in range(rest_n)]

dev_state = dict(state)
dev_last = branch_point
dev_commits = []   # (kind, files{path:lines}, msg)

def dev_edit(paths, tag):
    files = {}
    for p in paths:
        dev_state[p] = edit(dev_state[p], rnd.choice(BOT), f"develop {tag}")
        files[p] = dev_state[p]
    return files

k = 0
for grp in squash_groups:
    for paths in grp:
        k += 1
        dev_commits.append(("squash", dev_edit(paths, k), f"feat: squashed part {k}"))
for p in picked:
    k += 1
    line = LINES - 1
    dev_state[p] = edit(dev_state[p], line, f"picked {k}")
    dev_commits.append(("picked", {p: dev_state[p]}, f"fix: backport candidate {k}"))
for a, b in partial:
    k += 1
    dev_commits.append(("partial", dev_edit([a, b], k), f"feat: partial {k}"))
for paths in rest:
    k += 1
    dev_commits.append(("new", dev_edit(paths, k), f"{rnd.choice(['feat', 'fix', 'refactor', 'chore'])}: new work {k}"))
rnd.shuffle(dev_commits)
touched = {p for _, f, _ in dev_commits for p in f}

# develop's commits
t[0] = t[0] + 60
dev_marks = {}
dev_last = branch_point
first = True
for idx, (kind, files, msg) in enumerate(dev_commits):
    dev_last = commit("develop", msg, files, frm=branch_point if first else None)
    first = False
    dev_marks[idx] = dev_last

# staging continues: squashes, cherry-picks, partial overlap, plus noise
for gi, grp in enumerate(squash_groups):
    files = {}
    for paths in grp:
        for p in paths:
            files[p] = dev_state[p]
    for p, c in files.items():
        state[p] = c
    last = commit("staging", f"Merge pull request #{100 + gi} from team/feature-{gi}", files)
    staging_noise(n_after // 40)
for p in picked:
    state[p] = edit(state[p], rnd.choice(TOP), "staging touched too")   # blobs end up different...
    last = commit("staging", "chore: unrelated edit to a picked file", {p: state[p]})
    state[p] = edit(state[p], LINES - 1, next(f for kk, f, m in dev_commits if p in f and kk == "picked")[p][LINES - 1].rstrip("\n"))
    last = commit("staging", "fix: cherry-picked backport", {p: state[p]})
    staging_noise(n_after // 60)
for a, b in partial:
    state[a] = dev_state[a]
    last = commit("staging", "chore: ship one half early", {a: state[a]})
    staging_noise(n_after // 80)
staging_noise(n_after // 4)

fi.stdin.close(); fi.wait()

json.dump({
    "common_commits": n_common,
    "develop_commits": len(dev_commits),
    "files_touched_on_develop": len(touched),
    "expected": {"SHIPPED": 30, "PATCH_EQUIVALENT": 20, "PARTIAL": 10, "NEW": rest_n},
}, open(os.path.join(out, "expected.json"), "w"), indent=1)
print(f"wrote {out}: {n_common}+ staging commits, 150 develop commits, {len(touched)} files touched")
