//! Up to 4 conversation panes, each backed by one official CLI process.

use std::collections::HashMap;
use std::ffi::OsString;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};

use crate::events::UiEvent;
use crate::attachments::Outgoing;
use crate::link::{new_link, reply, Decision, Link};
use crate::providers::{codex_thread_request, Mode, Parser, Permission, Provider};

pub const MAX_PANES: usize = 4;
const MAX_STDERR_LINES: usize = 200;

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PaneConfig {
    pub provider: Provider,
    /// The project folder the CLI works in.
    pub cwd: String,
    pub permission: Permission,
    /// Which account's login to use (None = the CLI's default login).
    #[serde(default)]
    pub account_id: Option<String>,
    /// Model id for the CLI (None/empty = the CLI's default).
    #[serde(default)]
    pub model: Option<String>,
    /// Effort tier (None/empty = the model's default).
    #[serde(default)]
    pub effort: Option<String>,
    /// Study / Deep research (None = normal).
    #[serde(default)]
    pub mode: Option<Mode>,
}

struct Running {
    pid: Option<u32>,
    stdin_tx: Option<mpsc::UnboundedSender<String>>,
    kill_tx: Option<oneshot::Sender<()>>,
    alive: Arc<AtomicBool>,
}

struct Session {
    cfg: PaneConfig,
    /// Account env (CLAUDE_CONFIG_DIR / CODEX_HOME), or why this account
    /// can't be used right now.
    env: Result<Vec<(String, String)>, String>,
    channel: Channel<UiEvent>,
    run: Option<Running>,
    /// The CLI's own session/conversation id, for resuming.
    provider_session: Arc<Mutex<Option<String>>>,
    /// Bumped on every restart so events from an old process are ignored.
    generation: Arc<AtomicU64>,
    /// Shared with the running process's parser: approvals waiting for the
    /// user, messages waiting for the CLI to be ready. New for each process.
    link: Link,
    /// Wren's attachments folder (Claude Code may read it).
    attach_dir: Option<String>,
}

impl Session {
    fn is_running(&self) -> bool {
        self.run.as_ref().map(|r| r.alive.load(Ordering::SeqCst)).unwrap_or(false)
    }

    fn kill(&mut self) {
        if let Some(mut r) = self.run.take() {
            kill_tree(r.pid);
            if let Some(tx) = r.kill_tx.take() {
                let _ = tx.send(());
            }
        }
    }
}

#[derive(Default)]
pub struct Manager {
    sessions: Mutex<HashMap<String, Session>>,
    /// Wren's attachments folder: Claude Code gets read access to it (`--add-dir`).
    attach_dir: Mutex<Option<String>>,
}

impl Manager {
    pub fn set_attach_dir(&self, dir: String) {
        if let Ok(mut d) = self.attach_dir.lock() {
            *d = Some(dir);
        }
    }

    pub fn open(
        &self,
        pane_id: String,
        cfg: PaneConfig,
        env: Result<Vec<(String, String)>, String>,
        channel: Channel<UiEvent>,
    ) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(old) = map.get_mut(&pane_id) {
            old.kill();
        } else if map.len() >= MAX_PANES {
            return Err(format!("Wren supports at most {MAX_PANES} conversations at once."));
        }
        map.insert(
            pane_id,
            Session {
                cfg,
                env,
                channel,
                run: None,
                provider_session: Arc::new(Mutex::new(None)),
                generation: Arc::new(AtomicU64::new(0)),
                link: new_link(true),
                attach_dir: self.attach_dir.lock().ok().and_then(|d| d.clone()),
            },
        );
        Ok(())
    }

    /// Change provider, folder or permission. This starts a fresh conversation.
    pub fn update(
        &self,
        pane_id: &str,
        cfg: PaneConfig,
        env: Result<Vec<(String, String)>, String>,
    ) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        s.kill();
        s.generation.fetch_add(1, Ordering::SeqCst);
        *s.provider_session.lock().map_err(|e| e.to_string())? = None;
        s.cfg = cfg;
        s.env = env;
        Ok(())
    }

    pub fn send(&self, pane_id: &str, text: String, files: Vec<Outgoing>) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        if s.cfg.cwd.is_empty() {
            return Err("Choose a project folder for this pane first.".into());
        }
        if let Err(e) = &s.env {
            return Err(e.clone());
        }
        let provider = s.cfg.provider;
        let text = match (provider, s.cfg.mode) {
            (Provider::Antigravity, Some(m)) => m.wrap_message(&text),
            _ => text,
        };
        if !s.is_running() {
            spawn(s)?;
        }
        let tx = s.run.as_ref().and_then(|r| r.stdin_tx.as_ref()).ok_or("CLI is not accepting input")?;
        let mut link = s.link.lock().map_err(|e| e.to_string())?;
        if !link.ready {
            // Codex is still starting its thread; the parser sends it when ready.
            link.queued.push((text, files));
            return Ok(());
        }
        link.next_id += 1;
        let thread = s.provider_session.lock().map_err(|e| e.to_string())?.clone();
        let msg = provider.stdin_message(&text, &files, thread.as_deref(), link.next_id);
        tx.send(msg).map_err(|_| "CLI input is closed".to_string())?;
        Ok(())
    }

    /// The user answered an approval card.
    pub fn answer(&self, pane_id: &str, id: &str, decision: Decision) -> Result<(), String> {
        let map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get(pane_id).ok_or("unknown pane")?;
        let pending = s.link.lock().map_err(|e| e.to_string())?.pending.remove(id);
        let pending = pending.ok_or("This request is no longer waiting (the task was stopped or restarted).")?;
        let tx = s
            .run
            .as_ref()
            .filter(|_| s.is_running())
            .and_then(|r| r.stdin_tx.as_ref())
            .ok_or("This request is no longer waiting (the task was stopped).")?;
        tx.send(reply(&pending, decision)).map_err(|_| "CLI input is closed".to_string())
    }

    /// Change the permission WITHOUT losing the conversation (used by
    /// Antigravity's "Allow & retry" and the permission menu).
    pub fn set_permission(&self, pane_id: &str, permission: Permission) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        s.kill();
        s.cfg.permission = permission;
        Ok(())
    }

    /// Change model/effort WITHOUT losing the conversation: the CLI is
    /// restarted on the next message and resumes the same session.
    pub fn set_model(&self, pane_id: &str, model: Option<String>, effort: Option<String>) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        s.kill();
        s.cfg.model = model;
        s.cfg.effort = effort;
        Ok(())
    }

    /// Turn Study / Deep research on or off WITHOUT losing the conversation
    /// (the CLI restarts with the new flags and resumes the same session).
    pub fn set_mode(&self, pane_id: &str, mode: Option<Mode>) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        s.kill();
        s.cfg.mode = mode;
        Ok(())
    }

    /// Open a saved conversation in this pane and continue it.
    pub fn resume(
        &self,
        pane_id: &str,
        cfg: PaneConfig,
        env: Result<Vec<(String, String)>, String>,
        session_id: Option<String>,
    ) -> Result<(), String> {
        self.update(pane_id, cfg, env)?;
        let map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get(pane_id).ok_or("unknown pane")?;
        *s.provider_session.lock().map_err(|e| e.to_string())? = session_id.filter(|x| !x.is_empty());
        Ok(())
    }

    /// `\new`: forget the conversation and start fresh (same CLI, folder, permission).
    pub fn new_conversation(&self, pane_id: &str) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        s.kill();
        s.generation.fetch_add(1, Ordering::SeqCst);
        *s.provider_session.lock().map_err(|e| e.to_string())? = None;
        Ok(())
    }

    /// Stop the current work. The conversation can continue: the next
    /// message resumes it with the CLI's own session id.
    pub fn stop(&self, pane_id: &str) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        let s = map.get_mut(pane_id).ok_or("unknown pane")?;
        s.kill();
        Ok(())
    }

    pub fn close(&self, pane_id: &str) -> Result<(), String> {
        let mut map = self.sessions.lock().map_err(|e| e.to_string())?;
        if let Some(mut s) = map.remove(pane_id) {
            s.generation.fetch_add(1, Ordering::SeqCst);
            s.kill();
        }
        Ok(())
    }

    /// Called when the app quits.
    pub fn kill_all(&self) {
        if let Ok(mut map) = self.sessions.lock() {
            for s in map.values_mut() {
                s.kill();
            }
            map.clear();
        }
    }
}

fn spawn(s: &mut Session) -> Result<(), String> {
    let provider = s.cfg.provider;
    let resume = s.provider_session.lock().map_err(|e| e.to_string())?.clone();
    let path_var = search_path();
    let bin = which::which_in(provider.binary(), Some(&path_var), &s.cfg.cwd).map_err(|_| {
        format!(
            "Could not find the `{}` CLI. Install it and sign in once in a terminal, then try again.",
            provider.binary()
        )
    })?;

    let mut cmd = Command::new(bin);
    cmd.args(provider.args(
        s.cfg.permission,
        resume.as_deref(),
        s.cfg.model.as_deref(),
        s.cfg.effort.as_deref(),
        s.cfg.mode,
    ));
    if provider == Provider::Claude
        && let Some(d) = &s.attach_dir
    {
        // Attached files are read from Wren's private folder.
        cmd.arg("--add-dir").arg(d);
    }
    cmd
        .current_dir(&s.cfg.cwd)
        .env("PATH", &path_var)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in provider.env() {
        cmd.env(k, v);
    }
    for (k, v) in s.env.as_deref().unwrap_or_default() {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    cmd.process_group(0); // so Stop also ends the commands the agent started
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = cmd.spawn().map_err(|e| format!("Could not start {}: {e}", provider.binary()))?;
    let pid = child.id();
    let alive = Arc::new(AtomicBool::new(true));
    let my_gen = s.generation.load(Ordering::SeqCst);
    let generation = s.generation.clone();
    let channel = s.channel.clone();

    // stdin writer
    let stdin_tx = child.stdin.take().map(|mut stdin| {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        tauri::async_runtime::spawn(async move {
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() || stdin.flush().await.is_err() {
                    break;
                }
            }
        });
        tx
    });

    // stdout reader -> parser -> UI (and answers back to the CLI)
    let stdout = child.stdout.take().ok_or("no stdout")?;
    // A fresh link per process: old approval requests can't be answered any more.
    s.link = new_link(!provider.needs_handshake());
    let fallback = (provider == Provider::Codex && resume.is_some())
        .then(|| format!("{}\n", codex_thread_request("thread/start", s.cfg.permission, &s.cfg.cwd, None)));
    let mut parser = Parser::new(provider, s.provider_session.clone(), s.link.clone(), fallback);
    if let Some(tx) = &stdin_tx {
        for m in provider.startup_messages(s.cfg.permission, &s.cfg.cwd, resume.as_deref()) {
            let _ = tx.send(m);
        }
    }
    let (ch, g) = (channel.clone(), generation.clone());
    let reply_tx = stdin_tx.clone();
    let reader = tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if g.load(Ordering::SeqCst) != my_gen {
                break;
            }
            for ev in parser.parse(&line) {
                let _ = ch.send(ev);
            }
            if let Some(tx) = &reply_tx {
                for r in parser.take_replies() {
                    let _ = tx.send(r);
                }
            }
        }
    });

    // stderr reader -> log lines
    if let Some(stderr) = child.stderr.take() {
        let (ch, g) = (channel.clone(), generation.clone());
        tauri::async_runtime::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut n = 0;
            while let Ok(Some(line)) = lines.next_line().await {
                if g.load(Ordering::SeqCst) != my_gen || n >= MAX_STDERR_LINES {
                    continue;
                }
                n += 1;
                let _ = ch.send(UiEvent::Log { line });
            }
        });
    }

    // exit watcher
    let (kill_tx, kill_rx) = oneshot::channel::<()>();
    let alive2 = alive.clone();
    tauri::async_runtime::spawn(async move {
        let status = tokio::select! {
            st = child.wait() => st.ok(),
            _ = kill_rx => {
                let _ = child.kill().await;
                child.wait().await.ok()
            }
        };
        let _ = reader.await;
        alive2.store(false, Ordering::SeqCst);
        if generation.load(Ordering::SeqCst) == my_gen {
            let _ = channel.send(UiEvent::Exited { code: status.and_then(|s| s.code()) });
        }
    });

    s.run = Some(Running { pid, stdin_tx, kill_tx: Some(kill_tx), alive });
    Ok(())
}

/// End the CLI and everything it started.
fn kill_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(unix)]
    unsafe {
        libc::killpg(pid as libc::pid_t, libc::SIGTERM);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .creation_flags(0x0800_0000)
            .status();
    }
}

/// Apps opened from the Dock/Finder don't get the PATH from your shell
/// profile, so CLIs installed with npm, Homebrew or their own installers
/// would not be found. Build a PATH from the login shell plus common
/// install locations. Computed once.
pub fn search_path() -> OsString {
    // The login-shell part is slow to compute, so it is cached; the app's own
    // PATH is read each time.
    static EXTRA: OnceLock<Vec<std::path::PathBuf>> = OnceLock::new();
    let extra = EXTRA.get_or_init(|| {
        let mut dirs: Vec<std::path::PathBuf> = Vec::new();
        #[cfg(unix)]
        {
            if let Some(p) = login_shell_path() {
                dirs.extend(std::env::split_paths(&p));
            }
            if let Some(home) = std::env::var_os("HOME") {
                let home = std::path::PathBuf::from(home);
                for sub in [".local/bin", ".claude/local", ".npm-global/bin", ".bun/bin", ".volta/bin", ".cargo/bin"] {
                    dirs.push(home.join(sub));
                }
            }
            for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"] {
                dirs.push(d.into());
            }
        }
        #[cfg(windows)]
        {
            if let Some(appdata) = std::env::var_os("APPDATA") {
                dirs.push(std::path::PathBuf::from(appdata).join("npm"));
            }
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(std::path::PathBuf::from(local).join("agy").join("bin"));
            }
            if let Some(profile) = std::env::var_os("USERPROFILE") {
                dirs.push(std::path::PathBuf::from(profile).join(".local").join("bin"));
            }
        }
        dirs
    });
    let mut dirs: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    dirs.extend(extra.iter().cloned());
    let mut seen = std::collections::HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    std::env::join_paths(dirs).unwrap_or_default()
}

#[cfg(unix)]
fn login_shell_path() -> Option<String> {
    use std::time::Duration;
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let out = std::process::Command::new(shell)
            .args(["-ilc", "printf '__WREN__%s__WREN__' \"$PATH\""])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let _ = tx.send(out);
    });
    let out = rx.recv_timeout(Duration::from_secs(4)).ok()?.ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let start = s.find("__WREN__")? + "__WREN__".len();
    let end = s[start..].find("__WREN__")? + start;
    Some(s[start..end].to_string())
}

/// Which CLIs are installed (provider -> full path).
pub fn detect_clis() -> HashMap<Provider, Option<String>> {
    let path = search_path();
    let cwd = std::env::temp_dir();
    [Provider::Claude, Provider::Codex, Provider::Antigravity]
        .into_iter()
        .map(|p| {
            let found = which::which_in(p.binary(), Some(&path), &cwd)
                .ok()
                .map(|b| b.to_string_lossy().to_string());
            (p, found)
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};
    use tauri::ipc::InvokeResponseBody;

    fn script(dir: &std::path::Path, name: &str, body: &str) {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/bash\n{body}")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn wait_for(events: &Arc<Mutex<Vec<String>>>, needle: &str, n: usize) {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            if events.lock().unwrap().iter().filter(|e| e.contains(needle)).count() >= n {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {needle}: {:?}", events.lock().unwrap());
    }

    /// Collect a pane's events (test helper).
    fn collector() -> (Arc<Mutex<Vec<String>>>, Channel<UiEvent>) {
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev2 = events.clone();
        let channel: Channel<UiEvent> = Channel::new(move |body| {
            if let InvokeResponseBody::Json(s) = body {
                ev2.lock().unwrap().push(s);
            }
            Ok(())
        });
        (events, channel)
    }

    #[test]
    /// The real Codex app-server against a fake OpenAI API whose model runs
    /// `touch` (WREN_FAKE_CODEX_HOME = a CODEX_HOME whose config.toml points
    /// at the fake server). Manual: Deny keeps the file away, Allow creates it.
    /// Auto: no card, the file is created.
    fn real_codex_approvals() {
        let Ok(home) = std::env::var("WREN_FAKE_CODEX_HOME") else { return };
        let dir = std::path::PathBuf::from("/tmp/proj");
        let file = dir.join("cx.txt");
        let env = vec![("CODEX_HOME".to_string(), home), ("FAKE_KEY".to_string(), "x".to_string())];
        let cfg = |perm| PaneConfig {
            provider: Provider::Codex,
            cwd: dir.to_string_lossy().to_string(),
            permission: perm,
            account_id: None,
            model: None,
            effort: None,
            mode: None,
        };
        let card_id = |events: &Arc<Mutex<Vec<String>>>, n: usize| {
            let e = events.lock().unwrap();
            let card = e.iter().filter(|s| s.contains("\"kind\":\"approval\"")).nth(n).unwrap().clone();
            assert!(card.contains("touch /tmp/proj/cx.txt"), "{card}");
            serde_json::from_str::<serde_json::Value>(&card).unwrap()["id"].as_str().unwrap().to_string()
        };
        let m = Manager::default();
        tauri::async_runtime::block_on(async {
            // Manual + Deny
            let _ = std::fs::remove_file(&file);
            let (events, ch) = collector();
            m.open("c1".into(), cfg(Permission::Manual), Ok(env.clone()), ch).unwrap();
            m.send("c1", "make a file".into(), vec![]).unwrap();
            wait_for(&events, "\"kind\":\"approval\"", 1);
            m.answer("c1", &card_id(&events, 0), Decision::Deny).unwrap();
            wait_for(&events, "turn_done", 1);
            assert!(!file.exists(), "denied: nothing ran");
            assert!(events.lock().unwrap().iter().any(|s| s.contains("\"kind\":\"tool_result\"") && s.contains("\"is_error\":true")));

            // Manual + Allow (same conversation, second turn)
            m.send("c1", "again".into(), vec![]).unwrap();
            wait_for(&events, "\"kind\":\"approval\"", 2);
            m.answer("c1", &card_id(&events, 1), Decision::Allow).unwrap();
            wait_for(&events, "turn_done", 2);
            assert!(file.exists(), "allowed: the command ran");
            {
                let e = events.lock().unwrap();
                let done: Vec<&String> = e.iter().filter(|s| s.contains("turn_done")).collect();
                assert!(done[1].contains("\"input_tokens\":160"), "per-turn usage: {}", done[1]);
            }

            // Auto: no card
            let _ = std::fs::remove_file(&file);
            let (events, ch) = collector();
            m.open("c2".into(), cfg(Permission::Auto), Ok(env.clone()), ch).unwrap();
            m.send("c2", "make a file".into(), vec![]).unwrap();
            wait_for(&events, "turn_done", 1);
            assert!(!events.lock().unwrap().iter().any(|s| s.contains("\"kind\":\"approval\"")));
            assert!(file.exists(), "auto: ran without asking");
        });
        m.kill_all();
    }

    #[test]
    /// Manual mode with the real Claude Code against a fake API that runs
    /// `touch` twice (WREN_FAKE_API2=<url>): the first request becomes a card,
    /// "Allow for this chat" lets the second run without asking.
    fn real_claude_manual_approval() {
        let Ok(api) = std::env::var("WREN_FAKE_API2") else { return };
        let dir = std::path::PathBuf::from("/tmp/proj");
        let _ = std::fs::remove_file(dir.join("a.txt"));
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev2 = events.clone();
        let channel: Channel<UiEvent> = Channel::new(move |body| {
            if let InvokeResponseBody::Json(s) = body {
                ev2.lock().unwrap().push(s);
            }
            Ok(())
        });
        let m = Manager::default();
        let cfg = PaneConfig {
            provider: Provider::Claude,
            cwd: dir.to_string_lossy().to_string(),
            permission: Permission::Manual,
            account_id: None,
            model: None,
            effort: None,
            mode: None,
        };
        let cfgdir = std::env::temp_dir().join(format!("wren-manual-{}", std::process::id()));
        let env = vec![
            ("ANTHROPIC_BASE_URL".to_string(), api),
            ("ANTHROPIC_API_KEY".to_string(), "sk-test".to_string()),
            ("CLAUDE_CONFIG_DIR".to_string(), cfgdir.to_string_lossy().to_string()),
        ];
        tauri::async_runtime::block_on(async {
            m.open("a1".into(), cfg, Ok(env), channel).unwrap();
            m.send("a1", "make a file".into(), vec![]).unwrap();
            wait_for(&events, "\"kind\":\"approval\"", 1);
            let id = {
                let e = events.lock().unwrap();
                let card = e.iter().find(|s| s.contains("\"kind\":\"approval\"")).unwrap().clone();
                assert!(card.contains("touch /tmp/proj/a.txt"), "{card}");
                serde_json::from_str::<serde_json::Value>(&card).unwrap()["id"].as_str().unwrap().to_string()
            };
            m.answer("a1", &id, Decision::AllowSession).unwrap();
            wait_for(&events, "turn_done", 1);
        });
        let e = events.lock().unwrap();
        assert_eq!(e.iter().filter(|s| s.contains("\"kind\":\"approval\"")).count(), 1, "asked once: {e:?}");
        assert!(dir.join("a.txt").exists(), "the allowed command ran");
        m.kill_all();
    }

    #[test]
    /// The real Claude Code against a fake API (WREN_FAKE_API=<url>): a "hi" that
    /// makes Claude run a command (denied on its card) must still end its
    /// turn.
    fn real_claude_tool_turn_ends() {
        let Ok(api) = std::env::var("WREN_FAKE_API") else { return };
        let dir = std::env::temp_dir().join(format!("wren-real-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev2 = events.clone();
        let channel: Channel<UiEvent> = Channel::new(move |body| {
            if let InvokeResponseBody::Json(s) = body {
                ev2.lock().unwrap().push(s);
            }
            Ok(())
        });
        let m = Manager::default();
        let cfg = PaneConfig {
            provider: Provider::Claude,
            cwd: dir.to_string_lossy().to_string(),
            permission: Permission::Manual,
            account_id: None,
            model: Some("opus".into()),
            effort: None,
            mode: None,
        };
        let env = vec![
            ("ANTHROPIC_BASE_URL".to_string(), api),
            ("ANTHROPIC_API_KEY".to_string(), "sk-test".to_string()),
            ("CLAUDE_CONFIG_DIR".to_string(), dir.join("cfg").to_string_lossy().to_string()),
        ];
        tauri::async_runtime::block_on(async {
            m.open("r1".into(), cfg, Ok(env), channel).unwrap();
            m.send("r1", "hi".into(), vec![]).unwrap();
            // Manual: a command that needs approval becomes a card; Deny still ends the turn.
            wait_for(&events, "\"kind\":\"approval\"", 1);
            let card = events.lock().unwrap().iter().find(|s| s.contains("\"kind\":\"approval\"")).unwrap().clone();
            let id = serde_json::from_str::<serde_json::Value>(&card).unwrap()["id"].as_str().unwrap().to_string();
            m.answer("r1", &id, Decision::Deny).unwrap();
            wait_for(&events, "turn_done", 1);
        });
        let e = events.lock().unwrap();
        let kinds: Vec<&str> = e.iter().filter_map(|s| s.split("\"kind\":\"").nth(1)).map(|r| r.split('"').next().unwrap()).collect();
        assert!(kinds.contains(&"tool_use") && kinds.contains(&"tool_result"), "{kinds:?}");
        assert!(e.iter().any(|s| s.contains("Hi! Done.")), "{e:?}");
        m.kill_all();
    }

    #[test]
    fn fake_clis_end_to_end() {
        let dir = std::env::temp_dir().join(format!("wren-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Fake Claude Code: logs its args + privacy env, answers each stdin line.
        script(&dir, "claude", r#"
echo "ARGS $* PRIV=$CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC CFG=$CLAUDE_CONFIG_DIR" >&2
echo '{"type":"system","subtype":"init","session_id":"s1","model":"fake-model"}'
while read -r line; do
  echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"pong"}}}'
  echo '{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}'
  echo '{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.01,"usage":{"input_tokens":3,"output_tokens":1}}'
done
"#);
        // Fake Codex app-server: answers the thread request, asks to approve a
        // command on every turn, and finishes the turn once it gets an answer.
        script(&dir, "codex", r#"
echo "ARGS $*" >&2
while read -r line; do
  case "$line" in
    *'"thread/start"'*) echo '{"id":"thread","result":{"thread":{"id":"t9"},"model":"fake"}}' ;;
    *'"thread/resume"'*) echo "RESUME $line" >&2; echo '{"id":"thread","result":{"thread":{"id":"t9"},"model":"fake"}}' ;;
    *'"turn/start"'*)
      echo "TURN $line" >&2
      echo '{"method":"item/started","params":{"item":{"type":"commandExecution","id":"c1","command":"touch x","status":"inProgress"}}}'
      echo '{"id":41,"method":"item/commandExecution/requestApproval","params":{"itemId":"c1","reason":"write a file"}}' ;;
    *'"decision"'*)
      echo "DECISION $line" >&2
      echo '{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"c1","status":"completed","aggregatedOutput":"","exitCode":0}}}'
      echo '{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"m1","text":"ok"}}}'
      echo '{"method":"thread/tokenUsage/updated","params":{"tokenUsage":{"total":{"inputTokens":5,"cachedInputTokens":0,"outputTokens":2}}}}'
      echo '{"method":"turn/completed","params":{"turn":{"id":"u","status":"completed","error":null}}}' ;;
  esac
done
"#);
        let old = std::env::var("PATH").unwrap_or_default();
        // SAFETY: single test, set before any other thread reads PATH.
        unsafe { std::env::set_var("PATH", format!("{}:{old}", dir.display())) };

        let events = Arc::new(Mutex::new(Vec::<String>::new()));
        let ev2 = events.clone();
        let channel: Channel<UiEvent> = Channel::new(move |body| {
            if let InvokeResponseBody::Json(s) = body {
                ev2.lock().unwrap().push(s);
            }
            Ok(())
        });
        let m = Manager::default();
        let cwd = dir.to_string_lossy().to_string();
        let cfg = |p| PaneConfig {
            provider: p,
            cwd: cwd.clone(),
            permission: Permission::Manual,
            account_id: None,
            model: None,
            effort: None,
            mode: None,
        };

        tauri::async_runtime::block_on(async {
            // Claude: long-lived, multi-turn over stdin
            m.open("p1".into(), cfg(Provider::Claude), Ok(vec![("CLAUDE_CONFIG_DIR".into(), "/tmp/acct-1".into())]), channel.clone()).unwrap();
            m.send("p1", "ping".into(), vec![]).unwrap();
            wait_for(&events, "turn_done", 1);
            m.send("p1", "again".into(), vec![]).unwrap();
            wait_for(&events, "turn_done", 2);
            {
                let e = events.lock().unwrap();
                assert!(e.iter().any(|s| s.contains("--permission-mode default") && s.contains("PRIV=1") && s.contains("CFG=/tmp/acct-1")), "{e:?}");
                assert_eq!(e.iter().filter(|s| s.contains("ARGS")).count(), 1, "one process for two turns");
            }
            // Stop, then the next message resumes the CLI's session.
            m.stop("p1").unwrap();
            wait_for(&events, "exited", 1);
            m.send("p1", "after stop".into(), vec![]).unwrap();
            wait_for(&events, "turn_done", 3);
            assert!(events.lock().unwrap().iter().any(|s| s.contains("--resume s1")));
            // Changing the model keeps the conversation (resumes with the new model).
            m.set_model("p1", Some("opus".into()), Some("high".into())).unwrap();
            wait_for(&events, "exited", 2);
            m.send("p1", "with opus".into(), vec![]).unwrap();
            wait_for(&events, "turn_done", 4);
            assert!(events.lock().unwrap().iter().any(|s| s.contains("--resume s1 --model opus --effort high")));
            // Opening a saved conversation resumes its session id.
            m.resume("p1", cfg(Provider::Claude), Ok(vec![]), Some("saved-9".into())).unwrap();
            m.send("p1", "continue".into(), vec![]).unwrap();
            wait_for(&events, "turn_done", 5);
            assert!(events.lock().unwrap().iter().any(|s| s.contains("--resume saved-9")));

            // A 5th pane is refused.
            for i in 2..=4 {
                m.open(format!("p{i}"), cfg(Provider::Claude), Ok(vec![]), channel.clone()).unwrap();
            }
            assert!(m.open("p5".into(), cfg(Provider::Claude), Ok(vec![]), channel.clone()).is_err());
            // An account that can't be used (e.g. inactive Antigravity) refuses to send.
            m.update("p3", cfg(Provider::Claude), Err("not signed in".into())).unwrap();
            assert_eq!(m.send("p3", "x".into(), vec![]), Err("not signed in".to_string()));

            // Codex app-server: the first message waits for the thread, then the
            // approval card is answered and the turn finishes.
            events.lock().unwrap().clear();
            m.update("p2", cfg(Provider::Codex), Ok(vec![])).unwrap();
            m.send("p2", "hi".into(), vec![]).unwrap();
            wait_for(&events, "\"kind\":\"approval\"", 1);
            assert!(m.answer("p2", "codex-nope", Decision::Allow).is_err(), "unknown request");
            m.answer("p2", "codex-41", Decision::AllowSession).unwrap();
            wait_for(&events, "turn_done", 1);
            assert!(m.answer("p2", "codex-41", Decision::Allow).is_err(), "answered only once");
            {
                let e = events.lock().unwrap();
                assert!(e.iter().any(|s| s.contains("ARGS app-server")), "{e:?}");
                assert!(e.iter().any(|s| s.contains("TURN") && s.contains("\\\"threadId\\\":\\\"t9\\\"") && s.contains("hi")), "{e:?}");
                assert!(e.iter().any(|s| s.contains("DECISION") && s.contains("acceptForSession")), "{e:?}");
            }
            // After a stop, the next message resumes the same Codex thread.
            m.stop("p2").unwrap();
            wait_for(&events, "exited", 1);
            m.send("p2", "next".into(), vec![]).unwrap();
            wait_for(&events, "\"kind\":\"approval\"", 2);
            assert!(events.lock().unwrap().iter().any(|s| s.contains("RESUME") && s.contains("t9")));
            m.kill_all();
        });
        std::fs::remove_dir_all(&dir).ok();
    }
}
