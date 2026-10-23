#!/usr/bin/env bash
# Usage: scripts/bench.sh <repo-dir> [runs]   (repo from gen_bench_repo.py; branches staging=base, develop=head)
set -euo pipefail
repo=${1:?repo dir}; runs=${2:-10}
bin=${PRDIFF:-$(cd "$(dirname "$0")/.." && pwd)/target/release/prdiff}
cd "$repo"

# median wall time in ms of `runs` executions of the given command
bench() {
  local pre=$1; shift
  local ts=()
  for _ in $(seq "$runs"); do
    eval "$pre"
    local s=$(date +%s%N)
    "$@" >/dev/null
    ts+=($(( ($(date +%s%N) - s) / 1000 )))
  done
  printf '%s\n' "${ts[@]}" | sort -n | awk '{a[NR]=$1} END {printf "%8.1f ms median  (min %.1f, max %.1f)\n", a[int((NR+1)/2)]/1000, a[1]/1000, a[NR]/1000}'
}

echo "history: $(git rev-list --count staging) commits on base, $(git rev-list --count staging..develop) head-only, $(git diff --name-only "$(git merge-base staging develop)" develop | wc -l) files in net diff"
echo
printf '%-52s' "git log staging..develop (lists commits only)";            bench : git log --oneline staging..develop
printf '%-52s' "git cherry staging develop (git's own patch-id check)";     bench : git cherry staging develop
printf '%-52s' "prdiff compare, cold cache (default scope)";               bench 'rm -rf .git/prdiff' "$bin" compare --base staging --head develop --format json
printf '%-52s' "prdiff compare, warm cache";                              bench : "$bin" compare --base staging --head develop --format json
printf '%-52s' "prdiff compare, cold, --patch-id-scope all";               bench 'rm -rf .git/prdiff' "$bin" compare --base staging --head develop --format json --patch-id-scope all
printf '%-52s' "prdiff compare, warm, --patch-id-scope all";               bench : "$bin" compare --base staging --head develop --format json --patch-id-scope all
printf '%-52s' "prdiff conflicts only";                                    bench : "$bin" conflicts --base staging --head develop
echo
echo "stage timings (cold):"; rm -rf .git/prdiff; "$bin" -v compare --base staging --head develop --format json 2>&1 >/dev/null | sed 's/^prdiff: /  /'

if [ -f expected.json ]; then
  echo; echo "correctness vs generator's expected counts:"
  "$bin" compare --base staging --head develop --format json | python3 -c '
import json, sys
got = json.load(sys.stdin)["totals"]["by_class"]
want = json.load(open("expected.json"))["expected"]
print("  got   ", got); print("  want  ", want); sys.exit(0 if got == want else 1)'
fi
