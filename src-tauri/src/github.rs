//! GitHub accounts for panes, through the official GitHub CLI (`gh`).
//!
//! Like the AI accounts, Wren never reads or stores a GitHub token. Each
//! account is a label plus a private folder that `gh` keeps its own login in
//! (`GH_CONFIG_DIR=<data>/accounts/gh/<id>`; the token itself goes to the OS
//! keychain). "Use this computer's login" (`is_default`) is gh's normal login.
//!
//! A pane that picks a GitHub account starts its AI CLI with that account's
//! `gh` folder, and git is told (for that process only, through
//! `GIT_CONFIG_PARAMETERS`) to get github.com logins from `gh`. The user's
//! `~/.gitconfig` is never changed.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::accounts::{create_private_dir, unique_suffix, AccountStatus};

pub const HOST: &str = "github.com";
const DEFAULT_ID: &str = "default-gh";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GithubAccount {
    pub id: String,
    pub name: String,
    pub status: AccountStatus,
    /// GitHub username, known after sign-in.
    pub login: Option<String>,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct StoreFile {
    version: u32,
    accounts: Vec<GithubAccount>,
}

pub struct GithubStore {
    file: PathBuf,
    root: PathBuf,
    inner: Mutex<StoreFile>,
}

impl GithubStore {
    pub fn load(data_dir: &Path) -> Self {
        let file = data_dir.join("github.json");
        let root = data_dir.join("accounts").join("gh");
        let mut store: StoreFile = std::fs::read_to_string(&file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        store.version = 1;
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

    pub fn list(&self) -> Vec<GithubAccount> {
        self.inner.lock().expect("github lock").accounts.clone()
    }

    pub fn get(&self, id: &str) -> Result<GithubAccount, String> {
        self.inner
            .lock()
            .map_err(|e| e.to_string())?
            .accounts
            .iter()
            .find(|a| a.id == id)
            .cloned()
            .ok_or_else(|| "This GitHub account isn't in Wren any more. Pick another one in the pane.".into())
    }

    /// Adds a new account with its own login folder, or (`existing`) gh's
    /// normal login on this computer.
    pub fn add(&self, name: &str, existing: bool) -> Result<GithubAccount, String> {
        let name = check_name(name)?;
        let acc = GithubAccount {
            id: if existing { DEFAULT_ID.to_string() } else { format!("gh-{}", unique_suffix()) },
            name,
            status: if existing { AccountStatus::Unknown } else { AccountStatus::Pending },
            login: None,
            is_default: existing,
        };
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            if existing && g.accounts.iter().any(|a| a.id == DEFAULT_ID) {
                return Err("This computer's GitHub login is already in Wren.".into());
            }
            if existing {
                g.accounts.insert(0, acc.clone());
            } else {
                g.accounts.push(acc.clone());
            }
        }
        self.save()?;
        Ok(acc)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<(), String> {
        let name = check_name(name)?;
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            g.accounts.iter_mut().find(|a| a.id == id).ok_or("Unknown account")?.name = name;
        }
        self.save()
    }

    pub fn set_status(&self, id: &str, status: AccountStatus, login: Option<String>) -> Result<(), String> {
        {
            let mut g = self.inner.lock().map_err(|e| e.to_string())?;
            let a = g.accounts.iter_mut().find(|a| a.id == id).ok_or("Unknown account")?;
            // Never signed in yet stays "pending" rather than "signed out".
            a.status = if status == AccountStatus::SignedOut && a.status == AccountStatus::Pending { a.status } else { status };
            if login.is_some() {
                a.login = login;
            }
        }
        self.save()
    }

    /// Removes the label and the private folder (sign out first with `sign_out`).
    pub fn remove(&self, id: &str) -> Result<(), String> {
        let acc = self.get(id)?;
        if let Some(dir) = self.dir_for(&acc)
            && dir.starts_with(&self.root) && dir.exists() {
                std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
            }
        self.inner.lock().map_err(|e| e.to_string())?.accounts.retain(|a| a.id != id);
        self.save()
    }

    /// Wren-added accounts (with their own folder), for sign-out before a reset.
    pub fn added(&self) -> Vec<GithubAccount> {
        self.list().into_iter().filter(|a| !a.is_default).collect()
    }

    /// gh's config folder for this account (None = gh's normal one).
    pub fn dir_for(&self, acc: &GithubAccount) -> Option<PathBuf> {
        (!acc.is_default).then(|| self.root.join(&acc.id))
    }

    /// Environment for running `gh` itself as this account.
    pub fn gh_env(&self, id: &str) -> Result<Vec<(String, String)>, String> {
        let acc = self.get(id)?;
        let mut env: Vec<(String, String)> = QUIET.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        if let Some(dir) = self.dir_for(&acc) {
            create_private_dir(&dir)?;
            env.push(("GH_CONFIG_DIR".into(), dir.to_string_lossy().to_string()));
        }
        Ok(env)
    }

    /// Environment for a pane's AI CLI: gh as this account, and git asks gh
    /// for github.com logins. `gh_bin` = where gh is installed.
    pub fn pane_env(&self, id: &str, gh_bin: &Path) -> Result<Vec<(String, String)>, String> {
        let mut env = self.gh_env(id)?;
        env.push(("GIT_CONFIG_PARAMETERS".into(), git_config_parameters(gh_bin)));
        Ok(env)
    }
}

/// gh never stops to ask, and doesn't check for its own updates.
const QUIET: [(&str, &str); 3] = [
    ("GH_PROMPT_DISABLED", "1"),
    ("GH_NO_UPDATE_NOTIFIER", "1"),
    ("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1"),
];

fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 40 {
        return Err("Give the account a name (up to 40 characters).".into());
    }
    Ok(name.to_string())
}

/// The same settings `gh auth setup-git` writes to ~/.gitconfig, but only for
/// this process: drop other github.com helpers, then use `gh auth git-credential`.
/// Uses git's own quoting for `GIT_CONFIG_PARAMETERS` ('...' with ' as '\'').
pub fn git_config_parameters(gh_bin: &Path) -> String {
    // git runs helpers through sh (Git Bash on Windows): forward slashes, quoted.
    let bin = gh_bin.to_string_lossy().replace('\\', "/");
    let key = format!("credential.https://{HOST}.helper");
    let helper = format!("!\"{bin}\" auth git-credential");
    let q = |s: &str| format!("'{}'", s.replace('\'', r"'\''"));
    format!("{}={} {}={}", q(&key), q(""), q(&key), q(&helper))
}

pub fn gh_path() -> Option<PathBuf> {
    let path = crate::sessions::search_path();
    which::which_in("gh", Some(&path), std::env::temp_dir()).ok()
}

/// Args for gh's own browser sign-in. With prompts off (GH_PROMPT_DISABLED)
/// gh prints a one-time code and https://github.com/login/device, then waits
/// for the browser; it never asks to change the global git settings.
pub fn login_args() -> Vec<&'static str> {
    vec!["auth", "login", "--hostname", HOST, "--git-protocol", "https", "--web"]
}

/// Extra environment for the sign-in terminal: prompts off, and any git
/// setup gh might still do goes to a throwaway file, never ~/.gitconfig.
pub fn login_env(mut env: Vec<(String, String)>, scratch: &Path) -> Vec<(String, String)> {
    env.push(("GIT_CONFIG_GLOBAL".into(), scratch.join("gitconfig").to_string_lossy().to_string()));
    env
}

/// Ask gh whether this account is signed in, and as whom.
pub async fn check_status(env: &[(String, String)]) -> Result<(AccountStatus, Option<String>), String> {
    let (_, out) = run_gh(&["auth", "status", "--hostname", HOST], env).await?;
    Ok(parse_status(&out))
}

/// `gh auth status` text -> signed in + username of the ACTIVE account.
pub fn parse_status(text: &str) -> (AccountStatus, Option<String>) {
    let mut current: Option<(bool, String)> = None;
    let mut first_ok: Option<String> = None;
    for line in text.lines() {
        let l = line.trim();
        // gh 2.40+: "✓ Logged in to github.com account NAME (keyring)";
        // older: "✓ Logged in to github.com as NAME (...)";
        // a bad token: "X Failed to log in to github.com account NAME (...)".
        let ok = l.contains("Logged in to");
        let failed = l.contains("Failed to log in to");
        if ok || failed {
            let name = l
                .split_once(" account ")
                .or_else(|| l.split_once(" as "))
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .map(String::from);
            if let Some(n) = name {
                if ok && first_ok.is_none() {
                    first_ok = Some(n.clone());
                }
                current = Some((ok, n));
            }
        } else if l.contains("Active account: true")
            && let Some((ok, n)) = &current {
                let st = if *ok { AccountStatus::SignedIn } else { AccountStatus::SignedOut };
                return (st, Some(n.clone()));
            }
    }
    match first_ok {
        Some(n) => (AccountStatus::SignedIn, Some(n)),
        None => (AccountStatus::SignedOut, None),
    }
}

/// Sign out in gh (removes its keychain entry for this user).
pub async fn sign_out(env: &[(String, String)], login: Option<&str>) -> Result<(), String> {
    let mut args = vec!["auth", "logout", "--hostname", HOST];
    if let Some(u) = login {
        args.extend(["--user", u]);
    }
    run_gh(&args, env).await.map(|_| ())
}

async fn run_gh(args: &[&str], env: &[(String, String)]) -> Result<(Option<i32>, String), String> {
    let bin = gh_path().ok_or("The GitHub CLI (gh) isn't installed.")?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .env("PATH", crate::sessions::search_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(20), cmd.output())
        .await
        .map_err(|_| "gh took too long to answer.".to_string())?
        .map_err(|e| e.to_string())?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    Ok((out.status.code(), text))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("wren-gh-{}", unique_suffix()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn add_persist_env_remove() {
        let d = tmp();
        let s = GithubStore::load(&d);
        assert!(s.list().is_empty());
        let work = s.add("Work", false).unwrap();
        let me = s.add("Me", true).unwrap();
        assert!(s.add("Again", true).is_err());
        assert_eq!(s.list()[0].id, me.id, "this computer's login first");
        let env = s.gh_env(&work.id).unwrap();
        let dir = env.iter().find(|(k, _)| k == "GH_CONFIG_DIR").unwrap().1.clone();
        assert!(Path::new(&dir).is_dir());
        assert!(!s.gh_env(&me.id).unwrap().iter().any(|(k, _)| k == "GH_CONFIG_DIR"), "normal gh login");
        assert!(env.iter().any(|(k, v)| k == "GH_PROMPT_DISABLED" && v == "1"));
        let pane = s.pane_env(&work.id, Path::new("/opt/homebrew/bin/gh")).unwrap();
        assert!(pane.iter().any(|(k, _)| k == "GIT_CONFIG_PARAMETERS"));

        s.set_status(&work.id, AccountStatus::SignedOut, None).unwrap();
        assert_eq!(s.get(&work.id).unwrap().status, AccountStatus::Pending, "never signed in");
        s.set_status(&work.id, AccountStatus::SignedIn, Some("octo".into())).unwrap();
        s.rename(&work.id, "Job").unwrap();
        let s2 = GithubStore::load(&d);
        let w = s2.get(&work.id).unwrap();
        assert_eq!((w.name.as_str(), w.login.as_deref(), w.status), ("Job", Some("octo"), AccountStatus::SignedIn));
        s2.remove(&work.id).unwrap();
        assert!(!Path::new(&dir).exists());
        assert_eq!(s2.added().len(), 0);
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn git_parameters_quote_paths() {
        assert_eq!(
            git_config_parameters(Path::new("/opt/homebrew/bin/gh")),
            r#"'credential.https://github.com.helper'='' 'credential.https://github.com.helper'='!"/opt/homebrew/bin/gh" auth git-credential'"#
        );
        let win = git_config_parameters(Path::new(r"C:\Program Files\GitHub CLI\gh.exe"));
        assert!(win.contains(r#"!"C:/Program Files/GitHub CLI/gh.exe" auth git-credential"#), "{win}");
        assert!(git_config_parameters(Path::new("/it's/gh")).contains(r"/it'\''s/gh"));
    }

    /// Real git + real gh (skipped if either is missing): with a pane's
    /// environment, git gets github.com logins from THIS account's gh folder.
    #[test]
    fn real_git_asks_the_panes_gh_account() {
        let Some(bin) = gh_path() else { return };
        if which::which("git").is_err() {
            return;
        }
        let d = tmp();
        let s = GithubStore::load(&d);
        let acc = s.add("Test", false).unwrap();
        let dir = s.dir_for(&acc).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        // A dummy login stored as a plain file (never a real token).
        std::fs::write(
            dir.join("hosts.yml"),
            "github.com:\n    users:\n        wren-test:\n            oauth_token: gho_WRENTEST\n    git_protocol: https\n    oauth_token: gho_WRENTEST\n    user: wren-test\n",
        )
        .unwrap();
        let mut cmd = std::process::Command::new("git");
        cmd.args(["credential", "fill"])
            .env("GIT_CONFIG_GLOBAL", d.join("empty-gitconfig"))
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in s.pane_env(&acc.id, &bin).unwrap() {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(b"protocol=https\nhost=github.com\n\n").unwrap();
        let out = String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).to_string();
        assert!(out.contains("username=wren-test") && out.contains("password=gho_WRENTEST"), "{out}");
        std::fs::remove_dir_all(d).ok();
    }

    #[test]
    fn parse_gh_status() {
        // gh 2.64 shapes.
        let ok = "github.com\n  ✓ Logged in to github.com account octo (keyring)\n  - Active account: true\n  - Git operations protocol: https\n  - Token: gho_************************************\n";
        assert_eq!(parse_status(ok), (AccountStatus::SignedIn, Some("octo".into())));
        let two = "github.com\n  ✓ Logged in to github.com account other (keyring)\n  - Active account: false\n\n  ✓ Logged in to github.com account octo (keyring)\n  - Active account: true\n";
        assert_eq!(parse_status(two), (AccountStatus::SignedIn, Some("octo".into())));
        let bad = "github.com\n  X Failed to log in to github.com account testuser (/x/hosts.yml)\n  - Active account: true\n  - The token in /x/hosts.yml is invalid.\n";
        assert_eq!(parse_status(bad), (AccountStatus::SignedOut, Some("testuser".into())));
        let none = "You are not logged into any GitHub hosts. To log in, run: gh auth login\n";
        assert_eq!(parse_status(none), (AccountStatus::SignedOut, None));
        let old = "github.com\n  ✓ Logged in to github.com as octo (/x/hosts.yml)\n";
        assert_eq!(parse_status(old), (AccountStatus::SignedIn, Some("octo".into())));
    }
}
