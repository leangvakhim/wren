//! Phone link: paired phones control Wren through a relay (relay/src/core.ts).
//!
//! This side keeps one WebSocket to the relay, does the end-to-end handshake
//! with each phone (crypto.rs), asks the person on this computer before a new
//! phone is paired, stores phone uploads as attachments, sends Web Push
//! (push.rs), and hands every other phone request to the UI, which runs it
//! through the same pane code as typing on this computer and answers with
//! `send`. The relay sees only public keys, random nonces and sealed boxes.

pub mod awake;
pub mod crypto;
pub mod push;
pub mod store;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::attachments;
use crypto::{b64, random, unb64};
use store::{Device, Saved};

/// How long a QR code can be used.
const PAIR_MS: u64 = 10 * 60 * 1000;
/// A phone upload (sent in chunks) can't be bigger than this.
const MAX_UPLOAD: usize = 25 * 1024 * 1024;
/// Largest message to a phone (the relay allows 1 MiB per frame after sealing).
const MAX_SEND: usize = 700 * 1024;
/// One push per phone + pane + kind in this time.
const PUSH_QUIET_MS: u64 = 15_000;

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// What the UI hears about the phone link.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteEvent {
    Status { view: RemoteView },
    /// A paired phone finished the handshake.
    Connected { conn: u64, device: String, name: String },
    Disconnected { conn: u64 },
    /// Something a phone asks Wren to do (answer with `send(conn, {t:"res", id, ...})`).
    Request { conn: u64, device: String, msg: Value },
    /// A phone used the QR code: ask the person on this computer.
    PairRequest { conn: u64, name: String },
}

#[derive(Serialize, Clone, Debug)]
pub struct DeviceView {
    pub id: String,
    pub name: String,
    pub added_at: u64,
    pub last_seen: u64,
    pub connected: bool,
    pub push: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct RemoteView {
    pub enabled: bool,
    pub relay: String,
    pub has_key: bool,
    pub keep_awake: bool,
    /// off | connecting | online | offline | error
    pub status: String,
    pub error: Option<String>,
    pub host_name: String,
    pub devices: Vec<DeviceView>,
    /// ms; a QR code is shown until then.
    pub pairing_until: Option<u64>,
}

#[derive(Serialize, Clone, Debug)]
pub struct PairCode {
    pub url: String,
    pub svg: String,
    pub expires_at: u64,
}

enum Stage {
    /// Waiting for the phone's hello.
    Hello,
    /// A new phone; the person on this computer hasn't answered yet.
    Waiting { device: Device, nonce: Vec<u8> },
    Ready { device: String, ch: Box<crypto::Channel> },
}

struct Conn {
    stage: Stage,
    /// The phone's page is on screen (no push needed for it).
    visible: bool,
}

struct Upload {
    conn: u64,
    name: String,
    size: usize,
    buf: Vec<u8>,
}

struct Inner {
    app_data: PathBuf,
    saved: Saved,
    status: &'static str,
    error: Option<String>,
    conns: HashMap<u64, Conn>,
    ui: Option<Channel<RemoteEvent>>,
    pairing: Option<(Vec<u8>, u64)>,
    out: Option<mpsc::UnboundedSender<String>>,
    task: Option<tauri::async_runtime::JoinHandle<()>>,
    uploads: HashMap<String, Upload>,
    push_quiet: HashMap<String, u64>,
    awake: Option<awake::Guard>,
    host_name: String,
    attachments: Option<Arc<attachments::Store>>,
}

impl Inner {
    fn view(&self) -> RemoteView {
        let connected: Vec<&str> = self
            .conns
            .values()
            .filter_map(|c| match &c.stage {
                Stage::Ready { device, .. } => Some(device.as_str()),
                _ => None,
            })
            .collect();
        RemoteView {
            enabled: self.saved.enabled,
            relay: self.saved.relay.clone(),
            has_key: !self.saved.access_key.is_empty(),
            keep_awake: self.saved.keep_awake,
            status: self.status.to_string(),
            error: self.error.clone(),
            host_name: self.host_name.clone(),
            devices: self
                .saved
                .devices
                .iter()
                .map(|d| DeviceView {
                    id: d.id.clone(),
                    name: d.name.clone(),
                    added_at: d.added_at,
                    last_seen: d.last_seen,
                    connected: connected.contains(&d.id.as_str()),
                    push: d.push.is_some(),
                })
                .collect(),
            pairing_until: self.pairing.as_ref().map(|(_, t)| *t).filter(|t| *t > now_ms()),
        }
    }

    fn emit(&self, ev: RemoteEvent) {
        if let Some(ui) = &self.ui {
            let _ = ui.send(ev);
        }
    }

    fn emit_status(&self) {
        self.emit(RemoteEvent::Status { view: self.view() });
    }

    fn save(&self) {
        if let Err(e) = store::save(&self.app_data, &self.saved) {
            eprintln!("wren: couldn't save the phone link settings: {e}");
        }
    }

    fn relay_send(&self, frame: Value) {
        if let Some(out) = &self.out {
            let _ = out.send(frame.to_string());
        }
    }

    /// A plain (not encrypted) handshake message to a phone.
    fn plain(&self, conn: u64, msg: Value) {
        self.relay_send(json!({ "t": "msg", "c": conn, "d": msg.to_string() }));
    }

    fn kick(&mut self, conn: u64) {
        self.relay_send(json!({ "t": "kick", "c": conn }));
        self.uploads.retain(|_, u| u.conn != conn);
        if let Some(c) = self.conns.remove(&conn)
            && !matches!(c.stage, Stage::Hello)
        {
            self.emit(RemoteEvent::Disconnected { conn });
        }
    }

    fn deny(&mut self, conn: u64, why: &str) {
        self.plain(conn, json!({ "t": "deny", "why": why }));
        self.kick(conn);
    }

    /// Seal and send an app message to one ready phone.
    fn seal(&mut self, conn: u64, msg: &Value) -> Result<(), String> {
        let text = msg.to_string();
        if text.len() > MAX_SEND {
            return Err("message too big for the phone link".into());
        }
        let Some(Conn { stage: Stage::Ready { ch, .. }, .. }) = self.conns.get_mut(&conn) else {
            return Err("that phone isn't connected".into());
        };
        let d = json!({ "t": "e", "d": ch.send.seal(text.as_bytes()) }).to_string();
        self.relay_send(json!({ "t": "msg", "c": conn, "d": d }));
        Ok(())
    }

    fn reply(&mut self, conn: u64, id: &Value, result: Result<Value, String>) {
        let msg = match result {
            Ok(data) => json!({ "t": "res", "id": id, "ok": true, "data": data }),
            Err(e) => json!({ "t": "res", "id": id, "ok": false, "error": e }),
        };
        let _ = self.seal(conn, &msg);
    }

    fn host_secret(&self) -> Result<p256::SecretKey, String> {
        crypto::secret_key(&unb64(&self.saved.host_key)?)
    }

    fn vapid_public(&self) -> String {
        unb64(&self.saved.vapid_key)
            .ok()
            .and_then(|k| crypto::secret_key(&k).ok())
            .map(|k| b64(&crypto::public_raw(&k)))
            .unwrap_or_default()
    }

    /// The phone is allowed in: finish the handshake.
    fn accept(&mut self, conn: u64, device: &Device, phone_nonce: &[u8]) {
        let (Ok(host), Ok(key)) = (self.host_secret(), unb64(&device.key).and_then(|k| crypto::public_key(&k))) else {
            return self.deny(conn, "error");
        };
        let host_nonce = random::<16>();
        let ch = crypto::Channel::host(&host, &key, phone_nonce, &host_nonce);
        self.plain(conn, json!({ "t": "welcome", "n": b64(&host_nonce) }));
        let Some(c) = self.conns.get_mut(&conn) else { return };
        c.stage = Stage::Ready { device: device.id.clone(), ch: Box::new(ch) };
        if let Some(d) = self.saved.devices.iter_mut().find(|d| d.id == device.id) {
            d.last_seen = now_ms();
        }
        self.save();
        let ready = json!({
            "t": "ready",
            "host": self.host_name,
            "version": env!("CARGO_PKG_VERSION"),
            "vapid": self.vapid_public(),
            "device": { "id": device.id, "name": device.name },
            "push_kinds": self.saved.devices.iter().find(|d| d.id == device.id).map(|d| d.push_kinds.clone()).unwrap_or_default(),
        });
        let _ = self.seal(conn, &ready);
        self.emit(RemoteEvent::Connected { conn, device: device.id.clone(), name: device.name.clone() });
        self.emit_status();
    }

    /// The phone's first message on a connection.
    fn hello(&mut self, conn: u64, m: &Value) {
        let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("");
        let id = s("dev");
        let (Ok(key), Ok(nonce)) = (unb64(s("pub")), unb64(s("n"))) else { return self.deny(conn, "bad") };
        let id_ok = (8..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if m.get("t").and_then(Value::as_str) != Some("hello") || m.get("v").and_then(Value::as_u64) != Some(1) || !id_ok || nonce.len() != 16 || crypto::public_key(&key).is_err() {
            return self.deny(conn, "bad");
        }
        let key_b64 = b64(&key);
        if let Some(d) = self.saved.devices.iter().find(|d| d.id == id).cloned() {
            if d.key != key_b64 {
                return self.deny(conn, "unknown");
            }
            return self.accept(conn, &d, &nonce);
        }
        let Some(mac) = m.get("mac").and_then(Value::as_str).and_then(|x| unb64(x).ok()) else {
            return self.deny(conn, "unknown");
        };
        let Some((secret, until)) = self.pairing.clone() else { return self.deny(conn, "expired") };
        if now_ms() > until || !crypto::pair_proof_ok(&secret, &key, &nonce, &mac) {
            return self.deny(conn, "expired");
        }
        // One QR code = one phone.
        self.pairing = None;
        let name: String = s("name").chars().filter(|c| !c.is_control()).take(60).collect::<String>().trim().to_string();
        let name = if name.is_empty() { "Phone".to_string() } else { name };
        let device = Device { id: id.to_string(), name: name.clone(), key: key_b64, added_at: now_ms(), last_seen: now_ms(), push: None, push_kinds: vec![] };
        if let Some(c) = self.conns.get_mut(&conn) {
            c.stage = Stage::Waiting { device, nonce };
        }
        self.plain(conn, json!({ "t": "wait" }));
        self.emit(RemoteEvent::PairRequest { conn, name });
        self.emit_status();
    }

    /// A sealed app message from a ready phone.
    fn app(&mut self, conn: u64, device: String, msg: Value) {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        match msg.get("op").and_then(Value::as_str) {
            Some("visible") => {
                if let Some(c) = self.conns.get_mut(&conn) {
                    c.visible = msg.get("on").and_then(Value::as_bool).unwrap_or(false);
                }
            }
            Some("push") => {
                let r = self.set_push(&device, &msg);
                self.reply(conn, &id, r.map(|_| Value::Null));
            }
            Some("upload") => {
                let r = self.upload(conn, &msg);
                self.reply(conn, &id, r);
            }
            _ => self.emit(RemoteEvent::Request { conn, device, msg }),
        }
    }

    fn set_push(&mut self, device: &str, msg: &Value) -> Result<(), String> {
        let sub = msg.get("sub").filter(|v| !v.is_null());
        let sub = match sub {
            Some(s) => {
                let get = |p: &str| s.pointer(p).and_then(Value::as_str).map(String::from).ok_or("bad push subscription");
                let sub = push::Subscription { endpoint: get("/endpoint")?, p256dh: get("/keys/p256dh")?, auth: get("/keys/auth")? };
                if !push::known_service(&sub.endpoint) {
                    return Err("That push service isn't supported.".into());
                }
                Some(sub)
            }
            None => None,
        };
        let kinds: Vec<String> = msg
            .get("kinds")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).filter(|k| ["approval", "question", "done", "problems"].contains(k)).map(String::from).collect())
            .unwrap_or_default();
        let d = self.saved.devices.iter_mut().find(|d| d.id == device).ok_or("unknown phone")?;
        d.push = sub;
        d.push_kinds = kinds;
        self.save();
        self.emit_status();
        Ok(())
    }

    /// A file from the phone, in chunks: {u, name, size, last, data(base64)}.
    fn upload(&mut self, conn: u64, msg: &Value) -> Result<Value, String> {
        let s = |k: &str| msg.get(k).and_then(Value::as_str).unwrap_or("");
        let key = format!("{conn}:{}", s("u"));
        if s("u").is_empty() || s("u").len() > 40 {
            return Err("bad upload".into());
        }
        let chunk = unb64(s("data"))?;
        if !self.uploads.contains_key(&key) {
            let size = msg.get("size").and_then(Value::as_u64).unwrap_or(0) as usize;
            if size == 0 || size > MAX_UPLOAD {
                return Err(format!("Files sent from the phone can be at most {} MB.", MAX_UPLOAD / 1024 / 1024));
            }
            if self.uploads.values().filter(|u| u.conn == conn).count() >= 3 {
                return Err("Too many uploads at once.".into());
            }
            self.uploads.insert(key.clone(), Upload { conn, name: s("name").to_string(), size, buf: Vec::with_capacity(size) });
        }
        let up = self.uploads.get_mut(&key).expect("just inserted");
        if up.buf.len() + chunk.len() > up.size {
            self.uploads.remove(&key);
            return Err("The file was bigger than announced.".into());
        }
        up.buf.extend_from_slice(&chunk);
        if !msg.get("last").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(json!({ "received": up.buf.len() }));
        }
        let up = self.uploads.remove(&key).expect("present");
        if up.buf.len() != up.size {
            return Err("The file didn't arrive completely.".into());
        }
        let store = self.attachments.clone().ok_or("attachments aren't ready")?;
        let mut a = store.save(&up.name, &up.buf)?;
        a.preview = None;
        serde_json::to_value(a).map_err(|e| e.to_string())
    }

    /// Phone frames arriving through the relay.
    fn relay_frame(&mut self, text: &str) {
        let Ok(m) = serde_json::from_str::<Value>(text) else { return };
        let c = m.get("c").and_then(Value::as_u64);
        match (m.get("t").and_then(Value::as_str), c) {
            (Some("open"), Some(c)) => {
                self.conns.insert(c, Conn { stage: Stage::Hello, visible: false });
            }
            (Some("close"), Some(c)) => {
                self.uploads.retain(|_, u| u.conn != c);
                if let Some(conn) = self.conns.remove(&c) {
                    if !matches!(conn.stage, Stage::Hello) {
                        self.emit(RemoteEvent::Disconnected { conn: c });
                    }
                    self.emit_status();
                }
            }
            (Some("msg"), Some(c)) => {
                let Some(d) = m.get("d").and_then(Value::as_str) else { return };
                self.phone_frame(c, d);
            }
            _ => {}
        }
    }

    fn phone_frame(&mut self, conn: u64, d: &str) {
        let Ok(m) = serde_json::from_str::<Value>(d) else { return self.kick(conn) };
        let Some(c) = self.conns.get_mut(&conn) else { return };
        match &mut c.stage {
            Stage::Hello => self.hello(conn, &m),
            Stage::Waiting { .. } => {}
            Stage::Ready { device, ch } => {
                let device = device.clone();
                let opened: Result<Value, String> = m
                    .get("d")
                    .and_then(Value::as_str)
                    .filter(|_| m.get("t").and_then(Value::as_str) == Some("e"))
                    .ok_or_else(|| "not sealed".to_string())
                    .and_then(|s| ch.recv.open(s))
                    .and_then(|b| serde_json::from_slice::<Value>(&b).map_err(|e| e.to_string()));
                match opened {
                    Ok(msg) => self.app(conn, device, msg),
                    // Tampered, replayed or out of order: drop the connection (the phone reconnects).
                    Err(_) => self.kick(conn),
                }
            }
        }
    }

    fn drop_connections(&mut self) {
        let ready: Vec<u64> = self.conns.iter().filter(|(_, c)| !matches!(c.stage, Stage::Hello)).map(|(k, _)| *k).collect();
        self.conns.clear();
        self.uploads.clear();
        for conn in ready {
            self.emit(RemoteEvent::Disconnected { conn });
        }
    }

    fn set_status(&mut self, status: &'static str, error: Option<String>) {
        self.status = status;
        self.error = error;
        self.emit_status();
    }

    fn update_awake(&mut self) {
        let want = self.saved.enabled && self.saved.keep_awake;
        if want && self.awake.is_none() {
            self.awake = awake::Guard::start();
        } else if !want {
            self.awake = None;
        }
    }
}

/// The phone link (managed by Tauri).
#[derive(Clone)]
pub struct Remote {
    inner: Arc<Mutex<Inner>>,
}

fn host_name() -> String {
    #[cfg(target_os = "macos")]
    if let Ok(o) = std::process::Command::new("/usr/sbin/scutil").args(["--get", "ComputerName"]).output() {
        let n = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if o.status.success() && !n.is_empty() {
            return n;
        }
    }
    #[cfg(windows)]
    if let Ok(n) = std::env::var("COMPUTERNAME") {
        return n;
    }
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        if unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) } == 0 {
            let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
            let n = String::from_utf8_lossy(&buf[..end]).trim().to_string();
            if !n.is_empty() {
                return n;
            }
        }
    }
    "This computer".into()
}

impl Remote {
    pub fn new(app_data: PathBuf, attachments: Option<Arc<attachments::Store>>) -> Self {
        let saved = store::load(&app_data);
        let inner = Inner {
            app_data,
            saved,
            status: "off",
            error: None,
            conns: HashMap::new(),
            ui: None,
            pairing: None,
            out: None,
            task: None,
            uploads: HashMap::new(),
            push_quiet: HashMap::new(),
            awake: None,
            host_name: host_name(),
            attachments,
        };
        Self { inner: Arc::new(Mutex::new(inner)) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The UI's event channel (one per window load). Starts the link if it's on.
    pub fn attach(&self, ui: Channel<RemoteEvent>) -> RemoteView {
        let start = {
            let mut i = self.lock();
            i.ui = Some(ui);
            // A reloaded page knows nothing about connected phones: they reconnect.
            let ready: Vec<u64> = i.conns.iter().filter(|(_, c)| !matches!(c.stage, Stage::Hello)).map(|(k, _)| *k).collect();
            for c in ready {
                i.kick(c);
            }
            i.saved.enabled && i.task.is_none()
        };
        if start {
            self.restart();
        }
        self.view()
    }

    pub fn view(&self) -> RemoteView {
        self.lock().view()
    }

    pub fn configure(&self, enabled: bool, relay: String, access_key: Option<String>, keep_awake: bool) -> Result<RemoteView, String> {
        {
            let mut i = self.lock();
            let relay = if relay.trim().is_empty() { String::new() } else { store::check_relay(&relay)? };
            if let Some(k) = access_key {
                let k = k.trim().to_string();
                if !k.is_empty() && k.len() < 16 {
                    return Err("That access key is too short. Copy the whole key the relay printed.".into());
                }
                i.saved.access_key = k;
            }
            if enabled && relay.is_empty() {
                return Err("Enter the relay address first.".into());
            }
            if enabled && i.saved.access_key.is_empty() {
                return Err("Enter the relay's access key first.".into());
            }
            i.saved.enabled = enabled;
            i.saved.relay = relay;
            i.saved.keep_awake = keep_awake;
            i.save();
        }
        self.restart();
        Ok(self.view())
    }

    /// (Re)connect to the relay with the current settings, or stop.
    pub fn restart(&self) {
        let mut i = self.lock();
        if let Some(t) = i.task.take() {
            t.abort();
        }
        i.out = None;
        i.drop_connections();
        i.update_awake();
        if !i.saved.enabled {
            i.pairing = None;
            i.set_status("off", None);
            return;
        }
        i.set_status("connecting", None);
        let me = self.clone();
        i.task = Some(tauri::async_runtime::spawn(async move { me.run().await }));
    }

    pub fn stop(&self) {
        let mut i = self.lock();
        if let Some(t) = i.task.take() {
            t.abort();
        }
        i.out = None;
        i.conns.clear();
        i.awake = None;
    }

    async fn run(self) {
        let mut wait = 1u64;
        loop {
            let (relay, room, key) = {
                let i = self.lock();
                (i.saved.relay.clone(), i.saved.room.clone(), i.saved.access_key.clone())
            };
            let result = self.connect_once(&relay, &room, &key).await;
            let online_before = {
                let mut i = self.lock();
                i.out = None;
                i.drop_connections();
                i.status == "online"
            };
            match result {
                Err(Fatal(e)) => {
                    self.lock().set_status("error", Some(e));
                    return;
                }
                Ok(Err(e)) => {
                    self.lock().set_status("offline", Some(e));
                    if online_before {
                        wait = 1;
                    }
                }
                Ok(Ok(())) => {
                    self.lock().set_status("offline", Some("The relay closed the connection.".into()));
                    wait = 1;
                }
            }
            tokio::time::sleep(Duration::from_secs(wait)).await;
            wait = (wait * 2).min(60);
            self.lock().set_status("connecting", None);
        }
    }

    async fn connect_once(&self, relay: &str, room: &str, key: &str) -> Result<Result<(), String>, Fatal> {
        let url = store::host_socket_url(relay);
        let ws = match tokio::time::timeout(Duration::from_secs(15), open_socket(&url)).await {
            Ok(Ok(ws)) => ws,
            Ok(Err(e)) => return Ok(Err(e)),
            Err(_) => return Ok(Err("The relay didn't answer.".into())),
        };
        let (mut tx, mut rx) = ws.split();
        let auth = json!({ "t": "auth", "room": room, "key": key }).to_string();
        if let Err(e) = tx.send(Message::text(auth)).await {
            return Ok(Err(e.to_string()));
        }
        // The relay answers {"t":"ok"} or closes with a reason.
        let first = tokio::time::timeout(Duration::from_secs(15), rx.next()).await;
        match first {
            Ok(Some(Ok(Message::Text(t)))) if serde_json::from_str::<Value>(t.as_str()).ok().and_then(|v| v.get("t").cloned()) == Some(json!("ok")) => {}
            Ok(Some(Ok(Message::Close(Some(f))))) if u16::from(f.code) == 4001 => {
                return Err(Fatal("The relay didn't accept the access key. Check it in Settings → Phone.".into()));
            }
            Ok(Some(Ok(Message::Close(Some(f))))) => return Ok(Err(format!("The relay closed the connection: {}", f.reason))),
            Ok(Some(Err(e))) => return Ok(Err(e.to_string())),
            Ok(_) => return Ok(Err("The relay closed the connection.".into())),
            Err(_) => return Ok(Err("The relay didn't answer.".into())),
        }
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
        {
            let mut i = self.lock();
            i.out = Some(out_tx);
            i.set_status("online", None);
        }
        let mut ping = tokio::time::interval(Duration::from_secs(25));
        ping.tick().await;
        loop {
            tokio::select! {
                frame = rx.next() => match frame {
                    Some(Ok(Message::Text(t))) => self.lock().relay_frame(t.as_str()),
                    Some(Ok(Message::Close(Some(f)))) if u16::from(f.code) == 4000 => {
                        return Ok(Err("Another copy of Wren connected with the same phone link.".into()));
                    }
                    Some(Ok(Message::Close(_))) | None => return Ok(Ok(())),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Ok(Err(e.to_string())),
                },
                out = out_rx.recv() => {
                    let Some(text) = out else { return Ok(Ok(())) };
                    if let Err(e) = tx.send(Message::text(text)).await { return Ok(Err(e.to_string())); }
                },
                _ = ping.tick() => {
                    if let Err(e) = tx.send(Message::Ping(Vec::new().into())).await { return Ok(Err(e.to_string())); }
                }
            }
        }
    }

    pub fn pair(&self) -> Result<PairCode, String> {
        let mut i = self.lock();
        if !i.saved.enabled {
            return Err("Turn the phone link on first.".into());
        }
        let secret = random::<32>().to_vec();
        let until = now_ms() + PAIR_MS;
        let host_pub = b64(&crypto::public_raw(&i.host_secret()?));
        let url = format!("{}/#p={}.{}.{}", i.saved.relay, i.saved.room, host_pub, b64(&secret));
        i.pairing = Some((secret, until));
        i.emit_status();
        let svg = qrcode::QrCode::with_error_correction_level(url.as_bytes(), qrcode::EcLevel::M)
            .map_err(|e| e.to_string())?
            .render::<qrcode::render::svg::Color>()
            .min_dimensions(240, 240)
            .quiet_zone(true)
            .dark_color(qrcode::render::svg::Color("#17181c"))
            .light_color(qrcode::render::svg::Color("#ffffff"))
            .build();
        Ok(PairCode { url, svg, expires_at: until })
    }

    pub fn cancel_pairing(&self) {
        let mut i = self.lock();
        i.pairing = None;
        i.emit_status();
    }

    /// The person on this computer allowed (or refused) a new phone.
    pub fn confirm(&self, conn: u64, allow: bool) -> Result<(), String> {
        let mut i = self.lock();
        let Some(Conn { stage: Stage::Waiting { device, nonce }, .. }) = i.conns.get(&conn) else {
            return Err("That phone is no longer waiting.".into());
        };
        let (device, nonce) = (device.clone(), nonce.clone());
        if !allow {
            i.deny(conn, "declined");
            i.emit_status();
            return Ok(());
        }
        i.saved.devices.retain(|d| d.id != device.id);
        i.saved.devices.push(device.clone());
        i.save();
        i.accept(conn, &device, &nonce);
        Ok(())
    }

    pub fn remove_device(&self, id: &str) {
        let mut i = self.lock();
        i.saved.devices.retain(|d| d.id != id);
        i.save();
        let conns: Vec<u64> = i
            .conns
            .iter()
            .filter(|(_, c)| matches!(&c.stage, Stage::Ready { device, .. } if device == id))
            .map(|(k, _)| *k)
            .collect();
        for c in conns {
            i.deny(c, "removed");
        }
        i.emit_status();
    }

    pub fn rename_device(&self, id: &str, name: &str) -> Result<(), String> {
        let name: String = name.chars().filter(|c| !c.is_control()).take(60).collect::<String>().trim().to_string();
        if name.is_empty() {
            return Err("Enter a name.".into());
        }
        let mut i = self.lock();
        let d = i.saved.devices.iter_mut().find(|d| d.id == id).ok_or("unknown phone")?;
        d.name = name;
        i.save();
        i.emit_status();
        Ok(())
    }

    /// An app message to one phone, or to every connected phone (`conn` = None).
    pub fn send(&self, conn: Option<u64>, msg: &Value) -> Result<(), String> {
        let mut i = self.lock();
        match conn {
            Some(c) => i.seal(c, msg),
            None => {
                let ready: Vec<u64> = i.conns.iter().filter(|(_, c)| matches!(c.stage, Stage::Ready { .. })).map(|(k, _)| *k).collect();
                for c in ready {
                    i.seal(c, msg)?;
                }
                Ok(())
            }
        }
    }

    /// Push to phones that want this kind and don't have Wren on screen.
    pub fn push(&self, kind: &str, title: &str, body: &str, tag: &str) {
        let (targets, vapid, subject) = {
            let mut i = self.lock();
            if !i.saved.enabled {
                return;
            }
            let visible: Vec<String> = i
                .conns
                .values()
                .filter(|c| c.visible)
                .filter_map(|c| match &c.stage {
                    Stage::Ready { device, .. } => Some(device.clone()),
                    _ => None,
                })
                .collect();
            let now = now_ms();
            let mut targets = vec![];
            for d in &i.saved.devices {
                let Some(sub) = &d.push else { continue };
                if !d.push_kinds.iter().any(|k| k == kind) || visible.contains(&d.id) {
                    continue;
                }
                targets.push((d.id.clone(), sub.clone()));
            }
            targets.retain(|(id, _)| {
                let k = format!("{id}|{tag}|{kind}");
                let quiet = now.saturating_sub(*i.push_quiet.get(&k).unwrap_or(&0)) < PUSH_QUIET_MS;
                if !quiet {
                    i.push_quiet.insert(k, now);
                }
                !quiet
            });
            let Ok(vapid) = unb64(&i.saved.vapid_key).and_then(|k| crypto::secret_key(&k)) else { return };
            (targets, vapid, i.saved.relay.clone())
        };
        if targets.is_empty() {
            return;
        }
        let payload = json!({
            "title": title.chars().take(120).collect::<String>(),
            "body": body.chars().take(240).collect::<String>(),
            "tag": tag,
        });
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let client = http_client();
            for (id, sub) in targets {
                match push::send(&client, &vapid, &subject, &sub, &payload).await {
                    push::Outcome::Sent => {}
                    push::Outcome::Gone => {
                        let mut i = me.lock();
                        if let Some(d) = i.saved.devices.iter_mut().find(|d| d.id == id) {
                            d.push = None;
                        }
                        i.save();
                        i.emit_status();
                    }
                    push::Outcome::Failed(e) => eprintln!("wren: push to a phone failed: {e}"),
                }
            }
        });
    }
}

/// Bad settings (e.g. a wrong access key): don't keep retrying.
struct Fatal(String);

fn http_client() -> reqwest::Client {
    let _ = tokio_rustls::rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder().timeout(Duration::from_secs(20)).build().unwrap_or_default()
}

type Socket = tokio_tungstenite::WebSocketStream<Box<dyn Stream>>;
trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for T {}

/// TCP (+ TLS checked against the system's trusted certificates) + WebSocket.
async fn open_socket(url: &str) -> Result<Socket, String> {
    let (tls, rest) = if let Some(r) = url.strip_prefix("wss://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("ws://") {
        (false, r)
    } else {
        return Err("bad relay address".into());
    };
    let authority = rest.split('/').next().unwrap_or("");
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !h.is_empty() => (h, p.parse::<u16>().map_err(|e| e.to_string())?),
        _ => (authority, if tls { 443 } else { 80 }),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let tcp = tokio::net::TcpStream::connect((host, port)).await.map_err(|e| format!("Couldn't reach the relay: {e}"))?;
    tcp.set_nodelay(true).ok();
    let stream: Box<dyn Stream> = if tls {
        use rustls_platform_verifier::BuilderVerifierExt;
        use tokio_rustls::rustls;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_platform_verifier()
            .map_err(|e| e.to_string())?
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|e| e.to_string())?;
        let s = tokio_rustls::TlsConnector::from(Arc::new(config)).connect(name, tcp).await.map_err(|e| format!("Secure connection to the relay failed: {e}"))?;
        Box::new(s)
    } else {
        Box::new(tcp)
    };
    let (ws, _) = tokio_tungstenite::client_async(url, stream).await.map_err(|e| format!("The relay refused the connection: {e}"))?;
    Ok(ws)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_remote() -> (Remote, mpsc::UnboundedReceiver<String>) {
        let dir = std::env::temp_dir().join(format!("wren-remote-t-{}", b64(&random::<6>())));
        let r = Remote::new(dir, None);
        let (tx, rx) = mpsc::unbounded_channel();
        {
            let mut i = r.lock();
            i.saved.enabled = true;
            i.out = Some(tx);
        }
        (r, rx)
    }

    /// The phone's side of the handshake, as phone/src/protocol.ts does it.
    struct Phone {
        key: p256::SecretKey,
        nonce: [u8; 16],
        id: String,
    }

    impl Phone {
        fn new() -> Self {
            Self { key: crypto::secret_key(&crypto::new_secret()).unwrap(), nonce: random::<16>(), id: "phone-abc123".into() }
        }
        fn hello(&self, secret: Option<&[u8]>) -> String {
            let pubk = crypto::public_raw(&self.key);
            let mut m = json!({ "t": "hello", "v": 1, "dev": self.id, "pub": b64(&pubk), "n": b64(&self.nonce), "name": "Test phone" });
            if let Some(s) = secret {
                m["mac"] = json!(b64(&crypto::pair_proof(s, &pubk, &self.nonce)));
            }
            m.to_string()
        }
    }

    fn frames(rx: &mut mpsc::UnboundedReceiver<String>) -> Vec<Value> {
        let mut out = vec![];
        while let Ok(f) = rx.try_recv() {
            out.push(serde_json::from_str(&f).unwrap());
        }
        out
    }

    fn inner_msg(f: &Value) -> Value {
        serde_json::from_str(f["d"].as_str().unwrap()).unwrap()
    }

    #[test]
    fn pairing_needs_the_qr_secret_and_a_yes() {
        let (r, mut rx) = test_remote();
        let code = r.pair().unwrap();
        assert!(code.svg.contains("<svg") && code.svg.contains("<path"), "QR code is an SVG drawing");
        assert!(code.url.starts_with("/#p=") || code.url.contains("/#p="));
        let secret = unb64(code.url.rsplit('.').next().unwrap()).unwrap();
        let host_pub = {
            let i = r.lock();
            i.host_secret().unwrap().public_key()
        };
        // A phone without the secret is refused.
        let stranger = Phone::new();
        r.lock().relay_frame(&json!({ "t": "open", "c": 1 }).to_string());
        r.lock().relay_frame(&json!({ "t": "msg", "c": 1, "d": stranger.hello(Some(&random::<32>())) }).to_string());
        let f = frames(&mut rx);
        assert_eq!(inner_msg(&f[0])["t"], "deny");
        assert_eq!(f[1]["t"], "kick");

        // The right secret: waits for the person on the computer.
        let phone = Phone::new();
        r.lock().relay_frame(&json!({ "t": "open", "c": 2 }).to_string());
        r.lock().relay_frame(&json!({ "t": "msg", "c": 2, "d": phone.hello(Some(&secret)) }).to_string());
        assert_eq!(inner_msg(&frames(&mut rx)[0])["t"], "wait");
        // The QR code can't be used twice.
        let other = Phone { id: "phone-other1".into(), ..Phone::new() };
        r.lock().relay_frame(&json!({ "t": "open", "c": 3 }).to_string());
        r.lock().relay_frame(&json!({ "t": "msg", "c": 3, "d": other.hello(Some(&secret)) }).to_string());
        assert_eq!(inner_msg(&frames(&mut rx)[0])["why"], "expired");

        r.confirm(2, true).unwrap();
        let f = frames(&mut rx);
        let welcome = inner_msg(&f[0]);
        assert_eq!(welcome["t"], "welcome");
        let nh = unb64(welcome["n"].as_str().unwrap()).unwrap();
        let mut ch = crypto::Channel::phone(&phone.key, &host_pub, &phone.nonce, &nh);
        let sealed = inner_msg(&f[1]);
        let ready: Value = serde_json::from_slice(&ch.recv.open(sealed["d"].as_str().unwrap()).unwrap()).unwrap();
        assert_eq!(ready["t"], "ready");
        assert_eq!(ready["device"]["id"], "phone-abc123");
        assert_eq!(r.view().devices.len(), 1);
        assert!(r.view().devices[0].connected);

        // Later connections need no QR code, only the same key.
        r.lock().relay_frame(&json!({ "t": "open", "c": 4 }).to_string());
        let again = Phone { key: phone.key.clone(), nonce: random::<16>(), id: phone.id.clone() };
        r.lock().relay_frame(&json!({ "t": "msg", "c": 4, "d": again.hello(None) }).to_string());
        assert_eq!(inner_msg(&frames(&mut rx)[0])["t"], "welcome");
        // Same id, another key: refused.
        let fake = Phone { id: phone.id.clone(), ..Phone::new() };
        r.lock().relay_frame(&json!({ "t": "open", "c": 5 }).to_string());
        r.lock().relay_frame(&json!({ "t": "msg", "c": 5, "d": fake.hello(None) }).to_string());
        assert_eq!(inner_msg(&frames(&mut rx)[0])["why"], "unknown");

        // Removing the phone closes its connections.
        r.remove_device(&phone.id);
        assert!(frames(&mut rx).iter().any(|f| f["t"] == "kick" && f["c"] == 4));
        assert!(r.view().devices.is_empty());
    }

    /// Real relay (Node) + Wren's side + the phone's WebCrypto code (Node):
    /// `WREN_RELAY_E2E=1 cargo test real_relay_round_trip -- --nocapture`
    /// (needs `npm install` in relay/).
    #[test]
    fn real_relay_round_trip() {
        if std::env::var("WREN_RELAY_E2E").is_err() {
            return;
        }
        use sha2::Digest;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf();
        let key = format!("wren_test_{}", b64(&random::<24>()));
        let hash: String = sha2::Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        let port = 18000 + (random::<2>()[0] as u16) * 4;
        let mut relay = std::process::Command::new("node")
            .args(["--experimental-strip-types", "--no-warnings", "relay/src/node.ts"])
            .current_dir(&root)
            .env("WREN_RELAY_KEYS", &hash)
            .env("PORT", port.to_string())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(800));
        let base = format!("http://127.0.0.1:{port}");

        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let _guard = rt.enter();
        tauri::async_runtime::set(rt.handle().clone());
        let dir = std::env::temp_dir().join(format!("wren-remote-e2e-{}", b64(&random::<6>())));
        let r = Remote::new(dir.clone(), Some(Arc::new(attachments::Store::new(&dir))));
        let (ev_tx, ev_rx) = std::sync::mpsc::channel::<Value>();
        r.attach(Channel::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(s) = body {
                let _ = ev_tx.send(serde_json::from_str(&s).unwrap());
            }
            Ok(())
        }));
        // A wrong key is refused and not retried.
        r.configure(true, base.clone(), Some("wren_wrong_key_0000000000".into()), false).unwrap();
        let wait_status = |want: &str| loop {
            let ev = ev_rx.recv_timeout(Duration::from_secs(10)).expect("no status");
            if ev["kind"] == "status" && ev["view"]["status"] == want {
                return ev;
            }
        };
        let err = wait_status("error");
        assert!(err["view"]["error"].as_str().unwrap().contains("access key"));
        r.configure(true, base.clone(), Some(key), false).unwrap();
        wait_status("online");

        let code = r.pair().unwrap();
        let mut phone = std::process::Command::new("node")
            .args(["--experimental-strip-types", "--no-warnings", "phone/test/e2e-phone.ts", &base, &code.url])
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        // Play the UI: allow the new phone, answer its pings.
        let deadline = std::time::Instant::now() + Duration::from_secs(40);
        loop {
            if let Ok(Some(_)) = phone.try_wait() {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "the phone test took too long");
            let Ok(ev) = ev_rx.recv_timeout(Duration::from_millis(200)) else { continue };
            match ev["kind"].as_str() {
                Some("pair_request") => {
                    assert_eq!(ev["name"], "Node test phone");
                    r.confirm(ev["conn"].as_u64().unwrap(), true).unwrap();
                }
                Some("request") if ev["msg"]["op"] == "ping" => {
                    let reply = json!({ "t": "res", "id": ev["msg"]["id"], "ok": true, "data": { "echo": ev["msg"]["text"] } });
                    r.send(ev["conn"].as_u64(), &reply).unwrap();
                }
                _ => {}
            }
        }
        let out = phone.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        println!("{text}");
        relay.kill().ok();
        relay.wait().ok();
        r.stop();
        std::fs::remove_dir_all(&dir).ok();
        assert!(text.trim_end().ends_with("ALL OK"), "phone test failed:\n{text}");
    }

    #[test]
    fn tampered_boxes_close_the_connection_and_uploads_arrive() {
        let (r, mut rx) = test_remote();
        let phone = Phone::new();
        let device = Device { id: phone.id.clone(), name: "P".into(), key: b64(&crypto::public_raw(&phone.key)), added_at: 0, last_seen: 0, push: None, push_kinds: vec![] };
        let dir = r.lock().app_data.clone();
        r.lock().attachments = Some(Arc::new(attachments::Store::new(&dir)));
        r.lock().saved.devices.push(device);
        r.lock().relay_frame(&json!({ "t": "open", "c": 7 }).to_string());
        r.lock().relay_frame(&json!({ "t": "msg", "c": 7, "d": phone.hello(None) }).to_string());
        let f = frames(&mut rx);
        let nh = unb64(inner_msg(&f[0])["n"].as_str().unwrap()).unwrap();
        let host_pub = r.lock().host_secret().unwrap().public_key();
        let mut ch = crypto::Channel::phone(&phone.key, &host_pub, &phone.nonce, &nh);
        ch.recv.open(inner_msg(&f[1])["d"].as_str().unwrap()).unwrap();

        let send = |ch: &mut crypto::Channel, m: Value| {
            let d = json!({ "t": "e", "d": ch.send.seal(m.to_string().as_bytes()) }).to_string();
            r.lock().relay_frame(&json!({ "t": "msg", "c": 7, "d": d }).to_string());
        };
        let data = b"a,b\n1,2\n";
        send(&mut ch, json!({ "t": "req", "id": 1, "op": "upload", "u": "x1", "name": "t.csv", "size": data.len(), "last": false, "data": b64(&data[..4]) }));
        send(&mut ch, json!({ "t": "req", "id": 2, "op": "upload", "u": "x1", "last": true, "data": b64(&data[4..]) }));
        let res: Vec<Value> = frames(&mut rx).iter().map(|f| serde_json::from_slice(&ch.recv.open(inner_msg(f)["d"].as_str().unwrap()).unwrap()).unwrap()).collect();
        assert_eq!(res[1]["ok"], true, "{res:?}");
        let path = res[1]["data"]["path"].as_str().unwrap();
        assert_eq!(std::fs::read(path).unwrap(), data);

        // A box the relay sends a second time (a replay) drops the connection.
        let boxed = json!({ "t": "e", "d": ch.send.seal(br#"{"op":"visible","on":true}"#) }).to_string();
        let frame = json!({ "t": "msg", "c": 7, "d": boxed }).to_string();
        r.lock().relay_frame(&frame);
        assert!(r.lock().conns[&7].visible);
        r.lock().relay_frame(&frame);
        assert!(frames(&mut rx).iter().any(|f| f["t"] == "kick"));
        assert!(!r.lock().conns.contains_key(&7));
        std::fs::remove_dir_all(dir).ok();
    }
}
