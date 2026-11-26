//! Moving a conversation to another account of the same AI.
//!
//! Each account has its own CLI folder (CLAUDE_CONFIG_DIR / CODEX_HOME) and
//! the CLI keeps its saved conversations there. To continue a conversation
//! with another account, Wren copies the CLI's own session file into the other
//! account's folder (same relative path), then resumes it by id. Only the
//! conversation is copied; logins are never touched.

use std::path::{Path, PathBuf};

use crate::providers::Provider;

/// The CLI folder an account uses: its private folder, or the CLI's normal
/// one for "this computer's login".
pub fn cli_home(provider: Provider, private: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = private {
        return Some(p);
    }
    let home = crate::login::home_dir();
    match provider {
        Provider::Claude => Some(home.join(".claude")),
        Provider::Codex => Some(std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".codex"))),
        Provider::Antigravity => None,
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Files (relative to the CLI folder) that hold this conversation.
fn session_files(provider: Provider, from: &Path, id: &str) -> Vec<PathBuf> {
    let mut out = vec![];
    match provider {
        // projects/<folder key>/<id>.jsonl (+ projects/<folder key>/<id>/ for sub-agents and big tool results)
        Provider::Claude => {
            let Ok(dirs) = std::fs::read_dir(from.join("projects")) else { return out };
            for d in dirs.flatten() {
                let base = PathBuf::from("projects").join(d.file_name());
                if from.join(&base).join(format!("{id}.jsonl")).is_file() {
                    out.push(base.join(format!("{id}.jsonl")));
                    if from.join(&base).join(id).is_dir() {
                        out.push(base.join(id));
                    }
                }
            }
        }
        // sessions/YYYY/MM/DD/rollout-<time>-<id>.jsonl
        Provider::Codex => {
            let suffix = format!("-{id}.jsonl");
            let mut stack = vec![PathBuf::from("sessions")];
            while let Some(rel) = stack.pop() {
                let Ok(entries) = std::fs::read_dir(from.join(&rel)) else { continue };
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    let ft = match e.file_type() {
                        Ok(t) => t,
                        Err(_) => continue,
                    };
                    if ft.is_dir() && rel.components().count() < 4 {
                        stack.push(rel.join(&name));
                    } else if ft.is_file() && name.starts_with("rollout-") && name.ends_with(&suffix) {
                        out.push(rel.join(&name));
                    }
                }
            }
        }
        Provider::Antigravity => {}
    }
    out
}

fn copy_path(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            let ft = e.file_type()?;
            // Never follow links out of the CLI folder.
            if ft.is_symlink() {
                continue;
            }
            copy_path(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(p) = to.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Copy conversation `id` from one CLI folder to another. Returns false when
/// the CLI has no saved copy of it (the caller then sends a recap instead).
/// An older copy in the target (switching back and forth) is replaced: the
/// source always holds the latest turns.
pub fn copy_session(provider: Provider, from: &Path, to: &Path, id: &str) -> Result<bool, String> {
    if !valid_id(id) {
        return Err("Invalid conversation id.".into());
    }
    if from == to {
        return Ok(true);
    }
    let files = session_files(provider, from, id);
    if files.is_empty() {
        return Ok(false);
    }
    for rel in files {
        copy_path(&from.join(&rel), &to.join(&rel)).map_err(|e| format!("Couldn't copy the conversation: {e}"))?;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("wren-handoff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn copies_claude_session() {
        let (a, b) = (tmp("ca"), tmp("cb"));
        let p = a.join("projects/-Users-me-proj");
        std::fs::create_dir_all(p.join("abc-1/subagents")).unwrap();
        std::fs::write(p.join("abc-1.jsonl"), "{\"x\":1}\n").unwrap();
        std::fs::write(p.join("abc-1/subagents/s.jsonl"), "{}").unwrap();
        std::fs::write(p.join("other.jsonl"), "{}").unwrap();
        assert!(copy_session(Provider::Claude, &a, &b, "abc-1").unwrap());
        let q = b.join("projects/-Users-me-proj");
        assert_eq!(std::fs::read_to_string(q.join("abc-1.jsonl")).unwrap(), "{\"x\":1}\n");
        assert!(q.join("abc-1/subagents/s.jsonl").is_file());
        assert!(!q.join("other.jsonl").exists(), "only this conversation");
        // Switching back: the newer copy replaces the old one.
        std::fs::write(q.join("abc-1.jsonl"), "{\"x\":2}\n").unwrap();
        assert!(copy_session(Provider::Claude, &b, &a, "abc-1").unwrap());
        assert_eq!(std::fs::read_to_string(p.join("abc-1.jsonl")).unwrap(), "{\"x\":2}\n");
        assert!(!copy_session(Provider::Claude, &a, &b, "missing").unwrap());
        assert!(copy_session(Provider::Claude, &a, &b, "../etc").is_err());
    }

    #[test]
    fn copies_codex_rollout() {
        let (a, b) = (tmp("xa"), tmp("xb"));
        let d = a.join("sessions/2026/11/26");
        std::fs::create_dir_all(&d).unwrap();
        let name = "rollout-2026-11-26T10-00-00-019a-thread-7.jsonl";
        std::fs::write(d.join(name), "{}").unwrap();
        std::fs::write(d.join("rollout-2026-11-26T10-00-00-other.jsonl"), "{}").unwrap();
        assert!(copy_session(Provider::Codex, &a, &b, "019a-thread-7").unwrap());
        assert!(b.join("sessions/2026/11/26").join(name).is_file());
        assert!(!b.join("sessions/2026/11/26/rollout-2026-11-26T10-00-00-other.jsonl").exists());
        assert!(!copy_session(Provider::Codex, &a, &b, "nope").unwrap());
    }
}
