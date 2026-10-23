//! Parsers for `git diff-tree --raw -z` (and `--numstat -z`) output.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Entry {
    /// First letter of the status: A, M, D, R, T, ...
    pub status: u8,
    pub old_mode: u32,
    pub new_mode: u32,
    pub old_sha: String,
    pub new_sha: String,
    pub path: String,
    pub old_path: Option<String>,
}

impl Entry {
    pub fn is_submodule(&self) -> bool {
        self.old_mode == 0o160000 || self.new_mode == 0o160000
    }
    pub fn is_delete(&self) -> bool {
        is_zero(&self.new_sha)
    }
}

pub fn is_zero(sha: &str) -> bool {
    !sha.is_empty() && sha.bytes().all(|b| b == b'0')
}

fn tokens(bytes: &[u8]) -> impl Iterator<Item = &[u8]> {
    bytes.split(|b| *b == 0).filter(|t| !t.is_empty())
}

fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Parse one raw header token (`:100644 100644 <old> <new> M`) and pull its path(s) from `rest`.
fn entry<'a>(header: &[u8], rest: &mut impl Iterator<Item = &'a [u8]>) -> Option<Entry> {
    let h = std::str::from_utf8(header).ok()?.trim_start_matches(':');
    let mut f = h.split(' ');
    let old_mode = u32::from_str_radix(f.next()?, 8).ok()?;
    let new_mode = u32::from_str_radix(f.next()?, 8).ok()?;
    let old_sha = f.next()?.to_string();
    let new_sha = f.next()?.to_string();
    let status = *f.next()?.as_bytes().first()?;
    let first = lossy(rest.next()?);
    let (path, old_path) = if status == b'R' || status == b'C' {
        (lossy(rest.next()?), Some(first))
    } else {
        (first, None)
    };
    Some(Entry {
        status,
        old_mode,
        new_mode,
        old_sha,
        new_sha,
        path,
        old_path,
    })
}

/// Output of `git diff-tree --stdin -r --raw -z`: `<commit>\0` then its entries.
/// Commits whose diff is empty do not appear at all.
pub fn parse_stdin_raw(bytes: &[u8]) -> Vec<(String, Vec<Entry>)> {
    let mut out: Vec<(String, Vec<Entry>)> = Vec::new();
    let mut it = tokens(bytes);
    while let Some(t) = it.next() {
        if t.first() == Some(&b':') {
            if let (Some(e), Some(cur)) = (entry(t, &mut it), out.last_mut()) {
                cur.1.push(e);
            }
        } else {
            out.push((lossy(t), Vec::new()));
        }
    }
    out
}

pub type Stats = HashMap<String, (Option<u64>, Option<u64>)>;

/// Output of `git diff-tree -r --raw --numstat -z <a> <b>`: raw entries, then numstat records.
/// Binary files report `-` for both counts, stored as `None`.
pub fn parse_raw_numstat(bytes: &[u8]) -> (Vec<Entry>, Stats) {
    let mut entries = Vec::new();
    let mut stats: Stats = HashMap::new();
    let mut it = tokens(bytes);
    while let Some(t) = it.next() {
        if t.first() == Some(&b':') {
            if let Some(e) = entry(t, &mut it) {
                entries.push(e);
            }
            continue;
        }
        let s = lossy(t);
        let mut f = s.splitn(3, '\t');
        let (Some(a), Some(d), Some(rest)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let path = if rest.is_empty() {
            it.next(); // old path
            it.next().map(lossy).unwrap_or_default()
        } else {
            rest.to_string()
        };
        stats.insert(path, (a.parse().ok(), d.parse().ok()));
    }
    (entries, stats)
}
