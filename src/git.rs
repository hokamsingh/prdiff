//! One place that shells out to `git`: `LC_ALL=C`, a timeout, stderr capture, argv in every error.

use crate::error::git_err;
use anyhow::Result;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Git {
    pub dir: PathBuf,
    pub timeout: Duration,
}

pub struct Out {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Git {
    pub fn new(dir: impl Into<PathBuf>, timeout: Duration) -> Self {
        Git {
            dir: dir.into(),
            timeout,
        }
    }

    fn command(&self, args: &[&str], envs: &[(&str, &str)]) -> Command {
        let mut c = Command::new("git");
        c.arg("-C").arg(&self.dir).args(args);
        c.env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0");
        for (k, v) in envs {
            c.env(k, v);
        }
        c
    }

    /// Run git and return its output whatever the exit code.
    pub fn exec(
        &self,
        args: &[&str],
        stdin: Option<Vec<u8>>,
        envs: &[(&str, &str)],
    ) -> Result<Out> {
        let mut cmd = self.command(args, envs);
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| git_err(format!("cannot run git: {e}")))?;
        let writer = stdin.map(|data| {
            let mut si = child.stdin.take().expect("piped stdin");
            thread::spawn(move || {
                let _ = si.write_all(&data);
            })
        });
        let t_out = drain(child.stdout.take().expect("piped stdout"));
        let t_err = drain(child.stderr.take().expect("piped stderr"));
        let status = wait(&mut child, self.timeout, args)?;
        if let Some(w) = writer {
            let _ = w.join();
        }
        Ok(Out {
            code: status.code().unwrap_or(-1),
            stdout: t_out.join().unwrap_or_default(),
            stderr: t_err.join().unwrap_or_default(),
        })
    }

    /// Run git, failing (with argv, exit code and stderr) on a non-zero exit.
    pub fn run(&self, args: &[&str]) -> Result<Vec<u8>> {
        self.run_stdin(args, None)
    }

    pub fn run_stdin(&self, args: &[&str], stdin: Option<Vec<u8>>) -> Result<Vec<u8>> {
        let out = self.exec(args, stdin, &[])?;
        if out.code != 0 {
            return Err(failure(args, &out));
        }
        Ok(out.stdout)
    }

    pub fn text(&self, args: &[&str]) -> Result<String> {
        Ok(String::from_utf8_lossy(&self.run(args)?).trim().to_string())
    }

    /// `git diff-tree --stdin -p ... | git patch-id --stable` for the given commits.
    /// Returns patch-id's raw stdout (`<patch-id> <commit>` lines).
    pub fn patch_ids(&self, commits: &[String]) -> Result<Vec<u8>> {
        let dt_args = [
            "diff-tree",
            "--stdin",
            "-p",
            "-r",
            "--root",
            "--no-ext-diff",
        ];
        let mut dt = self.command(&dt_args, &[]);
        dt.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut dt = dt
            .spawn()
            .map_err(|e| git_err(format!("cannot run git: {e}")))?;

        let pid_args = ["patch-id", "--stable"];
        let mut pid = self.command(&pid_args, &[]);
        pid.stdin(Stdio::from(dt.stdout.take().expect("piped stdout")))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut pid = pid
            .spawn()
            .map_err(|e| git_err(format!("cannot run git: {e}")))?;

        let mut input = commits.join("\n").into_bytes();
        input.push(b'\n');
        let mut si = dt.stdin.take().expect("piped stdin");
        let writer = thread::spawn(move || {
            let _ = si.write_all(&input);
        });
        let t_out = drain(pid.stdout.take().expect("piped stdout"));
        let t_err = drain(pid.stderr.take().expect("piped stderr"));
        let t_dt_err = drain(dt.stderr.take().expect("piped stderr"));

        let st = wait(&mut pid, self.timeout, &pid_args)?;
        let st_dt = wait(&mut dt, self.timeout, &dt_args)?;
        let _ = writer.join();
        let out = t_out.join().unwrap_or_default();
        let err = t_err.join().unwrap_or_default();
        let dt_err = t_dt_err.join().unwrap_or_default();
        if !st_dt.success() {
            return Err(failure(
                &dt_args,
                &Out {
                    code: st_dt.code().unwrap_or(-1),
                    stdout: vec![],
                    stderr: dt_err,
                },
            ));
        }
        if !st.success() {
            return Err(failure(
                &pid_args,
                &Out {
                    code: st.code().unwrap_or(-1),
                    stdout: vec![],
                    stderr: err,
                },
            ));
        }
        Ok(out)
    }
}

fn drain<R: Read + Send + 'static>(mut r: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut v = Vec::new();
        let _ = r.read_to_end(&mut v);
        v
    })
}

fn wait(child: &mut Child, timeout: Duration, args: &[&str]) -> Result<ExitStatus> {
    let start = Instant::now();
    let mut nap = Duration::from_micros(50);
    loop {
        if let Some(st) = child
            .try_wait()
            .map_err(|e| git_err(format!("wait failed: {e}")))?
        {
            return Ok(st);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(git_err(format!(
                "git {} timed out after {:?}",
                args.join(" "),
                timeout
            )));
        }
        thread::sleep(nap);
        nap = (nap * 2).min(Duration::from_millis(5));
    }
}

fn failure(args: &[&str], out: &Out) -> anyhow::Error {
    git_err(format!(
        "git {} failed (exit {}): {}",
        args.join(" "),
        out.code,
        String::from_utf8_lossy(&out.stderr).trim()
    ))
}

/// `git version 2.43.0.windows.1` -> (2, 43)
pub fn version(git: &Git) -> Result<(u32, u32)> {
    let v = git.text(&["--version"])?;
    let mut nums = v
        .split_whitespace()
        .nth(2)
        .unwrap_or("0.0")
        .split('.')
        .map(|p| p.parse::<u32>().unwrap_or(0));
    Ok((nums.next().unwrap_or(0), nums.next().unwrap_or(0)))
}
