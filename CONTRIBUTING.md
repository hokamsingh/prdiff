# Contributing

## Set up

You need Rust (stable), Git 2.38 or newer, and a POSIX shell. Python 3 is used by the benchmark generator only.

```bash
git clone https://github.com/hokamsingh/prdiff && cd prdiff
cargo build
cargo test
```

Before you push, run what CI runs:

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --no-fail-fast
```

Some tests need nothing but `git`. A few use `tar`, `sha256sum` (or `shasum`) and `sh`. One test uses the real
[git-cliff](https://git-cliff.org) when it is on your `PATH` and is skipped otherwise.

## Layout

See [docs/architecture.md](docs/architecture.md) for the module map and the flow of a run. In short: `src/` is a library with a
thin `main.rs`, `tests/` holds integration tests that build fixture repositories with the real `git`, `scripts/` has the
benchmark, `packaging/` has the Homebrew renderer, and `docs/` is the documentation.

## Tests

Tests build throwaway repositories in temporary directories (`tests/common/mod.rs`) with fixed dates, so SHAs are reproducible.
Write a new scenario by building the history with `Repo::commit`, `Repo::git` and friends, then call `run`/`run_with` for the
library or invoke the binary for end-to-end behaviour.

- The golden test compares `compare --format json` with `tests/golden/squash.json`. After an intended output change, regenerate:
  `UPDATE_GOLDEN=1 cargo test --test cli golden`, then review the diff.
- Tests that start processes (the local server) must clean them up even when an assertion fails; see the guard in
  `tests/output_formats.rs`.
- Tests must pass on Ubuntu and macOS and on both old and new git. CI has already caught real differences: `\b` is not
  available in POSIX regular expressions on macOS, and newer git leaves whitespace-only files out of `diff --raw -w`.
- `tests/docs.rs` checks that every command-line option appears in `docs/commands.md` and that every local link in the
  documentation resolves. If you add an option, document it.

## Benchmark

```bash
python3 scripts/gen_bench_repo.py /tmp/bench 20000 1500   # history size, extra base commits
cargo build --release
scripts/bench.sh /tmp/bench                               # timings, and a check of the classification counts
```

## Documentation

Docs are plain Markdown in `docs/`. Keep claims checkable: say what was tested and what was not. The README's *Status* section
lists what has not been run against the real thing.

## Releasing

See "Releasing prdiff itself" in [docs/ci-and-github.md](docs/ci-and-github.md): bump `version` in `Cargo.toml`, push to `main`,
then push a tag or run the release workflow.
