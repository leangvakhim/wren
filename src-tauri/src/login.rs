//! Sign-in: runs the CLI's OWN sign-in command in a small terminal (PTY) that
//! Wren shows to you. Wren pulls the sign-in link out of the output so you
//! can copy it into any browser. If the CLI asks for a code, you type it
//! into the terminal view yourself. Wren never sees or stores the token.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::Mutex;

use base64::Engine;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};
use regex::Regex;
use serde::Serialize;
use tauri::ipc::Channel;

use crate::providers::Provider;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoginEvent {
    /// Raw terminal bytes, base64.
    Output { data: String },
    /// A sign-in link found in the output.
    Url { url: String },
    Exited { code: Option<u32> },
}

struct LoginSession {
    writer: Box<dyn Write + Send>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

#[derive(Default)]
pub struct LoginManager {
    sessions: Mutex<HashMap<String, LoginSession>>,
}

impl LoginManager {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        &self,
        login_id: String,
        provider: Provider,
        args: Vec<&'static str>,
        env: Vec<(String, String)>,
        cols: u16,
        rows: u16,
        channel: Channel<LoginEvent>,
    ) -> Result<(), String> {
        let path = crate::sessions::search_path();
        let home = home_dir();
        let bin = which::which_in(provider.binary(), Some(&path), &home)
            .map_err(|_| format!("The `{}` CLI isn't installed.", provider.binary()))?;
        self.start_with_bin(login_id, bin, provider, args, env, cols, rows, channel)
    }

    #[allow(clippy::too_many_arguments)]
    fn start_with_bin(
        &self,
        login_id: String,
        bin: std::path::PathBuf,
        provider: Provider,
        args: Vec<&'static str>,
        env: Vec<(String, String)>,
        cols: u16,
        rows: u16,
        channel: Channel<LoginEvent>,
    ) -> Result<(), String> {
        self.cancel(&login_id);
        let path = crate::sessions::search_path();
        let home = home_dir();
        let pair = native_pty_system()
            .openpty(PtySize { rows: rows.max(5), cols: cols.max(20), pixel_width: 0, pixel_height: 0 })
            .map_err(|e| e.to_string())?;
        let mut cmd = CommandBuilder::new(bin);
        cmd.args(args);
        cmd.cwd(&home);
        cmd.env("PATH", &path);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in provider.env() {
            cmd.env(k, v);
        }
        for (k, v) in &env {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let killer = child.clone_killer();

        // output -> UI, and find sign-in links
        let ch = channel.clone();
        let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            let _done = done_tx; // dropped when output ends
            let mut finder = UrlFinder::default();
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = &buf[..n];
                        let _ = ch.send(LoginEvent::Output {
                            data: base64::engine::general_purpose::STANDARD.encode(chunk),
                        });
                        for url in finder.feed(chunk) {
                            let _ = ch.send(LoginEvent::Url { url });
                        }
                    }
                }
            }
        });

        // exit watcher
        std::thread::spawn(move || {
            let code = child.wait().ok().map(|s| s.exit_code());
            // let the last output reach the UI first
            let _ = done_rx.recv_timeout(std::time::Duration::from_millis(1500));
            let _ = channel.send(LoginEvent::Exited { code });
        });

        self.sessions
            .lock()
            .map_err(|e| e.to_string())?
            .insert(login_id, LoginSession { writer, master: pair.master, killer });
        Ok(())
    }

    /// Keystrokes typed into the terminal view (e.g. a pasted code).
    pub fn input(&self, login_id: &str, data: &str) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(login_id).ok_or("Sign-in is not running")?;
        s.writer.write_all(data.as_bytes()).map_err(|e| e.to_string())?;
        s.writer.flush().map_err(|e| e.to_string())
    }

    pub fn resize(&self, login_id: &str, cols: u16, rows: u16) -> Result<(), String> {
        let map = self.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(s) = map.get(login_id) {
            s.master
                .resize(PtySize { rows: rows.max(5), cols: cols.max(20), pixel_width: 0, pixel_height: 0 })
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn cancel(&self, login_id: &str) {
        if let Ok(mut map) = self.sessions.lock()
            && let Some(mut s) = map.remove(login_id) {
                let _ = s.killer.kill();
            }
    }

    pub fn cancel_all(&self) {
        if let Ok(mut map) = self.sessions.lock() {
            for (_, mut s) in map.drain() {
                let _ = s.killer.kill();
            }
        }
    }
}

pub fn home_dir() -> std::path::PathBuf {
    std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(Into::into)
        .unwrap_or_else(std::env::temp_dir)
}

/// Finds https links in terminal output (with colors/cursor codes removed).
#[derive(Default)]
pub struct UrlFinder {
    text: String,
    seen: Vec<String>,
}

impl UrlFinder {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<String> {
        static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| Regex::new(r"https://[A-Za-z0-9\-._~:/?#\[\]@!$&'()*+,;=%]+").unwrap());
        let plain = strip_ansi_escapes::strip(bytes);
        self.text.push_str(&String::from_utf8_lossy(&plain));
        // keep the tail only
        if self.text.len() > 32_768 {
            let cut = self.text.len() - 16_384;
            let cut = (cut..self.text.len()).find(|i| self.text.is_char_boundary(*i)).unwrap_or(0);
            self.text = self.text[cut..].to_string();
        }
        let mut found = vec![];
        for m in re.find_iter(&self.text) {
            // a link at the very end may still be arriving
            if m.end() == self.text.len() {
                continue;
            }
            let url = m.as_str().trim_end_matches(['.', ',', ')', '\'', ';']).to_string();
            if url.len() < 16 {
                continue;
            }
            // a longer version of a link we already reported replaces it
            if self.seen.iter().any(|s| s == &url || s.starts_with(&url)) {
                continue;
            }
            self.seen.push(url.clone());
            found.push(url);
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_links_through_colors_and_chunks() {
        let mut f = UrlFinder::default();
        assert!(f.feed(b"Browser didn't open? Use: \x1b[1mhttps://claude.ai/oauth/auth").is_empty());
        let got = f.feed(b"orize?code=true&client_id=abc&state=xyz\x1b[0m\r\nPaste code here: ");
        assert_eq!(got, vec!["https://claude.ai/oauth/authorize?code=true&client_id=abc&state=xyz".to_string()]);
        // the same link again (screen redraw) is not repeated
        assert!(f.feed(b"\r\nhttps://claude.ai/oauth/authorize?code=true&client_id=abc&state=xyz\r\n").is_empty());
        let got = f.feed(b"1. Open https://auth.openai.com/codex/device.\n2. Enter ABCD-EFGH\n");
        assert_eq!(got, vec!["https://auth.openai.com/codex/device".to_string()]);
    }

    #[test]
    fn real_claude_auth_login_output() {
        // Captured from `claude auth login` (Claude Code 2.1.330) in a PTY.
        let raw = b"Opening browser to sign in\xe2\x80\xa6\r\nIf the browser didn't open, visit:\r\n\x1b]8;;https://claude.ai/oauth/authorize?z-ry8aC6cUbUg9SG8wy27vQ7mgXVfa09C6BE6jqp8w8WM\x07\x1b[94mhttps://claude.ai/oauth/authorize?z-ry8aC6cUbUg9SG8wy27vQ7mgXVfa09C6BE6jqp8w8WM\x1b[39m\x1b]8;;\x07\r\nPaste code here if prompted > ";
        let got = UrlFinder::default().feed(raw);
        assert_eq!(got, vec!["https://claude.ai/oauth/authorize?z-ry8aC6cUbUg9SG8wy27vQ7mgXVfa09C6BE6jqp8w8WM".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn pty_runs_and_reports_link() {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let tx = Mutex::new(tx);
        let ch: Channel<LoginEvent> = Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(s) = body {
                let _ = tx.lock().unwrap().send(s);
            }
            Ok(())
        });
        // A fake CLI that prints a sign-in link and waits for a code.
        let dir = std::env::temp_dir().join(format!("wren-login-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join("codex");
        std::fs::write(&fake, "#!/bin/sh\necho \"Open https://auth.openai.com/codex/device now\"\nread code\necho got:$code\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
        let m = LoginManager::default();
        m.start_with_bin("l1".into(), fake.clone(), Provider::Codex, vec!["login"], vec![], 80, 24, ch)
            .unwrap();
        let mut saw_url = false;
        let mut saw_code = false;
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(10) {
            if let Ok(s) = rx.recv_timeout(std::time::Duration::from_millis(200)) {
                if s.contains("\"kind\":\"url\"") && s.contains("auth.openai.com/codex/device") {
                    saw_url = true;
                    m.input("l1", "1234\r").unwrap();
                }
                if s.contains("\"kind\":\"output\"") {
                    let data: serde_json::Value = serde_json::from_str(&s).unwrap();
                    let bytes = base64::engine::general_purpose::STANDARD.decode(data["data"].as_str().unwrap()).unwrap();
                    if String::from_utf8_lossy(&bytes).contains("got:1234") {
                        saw_code = true;
                    }
                }
                if s.contains("\"kind\":\"exited\"") && saw_code {
                    break;
                }
            }
        }
        m.cancel_all();
        std::fs::remove_dir_all(&dir).ok();
        assert!(saw_url, "link found");
        assert!(saw_code, "typed code reached the CLI");
    }
}
