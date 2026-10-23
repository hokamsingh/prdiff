use anyhow::{Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use prdiff::analyze::{Options, analyze, conflicts_only};
use prdiff::config::{self, Config};
use prdiff::error::{Error, bad};
use prdiff::git::Git;
use prdiff::patchid::Scope;
use prdiff::render;
use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

#[derive(Parser)]
#[command(
    name = "prdiff",
    version,
    about = "Real-diff checker for PR descriptions: what actually changes when head lands on base"
)]
struct Cli {
    /// Repository directory.
    #[arg(short = 'C', long, global = true, default_value = ".")]
    repo: PathBuf,
    /// Config file (default: prdiff.toml at the repository root, if present).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Print per-stage timings to stderr.
    #[arg(short, long, global = true)]
    verbose: bool,
    /// Timeout for each git call, in seconds.
    #[arg(long, global = true, default_value_t = 300)]
    timeout: u64,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Args, Clone)]
struct Common {
    /// Branch or ref the change would land on, e.g. origin/staging. Default: `base` in prdiff.toml, else
    /// the remote's default branch.
    #[arg(long)]
    base: Option<String>,
    /// Branch or ref carrying the change, e.g. origin/develop. Default: `head` in prdiff.toml, else the
    /// current branch.
    #[arg(long)]
    head: Option<String>,
    /// Only consider files matching this glob (repeatable). `**` crosses directories.
    #[arg(long = "path")]
    paths: Vec<String>,
    /// Leave files matching this glob out of file lists and stats (repeatable), e.g. lockfiles.
    #[arg(long = "exclude")]
    excludes: Vec<String>,
    /// Treat whitespace-only differences as no difference.
    #[arg(short = 'w', long)]
    ignore_whitespace: bool,
    /// Treat every PARTIAL commit as NEW.
    #[arg(long)]
    strict: bool,
    /// Skip hunk analysis for NEW commits (only PARTIAL ones get it). Faster, but a change that base
    /// later edited around looks new.
    #[arg(long)]
    shallow: bool,
    /// Keep commits with no file changes.
    #[arg(long)]
    include_empty: bool,
    /// How much of base's history is searched for cherry-picks.
    #[arg(long, value_enum, default_value_t = ScopeArg::Auto)]
    patch_id_scope: ScopeArg,
    /// Do not read or write .git/prdiff.
    #[arg(long)]
    no_cache: bool,
    /// Always run `git merge-tree` instead of first proving the two sides are disjoint.
    #[arg(long)]
    exact_conflicts: bool,
    /// Exit 4 when base and head conflict.
    #[arg(long)]
    fail_on_conflict: bool,
}

#[derive(ValueEnum, Clone, Copy)]
enum ScopeArg {
    /// Base commits since the oldest head-only commit (minus two days of slack).
    Auto,
    /// All of base's history.
    All,
    /// Only commits on base that are not on head.
    BaseOnly,
}

#[derive(ValueEnum, Clone, Copy, PartialEq)]
enum Format {
    Text,
    Json,
    Markdown,
}

#[derive(Subcommand)]
enum Cmd {
    /// Classify every head-only commit and compute the net diff.
    Compare {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Emit a Markdown PR description generated from the report.
    Describe {
        #[command(flatten)]
        c: Common,
        #[arg(long)]
        title: Option<String>,
    },
    /// List files that would conflict, without touching the worktree.
    Conflicts {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Write a self-contained HTML report: commit timeline by class, files, and colour-coded hunks.
    View {
        #[command(flatten)]
        c: Common,
        /// Output file, or `-` for stdout.
        #[arg(long, default_value = "prdiff-report.html")]
        out: String,
        /// Open the report in the default browser.
        #[arg(long)]
        open: bool,
        /// Show at most this many diff lines per file.
        #[arg(long, default_value_t = 400)]
        max_lines: usize,
        /// Do not embed file contents (no side-by-side viewer); keeps the report small.
        #[arg(long)]
        no_sides: bool,
        /// Most file content to embed for the side-by-side viewer, in MB.
        #[arg(long, default_value_t = 8)]
        max_embed_mb: usize,
    },
    /// Fail (exit 5) if a PR description lists stale commits or omits new ones.
    Check {
        #[command(flatten)]
        c: Common,
        /// File holding the PR body, or `-` for stdin.
        #[arg(long)]
        body: String,
    },
}

fn options(cli: &Cli, c: &Common) -> Result<(Options, Config)> {
    let cfg = config::load(&cli.repo, cli.config.as_deref())?;
    let git = Git::new(&cli.repo, Duration::from_secs(cli.timeout));
    let head = match c.head.clone().or_else(|| cfg.head.clone()) {
        Some(h) => h,
        None => {
            let h = config::infer_head(&git)?;
            eprintln!("prdiff: --head not given, using {h}");
            h
        }
    };
    let base = match c.base.clone().or_else(|| cfg.base.clone()) {
        Some(b) => b,
        None => {
            let b = config::infer_base(&git)?;
            eprintln!("prdiff: --base not given, using {b}");
            b
        }
    };
    let mut o = Options::new(&cli.repo, &base, &head);
    o.paths = if c.paths.is_empty() {
        cfg.paths.clone()
    } else {
        c.paths.clone()
    };
    o.exclude = cfg.exclude.iter().chain(&c.excludes).cloned().collect();
    o.ignore_whitespace = c.ignore_whitespace || cfg.ignore_whitespace;
    o.strict = c.strict;
    o.deep = !c.shallow;
    o.include_empty = c.include_empty;
    o.scope = match c.patch_id_scope {
        ScopeArg::Auto => Scope::Auto,
        ScopeArg::All => Scope::All,
        ScopeArg::BaseOnly => Scope::BaseOnly,
    };
    o.use_cache = !c.no_cache;
    o.exact_conflicts = c.exact_conflicts;
    o.verbose = cli.verbose;
    o.timeout = Duration::from_secs(cli.timeout);
    Ok((o, cfg))
}

fn conflict_gate(c: &Common, conflicts: &[String]) -> Result<()> {
    if c.fail_on_conflict && !conflicts.is_empty() {
        return Err(Error::Conflicts(format!("{} conflicted file(s)", conflicts.len())).into());
    }
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    match &cli.cmd {
        Cmd::Compare { c, format } => {
            let r = analyze(&options(&cli, c)?.0)?;
            print!(
                "{}",
                match format {
                    Format::Text => render::text::render(&r),
                    Format::Json => render::json::render(&r),
                    Format::Markdown => render::markdown::render(&r, None),
                }
            );
            conflict_gate(c, &r.conflicts)
        }
        Cmd::Describe { c, title } => {
            let r = analyze(&options(&cli, c)?.0)?;
            print!("{}", render::markdown::render(&r, title.as_deref()));
            conflict_gate(c, &r.conflicts)
        }
        Cmd::Conflicts { c, format } => {
            let (base, head, paths, best_effort) = conflicts_only(&options(&cli, c)?.0)?;
            if *format == Format::Json {
                let v = serde_json::json!({
                    "schema_version": prdiff::report::SCHEMA_VERSION,
                    "base": base, "head": head, "conflicts": paths, "conflicts_best_effort": best_effort,
                });
                println!("{}", serde_json::to_string_pretty(&v)?);
            } else if paths.is_empty() {
                println!("no conflicts between {} and {}", base.name, head.name);
            } else {
                for p in &paths {
                    println!("{p}");
                }
            }
            conflict_gate(c, &paths)
        }
        Cmd::View {
            c,
            out,
            open,
            max_lines,
            no_sides,
            max_embed_mb,
        } => {
            let mut o = options(&cli, c)?.0;
            o.detail = Some(*max_lines);
            o.sides_budget = if *no_sides {
                0
            } else {
                max_embed_mb * 1024 * 1024
            };
            let r = analyze(&o)?;
            let html = render::view::render(&r);
            if out == "-" {
                print!("{html}");
            } else {
                std::fs::write(out, html).map_err(|e| bad(format!("cannot write {out}: {e}")))?;
                let abs = std::fs::canonicalize(out).unwrap_or_else(|_| PathBuf::from(out));
                eprintln!("prdiff: wrote {}", abs.display());
                if *open {
                    let opener = if cfg!(target_os = "macos") {
                        "open"
                    } else {
                        "xdg-open"
                    };
                    if std::process::Command::new(opener)
                        .arg(&abs)
                        .spawn()
                        .is_err()
                    {
                        eprintln!("prdiff: could not run {opener}; open the file manually");
                    }
                }
            }
            conflict_gate(c, &r.conflicts)
        }
        Cmd::Check { c, body } => {
            let text = if body == "-" {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                std::fs::read_to_string(body)
                    .map_err(|e| bad(format!("cannot read {body}: {e}")))?
            };
            let r = analyze(&options(&cli, c)?.0)?;
            let m = prdiff::check::check(&r, &text);
            if m.is_empty() {
                println!(
                    "description matches the report ({} listed commits)",
                    r.totals.listed
                );
                return Ok(());
            }
            for x in &m.missing {
                eprintln!("missing from description: {x}");
            }
            for x in &m.stale {
                eprintln!("already on base but listed: {x}");
            }
            Err(Error::Mismatch(format!(
                "description disagrees with the report ({} missing, {} stale)",
                m.missing.len(),
                m.stale.len()
            ))
            .into())
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).context("prdiff") {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            let code = e.downcast_ref::<Error>().map(Error::exit_code).unwrap_or(3);
            // `context` wraps the error; print the root cause message once.
            eprintln!("prdiff: {}", e.root_cause());
            ExitCode::from(code)
        }
    }
}
