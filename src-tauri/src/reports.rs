//! Deep research: Wren saves the final report as a Markdown file in the
//! pane's project folder (`<folder>/research/<date>-<title>.md`).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_REPORT_BYTES: usize = 5 * 1024 * 1024;

/// A safe file-name part from the report title: lowercase letters/digits
/// (any script) and dashes, at most 60 chars.
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
        if out.chars().count() >= 60 {
            break;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() { "report".into() } else { out }
}

/// Title = the first Markdown heading, else the first non-empty line.
pub fn title_of(text: &str) -> String {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with('#'))
        .or_else(|| text.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or("Research report");
    first.trim_start_matches('#').trim().to_string()
}

/// Writes the report and returns its path. Never overwrites a file.
/// `stamp` is the local date/time from the UI, e.g. "2026-11-24-0915".
pub fn save(cwd: &str, stamp: &str, text: &str) -> Result<PathBuf, String> {
    if text.len() > MAX_REPORT_BYTES {
        return Err("The report is too large to save.".into());
    }
    let root = Path::new(cwd);
    if cwd.is_empty() || !root.is_absolute() || !root.is_dir() {
        return Err("The project folder doesn't exist.".into());
    }
    let stamp: String = stamp.chars().filter(|c| c.is_ascii_digit() || *c == '-').take(20).collect();
    let dir = root.join("research");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
    let base = format!("{stamp}-{}", slug(&title_of(text)));
    for n in 0..100 {
        let name = if n == 0 { format!("{base}.md") } else { format!("{base}-{n}.md") };
        let path = dir.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut f) => {
                f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not save the report: {e}")),
        }
    }
    Err("Too many reports with the same name.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_slugs() {
        assert_eq!(title_of("intro\n# Rust vs Go: 2026 ✓\nbody"), "Rust vs Go: 2026 ✓");
        assert_eq!(slug("Rust vs Go: 2026 ✓"), "rust-vs-go-2026");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("???"), "report");
        let khmer = slug("ការស្រាវជ្រាវ AI");
        assert!(khmer.ends_with("-ai") && !khmer.contains('/'), "{khmer}");
    }

    #[test]
    fn saves_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("wren-report-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_string_lossy().to_string();
        let a = save(&cwd, "2026-11-24-0915", "# My Topic\nhello").unwrap();
        let b = save(&cwd, "2026-11-24-0915", "# My Topic\nagain").unwrap();
        assert_eq!(a.file_name().unwrap(), "2026-11-24-0915-my-topic.md");
        assert_eq!(b.file_name().unwrap(), "2026-11-24-0915-my-topic-1.md");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "# My Topic\nhello");
        assert!(save("relative/path", "x", "y").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
