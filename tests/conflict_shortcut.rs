//! The disjoint-sides shortcut must never hide a conflict `git merge-tree` would report.
mod common;
use common::*;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % n
    }
}

const POOL: &[&str] = &[
    "a/new.txt",
    "b/n.txt",
    "a/x/n.txt",
    "c",
    "c/d/e.txt",
    "a/1.txt",
    "b2/4.txt",
    "dir/z.txt",
];

fn content(seed: &str, extra: &str) -> String {
    (0..12)
        .map(|i| format!("{seed} line {i}\n"))
        .collect::<String>()
        + extra
}

fn random_ops(r: &Repo, rng: &mut Lcg, side: &str) {
    for step in 0..(1 + rng.next(3)) {
        let files: Vec<String> = r.git(&["ls-files"]).lines().map(str::to_string).collect();
        let msg = format!("{side} op {step}");
        match rng.next(6) {
            0 if !files.is_empty() => {
                let f = &files[rng.next(files.len())];
                let old = std::fs::read_to_string(r.path().join(f)).unwrap_or_default();
                let mut lines: Vec<String> = old.lines().map(str::to_string).collect();
                if !lines.is_empty() {
                    let i = rng.next(lines.len());
                    lines[i] = format!("{side} edit {step}");
                }
                r.write(f, &(lines.join("\n") + "\n"));
            }
            1 if !files.is_empty() => {
                let _ = r.try_git(&["rm", "-q", &files[rng.next(files.len())]]);
            }
            2 => {
                let p = POOL[rng.next(POOL.len())];
                if !r.path().join(p).is_dir() {
                    r.write(p, &content(p, side));
                }
            }
            3 if !files.is_empty() => {
                let to = POOL[rng.next(POOL.len())];
                let _ = std::fs::create_dir_all(r.path().join(to).parent().unwrap());
                let _ = r.try_git(&["mv", &files[rng.next(files.len())], to]);
            }
            4 => {
                let _ = r.try_git(&["mv", "a", "b2"]);
            }
            _ => {
                let _ = r.try_git(&["mv", "b", "dir"]);
            }
        }
        r.git(&["add", "-A"]);
        let _ = r.try_git(&["commit", "-q", "--allow-empty", "-m", &msg]);
    }
}

#[test]
fn shortcut_agrees_with_merge_tree_on_random_histories() {
    let mut rng = Lcg(7);
    let mut conflicting = 0;
    let mut skipped = 0;
    // 120 trials locally; CI sets fewer because process spawning is slow on macOS runners
    let trials: usize = std::env::var("PRDIFF_SHORTCUT_TRIALS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    for trial in 0..trials {
        let r = Repo::new();
        r.commit(
            "init",
            &[
                ("a/1.txt", &content("one", "")),
                ("a/2.txt", &content("two", "")),
                ("a/x/3.txt", &content("three", "")),
                ("b/4.txt", &content("four", "")),
                ("b/5.txt", &content("five", "")),
            ],
        );
        r.git(&["branch", "staging"]);
        r.git(&["checkout", "-q", "-b", "develop"]);
        random_ops(&r, &mut rng, "develop");
        r.git(&["checkout", "-q", "staging"]);
        random_ops(&r, &mut rng, "staging");

        let fast = run_with(&r, "staging", "develop", |o| o.use_cache = false);
        let exact = run_with(&r, "staging", "develop", |o| {
            o.use_cache = false;
            o.exact_conflicts = true;
        });
        assert_eq!(fast.conflicts, exact.conflicts, "trial {trial}");
        conflicting += !exact.conflicts.is_empty() as usize;
        skipped += fast.conflicts.is_empty() as usize;
    }
    assert!(
        conflicting >= trials / 12,
        "the generator should produce real conflicts, got {conflicting}"
    );
    assert!(skipped >= trials / 12, "and clean merges, got {skipped}");
}
