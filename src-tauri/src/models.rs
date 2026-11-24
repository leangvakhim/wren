//! Which models and effort tiers each AI offers, with full names
//! ("Opus 5.5", "GPT-6.2-Sol", "Gemini 4 Carbon") read from each CLI.
//! - Claude Code: its own model list (`list_models` control request in
//!   stream-json mode; no prompt is sent). Wren keeps the aliases (opus, ...)
//!   so a pane follows the newest model, and shows the version they point to.
//! - Codex: the CLI's own live catalog (`codex debug models`).
//! - Antigravity: the CLI's own list (`agy models`), with a fallback list.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use serde::Serialize;
use serde_json::Value;

use crate::providers::Provider;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ModelInfo {
    /// Value passed to the CLI (`--model` / `-m`).
    pub id: String,
    pub label: String,
    /// Effort tiers this model accepts (empty = no effort setting).
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    /** One line about the model ("Best for everyday, complex tasks"). */
    pub description: Option<String>,
    /// The exact model an alias points to right now (Claude: opus -> claude-opus-5-5).
    pub resolved: Option<String>,
    /// The model the CLI uses when none is chosen.
    pub is_default: bool,
}

fn m(id: &str, label: &str, efforts: &[&str], default: Option<&str>) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        label: label.into(),
        efforts: efforts.iter().map(|s| s.to_string()).collect(),
        default_effort: default.map(String::from),
        description: None,
        resolved: None,
        is_default: false,
    }
}

fn with(mut x: ModelInfo, description: &str, resolved: &str, is_default: bool) -> ModelInfo {
    x.description = Some(description.into());
    x.resolved = Some(resolved.into());
    x.is_default = is_default;
    x
}

const CLAUDE_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];
const AGY_EFFORTS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

/// Used only when Claude Code can't be asked (not installed / too old).
pub fn claude_models() -> Vec<ModelInfo> {
    // Aliases always point at the newest model of each family.
    vec![
        with(m("opus", "Opus 5.5", CLAUDE_EFFORTS, None), "Best for everyday, complex tasks", "claude-opus-5-5", true),
        with(m("fable", "Fable 5.2", CLAUDE_EFFORTS, None), "Most capable for your hardest and longest-running tasks", "claude-fable-5-2", false),
        with(m("sonnet", "Sonnet 5.5", CLAUDE_EFFORTS, None), "Efficient for routine tasks", "claude-sonnet-5-5", false),
        with(m("haiku", "Haiku 5.5", CLAUDE_EFFORTS, None), "Fastest for quick answers", "claude-haiku-5-5", false),
    ]
}

/// Claude Code's own `list_models` answer -> models with full names.
/// Each entry: value (alias), resolvedModel, displayName ("Opus"),
/// description ("Opus 5.5 · Best for everyday, complex tasks · $4/$20 per Mtok").
pub fn parse_claude_models(resp: &Value) -> Vec<ModelInfo> {
    let list = resp["models"].as_array().cloned().unwrap_or_default();
    let default_resolved = list
        .iter()
        .find(|x| x["value"] == "default")
        .and_then(|x| x["resolvedModel"].as_str())
        .map(String::from);
    list.iter()
        .filter(|x| x["value"] != "default")
        .filter_map(|x| {
            let id = x["value"].as_str()?.to_string();
            let display = x["displayName"].as_str().unwrap_or(&id).to_string();
            let parts: Vec<&str> = x["description"].as_str().unwrap_or("").split(" · ").map(str::trim).collect();
            // "Opus 5.5" when the first part names the model, else the display name
            let first = parts.first().copied().unwrap_or("");
            let has_version = !first.is_empty() && first.to_lowercase().starts_with(&display.to_lowercase());
            let label = if has_version { first.to_string() } else { display.clone() };
            let description: Vec<&str> = parts
                .iter()
                .skip(usize::from(has_version))
                .filter(|p| !p.is_empty() && !p.starts_with('$') && !p.contains("per Mtok"))
                .copied()
                .collect();
            let resolved = x["resolvedModel"].as_str().map(String::from);
            let efforts = if x["supportsEffort"] == false {
                vec![]
            } else {
                x["supportedEffortLevels"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|e| e.as_str().map(String::from)).collect())
                    .unwrap_or_else(|| CLAUDE_EFFORTS.iter().map(|e| e.to_string()).collect())
            };
            Some(ModelInfo {
                is_default: resolved.is_some() && resolved == default_resolved,
                id,
                label,
                efforts,
                default_effort: None,
                description: (!description.is_empty()).then(|| description.join(" · ")),
                resolved,
            })
        })
        .collect()
}

/// Ask Claude Code for its model list (no prompt is sent; works signed out too).
async fn claude_live(env: &[(String, String)]) -> Option<Vec<ModelInfo>> {
    let path = crate::sessions::search_path();
    let bin = which::which_in("claude", Some(&path), std::env::temp_dir()).ok()?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
    ])
    .current_dir(std::env::temp_dir())
    .env("PATH", &path)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    for (k, v) in Provider::Claude.env() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let mut child = cmd.spawn().ok()?;
    let mut stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    let req = r#"{"type":"control_request","request_id":"wren-models","request":{"subtype":"list_models"}}"#;
    stdin.write_all(format!("{req}\n").as_bytes()).await.ok()?;
    stdin.flush().await.ok()?;
    let answer = tokio::time::timeout(Duration::from_secs(20), async {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["type"] == "control_response" && v["response"]["request_id"] == "wren-models" {
                return Some(v);
            }
        }
        None
    })
    .await
    .ok()
    .flatten();
    drop(stdin);
    let _ = child.kill().await;
    let v = answer?;
    let list = parse_claude_models(&v["response"]["response"]);
    (!list.is_empty()).then_some(list)
}

/// "gemini-3.1-pro-high" -> "Gemini 3.1 Pro (High)", "claude-opus-5-5" -> "Claude Opus 5.5".
/// Names that already look like names ("Gemini 4 Carbon") are kept.
pub fn pretty_model_name(id: &str) -> String {
    if id.contains(' ') || id.chars().any(|c| c.is_uppercase()) {
        return id.to_string();
    }
    let mut words: Vec<String> = vec![];
    let mut suffix: Vec<String> = vec![];
    let mut num: Option<String> = None;
    let flush = |num: &mut Option<String>, words: &mut Vec<String>| {
        if let Some(n) = num.take() {
            words.push(n);
        }
    };
    for t in id.split(['-', '_']).filter(|t| !t.is_empty()) {
        if t.chars().all(|c| c.is_ascii_digit() || c == '.') {
            // consecutive number parts are one version: 5-5 -> 5.5
            num = Some(match num.take() {
                Some(n) if !n.contains('.') && !t.contains('.') => format!("{n}.{t}"),
                Some(n) => {
                    words.push(n);
                    t.to_string()
                }
                None => t.to_string(),
            });
            continue;
        }
        flush(&mut num, &mut words);
        let w = match t {
            "gpt" | "oss" | "ai" => t.to_uppercase(),
            "low" | "medium" | "high" | "xhigh" | "max" | "minimal" | "thinking" if !words.is_empty() => {
                suffix.push(match t {
                    "xhigh" => "Extra high".into(),
                    _ => capitalize(t),
                });
                continue;
            }
            _ if t.ends_with('b') && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit()) && t.len() > 1 => t.to_uppercase(),
            _ => capitalize(t),
        };
        words.push(w);
    }
    flush(&mut num, &mut words);
    let mut out = words.join(" ");
    if !suffix.is_empty() {
        out.push_str(&format!(" ({})", suffix.join(", ")));
    }
    if out.is_empty() { id.to_string() } else { out }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

pub fn agy_fallback() -> Vec<ModelInfo> {
    [
        "Gemini 4 Carbon",
        "Gemini 4 Argon",
        "Gemini 4 Hydrogen",
        "Gemini 3.8 Flash",
        "Claude Opus 5.5 (thinking)",
        "Claude Sonnet 5.5 (thinking)",
    ]
    .iter()
    .map(|n| m(n, n, AGY_EFFORTS, None))
    .collect()
}

/// Parse `codex debug models` JSON: only models meant to be listed, in
/// Codex's own order (`priority`; the first is its default unless
/// config.toml picks another).
pub fn parse_codex_catalog(json: &str) -> Vec<ModelInfo> {
    let Ok(v) = serde_json::from_str::<Value>(json) else { return vec![] };
    let mut list: Vec<(i64, ModelInfo)> = v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|x| x["visibility"].as_str().unwrap_or("list") == "list")
        .filter_map(|x| {
            let id = x["slug"].as_str()?.to_string();
            let label = x["display_name"].as_str().unwrap_or(&id).to_string();
            let efforts = x["supported_reasoning_levels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| e["effort"].as_str().map(String::from))
                .collect();
            let info = ModelInfo {
                id,
                label,
                efforts,
                default_effort: x["default_reasoning_level"].as_str().map(String::from),
                description: x["description"].as_str().filter(|d| !d.is_empty()).map(String::from),
                resolved: None,
                is_default: false,
            };
            Some((x["priority"].as_i64().unwrap_or(i64::MAX), info))
        })
        .collect();
    list.sort_by_key(|(p, _)| *p); // stable: equal priorities keep the CLI's order
    let mut out: Vec<ModelInfo> = list.into_iter().map(|(_, x)| x).collect();
    if let Some(first) = out.first_mut() {
        first.is_default = true;
    }
    out
}

/// Parse `agy models`: one model per line (bullets, numbering and
/// "Available models:" style headers are ignored).
pub fn parse_agy_models(text: &str) -> Vec<ModelInfo> {
    let plain = String::from_utf8_lossy(&strip_ansi_escapes::strip(text.as_bytes())).to_string();
    let mut out: Vec<ModelInfo> = vec![];
    for line in plain.lines() {
        let l = line.trim().trim_start_matches(['-', '*', '•', '>', ' ']).trim();
        let l = l.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ')').trim();
        if l.is_empty() || l.ends_with(':') || l.len() > 80 || l.to_lowercase().starts_with("available") {
            continue;
        }
        // "id   description" (two or more spaces between columns)
        let mut cols = l.splitn(2, "  ");
        let id = cols.next().unwrap_or(l).trim().to_string();
        let desc = cols.next().map(str::trim).filter(|d| !d.is_empty()).map(String::from);
        if !out.iter().any(|x| x.id == id) {
            // An id that already ends with an effort ("...-high") has no separate effort setting.
            let effort_in_id = ["-low", "-medium", "-high", "-xhigh", "-max", "-minimal"].iter().any(|e| id.ends_with(e));
            let mut x = m(&id, &pretty_model_name(&id), if effort_in_id { &[] } else { AGY_EFFORTS }, None);
            x.description = desc;
            out.push(x);
        }
    }
    out
}

pub async fn list_models(p: Provider, env: &[(String, String)]) -> Vec<ModelInfo> {
    match p {
        Provider::Claude => claude_live(env).await.unwrap_or_else(claude_models),
        Provider::Codex => {
            let mut list = match run(p, &["debug", "models"], env).await {
                Some(out) => parse_codex_catalog(&out),
                None => vec![],
            };
            if list.is_empty()
                && let Some(out) = run(p, &["debug", "models", "--bundled"], env).await {
                    list = parse_codex_catalog(&out);
                }
            list
        }
        Provider::Antigravity => {
            let list = run(p, &["models"], env).await.map(|o| parse_agy_models(&o)).unwrap_or_default();
            if list.is_empty() { agy_fallback() } else { list }
        }
    }
}

async fn run(p: Provider, args: &[&str], env: &[(String, String)]) -> Option<String> {
    let path = crate::sessions::search_path();
    let bin = which::which_in(p.binary(), Some(&path), std::env::temp_dir()).ok()?;
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .env("PATH", &path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (k, v) in p.env() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    let out = tokio::time::timeout(Duration::from_secs(25), cmd.output()).await.ok()?.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_catalog() {
        let json = r#"{"models":[
          {"slug":"gpt-6.2-sol","display_name":"GPT-6.2-Sol","visibility":"list","default_reasoning_level":"low",
           "supported_reasoning_levels":[{"effort":"low"},{"effort":"high"},{"effort":"ultra"}]},
          {"slug":"codex-auto-review","display_name":"Codex Auto Review","visibility":"hide","supported_reasoning_levels":[]}
        ]}"#;
        let l = parse_codex_catalog(json);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].id, "gpt-6.2-sol");
        assert_eq!(l[0].efforts, vec!["low", "high", "ultra"]);
        assert_eq!(l[0].default_effort.as_deref(), Some("low"));
        assert!(l[0].is_default);
    }

    #[test]
    fn claude_live_list() {
        // Real answer from Claude Code 2.1.331 (shortened).
        let v: Value = serde_json::from_str(r#"{"models":[
          {"value":"default","resolvedModel":"claude-opus-5-5","displayName":"Default (recommended)","description":"Use the default model (currently Opus 5.5) · $4/$20 per Mtok","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},
          {"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus","description":"Opus 5.5 · Best for everyday, complex tasks · $4/$20 per Mtok","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},
          {"value":"fable","resolvedModel":"claude-fable-5-2","displayName":"Fable","description":"Fable 5.2 · Most capable for your hardest and longest-running tasks · $8/$40 per Mtok","supportsEffort":true,"supportedEffortLevels":["low","medium","high","xhigh","max"]},
          {"value":"haiku","resolvedModel":"claude-haiku-5-5","displayName":"Haiku","description":"Haiku 5.5 · Fastest for quick answers · $0.10/$0.50 per Mtok ($0.50/$2.50 for prompts over 100k)","supportsEffort":false}
        ]}"#).unwrap();
        let l = parse_claude_models(&v);
        let labels: Vec<&str> = l.iter().map(|x| x.label.as_str()).collect();
        assert_eq!(labels, ["Opus 5.5", "Fable 5.2", "Haiku 5.5"]);
        assert_eq!(l[0].id, "opus", "the alias is kept, so the pane follows the newest Opus");
        assert_eq!(l[0].resolved.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(l[0].description.as_deref(), Some("Best for everyday, complex tasks"));
        assert!(l[0].is_default && !l[1].is_default);
        assert_eq!(l[2].description.as_deref(), Some("Fastest for quick answers"), "prices are dropped");
        assert!(l[2].efforts.is_empty());
    }

    #[test]
    fn pretty_names() {
        assert_eq!(pretty_model_name("gemini-3.1-pro-high"), "Gemini 3.1 Pro (High)");
        assert_eq!(pretty_model_name("claude-opus-5-5-thinking"), "Claude Opus 5.5 (Thinking)");
        assert_eq!(pretty_model_name("gpt-oss-120b"), "GPT OSS 120B");
        assert_eq!(pretty_model_name("gemini-4-carbon"), "Gemini 4 Carbon");
        assert_eq!(pretty_model_name("Gemini 4 Argon"), "Gemini 4 Argon");
    }

    #[test]
    fn real_claude_model_list() {
        if std::env::var("WREN_REAL_CLI").is_err() {
            return;
        }
        let l = tauri::async_runtime::block_on(claude_live(&[])).expect("claude answered");
        assert!(l.iter().any(|x| x.id == "opus" && x.label.starts_with("Opus ") && x.resolved.is_some()), "{l:?}");
    }

    #[test]
    fn agy_list() {
        let l = parse_agy_models("Available models:\n  - gemini-3.8-flash\n  - Gemini 4 Argon    Efficient frontier reasoning\n\n  - gemini-3.8-flash\n");
        let ids: Vec<_> = l.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["gemini-3.8-flash", "Gemini 4 Argon"]);
        assert_eq!(l[0].label, "Gemini 3.8 Flash");
        assert_eq!(l[1].description.as_deref(), Some("Efficient frontier reasoning"));
        let e = parse_agy_models("gemini-3.1-pro-high\n");
        assert_eq!(e[0].label, "Gemini 3.1 Pro (High)");
        assert!(e[0].efforts.is_empty(), "effort is part of the id");
    }
}
