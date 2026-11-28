//! The browser panel next to a pane, and the tiny local web server that shows
//! the AI's visualizations in it.
//!
//! Safety rules (the panel shows any website and pages the AI wrote):
//! - The panel is its own webview. Wren's commands are only allowed for the
//!   main webview (capability `webviews: ["main"]` + the app command
//!   allowlist in build.rs), so nothing loaded in the panel can call Wren.
//! - The panel only ever opens http(s) pages. Wren's own pages (`tauri://`,
//!   the dev server) and other schemes are refused, so a page can't turn
//!   the panel into a copy of Wren's window.
//! - Sign-in pages of Claude.ai, ChatGPT/OpenAI and Google accounts are not
//!   opened in the panel: signing in always happens through the CLIs' own
//!   flows in your normal browser (Anthropic's policy for third-party apps).
//! - AI-written pages are served from 127.0.0.1 under a random token, one
//!   token per folder, and only files inside that folder can be read.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager as _, Url, WebviewUrl};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// The folder (inside the project) where Visualize mode saves its pages.
pub const VISUALS_DIR: &str = "visuals";
/// Largest file the visuals server sends.
const MAX_SERVE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BrowserEvent {
    /// A page started loading.
    Loading { url: String },
    /// A page finished loading.
    Loaded { url: String },
    /// The page's title changed.
    Title { title: String },
    /// Wren didn't open this address in the panel (`reason` says why).
    Blocked { url: String, reason: String },
}

/// Sign-in sites that must not be used inside Wren's panel.
const SIGN_IN_HOSTS: &[&str] = &[
    "claude.ai",
    "console.anthropic.com",
    "platform.claude.com",
    "chatgpt.com",
    "chat.openai.com",
    "auth.openai.com",
    "platform.openai.com",
    "accounts.google.com",
];

fn is_sign_in_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    SIGN_IN_HOSTS.iter().any(|h| host == *h || host.ends_with(&format!(".{h}")))
}

/// May the panel open this address? `dev` = Wren's own dev-server address
/// (debug builds), which must never load in the panel.
pub fn check_url(url: &Url, dev: Option<&Url>) -> Result<(), String> {
    match url.scheme() {
        "http" | "https" => {}
        // Frames and pages a visualization makes itself.
        "about" | "blob" | "data" => return Ok(()),
        other => return Err(format!("Wren's browser only opens web pages (http/https), not \"{other}:\" addresses.")),
    }
    let host = url.host_str().unwrap_or_default();
    if host.is_empty() {
        return Err("That address has no host.".into());
    }
    if is_sign_in_host(host) {
        return Err(format!(
            "{host} is a sign-in site. For your account's safety Wren doesn't open it in its own browser. Use \"Open in your browser\" instead."
        ));
    }
    // localhost, 127.0.0.1 and [::1] all reach the same dev server.
    let loopback = |h: Option<&str>| matches!(h, Some("localhost" | "127.0.0.1" | "[::1]"));
    if let Some(dev) = dev
        && (url.host_str() == dev.host_str() || (loopback(url.host_str()) && loopback(dev.host_str())))
        && url.port_or_known_default() == dev.port_or_known_default()
    {
        return Err("That is Wren's own address.".into());
    }
    Ok(())
}

/// Turn what was typed in the address bar into a URL: "example.com" ->
/// https, "localhost:5173" -> http.
pub fn normalize_address(input: &str) -> Result<Url, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("Type an address.".into());
    }
    let with_scheme = if s.contains("://") || s.starts_with("about:") {
        s.to_string()
    } else {
        let host = s.split(['/', '?', '#']).next().unwrap_or_default();
        let bare = host.split(':').next().unwrap_or_default();
        let local = bare == "localhost" || bare.ends_with(".localhost") || bare.parse::<std::net::IpAddr>().is_ok();
        if local { format!("http://{s}") } else { format!("https://{s}") }
    };
    Url::parse(&with_scheme).map_err(|_| format!("\"{s}\" isn't a web address."))
}

// ---------------- the visuals server ----------------

/// Serves AI-written pages (and the files next to them) on 127.0.0.1.
#[derive(Default)]
pub struct Visuals {
    port: Mutex<Option<u16>>,
    /// token -> folder
    roots: Arc<Mutex<HashMap<String, PathBuf>>>,
}

/// 128 random bits as hex (std's hasher keys are random per process).
fn random_token() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for i in 0..2u64 {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(i);
        h.write_u128(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default());
        out.push_str(&format!("{:016x}", h.finish()));
    }
    out
}

impl Visuals {
    /// Start the server (once). Returns its port.
    pub async fn start(&self) -> Result<u16, String> {
        if let Some(p) = *self.port.lock().map_err(|e| e.to_string())? {
            return Ok(p);
        }
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(|e| format!("Could not start the visuals server: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        let roots = self.roots.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let roots = roots.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(30), serve(stream, roots)).await;
                });
            }
        });
        let mut slot = self.port.lock().map_err(|e| e.to_string())?;
        // Another call may have started one meanwhile; keep the first.
        Ok(*slot.get_or_insert(port))
    }

    /// The address of a page inside the project folder `cwd`. `path` may be
    /// relative to `cwd`. Only .html/.htm/.svg files inside `cwd` are allowed.
    pub async fn url_for(&self, cwd: &str, path: &str) -> Result<String, String> {
        let (root, rel) = page_in_project(cwd, path)?;
        let port = self.start().await?;
        let token = {
            let mut roots = self.roots.lock().map_err(|e| e.to_string())?;
            match roots.iter().find(|(_, r)| **r == root) {
                Some((t, _)) => t.clone(),
                None => {
                    let t = random_token();
                    roots.insert(t.clone(), root);
                    t
                }
            }
        };
        let rel: Vec<String> = rel.components().map(|c| percent_encode(&c.as_os_str().to_string_lossy())).collect();
        Ok(format!("http://127.0.0.1:{port}/{token}/{}", rel.join("/")))
    }
}

/// Checks that `path` is a page inside `cwd`; returns (the page's folder, the
/// page's name inside it).
pub fn page_in_project(cwd: &str, path: &str) -> Result<(PathBuf, PathBuf), String> {
    let project = std::fs::canonicalize(cwd).map_err(|_| "The project folder doesn't exist.".to_string())?;
    let p = Path::new(path.trim());
    let full = if p.is_absolute() { p.to_path_buf() } else { project.join(p) };
    let full = std::fs::canonicalize(&full).map_err(|_| format!("{path} doesn't exist."))?;
    if !full.starts_with(&project) {
        return Err("Only pages inside this pane's project folder can be opened.".into());
    }
    let ext = full.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if !matches!(ext.as_str(), "html" | "htm" | "svg") || !full.is_file() {
        return Err("Only .html and .svg pages can be opened in Wren's browser.".into());
    }
    let root = full.parent().ok_or("Bad path")?.to_path_buf();
    let name = PathBuf::from(full.file_name().ok_or("Bad path")?);
    Ok((root, name))
}

fn percent_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let v = u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?;
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase().as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "wasm" => "application/wasm",
        "csv" => "text/csv; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

/// Which file a request path points to, or None if it isn't allowed.
pub fn resolve_request(roots: &HashMap<String, PathBuf>, target: &str) -> Option<PathBuf> {
    let path = target.split(['?', '#']).next()?;
    let mut parts = path.trim_start_matches('/').splitn(2, '/');
    let root = roots.get(parts.next()?)?;
    let rel = percent_decode(parts.next().unwrap_or_default())?;
    let rel = Path::new(&rel);
    // Plain names only: no "..", no absolute parts.
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let mut full = root.join(rel);
    if full.is_dir() {
        full = full.join("index.html");
    }
    let full = std::fs::canonicalize(full).ok()?;
    let root = std::fs::canonicalize(root).ok()?;
    (full.starts_with(&root) && full.is_file()).then_some(full)
}

async fn serve(mut stream: TcpStream, roots: Arc<Mutex<HashMap<String, PathBuf>>>) {
    let mut buf = vec![0u8; 8192];
    let mut n = 0;
    // Read until the end of the request headers.
    loop {
        match stream.read(&mut buf[n..]).await {
            Ok(0) | Err(_) => return,
            Ok(k) => n += k,
        }
        if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") || n == buf.len() {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf[..n]);
    let mut first = head.lines().next().unwrap_or_default().split(' ');
    let (method, target) = (first.next().unwrap_or_default(), first.next().unwrap_or_default());
    // Only browsers on this computer showing 127.0.0.1 pages get answers
    // (a page on another site can't use a "Host: evil.com" rebinding trick).
    let host_ok = head.lines().any(|l| {
        let l = l.to_ascii_lowercase();
        l.starts_with("host:") && l["host:".len()..].trim().starts_with("127.0.0.1:")
    });
    let file = if host_ok && matches!(method, "GET" | "HEAD") {
        let roots = roots.lock().map(|r| r.clone()).unwrap_or_default();
        resolve_request(&roots, target)
    } else {
        None
    };
    let Some(file) = file else {
        let body = "Not found";
        let _ = stream
            .write_all(format!("HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes())
            .await;
        return;
    };
    let data = match tokio::fs::metadata(&file).await {
        Ok(m) if m.len() <= MAX_SERVE_BYTES => tokio::fs::read(&file).await.unwrap_or_default(),
        _ => Vec::new(),
    };
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        mime_for(&file),
        data.len()
    );
    if stream.write_all(header.as_bytes()).await.is_ok() && method == "GET" {
        let _ = stream.write_all(&data).await;
    }
    let _ = stream.flush().await;
}

// ---------------- page source (the panel's Code tab) ----------------

/// Largest source the Code tab shows or saves.
pub const MAX_SOURCE_BYTES: usize = 5 * 1024 * 1024;

/// Local files the Code tab may change (text the page uses, never programs or pictures).
const EDITABLE_EXTS: &[&str] = &["html", "htm", "svg", "css", "js", "mjs", "json", "txt", "md", "csv"];

/// What the panel's Code tab shows.
#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// "file" = a page from your project (served by Wren), "web" = a copy of
    /// an internet page's HTML, fetched again without your cookies.
    pub kind: &'static str,
    pub name: String,
    /// The local file (kind "file" only).
    pub path: Option<String>,
    /// None = the file hasn't changed since `known_mtime`.
    pub text: Option<String>,
    /// Last change of the local file in ms (0 for web pages).
    pub mtime: u64,
    pub editable: bool,
    /// Why it's read-only or incomplete, if it is.
    pub note: Option<String>,
}

fn mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn editable_ext(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    EDITABLE_EXTS.contains(&ext.as_str())
}

impl Visuals {
    /// The local file behind an address of this server, or None if the
    /// address is anything else.
    pub fn file_for(&self, address: &str) -> Option<PathBuf> {
        let url = Url::parse(address).ok()?;
        let port = (*self.port.lock().ok()?)?;
        if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") || url.port() != Some(port) {
            return None;
        }
        let roots = self.roots.lock().ok()?.clone();
        resolve_request(&roots, url.path())
    }

    /// The source of the page at `address`: the file itself for Wren's own
    /// pages, otherwise a fresh copy from the web.
    pub async fn source(&self, address: &str, known_mtime: Option<u64>, dev: Option<&Url>) -> Result<Source, String> {
        let Some(file) = self.file_for(address) else {
            return fetch_source(address, dev).await;
        };
        read_source(&file, known_mtime)
    }

    /// Write the Code tab's text back to the page's file. Returns the new mtime.
    pub fn save(&self, address: &str, text: &str) -> Result<u64, String> {
        let file = self.file_for(address).ok_or("Only pages from your project can be changed. Use Save as… for a copy.")?;
        write_source(&file, text)
    }
}

pub fn read_source(file: &Path, known_mtime: Option<u64>) -> Result<Source, String> {
    let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mtime = mtime_ms(file);
    let base = |text, editable, note| Source { kind: "file", name: name.clone(), path: Some(file.to_string_lossy().to_string()), text, mtime, editable, note };
    if known_mtime == Some(mtime) {
        return Ok(base(None, editable_ext(file), None));
    }
    let len = std::fs::metadata(file).map(|m| m.len()).unwrap_or_default();
    if len as usize > MAX_SOURCE_BYTES {
        return Ok(base(Some(String::new()), false, Some(format!("This file is too big to show here ({} MB).", len / (1024 * 1024)))));
    }
    let bytes = std::fs::read(file).map_err(|e| format!("Couldn't read {name}: {e}"))?;
    Ok(match String::from_utf8(bytes) {
        Ok(text) => base(Some(text), editable_ext(file), (!editable_ext(file)).then(|| "This kind of file can't be changed here.".to_string())),
        Err(e) => base(Some(String::from_utf8_lossy(e.as_bytes()).to_string()), false, Some("This file isn't plain UTF-8 text, so it's read-only here.".into())),
    })
}

/// Replace `file` with `text` (all at once: a crash never leaves half a file).
pub fn write_source(file: &Path, text: &str) -> Result<u64, String> {
    if !editable_ext(file) {
        return Err("This kind of file can't be changed here.".into());
    }
    if text.len() > MAX_SOURCE_BYTES {
        return Err("That is too much text to save (max 5 MB).".into());
    }
    let name = file.file_name().ok_or("Bad path")?.to_string_lossy().to_string();
    let tmp = file.with_file_name(format!(".{name}.wren-save"));
    std::fs::write(&tmp, text).map_err(|e| format!("Couldn't save {name}: {e}"))?;
    if let Ok(meta) = std::fs::metadata(file) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't save {name}: {e}")
    })?;
    Ok(mtime_ms(file))
}

/// A web page's HTML, fetched again by Wren (no cookies, so pages that need
/// a sign-in show their signed-out version). Same address rules as the panel.
pub async fn fetch_source(address: &str, dev: Option<&Url>) -> Result<Source, String> {
    let url = Url::parse(address).map_err(|_| "That isn't a web address.".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only web pages (http/https) have a source to show.".into());
    }
    check_url(&url, dev)?;
    let dev = dev.cloned();
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("Mozilla/5.0 (compatible; Wren/", env!("CARGO_PKG_VERSION"), ")"))
        .redirect(reqwest::redirect::Policy::custom(move |a| {
            let ok = matches!(a.url().scheme(), "http" | "https") && check_url(a.url(), dev.as_ref()).is_ok();
            if a.previous().len() > 8 || !ok { a.stop() } else { a.follow() }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let fail = |e: String| format!("Couldn't get the page's source ({e}).");
    let mut resp = client
        .get(url.clone())
        .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,*/*;q=0.8")
        .send()
        .await
        .map_err(|e| fail(e.to_string()))?;
    if resp.status().is_redirection() {
        return Err(fail("it sends you to an address Wren doesn't open".into()));
    }
    if !resp.status().is_success() {
        return Err(fail(format!("the site answered {}", resp.status())));
    }
    let mut bytes = Vec::new();
    let mut cut = false;
    while let Some(chunk) = resp.chunk().await.map_err(|e| fail(e.to_string()))? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_SOURCE_BYTES {
            bytes.truncate(MAX_SOURCE_BYTES);
            cut = true;
            break;
        }
    }
    let name = url.path_segments().and_then(|mut s| s.next_back()).filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| url.host_str().unwrap_or("page").to_string());
    let mut note = "A web page: read-only. Wren fetched its HTML again without your cookies, so it can differ from what the page shows now.".to_string();
    if cut {
        note.push_str(" Only the first 5 MB are shown.");
    }
    Ok(Source { kind: "web", name, path: None, text: Some(String::from_utf8_lossy(&bytes).to_string()), mtime: 0, editable: false, note: Some(note) })
}

/// Show a file in Finder / Explorer / the file manager.
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg("-R").arg(path).spawn();
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(path.parent().unwrap_or(path)).spawn();
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        let mut arg = std::ffi::OsString::from("/select,");
        arg.push(path);
        std::process::Command::new("explorer").raw_arg(arg).creation_flags(0x0800_0000).spawn()
    };
    r.map(|_| ()).map_err(|e| format!("Couldn't open the folder: {e}"))
}

/// The newest pages in `<cwd>/visuals` changed at or after `since_ms`.
pub fn recent_visuals(cwd: &str, since_ms: u64) -> Vec<String> {
    let dir = Path::new(cwd).join(VISUALS_DIR);
    let Ok(entries) = std::fs::read_dir(&dir) else { return vec![] };
    let mut found: Vec<(u128, String)> = entries
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            if !matches!(ext.as_str(), "html" | "htm") {
                return None;
            }
            let t = e.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_millis();
            (t >= since_ms as u128).then(|| (t, p.to_string_lossy().to_string()))
        })
        .collect();
    found.sort_by_key(|f| std::cmp::Reverse(f.0));
    found.into_iter().map(|(_, p)| p).collect()
}

// ---------------- the panel (a child webview in the main window) ----------------

/// Live panels: pane id -> where its page events go.
#[derive(Default)]
pub struct Panels {
    channels: Arc<Mutex<HashMap<String, Channel<BrowserEvent>>>>,
}

fn label_for(pane_id: &str) -> Result<String, String> {
    let ok = pane_id.strip_prefix("pane-").is_some_and(|n| matches!(n, "1" | "2" | "3" | "4"));
    if !ok {
        return Err("unknown pane".into());
    }
    Ok(format!("browser-{pane_id}"))
}

/// Wren's dev-server address (debug builds only), never allowed in the panel.
pub fn dev_url(app: &AppHandle) -> Option<Url> {
    if cfg!(debug_assertions) {
        app.config().build.dev_url.clone()
    } else {
        None
    }
}

#[derive(serde::Deserialize, Clone, Copy, Debug)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Height of Wren's page (CSS `innerHeight`), to find how far macOS
    /// pushes the page down under the title bar (0 = unknown).
    #[serde(default)]
    pub vh: f64,
}

/// Put the panel's page over `b` (CSS pixels of Wren's page).
///
/// macOS: wry measures a child webview's position against a different view
/// than the one Wren's page fills (they differ by about the title bar in a
/// normal window and match in full screen), so its own placement is off.
/// Instead AppKit converts the box from Wren's page to the panel's parent
/// view, which is exact in every window state.
fn put(app: &AppHandle, wv: &tauri::Webview, b: Bounds) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let main = main_view(app)?;
        return wv
            .with_webview(move |p| unsafe { mac::place(main as *mut std::ffi::c_void, p.inner(), b) })
            .map_err(|e| e.to_string());
    }
    #[allow(unreachable_code)]
    {
        let _ = app;
        wv.set_position(LogicalPosition::new(b.x, b.y)).map_err(|e| e.to_string())?;
        wv.set_size(LogicalSize::new(b.w, b.h)).map_err(|e| e.to_string())
    }
}

/// The native view of Wren's own page (asked once; it lives as long as Wren).
#[cfg(target_os = "macos")]
fn main_view(app: &AppHandle) -> Result<usize, String> {
    static MAIN: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    if let Some(p) = MAIN.get() {
        return Ok(*p);
    }
    let main = app.get_webview("main").ok_or("Wren's window isn't open.")?;
    let (tx, rx) = std::sync::mpsc::channel();
    main.with_webview(move |p| {
        let _ = tx.send(p.inner() as usize);
    })
    .map_err(|e| e.to_string())?;
    let p = rx.recv_timeout(Duration::from_secs(2)).map_err(|_| "Wren's window didn't answer.".to_string())?;
    Ok(*MAIN.get_or_init(|| p))
}

#[cfg(target_os = "macos")]
mod mac {
    use super::Bounds;
    use objc2_app_kit::NSView;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// Runs on the main thread. `main` and `child` are WKWebViews.
    pub unsafe fn place(main: *mut std::ffi::c_void, child: *mut std::ffi::c_void, b: Bounds) {
        let (Some(main), Some(child)) = (unsafe { (main as *const NSView).as_ref() }, unsafe { (child as *const NSView).as_ref() }) else {
            return;
        };
        let Some(parent) = (unsafe { child.superview() }) else { return };
        // Wren's view reaches up under the title bar and macOS pads the page
        // down by that much (0 in full screen): CSS y = 0 is `inset` points
        // below the view's top. CSS pixels = points at zoom 1.
        let view_h = main.bounds().size.height;
        let inset = if b.vh > 0.0 { (view_h - b.vh).max(0.0) } else { 0.0 };
        let top = b.y + inset;
        let y = if main.isFlipped() { top } else { view_h - top - b.h };
        let rect = NSRect::new(NSPoint::new(b.x, y), NSSize::new(b.w, b.h));
        let frame = parent.convertRect_fromView(rect, Some(main));
        child.setFrame(frame);
    }
}

impl Panels {
    fn send(channels: &Arc<Mutex<HashMap<String, Channel<BrowserEvent>>>>, pane: &str, ev: BrowserEvent) {
        if let Ok(map) = channels.lock()
            && let Some(ch) = map.get(pane)
        {
            let _ = ch.send(ev);
        }
    }

    /// Open `address` in the pane's panel (created on first use).
    pub fn open(&self, app: &AppHandle, pane_id: &str, address: &str, b: Bounds, channel: Channel<BrowserEvent>) -> Result<String, String> {
        let label = label_for(pane_id)?;
        let url = normalize_address(address)?;
        let dev = dev_url(app);
        check_url(&url, dev.as_ref())?;
        self.channels.lock().map_err(|e| e.to_string())?.insert(pane_id.to_string(), channel);
        if let Some(wv) = app.get_webview(&label) {
            wv.navigate(url.clone()).map_err(|e| e.to_string())?;
            self.place(app, pane_id, b, true)?;
            return Ok(url.to_string());
        }
        let window = app.get_window("main").ok_or("Wren's window isn't open.")?;
        let (pane, ch1, ch2, ch3, ch4) = (pane_id.to_string(), self.channels.clone(), self.channels.clone(), self.channels.clone(), self.channels.clone());
        let (p2, p3, p4) = (pane.clone(), pane.clone(), pane.clone());
        let (dev1, dev2) = (dev.clone(), dev);
        let (app2, label2) = (app.clone(), label.clone());
        let builder = WebviewBuilder::new(&label, WebviewUrl::External(url.clone()))
            .on_navigation(move |u| match check_url(u, dev1.as_ref()) {
                Ok(()) => true,
                Err(reason) => {
                    Panels::send(&ch1, &pane, BrowserEvent::Blocked { url: u.to_string(), reason });
                    false
                }
            })
            // Links that want a new window open in the panel itself.
            .on_new_window(move |u, _| {
                match check_url(&u, dev2.as_ref()) {
                    Ok(()) => {
                        if let Some(wv) = app2.get_webview(&label2) {
                            let _ = wv.navigate(u);
                        }
                    }
                    Err(reason) => Panels::send(&ch2, &p2, BrowserEvent::Blocked { url: u.to_string(), reason }),
                }
                NewWindowResponse::Deny
            })
            .on_page_load(move |_wv, payload| {
                let url = payload.url().to_string();
                let ev = match payload.event() {
                    PageLoadEvent::Started => BrowserEvent::Loading { url },
                    PageLoadEvent::Finished => BrowserEvent::Loaded { url },
                };
                Panels::send(&ch3, &p3, ev);
            })
            .on_document_title_changed(move |_wv, title| Panels::send(&ch4, &p4, BrowserEvent::Title { title }))
            .devtools(cfg!(debug_assertions));
        let wv = window
            .add_child(builder, LogicalPosition::new(b.x, b.y), LogicalSize::new(b.w.max(1.0), b.h.max(1.0)))
            .map_err(|e| format!("Could not open the browser panel: {e}"))?;
        put(app, &wv, b)?;
        Ok(url.to_string())
    }

    /// Move/resize the panel to the given box (CSS pixels of the main window), or hide it.
    pub fn place(&self, app: &AppHandle, pane_id: &str, b: Bounds, visible: bool) -> Result<(), String> {
        let Some(wv) = app.get_webview(&label_for(pane_id)?) else { return Ok(()) };
        if !visible || b.w < 2.0 || b.h < 2.0 {
            return wv.hide().map_err(|e| e.to_string());
        }
        put(app, &wv, b)?;
        wv.show().map_err(|e| e.to_string())
    }

    /// Back / forward / reload / stop.
    pub fn go(&self, app: &AppHandle, pane_id: &str, action: &str) -> Result<(), String> {
        let wv = app.get_webview(&label_for(pane_id)?).ok_or("The browser panel isn't open.")?;
        match action {
            "back" => wv.eval("history.back()"),
            "forward" => wv.eval("history.forward()"),
            "reload" => wv.reload(),
            "stop" => wv.eval("window.stop()"),
            _ => return Err("unknown action".into()),
        }
        .map_err(|e| e.to_string())
    }

    pub fn close(&self, app: &AppHandle, pane_id: &str) -> Result<(), String> {
        let label = label_for(pane_id)?;
        if let Ok(mut m) = self.channels.lock() {
            m.remove(pane_id);
        }
        if let Some(wv) = app.get_webview(&label) {
            wv.close().map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Open an http(s) address in the computer's normal browser.
pub fn open_external(address: &str) -> Result<(), String> {
    let url = normalize_address(address)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only web addresses can be opened.".into());
    }
    let s = url.to_string();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&s).spawn();
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(&s).spawn();
    #[cfg(windows)]
    let r = {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &s]).creation_flags(0x0800_0000).spawn()
    };
    r.map(|_| ()).map_err(|e| format!("Could not open your browser: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn addresses_are_normalized() {
        assert_eq!(normalize_address("example.com").unwrap().as_str(), "https://example.com/");
        assert_eq!(normalize_address("localhost:5173/x").unwrap().as_str(), "http://localhost:5173/x");
        assert_eq!(normalize_address("127.0.0.1:8000").unwrap().as_str(), "http://127.0.0.1:8000/");
        assert_eq!(normalize_address(" https://a.b/c?d ").unwrap().as_str(), "https://a.b/c?d");
        assert!(normalize_address("").is_err());
    }

    #[test]
    fn only_web_pages_and_no_sign_in_sites() {
        let dev = u("http://localhost:1420");
        assert!(check_url(&u("https://example.com/"), Some(&dev)).is_ok());
        assert!(check_url(&u("http://localhost:5173/"), Some(&dev)).is_ok());
        assert!(check_url(&u("http://127.0.0.1:4000/tok/a.html"), None).is_ok());
        assert!(check_url(&u("about:blank"), None).is_ok());
        // Wren's own pages and other schemes
        assert!(check_url(&u("tauri://localhost/"), None).is_err());
        assert!(check_url(&u("http://localhost:1420/"), Some(&dev)).is_err());
        assert!(check_url(&u("http://127.0.0.1:1420/"), Some(&dev)).is_err());
        assert!(check_url(&u("file:///etc/passwd"), None).is_err());
        assert!(check_url(&u("ipc://localhost/open_pane"), None).is_err());
        // sign-in sites (and their subdomains), but not look-alikes
        for s in ["https://claude.ai/login", "https://www.claude.ai/", "https://accounts.google.com/", "https://chatgpt.com/", "https://auth.openai.com/x"] {
            assert!(check_url(&u(s), None).is_err(), "{s}");
        }
        assert!(check_url(&u("https://notclaude.ai/"), None).is_ok());
        assert!(check_url(&u("https://docs.claude.com/"), None).is_ok());
    }

    #[test]
    fn pages_must_be_inside_the_project() {
        let tmp = std::env::temp_dir().join(format!("wren-vis-{}", random_token()));
        let vis = tmp.join("proj").join("visuals");
        std::fs::create_dir_all(&vis).unwrap();
        std::fs::write(vis.join("sort.html"), "<p>hi</p>").unwrap();
        std::fs::write(tmp.join("outside.html"), "x").unwrap();
        std::fs::write(vis.join("notes.txt"), "x").unwrap();
        let cwd = tmp.join("proj").to_string_lossy().to_string();
        let (root, name) = page_in_project(&cwd, "visuals/sort.html").unwrap();
        assert!(root.ends_with("visuals"));
        assert_eq!(name, PathBuf::from("sort.html"));
        assert!(page_in_project(&cwd, "../outside.html").is_err());
        assert!(page_in_project(&cwd, &tmp.join("outside.html").to_string_lossy()).is_err());
        assert!(page_in_project(&cwd, "visuals/notes.txt").is_err());
        assert!(page_in_project(&cwd, "visuals/missing.html").is_err());
        let found = recent_visuals(&cwd, 0);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("sort.html"));
        let future = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64 + 60_000;
        assert!(recent_visuals(&cwd, future).is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_server_only_reads_inside_its_folders() {
        let tmp = std::env::temp_dir().join(format!("wren-srv-{}", random_token()));
        let root = tmp.join("visuals");
        std::fs::create_dir_all(root.join("lib")).unwrap();
        std::fs::write(root.join("a b.html"), "a").unwrap();
        std::fs::write(root.join("lib").join("x.js"), "x").unwrap();
        std::fs::write(tmp.join("secret.txt"), "s").unwrap();
        let roots: HashMap<String, PathBuf> = [("tok".to_string(), root.clone())].into();
        assert!(resolve_request(&roots, "/tok/a%20b.html").is_some());
        assert!(resolve_request(&roots, "/tok/lib/x.js?v=1").is_some());
        assert!(resolve_request(&roots, "/tok/../secret.txt").is_none());
        assert!(resolve_request(&roots, "/tok/%2e%2e/secret.txt").is_none());
        assert!(resolve_request(&roots, "/tok/..%2Fsecret.txt").is_none());
        assert!(resolve_request(&roots, "/other/a%20b.html").is_none());
        assert!(resolve_request(&roots, "/tok/missing.html").is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn the_code_tab_reads_and_saves_only_served_files() {
        let tmp = std::env::temp_dir().join(format!("wren-src-{}", random_token()));
        let vis = tmp.join("visuals");
        std::fs::create_dir_all(&vis).unwrap();
        std::fs::write(vis.join("demo.html"), "<h1>one</h1>").unwrap();
        std::fs::write(vis.join("pic.png"), b"\x89PNG").unwrap();
        std::fs::write(tmp.join("secret.html"), "s").unwrap();
        let v = Visuals::default();
        let url = v.url_for(&tmp.to_string_lossy(), "visuals/demo.html").await.unwrap();
        let src = v.source(&url, None, None).await.unwrap();
        assert_eq!((src.kind, src.text.as_deref(), src.editable), ("file", Some("<h1>one</h1>"), true));
        // unchanged -> no text
        assert!(v.source(&url, Some(src.mtime), None).await.unwrap().text.is_none());
        let m = v.save(&url, "<h1>two</h1>").unwrap();
        assert_eq!(std::fs::read_to_string(vis.join("demo.html")).unwrap(), "<h1>two</h1>");
        assert!(!vis.join(".demo.html.wren-save").exists());
        assert_eq!(v.source(&url, Some(0), None).await.unwrap().mtime, m);
        // pictures and files outside the served folder can't be written
        let png = url.replace("demo.html", "pic.png");
        assert!(!v.source(&png, None, None).await.unwrap().editable);
        assert!(v.save(&png, "x").is_err());
        assert!(v.save(&url.replace("demo.html", "../secret.html"), "x").is_err());
        assert!(v.save(&url.replace("127.0.0.1", "localhost"), "x").is_err());
        assert!(v.save("https://example.com/demo.html", "x").is_err());
        assert_eq!(std::fs::read_to_string(tmp.join("secret.html")).unwrap(), "s");
        // other addresses are web pages: never Wren's own or sign-in sites
        assert!(fetch_source("tauri://localhost/", None).await.is_err());
        assert!(fetch_source("https://claude.ai/", None).await.is_err());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn web_sources_are_fetched_with_the_panels_rules() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = vec![0u8; 4096];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let reply = if head.starts_with("GET /away") {
                    "HTTP/1.1 302 Found\r\nLocation: https://accounts.google.com/x\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else if head.starts_with("GET /hop") {
                    "HTTP/1.1 302 Found\r\nLocation: /page.html\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
                } else {
                    let body = "<!doctype html><p>caf\u{e9}</p>";
                    format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                };
                let _ = s.write_all(reply.as_bytes()).await;
            }
        });
        let src = fetch_source(&format!("http://127.0.0.1:{port}/hop"), None).await.unwrap();
        assert_eq!((src.kind, src.editable, src.name.as_str()), ("web", false, "hop"));
        assert_eq!(src.text.as_deref(), Some("<!doctype html><p>caf\u{e9}</p>"));
        // a redirect to a sign-in site is not followed
        assert!(fetch_source(&format!("http://127.0.0.1:{port}/away"), None).await.is_err());
        // Wren's own dev server is refused
        let dev = Url::parse(&format!("http://localhost:{port}")).unwrap();
        assert!(fetch_source(&format!("http://127.0.0.1:{port}/"), Some(&dev)).await.is_err());
    }

    #[tokio::test]
    async fn the_server_answers_over_http() {
        let tmp = std::env::temp_dir().join(format!("wren-http-{}", random_token()));
        let vis = tmp.join("visuals");
        std::fs::create_dir_all(&vis).unwrap();
        std::fs::write(vis.join("demo.html"), "<h1>demo</h1>").unwrap();
        let v = Visuals::default();
        let url = v.url_for(&tmp.to_string_lossy(), "visuals/demo.html").await.unwrap();
        assert!(url.starts_with("http://127.0.0.1:"));
        let parsed = Url::parse(&url).unwrap();
        let port = parsed.port().unwrap();
        async fn get(port: u16, host: &str, path: &str) -> String {
            let mut s = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n").as_bytes()).await.unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).await.unwrap();
            out
        }
        let ok = get(port, &format!("127.0.0.1:{port}"), parsed.path()).await;
        assert!(ok.starts_with("HTTP/1.1 200"), "{ok}");
        assert!(ok.contains("text/html") && ok.ends_with("<h1>demo</h1>"));
        let rebound = get(port, "evil.example:80", parsed.path()).await;
        assert!(rebound.starts_with("HTTP/1.1 404"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
