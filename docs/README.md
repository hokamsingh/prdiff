# prdiff documentation

`prdiff` tells you what **actually** changes when one branch lands on another, judged by content rather than by commit
subject or hash. These pages explain how to use it and how it works.

| Page | Read it when you want to |
|---|---|
| [Getting started](getting-started.md) | install it and run it for the first time |
| [Concepts](concepts.md) | understand NEW, PARTIAL, SHIPPED and PATCH_EQUIVALENT, and how each is decided |
| [Command reference](commands.md) | look up a command, a flag, an exit code |
| [Configuration](configuration.md) | set repository defaults in `prdiff.toml` |
| [The HTML report](report-viewer.md) | use `prdiff view`: tabs, side-by-side, risk badges, live mode |
| [JSON output](json-output.md) | script against `--format json` |
| [CI and GitHub](ci-and-github.md) | gate or publish PR descriptions, use the GitHub Action, cut a release |
| [Recipes](recipes.md) | follow a worked example: release PRs, release trains, force-pushes, monorepos |
| [Troubleshooting](troubleshooting.md) | fix an error message or a surprising result |
| [Architecture](architecture.md) | see how it is built, how fast it is, how it is tested |
| [LLD](LLD.md) | read the original design document (some choices changed; see Architecture) |

New here? Read [Getting started](getting-started.md), then [Concepts](concepts.md).
