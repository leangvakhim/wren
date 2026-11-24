//! Files attached to a message (paste, file picker, drag & drop): images,
//! spreadsheets, PDFs, documents, archives, code — any file that isn't a
//! program or installer.
//!
//! Every file is copied into Wren's private data folder
//! (`<app_data>/attachments/<id>/<name>`, readable only by you) and sent from
//! there:
//! - Claude Code: images as image blocks, PDFs as document blocks, every other
//!   file by path (Claude gets read access to the attachments folder with
//!   `--add-dir` and opens it with its own tools).
//! - Codex: images as `localImage` inputs, other files by path (Codex can read
//!   anywhere in its sandbox).
//! - Antigravity: `@<path>` for every file (its headless mode has no file input).
//!
//! Only files inside that folder can be sent, so a message can never make
//! Wren read some other file on the computer. Wren never opens or runs an
//! attachment itself.

use std::io::Read;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::Serialize;

/// Largest file (user's choice).
pub const MAX_FILE_BYTES: u64 = 100 * 1024 * 1024;
/// Images up to this size are sent as pictures; larger ones as files.
pub const MAX_INLINE_IMAGE_BYTES: u64 = 5 * 1024 * 1024;
/// PDFs up to this size go to Claude as documents; larger ones as files.
pub const MAX_INLINE_PDF_BYTES: u64 = 30 * 1024 * 1024;
pub const MAX_FILES: usize = 10;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Attachment {
    /// Absolute path of Wren's private copy.
    pub path: String,
    /// The original file name.
    pub name: String,
    pub size: u64,
    /// Best guess from the contents / name ("image/png", "application/pdf", "text/csv", ...).
    pub mime: String,
    /// "image" (shown as a picture) or "file".
    pub kind: String,
    /// `data:` URL thumbnail, images only.
    pub preview: Option<String>,
}

/// The image type from the file's first bytes (never trust the name).
pub fn sniff_image(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Programs and installers are refused (by contents AND by name). Scripts
/// are allowed: they're code to read, and Wren never runs attachments.
pub fn harmful(name: &str, head: &[u8]) -> Option<&'static str> {
    let magic: &[(&[u8], &str)] = &[
        (&[0x7F, b'E', b'L', b'F'], "a Linux program"),
        (&[0xFE, 0xED, 0xFA, 0xCE], "a Mac program"),
        (&[0xFE, 0xED, 0xFA, 0xCF], "a Mac program"),
        (&[0xCE, 0xFA, 0xED, 0xFE], "a Mac program"),
        (&[0xCF, 0xFA, 0xED, 0xFE], "a Mac program"),
        (&[0xCA, 0xFE, 0xBA, 0xBE], "a Mac or Java program"),
        (b"MZ", "a Windows program"),
        (b"xar!", "a Mac installer"),
    ];
    for (m, what) in magic {
        if head.starts_with(m) {
            return Some(what);
        }
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "exe" | "msi" | "msix" | "msp" | "dll" | "scr" | "com" | "cpl" | "sys" | "drv" | "ocx" => Some("a Windows program"),
        "app" | "dmg" | "pkg" | "mpkg" | "kext" | "dylib" => Some("a Mac program or installer"),
        "so" | "deb" | "rpm" | "appimage" | "run" | "snap" | "flatpak" => Some("a Linux program or installer"),
        "apk" | "aab" | "ipa" | "xapk" => Some("a phone app"),
        "jar" | "class" => Some("a Java program"),
        "iso" | "img" | "vhd" | "vhdx" | "vmdk" => Some("a disk image"),
        "lnk" => Some("a Windows shortcut"),
        _ => None,
    }
}

/// A rough type for display and for the AI ("text/csv", ...). Contents win over names.
pub fn mime_of(name: &str, head: &[u8]) -> String {
    if let Some(m) = sniff_image(head) {
        return m.into();
    }
    if head.starts_with(b"%PDF-") {
        return "application/pdf".into();
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "xls" => "application/vnd.ms-excel",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "json" => "application/json",
        "md" => "text/markdown",
        "txt" | "log" => "text/plain",
        "svg" => "image/svg+xml",
        _ if head.starts_with(b"PK\x03\x04") => "application/zip",
        _ => "application/octet-stream",
    }
    .into()
}

/// A safe file name: no folders, no control characters, at most 120 chars.
pub fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    out = out.trim().trim_start_matches('.').to_string();
    if out.chars().count() > 120 {
        let ext = out.rsplit_once('.').map(|(_, e)| e.to_string()).filter(|e| e.len() <= 10);
        let stem: String = out.chars().take(100).collect();
        out = match ext {
            Some(e) => format!("{stem}.{e}"),
            None => stem,
        };
    }
    if out.is_empty() { "file".into() } else { out }
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(app_data: &Path) -> Self {
        Self { dir: app_data.join("attachments") }
    }

    /// The attachments folder (Claude Code gets read access to it).
    pub fn dir(&self) -> PathBuf {
        let _ = self.ensure_dir();
        self.dir.clone()
    }

    fn ensure_dir(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    /// A new private folder for one file, so its own name can be kept.
    fn slot(&self, name: &str) -> Result<PathBuf, String> {
        self.ensure_dir()?;
        let sub = self.dir.join(unique_name());
        std::fs::create_dir_all(&sub).map_err(|e| e.to_string())?;
        Ok(sub.join(clean_name(name)))
    }

    fn checked(&self, name: &str, size: u64, head: &[u8]) -> Result<(), String> {
        if size > MAX_FILE_BYTES {
            return Err(format!("\"{}\" is larger than {} MB.", clean_name(name), MAX_FILE_BYTES / (1024 * 1024)));
        }
        if size == 0 {
            return Err(format!("\"{}\" is empty.", clean_name(name)));
        }
        if let Some(what) = harmful(name, head) {
            return Err(format!("\"{}\" looks like {what}, so it can't be attached.", clean_name(name)));
        }
        Ok(())
    }

    /// Saves pasted or dropped bytes as a new private copy.
    pub fn save(&self, name: &str, bytes: &[u8]) -> Result<Attachment, String> {
        self.checked(name, bytes.len() as u64, &bytes[..bytes.len().min(16)])?;
        let path = self.slot(name)?;
        std::fs::write(&path, bytes).map_err(|e| format!("Could not save the file: {e}"))?;
        Ok(describe(&path, bytes.len() as u64, &bytes[..bytes.len().min(16)], Some(bytes)))
    }

    /// Copies a file the user picked.
    pub fn import(&self, path: &str) -> Result<Attachment, String> {
        let src = Path::new(path);
        let meta = std::fs::metadata(src).map_err(|_| "That file can't be read.".to_string())?;
        let name = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
        if meta.is_dir() {
            return Err(format!("\"{name}\" is a folder. Zip it first to attach it."));
        }
        let mut head = [0u8; 16];
        let n = std::fs::File::open(src).and_then(|mut f| f.read(&mut head)).map_err(|e| format!("Could not read the file: {e}"))?;
        self.checked(&name, meta.len(), &head[..n])?;
        let dest = self.slot(&name)?;
        std::fs::copy(src, &dest).map_err(|e| format!("Could not copy the file: {e}"))?;
        let small = (meta.len() <= MAX_INLINE_IMAGE_BYTES && sniff_image(&head[..n]).is_some())
            .then(|| std::fs::read(&dest).ok())
            .flatten();
        Ok(describe(&dest, meta.len(), &head[..n], small.as_deref()))
    }

    /// The private copy at `path`, only if it really is one of Wren's attachments.
    pub fn check(&self, path: &str) -> Result<PathBuf, String> {
        let dir = std::fs::canonicalize(&self.dir).map_err(|_| "That file is no longer available.".to_string())?;
        let file = std::fs::canonicalize(path).map_err(|_| "That file is no longer available.".to_string())?;
        if !file.starts_with(&dir) || file == dir || !file.is_file() {
            return Err("Only files attached in Wren can be sent.".into());
        }
        Ok(file)
    }

    /// Everything needed to send one attachment.
    pub fn outgoing(&self, path: &str) -> Result<Outgoing, String> {
        let file = self.check(path)?;
        let size = std::fs::metadata(&file).map(|m| m.len()).map_err(|e| e.to_string())?;
        let mut head = [0u8; 16];
        let n = std::fs::File::open(&file).and_then(|mut f| f.read(&mut head)).map_err(|e| e.to_string())?;
        let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let mime = mime_of(&name, &head[..n]);
        let image = mime.starts_with("image/") && mime != "image/svg+xml" && size <= MAX_INLINE_IMAGE_BYTES;
        let pdf = mime == "application/pdf" && size <= MAX_INLINE_PDF_BYTES;
        let inline = if image || pdf {
            let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
            Some(base64::engine::general_purpose::STANDARD.encode(bytes))
        } else {
            None
        };
        Ok(Outgoing { path: file.to_string_lossy().to_string(), name, size, mime, inline_base64: inline })
    }

    /// Thumbnail of an image attachment in a saved conversation.
    pub fn preview(&self, path: &str) -> Result<String, String> {
        let file = self.check(path)?;
        let bytes = std::fs::read(&file).map_err(|e| e.to_string())?;
        let mime = sniff_image(&bytes).ok_or("Not an image.")?;
        Ok(data_url(mime, &bytes))
    }
}

fn describe(path: &Path, size: u64, head: &[u8], bytes: Option<&[u8]>) -> Attachment {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mime = mime_of(&name, head);
    let image = sniff_image(head).is_some() && size <= MAX_INLINE_IMAGE_BYTES;
    Attachment {
        path: path.to_string_lossy().to_string(),
        name,
        size,
        preview: if image { bytes.map(|b| data_url(&mime, b)) } else { None },
        kind: if image { "image" } else { "file" }.into(),
        mime,
    }
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

fn unique_name() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("a-{t:x}-{:x}", N.fetch_add(1, Ordering::SeqCst))
}

/// One attachment to send, read and checked before the message is written.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub mime: String,
    /// Images (≤5 MB) and PDFs (≤30 MB): the bytes, for CLIs that take them inline.
    pub inline_base64: Option<String>,
}

impl Outgoing {
    pub fn is_inline_image(&self) -> bool {
        self.inline_base64.is_some() && self.mime.starts_with("image/")
    }
    pub fn is_inline_pdf(&self) -> bool {
        self.inline_base64.is_some() && self.mime == "application/pdf"
    }
}

/// "12 KB", "3.4 MB"
pub fn human_size(n: u64) -> String {
    if n >= 1024 * 1024 {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    } else if n >= 1024 {
        format!("{} KB", n.div_ceil(1024))
    } else {
        format!("{n} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0];

    #[test]
    fn types_and_harm() {
        assert_eq!(sniff_image(PNG), Some("image/png"));
        assert_eq!(mime_of("data.csv", b"a,b\n1,2"), "text/csv");
        assert_eq!(mime_of("report.bin", b"%PDF-1.7"), "application/pdf", "contents win");
        assert_eq!(mime_of("x", b"PK\x03\x04...."), "application/zip");
        // Programs are refused by contents even when renamed, and by name.
        assert!(harmful("notes.txt", &[0xCF, 0xFA, 0xED, 0xFE]).is_some());
        assert!(harmful("photo.png", b"MZ\x90\x00").is_some());
        assert!(harmful("setup.exe", b"").is_some());
        assert!(harmful("Wren.dmg", b"").is_some());
        // Scripts, data, documents and archives are fine.
        for ok in ["deploy.sh", "run.command", "build.ps1", "data.xlsx", "a.zip", "paper.pdf", "notes.md"] {
            assert!(harmful(ok, b"#!/bin/sh").is_none(), "{ok}");
        }
    }

    #[test]
    fn names_are_cleaned() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\me\\sales Q3.xlsx"), "sales Q3.xlsx");
        assert_eq!(clean_name(".hidden"), "hidden");
        assert_eq!(clean_name("a\u{0}b?.csv"), "ab.csv");
        assert_eq!(clean_name(""), "file");
        assert!(clean_name(&format!("{}.csv", "x".repeat(300))).ends_with(".csv"));
    }

    #[test]
    fn save_import_check_outgoing() {
        let root = std::env::temp_dir().join(format!("wren-att-{}", std::process::id()));
        let store = Store::new(&root);
        let img = store.save("shot.png", PNG).unwrap();
        assert_eq!(img.kind, "image");
        assert!(img.preview.as_deref().unwrap().starts_with("data:image/png;base64,"));
        let csv = store.save("sales.csv", b"region,total\nKH,10\n").unwrap();
        assert_eq!((csv.kind.as_str(), csv.name.as_str(), csv.mime.as_str()), ("file", "sales.csv", "text/csv"));
        assert!(csv.preview.is_none());
        // Import copies a picked file and keeps its name.
        let picked = root.join("picked.zip");
        std::fs::write(&picked, b"PK\x03\x04zipdata").unwrap();
        let z = store.import(&picked.to_string_lossy()).unwrap();
        assert!(z.path.ends_with("/picked.zip") && z.path != picked.to_string_lossy());
        assert!(store.import(&root.to_string_lossy()).is_err(), "folders need zipping");
        // Outgoing: images and PDFs inline, the rest by path.
        assert!(store.outgoing(&img.path).unwrap().is_inline_image());
        let pdf = store.save("paper.pdf", b"%PDF-1.7 tiny").unwrap();
        assert!(store.outgoing(&pdf.path).unwrap().is_inline_pdf());
        let o = store.outgoing(&csv.path).unwrap();
        assert!(o.inline_base64.is_none() && o.name == "sales.csv" && o.size == 19);
        // Nothing outside the attachments folder can be sent.
        assert!(store.check(&picked.to_string_lossy()).is_err());
        assert!(store.check("/etc/passwd").is_err());
        assert!(store.check(&format!("{}/attachments/../picked.zip", root.display())).is_err());
        assert!(store.check(&store.dir().to_string_lossy()).is_err(), "the folder itself");
        // Refused: programs, empty, too large.
        assert!(store.save("tool.txt", b"\x7FELF\x02\x01").is_err());
        assert!(store.save("empty.txt", b"").is_err());
        assert_eq!(human_size(19), "19 bytes");
        assert_eq!(human_size(1536), "2 KB");
        std::fs::remove_dir_all(&root).ok();
    }
}
