//! Accounts: any number per AI (Claude Code, Codex, Antigravity).
//!
//! Wren never reads or stores login tokens. Each account is only a label plus
//! a private config folder that the official CLI keeps its own login in:
//! - Claude Code: `CLAUDE_CONFIG_DIR=<data>/accounts/claude/<id>`
//! - Codex:       `CODEX_HOME=<data>/accounts/codex/<id>`
//! - Antigravity: no per-folder login exists (one Google login per computer),
//!   so only one Antigravity account is "active" (signed in) at a time.
//!
//! A fresh Wren has NO accounts. For each AI the user can add the CLI's normal
//! login on this computer (the one made in a terminal) as an account
//! (`is_default`, id `default-<bin>`), or add new accounts with their own
//! private login folder. Removing an `is_default` account only takes it off
//! Wren's list; the CLI's own login is never touched.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::providers::Provider;


#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    /// Not checked yet.
    Unknown,
    /// Added, never signed in.
    Pending,
    SignedIn,
    SignedOut,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub name: String,
    pub status: AccountStatus,
    pub email: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct AccountsView {
    pub accounts: Vec<Account>,
    /// The Antigravity account that is currently signed in on this computer.
    pub active_antigravity: Option<String>,
}

#[derive(Serialize, Deserialize, Default)]
struct StoreFile {
    version: u32,
    accounts: Vec<Account>,
    active_antigravity: Option<String>,
}

pub struct AccountStore {
    file: PathBuf,
    root: PathBuf,
    inner: Mutex<StoreFile>,
}

pub fn default_id(p: Provider) -> String {
    format!("default-{}", p.binary())
}

impl AccountStore {
    /// `data_dir` is Wren's private app-data folder on this computer.
    pub fn load(data_dir: &Path) -> Self {
        let root = data_dir.join("accounts");
        let file = data_dir.join("accounts.json");
        let mut store: StoreFile = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Version 1 always listed a Default account per AI; those saved
        // entries are kept as they are. New stores start empty.
        store.version = 2;
        Self { file, root, inner: Mutex::new(store) }
    }

    fn save(&self) -> Result<(), String> {
        let g = self.inner.lock().map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&*g).map_err(|e| e.to_string())?;
        if let Some(dir) = self.file.parent() {
            create_private_dir(dir)?;
        }
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, json).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.file).map_err(|e| e.to_string())
    }

    pub fn view(&self) -> AccountsView {
        let g = self.inner.lock().expect("accounts lock");
        AccountsView { accounts: g.accounts.clone(), active_antigravity: g.active_antigravity.clone() }
    }

    pub fn get(&self, id: &str) -> Result<Account, String> {
        let g = self.inner.lock().map_err(|e| e.to_string())?;
        g.accounts
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| "This account isn't in Wren. Open Accounts to add one.".into())
    }

    /// Wren-added accounts (with their own login folder), for sign-out before a reset.
    pub fn added(&self) -> Vec<Account> {
        let g = self.inner.lock().expect("accounts lock");
        g.accounts.iter().filter(|a| !a.is_default).cloned().collect()
    }

    pub fn add(&self, provider: Provider, name: &str) -> Result<Account, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("Give the account a name (up to 40 characters).".into());
        }
        let acc = Account {
            id: format!("{}-{}", provider.binary(), unique_suffix()),
            provider,
            name: name.to_string(),
            status: AccountStatus::Pending,
            email: None,
            is_default: false,
        };
        self.inner.lock().map_err(|e| e.to_string())?.accounts.push(acc.clone());
        self.save()?;
        Ok(acc)
    }

    /// Adds the CLI's normal login on this computer (no private folder) as an account.
    pub fn add_existing(&self, provider: Provider, name: &str) -> Result<Account, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("Give the account a name (up to 40 characters).".into());
        }
        let id = default_id(provider);
        let acc = Account {
            id: id.clone(),
            provider,
            name: name.to_string(),
            status: AccountStatus::Unknown,
            email: None,
            is_default: true,
        };
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            if g.accounts.iter().any(|a| a.id == id) {
                return Err("This computer's login is already in Wren.".into());
            }
            // Keep it first among this AI's accounts.
            let at = g.accounts.iter().position(|a| a.provider == provider).unwrap_or(g.accounts.len());
            g.accounts.insert(at, acc.clone());
        }
        self.save()?;
        Ok(acc)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err("Give the account a name (up to 40 characters).".into());
        }
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            let a = g.accounts.iter_mut().find(|a| a.id == id).ok_or("Unknown account")?;
            a.name = name.to_string();
        }
        self.save()
    }

    /// Removes the label and its private folder. Call `sign_out` first so the
    /// CLI clears its own login (e.g. the macOS Keychain item). For the
    /// computer's own login (`is_default`) only the label goes.
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let acc = self.get(id)?;
        if let Some(dir) = self.dir_for(&acc)
            && dir.starts_with(&self.root) && dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            }
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            g.accounts.retain(|a| a.id != id);
            if g.active_antigravity.as_deref() == Some(id) {
                g.active_antigravity = None;
            }
        }
        self.save()
    }

    pub fn set_status(&self, id: &str, status: AccountStatus, email: Option<String>) -> Result<(), String> {
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            let a = g.accounts.iter_mut().find(|a| a.id == id).ok_or("Unknown account")?;
            a.status = status;
            if email.is_some() {
                a.email = email;
            }
        }
        self.save()
    }

    /// Antigravity: mark `id` as the one signed in; all others become signed out.
    pub fn set_active_antigravity(&self, id: &str) -> Result<(), String> {
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            if !g.accounts.iter().any(|a| a.id == id && a.provider == Provider::Antigravity) {
                return Err("Unknown Antigravity account".into());
            }
            g.active_antigravity = Some(id.to_string());
            for a in g.accounts.iter_mut().filter(|a| a.provider == Provider::Antigravity) {
                a.status = if a.id == id { AccountStatus::SignedIn } else { AccountStatus::SignedOut };
            }
        }
        self.save()
    }

    /// The account's private config folder (None = CLI default / Antigravity).
    pub fn dir_for(&self, acc: &Account) -> Option<PathBuf> {
        if acc.is_default || acc.provider == Provider::Antigravity {
            return None;
        }
        Some(self.root.join(acc.provider.binary()).join(&acc.id))
    }

    /// Environment variables that point the CLI at this account's login.
    pub fn env_for(&self, id: &str) -> Result<Vec<(String, String)>, String> {
        let acc = self.get(id)?;
        if acc.provider == Provider::Antigravity && !acc.is_default {
            let active = self.inner.lock().map_err(|e| e.to_string())?.active_antigravity.clone();
            if active.as_deref() != Some(id) {
                return Err(format!(
                    "The Antigravity account \"{}\" isn't the one signed in. Open Accounts and sign in to it first.",
                    acc.name
                ));
            }
        }
        let Some(dir) = self.dir_for(&acc) else { return Ok(vec![]) };
        create_private_dir(&dir)?;
        let var = match acc.provider {
            Provider::Claude => "CLAUDE_CONFIG_DIR",
            Provider::Codex => "CODEX_HOME",
            Provider::Antigravity => return Ok(vec![]),
        };
        Ok(vec![(var.to_string(), dir.to_string_lossy().to_string())])
    }
}

pub(crate) fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    format!("{:x}{:x}", t, N.fetch_add(1, Ordering::SeqCst))
}

/// Folders holding logins are readable only by you.
pub(crate) fn create_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

// ---------------- talking to the CLIs (status / sign out) ----------------

/// Args for the CLI's own sign-in command (run in a terminal view).
pub fn login_args(p: Provider, device_code: bool) -> Vec<&'static str> {
    match p {
        Provider::Claude => vec!["auth", "login"],
        Provider::Codex if device_code => vec!["login", "--device-auth"],
        Provider::Codex => vec!["login"],
        // agy signs in when it starts; the user types /quit when done.
        Provider::Antigravity => vec![],
    }
}

/// Ask the CLI whether this account is signed in. Antigravity has no status
/// command, so its status comes from the user finishing sign-in.
pub async fn check_status(
    p: Provider,
    env: &[(String, String)],
) -> Result<(AccountStatus, Option<String>), String> {
    let args: &[&str] = match p {
        Provider::Claude => &["auth", "status", "--json"],
        Provider::Codex => &["login", "status"],
        Provider::Antigravity => return Ok((AccountStatus::Unknown, None)),
    };
    let (code, stdout, stderr) = run_cli(p, args, env).await?;
    Ok(parse_status(p, code, &stdout, &stderr))
}

pub fn parse_status(p: Provider, code: Option<i32>, stdout: &str, stderr: &str) -> (AccountStatus, Option<String>) {
    match p {
        Provider::Claude => {
            if let Ok(v) = serde_json::from_str::<Value>(stdout.trim()) {
                let logged_in = v["loggedIn"].as_bool().or(v["logged_in"].as_bool()).unwrap_or(code == Some(0));
                let email = v["email"]
                    .as_str()
                    .or(v["account"]["email"].as_str())
                    .or(v["oauthAccount"]["emailAddress"].as_str())
                    .map(String::from);
                let st = if logged_in { AccountStatus::SignedIn } else { AccountStatus::SignedOut };
                return (st, email);
            }
            let st = if code == Some(0) { AccountStatus::SignedIn } else { AccountStatus::SignedOut };
            (st, None)
        }
        Provider::Codex => {
            let text = format!("{stdout}\n{stderr}").to_lowercase();
            let st = if code == Some(0) && !text.contains("not logged in") {
                AccountStatus::SignedIn
            } else {
                AccountStatus::SignedOut
            };
            (st, None)
        }
        Provider::Antigravity => (AccountStatus::Unknown, None),
    }
}

/// Ask the CLI to sign this account out (clears its stored login).
pub async fn sign_out(p: Provider, env: &[(String, String)]) -> Result<(), String> {
    let args: &[&str] = match p {
        Provider::Claude => &["auth", "logout"],
        Provider::Codex => &["logout"],
        Provider::Antigravity => return Ok(()), // done with /logout inside agy
    };
    run_cli(p, args, env).await.map(|_| ())
}

async fn run_cli(p: Provider, args: &[&str], env: &[(String, String)]) -> Result<(Option<i32>, String, String), String> {
    let path = crate::sessions::search_path();
    let home = std::env::temp_dir();
    let bin = which::which_in(p.binary(), Some(&path), &home)
        .map_err(|_| format!("The `{}` CLI isn't installed.", p.binary()))?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .env("PATH", &path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in p.env() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .map_err(|_| "The CLI took too long to answer.".to_string())?
        .map_err(|e| e.to_string())?;
    Ok((
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("wren-acc-{}", unique_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn starts_empty_add_existing_persist_env_remove() {
        let d = tmp();
        let s = AccountStore::load(&d);
        assert!(s.view().accounts.is_empty(), "a fresh Wren has no accounts");
        assert!(s.get("default-claude").is_err());
        let me = s.add_existing(Provider::Claude, "Default").unwrap();
        assert!(me.is_default);
        assert!(s.add_existing(Provider::Claude, "Again").is_err());
        assert!(s.env_for("default-claude").unwrap().is_empty());
        s.add_existing(Provider::Codex, "Default").unwrap();
        s.add_existing(Provider::Antigravity, "Default").unwrap();

        let c1 = s.add(Provider::Claude, "Work").unwrap();
        let c2 = s.add(Provider::Claude, "Personal").unwrap();
        let x1 = s.add(Provider::Codex, "Main").unwrap();
        assert_ne!(c1.id, c2.id);
        let env = s.env_for(&c1.id).unwrap();
        assert_eq!(env[0].0, "CLAUDE_CONFIG_DIR");
        assert!(Path::new(&env[0].1).is_dir());
        assert_ne!(env, s.env_for(&c2.id).unwrap());
        assert_eq!(s.env_for(&x1.id).unwrap()[0].0, "CODEX_HOME");

        // persisted
        let s2 = AccountStore::load(&d);
        assert_eq!(s2.view().accounts.len(), 6);

        // remove deletes the private folder
        let dir = PathBuf::from(&env[0].1);
        s2.remove(&c1.id).unwrap();
        assert!(!dir.exists());
        assert!(s2.add(Provider::Codex, "   ").is_err());
        // The computer's login can be renamed, and the name is kept after restart.
        s2.rename("default-claude", "My Claude").unwrap();
        let s3 = AccountStore::load(&d);
        assert_eq!(s3.get("default-claude").unwrap().name, "My Claude");
        assert!(s3.get("default-claude").unwrap().is_default);
        // ...and removed from Wren's list (the CLI's login itself is untouched).
        s3.remove("default-claude").unwrap();
        assert!(AccountStore::load(&d).get("default-claude").is_err());
        assert_eq!(s3.added().len(), 2);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn old_store_keeps_its_default_accounts() {
        let d = tmp();
        std::fs::write(
            d.join("accounts.json"),
            r#"{"version":1,"accounts":[{"id":"default-claude","provider":"claude","name":"Leang","status":"signed_in","email":"a@b.c","is_default":true}],"active_antigravity":null}"#,
        )
        .unwrap();
        let s = AccountStore::load(&d);
        assert_eq!(s.view().accounts.len(), 1);
        assert_eq!(s.get("default-claude").unwrap().name, "Leang");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn antigravity_single_active() {
        let d = tmp();
        let s = AccountStore::load(&d);
        let a = s.add(Provider::Antigravity, "Google A").unwrap();
        let b = s.add(Provider::Antigravity, "Google B").unwrap();
        assert!(s.env_for(&a.id).is_err(), "inactive account is refused");
        s.set_active_antigravity(&a.id).unwrap();
        assert!(s.env_for(&a.id).unwrap().is_empty());
        assert!(s.env_for(&b.id).is_err());
        s.set_active_antigravity(&b.id).unwrap();
        let v = s.view();
        assert_eq!(v.active_antigravity.as_deref(), Some(b.id.as_str()));
        assert_eq!(v.accounts.iter().find(|x| x.id == a.id).unwrap().status, AccountStatus::SignedOut);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn parse_claude_and_codex_status() {
        let (st, email) = parse_status(Provider::Claude, Some(0), r#"{"loggedIn":true,"authMethod":"claude.ai","email":"me@example.com"}"#, "");
        assert_eq!(st, AccountStatus::SignedIn);
        assert_eq!(email.as_deref(), Some("me@example.com"));
        let (st, _) = parse_status(Provider::Claude, Some(1), r#"{"loggedIn":false}"#, "");
        assert_eq!(st, AccountStatus::SignedOut);
        assert_eq!(parse_status(Provider::Codex, Some(0), "Logged in using ChatGPT", "").0, AccountStatus::SignedIn);
        assert_eq!(parse_status(Provider::Codex, Some(1), "Not logged in", "").0, AccountStatus::SignedOut);
    }
}
