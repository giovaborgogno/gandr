//! Comparison resolution that isn't part of the [`GitBackend`] trait: the
//! smart-selection fallback chain, and PR resolution via the `gh` CLI.

use super::{CompareSpec, GitBackend};
use anyhow::{Context, Result};
use std::process::Command;

/// A resolved comparison plus an optional header label (e.g. a PR title).
#[derive(Debug, Clone)]
pub struct Resolved {
    pub spec: CompareSpec,
    pub title: Option<String>,
}

impl Resolved {
    fn plain(spec: CompareSpec) -> Self {
        Self { spec, title: None }
    }
}

/// Resolve a requested comparison to a concrete one the backend understands.
///
/// - A `Pr` is resolved via `gh`.
/// - When `smart` is on and the request is `Uncommitted` with no pending changes,
///   fall back to branch-vs-base (merge-base), then to the current branch's PR.
pub fn resolve(
    backend: &dyn GitBackend,
    requested: CompareSpec,
    smart: bool,
    base_branches: &[String],
) -> Result<Resolved> {
    if let CompareSpec::Pr(n) = requested {
        return resolve_pr(n);
    }

    if smart {
        if let CompareSpec::Uncommitted = requested {
            let clean = backend
                .changed_files(&CompareSpec::Uncommitted)
                .map(|f| f.is_empty())
                .unwrap_or(false);
            if clean {
                if let Some(base) = backend.detect_base(base_branches)? {
                    return Ok(Resolved {
                        spec: CompareSpec::WorkdirVs(base),
                        title: Some("branch vs base".to_string()),
                    });
                }
                if let Ok(pr) = resolve_pr(None) {
                    return Ok(pr);
                }
            }
        }
    }

    Ok(Resolved::plain(requested))
}

/// Resolve a PR (the current branch's if `number` is `None`) into a commit range
/// via `gh pr view`. Requires `gh` on PATH and network access.
pub fn resolve_pr(number: Option<u32>) -> Result<Resolved> {
    let mut cmd = Command::new("gh");
    cmd.arg("pr").arg("view");
    if let Some(n) = number {
        cmd.arg(n.to_string());
    }
    // Title goes last so a tab inside it doesn't shift the other fields.
    cmd.args([
        "--json",
        "number,title,baseRefOid,headRefOid",
        "-q",
        "[(.number|tostring),.baseRefOid,.headRefOid,.title]|@tsv",
    ]);

    let output = cmd
        .output()
        .context("running `gh pr view` (is gh installed?)")?;
    if !output.status.success() {
        anyhow::bail!(
            "gh pr view failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = stdout.trim_end(); // trims trailing \r and \n
                                  // splitn(4) keeps any tabs in the (last) title field intact.
    let parts: Vec<&str> = line.splitn(4, '\t').collect();
    let [num, base, head, title] = parts.as_slice() else {
        anyhow::bail!("unexpected gh output: {line:?}");
    };

    Ok(Resolved {
        spec: CompareSpec::Range((*base).to_string(), (*head).to_string()),
        title: Some(format!("PR #{num}: {title}")),
    })
}
