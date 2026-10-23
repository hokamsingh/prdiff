//! CODEOWNERS: who has to approve each file. Read from the target branch, last matching rule wins.

use crate::git::Git;
use regex::Regex;

pub struct Owners {
    rules: Vec<(Regex, Vec<String>)>,
}

const LOCATIONS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

impl Owners {
    /// The CODEOWNERS file as it is on `rev`, if there is one.
    pub fn load(git: &Git, rev: &str) -> Option<Owners> {
        LOCATIONS.iter().find_map(|loc| {
            let spec = format!("{rev}:{loc}");
            let out = git.exec(&["show", &spec], None, &[]).ok()?;
            (out.code == 0).then(|| Owners::parse(&String::from_utf8_lossy(&out.stdout)))
        })
    }

    pub fn parse(text: &str) -> Owners {
        let mut rules = Vec::new();
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or("").trim();
            let mut parts = line.split_whitespace();
            let Some(pat) = parts.next() else { continue };
            let owners: Vec<String> = parts.map(str::to_string).collect();
            if let Ok(re) = Regex::new(&to_regex(pat)) {
                rules.push((re, owners));
            }
        }
        Owners { rules }
    }

    pub fn of(&self, path: &str) -> Vec<String> {
        self.rules
            .iter()
            .rev()
            .find(|(re, _)| re.is_match(path))
            .map(|(_, o)| o.clone())
            .unwrap_or_default()
    }
}

/// gitignore-style: a leading or inner `/` anchors to the root, a trailing `/` means "everything under".
fn to_regex(pat: &str) -> String {
    let dir = pat.ends_with('/');
    let p = pat.trim_end_matches('/');
    let anchored = p.starts_with('/') || p.trim_start_matches('/').contains('/');
    let p = p.trim_start_matches('/');
    let mut re = String::from("^");
    if !anchored {
        re.push_str("(?:.*/)?");
    }
    re.push_str(&crate::glob::body(p));
    re.push_str(if dir { "/.*" } else { "(?:/.*)?" });
    re.push('$');
    re
}
