//! Flags on files that deserve a second look before they ship.

use crate::glob::PathFilter;
use regex::Regex;
use std::sync::OnceLock;

pub const LARGE_DIFF_LINES: u64 = 500;
pub const HOT_FILE_COMMITS: usize = 5;
pub const MANY_AUTHORS: usize = 3;

struct Rule {
    flag: &'static str,
    re: Regex,
}

fn rules() -> &'static [Rule] {
    static RULES: OnceLock<Vec<Rule>> = OnceLock::new();
    RULES.get_or_init(|| {
        let r = |flag, pat: &str| Rule { flag, re: Regex::new(pat).unwrap() };
        vec![
            r("migration", r"(?i)(^|/)(migrations?|db/migrate|alembic/versions)/|\.sql$|(^|/)schema\.(rb|prisma)$"),
            r("lockfile", r"(^|/)(package-lock\.json|yarn\.lock|pnpm-lock\.yaml|Cargo\.lock|go\.sum|poetry\.lock|Gemfile\.lock|composer\.lock|Pipfile\.lock)$"),
            r("dependencies", r"(^|/)(package\.json|Cargo\.toml|go\.mod|requirements[^/]*\.txt|pyproject\.toml|Gemfile|pom\.xml|build\.gradle(\.kts)?)$"),
            r("deploy-config", r"(?i)(^|/)(Dockerfile[^/]*|docker-compose[^/]*\.ya?ml|\.github/workflows/[^/]+|\.gitlab-ci\.ya?ml|Jenkinsfile)$|(^|/)(k8s|kubernetes|helm|terraform|deploy|infra)/|\.tf$"),
            r("env-config", r"(^|/)\.env[^/]*$|(^|/)config/(production|prod|staging)[^/]*$"),
            r("secrets", r"(?i)\.(pem|key|p12|pfx|jks)$|(^|/)id_(rsa|ed25519)|(^|/)(credentials|secrets?)(\.|/|$)"),
        ]
    })
}

pub struct RiskRules {
    custom: Option<PathFilter>,
}

impl RiskRules {
    pub fn new(extra_globs: &[String]) -> anyhow::Result<Self> {
        Ok(RiskRules {
            custom: if extra_globs.is_empty() {
                None
            } else {
                Some(PathFilter::new(extra_globs)?)
            },
        })
    }

    /// Flags for one file. `lines` = insertions + deletions (None for binary).
    pub fn flags(
        &self,
        path: &str,
        lines: Option<u64>,
        commits: usize,
        authors: usize,
    ) -> Vec<String> {
        let mut out: Vec<String> = rules()
            .iter()
            .filter(|r| r.re.is_match(path))
            .map(|r| r.flag.to_string())
            .collect();
        if self.custom.as_ref().is_some_and(|c| c.matches(path)) {
            out.push("custom".into());
        }
        if lines.is_some_and(|n| n >= LARGE_DIFF_LINES) {
            out.push("large-diff".into());
        }
        if commits >= HOT_FILE_COMMITS {
            out.push("hot-file".into());
        }
        if authors >= MANY_AUTHORS {
            out.push("many-authors".into());
        }
        out
    }
}
