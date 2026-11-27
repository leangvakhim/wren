//! What Wren remembers about the phone link: `<app_data>/remote.json`, readable
//! only by this user (0600). It holds Wren's own secrets for the link (its
//! key, its push key, the relay access key); the user chose a private file
//! over the keychain, which would ask for the Mac password after every update
//! of an ad-hoc signed app. The CLIs' logins are never here.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::crypto::{b64, new_secret, random};
use super::push::Subscription;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// The phone's public key (base64url, 65 bytes).
    pub key: String,
    /// ms
    pub added_at: u64,
    pub last_seen: u64,
    #[serde(default)]
    pub push: Option<Subscription>,
    /// Kinds of push the phone wants ("approval", "question", "done", "problems").
    #[serde(default)]
    pub push_kinds: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Saved {
    pub enabled: bool,
    /// https://… (http:// only for this computer, for testing).
    pub relay: String,
    pub access_key: String,
    /// Wren's room on the relay (random, 16 bytes base64url).
    pub room: String,
    /// Wren's private key for the link (base64url, 32 bytes).
    pub host_key: String,
    /// Wren's private key for Web Push (VAPID).
    pub vapid_key: String,
    pub keep_awake: bool,
    pub devices: Vec<Device>,
}

impl Saved {
    fn fresh() -> Self {
        Self {
            enabled: false,
            relay: String::new(),
            access_key: String::new(),
            room: b64(&random::<16>()),
            host_key: b64(&new_secret()),
            vapid_key: b64(&new_secret()),
            keep_awake: true,
            devices: vec![],
        }
    }
}

pub fn path(app_data: &Path) -> PathBuf {
    app_data.join("remote.json")
}

pub fn load(app_data: &Path) -> Saved {
    match fs::read_to_string(path(app_data)).ok().and_then(|s| serde_json::from_str::<Saved>(&s).ok()) {
        Some(s) if !s.room.is_empty() && !s.host_key.is_empty() && !s.vapid_key.is_empty() => s,
        _ => Saved::fresh(),
    }
}

/// Atomic write, private to this user.
pub fn save(app_data: &Path, s: &Saved) -> Result<(), String> {
    fs::create_dir_all(app_data).map_err(|e| e.to_string())?;
    let tmp = app_data.join("remote.json.tmp");
    let text = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().ok();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600)).ok();
    }
    fs::rename(&tmp, path(app_data)).map_err(|e| e.to_string())
}

/// Accepts "https://relay.example.com" (and http:// only for this computer).
/// Returns the base URL without a trailing slash.
pub fn check_relay(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/');
    let (scheme, rest) = url.split_once("://").ok_or("The relay address must start with https://")?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let name = host.rsplit_once(':').map(|(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host }).unwrap_or(host);
    if host.is_empty() || host.contains('@') || rest.contains(['?', '#']) {
        return Err("That isn't a relay address (example: https://remote.example.com).".into());
    }
    let local = matches!(name, "localhost" | "127.0.0.1" | "[::1]");
    match scheme {
        "https" => Ok(url.to_string()),
        "http" if local => Ok(url.to_string()),
        "http" => Err("The relay address must use https:// (plain http is only allowed for this computer).".into()),
        _ => Err("The relay address must start with https://".into()),
    }
}

/// wss://host/path/host for a relay base URL.
pub fn host_socket_url(relay: &str) -> String {
    if let Some(rest) = relay.strip_prefix("https://") {
        format!("wss://{rest}/host")
    } else {
        format!("ws://{}/host", relay.trim_start_matches("http://"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_addresses() {
        assert_eq!(check_relay(" https://remote.example.com/ ").unwrap(), "https://remote.example.com");
        assert_eq!(check_relay("https://example.com/wren").unwrap(), "https://example.com/wren");
        assert_eq!(check_relay("http://localhost:8787").unwrap(), "http://localhost:8787");
        assert_eq!(check_relay("http://127.0.0.1:8787").unwrap(), "http://127.0.0.1:8787");
        assert!(check_relay("http://remote.example.com").is_err());
        assert!(check_relay("http://localhost.evil.com").is_err());
        assert!(check_relay("ftp://x").is_err());
        assert!(check_relay("https://user@x.com").is_err());
        assert!(check_relay("remote.example.com").is_err());
        assert_eq!(host_socket_url("https://example.com/wren"), "wss://example.com/wren/host");
        assert_eq!(host_socket_url("http://localhost:8787"), "ws://localhost:8787/host");
    }

    #[test]
    fn saved_file_is_private_and_reloads() {
        let dir = std::env::temp_dir().join(format!("wren-remote-{}", b64(&random::<6>())));
        let mut s = load(&dir);
        assert!(!s.enabled);
        assert_eq!(s.room.len(), 22);
        s.relay = "https://r.example".into();
        save(&dir, &s).unwrap();
        let again = load(&dir);
        assert_eq!(again.room, s.room);
        assert_eq!(again.host_key, s.host_key);
        assert_eq!(again.relay, "https://r.example");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(path(&dir)).unwrap().permissions().mode() & 0o777, 0o600);
        }
        fs::remove_dir_all(dir).ok();
    }
}
