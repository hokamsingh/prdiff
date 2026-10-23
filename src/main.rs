use anyhow::{Context, Result};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use prdiff::analyze::{Options, analyze, conflicts_only};
use prdiff::config::{self, Config};
use prdiff::error::{Error, bad};
use prdiff::git::Git;
use prdiff::github;
use prdiff::patchid::Scope;
use prdiff::render;
use prdiff::rewrite;
use prdiff::train;
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
    /// Compare a GitHub pull request (needs the `gh` CLI): fetches its base and head and uses them
    /// instead of --base/--head.
    #[arg(long)]
    pr: Option<u32>,
    /// Do not read CODEOWNERS.
    #[arg(long)]
    no_owners: bool,
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
enum PublishMode {
    /// Between `<!-- prdiff:start -->` and `<!-- prdiff:end -->` in the PR description.
    Body,
    /// One comment, found by its marker and edited on later runs.
    Comment,
}

#[derive(ValueEnum, Clone, Copy, PartialEq)]
enum Format {
    Text,
    Json,
    Markdown,
    /// Slack mrkdwn.
    Slack,
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
        /// markdown (default), slack (mrkdwn) or json.
        #[arg(long, value_enum, default_value_t = Format::Markdown)]
        format: Format,
    },
    /// Release notes grouped by kind: breaking changes first, PR and ticket links.
    Notes {
        #[command(flatten)]
        c: Common,
        #[arg(long)]
        title: Option<String>,
        /// Base URL for `(#123)` links. Default: `notes.repo_url` in prdiff.toml, else derived from `origin`.
        #[arg(long)]
        repo_url: Option<String>,
        /// Ticket link template with `{id}`, e.g. https://jira.example.com/browse/{id}.
        #[arg(long)]
        ticket_url: Option<String>,
        /// markdown (default) or json.
        #[arg(long, value_enum, default_value_t = Format::Markdown)]
        format: Format,
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
        /// Serve the report on localhost instead of writing a file; it reloads when base or head moves.
        #[arg(long)]
        serve: bool,
        /// Port for --serve (0 = pick a free one).
        #[arg(long, default_value_t = 8765)]
        port: u16,
        /// Previous version of the head, for a Rewritten tab (force-pushed branches).
        #[arg(long)]
        old: Option<String>,
        /// Environments for a Train tab, as name=ref, repeatable. Default: [[environments]] in prdiff.toml.
        #[arg(long = "env")]
        envs: Vec<String>,
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
    /// What changed between the old and new version of a force-pushed branch (via git range-diff).
    Rewritten {
        #[command(flatten)]
        c: Common,
        /// The previous version. Default: `<head>@{1}` (the remote-tracking ref before the last fetch).
        #[arg(long)]
        old: Option<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Print shell completions (bash, zsh, fish, powershell, elvish).
    Completions { shell: clap_complete::Shell },
    /// Print the man page (roff) to stdout.
    Man,
    /// Where does a commit stand per environment? e.g. `prdiff where abc1234 --env production=origin/production`.
    Where {
        /// A revision, or `#123` for the commit mentioning that PR (searched in --head).
        rev: String,
        #[command(flatten)]
        c: Common,
        /// Environment as name=ref, repeatable, furthest along first. Default: [[environments]] in prdiff.toml.
        #[arg(long = "env")]
        envs: Vec<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Every head-only commit against every environment (the release train).
    Matrix {
        #[command(flatten)]
        c: Common,
        #[arg(long = "env")]
        envs: Vec<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Post the generated description to a GitHub PR (needs `gh`): into the PR body between markers, or
    /// as one sticky comment that is updated in place.
    Publish {
        #[command(flatten)]
        c: Common,
        #[arg(long, value_enum, default_value_t = PublishMode::Body)]
        mode: PublishMode,
        #[arg(long)]
        title: Option<String>,
        /// Print what would be posted and change nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Fail (exit 5) if a PR description lists stale commits or omits new ones.
    Check {
        #[command(flatten)]
        c: Common,
        /// File holding the PR body, or `-` for stdin. With --pr this defaults to the PR's own body.
        #[arg(long)]
        body: Option<String>,
    },
}

fn options(cli: &Cli, c: &Common) -> Result<(Options, Config)> {
    options_with(cli, c, true)
}

/// `infer_base = false` for commands that compare against environments instead of one base.
fn options_with(cli: &Cli, c: &Common, infer_base: bool) -> Result<(Options, Config)> {
    let cfg = config::load(&cli.repo, cli.config.as_deref())?;
    let git = Git::new(&cli.repo, Duration::from_secs(cli.timeout));
    let pr = match c.pr {
        Some(n) => {
            let info = github::pr_info(&cli.repo, n)?;
            let (b, h) = github::fetch_pr(&git, n, &info)?;
            Some((b, h, info.head_ref))
        }
        None => None,
    };
    let head = match pr
        .as_ref()
        .map(|p| p.1.clone())
        .or_else(|| c.head.clone())
        .or_else(|| cfg.head.clone())
    {
        Some(h) => h,
        None => {
            let h = config::infer_head(&git)?;
            eprintln!("prdiff: --head not given, using {h}");
            h
        }
    };
    let base = match pr
        .as_ref()
        .map(|p| p.0.clone())
        .or_else(|| c.base.clone())
        .or_else(|| cfg.base.clone())
    {
        Some(b) => b,
        None if !infer_base => String::new(),
        None => {
            let b = config::infer_base(&git)?;
            eprintln!("prdiff: --base not given, using {b}");
            b
        }
    };
    let mut o = Options::new(&cli.repo, &base, &head);
    o.head_label = pr.map(|p| p.2);
    o.paths = if c.paths.is_empty() {
        cfg.paths.clone()
    } else {
        c.paths.clone()
    };
    o.exclude = cfg.exclude.iter().chain(&c.excludes).cloned().collect();
    o.ignore_whitespace = c.ignore_whitespace || cfg.ignore_whitespace;
    o.risky = cfg.risky.clone();
    o.owners = !c.no_owners;
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

/// `--env name=ref` flags win over `[[environments]]` in the config.
fn env_list(flags: &[String], cfg: &Config) -> Result<Vec<config::EnvRef>> {
    if flags.is_empty() {
        return Ok(cfg.environments.clone());
    }
    flags.iter().map(|s| train::parse_env(s)).collect()
}

const LIVE_SCRIPT: &str = "<script>(()=>{let sig=null;const tick=()=>fetch('/__sig',{cache:'no-store'}).then(r=>r.text()).then(s=>{if(sig!==null&&s!==sig)location.reload();sig=s}).catch(()=>{});tick();setInterval(tick,2000)})()</script>";

fn open_in_browser(target: &str) {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if std::process::Command::new(opener)
        .arg(target)
        .spawn()
        .is_err()
    {
        eprintln!("prdiff: could not run {opener}; open {target} manually");
    }
}

/// Minimal localhost server: `/` rebuilds the report, `/__sig` says whether base or head moved.
fn serve(o: &Options, port: u16, open: bool, build: &dyn Fn() -> Result<String>) -> Result<()> {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| bad(format!("cannot listen on 127.0.0.1:{port}: {e}")))?;
    let addr = listener.local_addr()?;
    let git = Git::new(&o.repo, o.timeout);
    let sig = || -> String {
        let spec = |r: &str| format!("{r}^{{commit}}");
        git.text(&["rev-parse", &spec(&o.base), &spec(&o.head)])
            .unwrap_or_else(|e| e.to_string())
    };
    eprintln!(
        "prdiff: serving http://{addr}  (reloads when {} or {} moves; Ctrl-C to stop)",
        o.base, o.head
    );
    if open {
        open_in_browser(&format!("http://{addr}"));
    }
    for stream in listener.incoming() {
        let Ok(mut s) = stream else { continue };
        let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
        // read the whole request head (it can arrive in several segments) before answering
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        while !buf.windows(4).any(|w| w == b"\r\n\r\n") && buf.len() < 16 * 1024 {
            match s.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        let req = String::from_utf8_lossy(&buf);
        let path = req.split_whitespace().nth(1).unwrap_or("/").to_string();
        let (status, ctype, body) = match path.as_str() {
            "/" | "/index.html" => match build() {
                Ok(html) => (
                    "200 OK",
                    "text/html; charset=utf-8",
                    html.replacen("</body>", &format!("{LIVE_SCRIPT}</body>"), 1),
                ),
                Err(e) => (
                    "500 Internal Server Error",
                    "text/plain; charset=utf-8",
                    format!("prdiff: {}\n", e.root_cause()),
                ),
            },
            "/__sig" => ("200 OK", "text/plain", sig()),
            _ => ("404 Not Found", "text/plain", "not found\n".to_string()),
        };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = s
            .write_all(head.as_bytes())
            .and_then(|_| s.write_all(body.as_bytes()));
    }
    Ok(())
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
                    Format::Slack => render::slack::render(&r, None),
                }
            );
            conflict_gate(c, &r.conflicts)
        }
        Cmd::Describe { c, title, format } => {
            let r = analyze(&options(&cli, c)?.0)?;
            match format {
                Format::Slack => print!("{}", render::slack::render(&r, title.as_deref())),
                Format::Json => print!("{}", render::json::render(&r)),
                _ => print!("{}", render::markdown::render(&r, title.as_deref())),
            }
            conflict_gate(c, &r.conflicts)
        }
        Cmd::Notes {
            c,
            title,
            repo_url,
            ticket_url,
            format,
        } => {
            let (o, cfg) = options(&cli, c)?;
            let r = analyze(&o)?;
            if *format == Format::Json {
                print!("{}", render::notes::render_json(&r));
            } else {
                let git = Git::new(&cli.repo, o.timeout);
                let repo_url = repo_url
                    .clone()
                    .or_else(|| cfg.notes.repo_url.clone())
                    .or_else(|| {
                        git.text(&["remote", "get-url", "origin"])
                            .ok()
                            .and_then(|u| render::notes::repo_url_from_remote(&u))
                    });
                let opts = render::notes::NotesOptions {
                    title: title.clone(),
                    repo_url,
                    ticket_url: ticket_url.clone().or_else(|| cfg.notes.ticket_url.clone()),
                };
                print!("{}", render::notes::render(&r, &opts));
            }
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
            envs,
            old,
            serve: serve_flag,
            port,
        } => {
            let (mut o, cfg) = options(&cli, c)?;
            o.detail = Some(*max_lines);
            o.sides_budget = if *no_sides {
                0
            } else {
                max_embed_mb * 1024 * 1024
            };
            let train_envs = env_list(envs, &cfg)?;
            let build = || -> Result<(prdiff::report::Report, String)> {
                let mut r = analyze(&o)?;
                if !train_envs.is_empty() {
                    r.train = Some(train::matrix(&o, &train_envs)?);
                }
                if let Some(old) = old {
                    let git = Git::new(&o.repo, o.timeout);
                    r.rewritten = Some(rewrite::rewritten(&git, &o.base, Some(old), &o.head)?);
                }
                let html = render::view::render(&r);
                Ok((r, html))
            };
            if *serve_flag {
                return serve(&o, *port, *open, &|| build().map(|x| x.1));
            }
            let (r, html) = build()?;
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
        Cmd::Rewritten { c, old, format } => {
            let (o, _) = options(&cli, c)?;
            let git = Git::new(&o.repo, o.timeout);
            let r = rewrite::rewritten(&git, &o.base, old.as_deref(), &o.head)?;
            if *format == Format::Json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                print!("{}", rewrite::render_text(&r));
            }
            Ok(())
        }
        Cmd::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "prdiff",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Cmd::Man => {
            clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;
            Ok(())
        }
        Cmd::Where {
            rev,
            c,
            envs,
            format,
        } => {
            let (mut o, cfg) = options_with(&cli, c, false)?;
            let git = Git::new(&cli.repo, o.timeout);
            let commit = train::resolve_commit(&git, rev, &o.head)?;
            o.head = commit.clone();
            let w = train::locate(&o, &env_list(envs, &cfg)?, &commit)?;
            if *format == Format::Json {
                println!("{}", serde_json::to_string_pretty(&w)?);
            } else {
                print!("{}", train::render_where(&w));
            }
            Ok(())
        }
        Cmd::Matrix { c, envs, format } => {
            let (o, cfg) = options_with(&cli, c, false)?;
            let m = train::matrix(&o, &env_list(envs, &cfg)?)?;
            match format {
                Format::Json => println!("{}", serde_json::to_string_pretty(&m)?),
                Format::Markdown | Format::Slack => {
                    print!("{}", train::render_matrix_markdown(&m))
                }
                Format::Text => print!("{}", train::render_matrix(&m)),
            }
            Ok(())
        }
        Cmd::Publish {
            c,
            mode,
            title,
            dry_run,
        } => {
            let Some(number) = c.pr else {
                return Err(bad("publish needs --pr <number>"));
            };
            let r = analyze(&options(&cli, c)?.0)?;
            let md = render::markdown::render(&r, title.as_deref());
            let current = github::pr_info(&cli.repo, number)?.body;
            if *dry_run {
                match mode {
                    PublishMode::Body => print!("{}", github::merge_body(&current, &md)),
                    PublishMode::Comment => print!("{}", github::comment_body(&md)),
                }
                println!();
                return Ok(());
            }
            let done = match mode {
                PublishMode::Body => github::publish_body(&cli.repo, number, &current, &md)?,
                PublishMode::Comment => github::publish_comment(&cli.repo, number, &md)?,
            };
            eprintln!("prdiff: PR #{number}: {done:?}");
            conflict_gate(c, &r.conflicts)
        }
        Cmd::Check { c, body } => {
            let text = match (body.as_deref(), c.pr) {
                (Some("-"), _) => {
                    let mut s = String::new();
                    std::io::stdin().read_to_string(&mut s)?;
                    s
                }
                (Some(path), _) => std::fs::read_to_string(path)
                    .map_err(|e| bad(format!("cannot read {path}: {e}")))?,
                (None, Some(n)) => github::pr_info(&cli.repo, n)?.body,
                (None, None) => return Err(bad("check needs --body <file|-> or --pr <number>")),
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
