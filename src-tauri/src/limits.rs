//! Plan limits (5-hour / weekly windows, daily tokens) of one account, as the
//! AI's own CLI reports them. Wren never reads or sends login tokens: it asks
//! the official CLI, which uses its own sign-in.
//!
//! - Claude Code: the `get_usage` control request in stream-json mode (no
//!   prompt is sent, so it costs nothing), plus the `rate_limit_event` lines
//!   it writes during every conversation (parsed in providers.rs).
//! - Codex: `codex app-server` (JSON-RPC over stdio): `account/rateLimits/read`
//!   and `account/usage/read` (tokens per day).
//! - Antigravity: no way to read the quota outside its own `/usage` screen.

use std::process::Stdio;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::events::{claude_label, window_label, DailyTokens, LimitWindow, PlanLimits};
use crate::providers::Provider;

const READ_TIMEOUT: Duration = Duration::from_secs(25);
/// Pause before the one retry when Claude's usage service didn't answer.
const RETRY_PAUSE: Duration = Duration::from_secs(3);

/// Claude Code asked Claude's usage service but got nothing back in time
/// (it waits only ~5 s). Worth one retry; the numbers also arrive with replies.
pub const CLAUDE_NO_ANSWER: &str = "Claude's usage service didn't answer in time (Claude Code waits only 5 seconds, so a slow connection can miss it). The numbers also update by themselves after each reply.";

/// Claude: the windows Wren shows, in order.
const CLAUDE_KEYS: &[&str] = &["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet", "seven_day_oauth_apps"];

// ---------------- parsing (pure, tested) ----------------

/// A Claude `rate_limit_event` line → limits. Utilization there is a
/// fraction (0-1); Wren shows percent. Returns None when the line has
/// nothing usable.
pub fn parse_claude_rate_event(v: &Value) -> Option<PlanLimits> {
    let info = &v["rate_limit_info"];
    if !info.is_object() {
        return None;
    }
    let mut out = PlanLimits { status: info["status"].as_str().map(String::from), ..Default::default() };
    if let Some(w) = info["unifiedWindows"].as_object() {
        for (key, win) in w {
            if let Some(u) = win["utilization"].as_f64() {
                out.windows.push(LimitWindow {
                    key: key.clone(),
                    label: claude_label(key),
                    used_percent: round1(u * 100.0),
                    resets_at: win["resetsAt"].as_i64(),
                });
            }
        }
    } else if let (Some(kind), Some(u)) = (info["rateLimitType"].as_str(), info["utilization"].as_f64()) {
        out.windows.push(LimitWindow {
            key: kind.to_string(),
            label: claude_label(kind),
            used_percent: round1(u * 100.0),
            resets_at: info["resetsAt"].as_i64(),
        });
    }
    sort_windows(&mut out.windows);
    (out.status.is_some() || !out.windows.is_empty()).then_some(out)
}

/// The answer to Claude's `get_usage` control request (utilization 0-100,
/// resets_at ISO 8601).
pub fn parse_claude_usage(resp: &Value) -> Result<PlanLimits, String> {
    if resp["subscription_type"].is_null() {
        return Err("No plan limits for this account: it isn't signed in with a Claude subscription (or uses an API key).".into());
    }
    if resp["rate_limits_available"] == false {
        return Err("This sign-in can't read plan limits (it lacks Claude's profile permission). Sign in to this account again in Accounts. The numbers still update after each reply.".into());
    }
    if resp["rate_limits"].is_null() {
        return Err(CLAUDE_NO_ANSWER.into());
    }
    let rl = &resp["rate_limits"];
    let mut out = PlanLimits { plan: resp["subscription_type"].as_str().map(String::from), ..Default::default() };
    for key in CLAUDE_KEYS {
        let w = &rl[*key];
        if let Some(u) = w["utilization"].as_f64() {
            out.windows.push(LimitWindow {
                key: key.to_string(),
                label: claude_label(key),
                used_percent: round1(u),
                resets_at: w["resets_at"].as_str().and_then(parse_iso),
            });
        }
    }
    for m in rl["model_scoped"].as_array().into_iter().flatten() {
        if let (Some(name), Some(u)) = (m["display_name"].as_str(), m["utilization"].as_f64()) {
            out.windows.push(LimitWindow {
                key: format!("model:{name}"),
                label: format!("Weekly ({name})"),
                used_percent: round1(u),
                resets_at: m["resets_at"].as_str().and_then(parse_iso),
            });
        }
    }
    Ok(out)
}

/// Codex `account/rateLimits/read` (+ optional `account/usage/read`) answers.
pub fn parse_codex(rate: &Value, usage: Option<&Value>) -> PlanLimits {
    let main = &rate["rateLimits"];
    let mut out = PlanLimits {
        plan: main["planType"].as_str().map(String::from),
        status: Some(if !main["rateLimitReachedType"].is_null() || rate["ordinaryUsageAllowed"] == false {
            "rejected".into()
        } else {
            "allowed".into()
        }),
        ..Default::default()
    };
    push_codex_snapshot(&mut out.windows, main, None);
    let main_id = main["limitId"].as_str();
    if let Some(by_id) = rate["rateLimitsByLimitId"].as_object() {
        for (id, snap) in by_id {
            if Some(id.as_str()) == main_id {
                continue;
            }
            let name = snap["limitName"].as_str().unwrap_or(id).to_string();
            push_codex_snapshot(&mut out.windows, snap, Some(&name));
        }
    }
    sort_windows(&mut out.windows);
    if let Some(u) = usage {
        let days: Vec<DailyTokens> = u["dailyUsageBuckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| {
                let date = b["startDate"].as_str()?.chars().take(10).collect();
                let tokens = b["tokens"].as_u64().or_else(|| b["tokens"].as_str().and_then(|s| s.parse().ok()))?;
                Some(DailyTokens { date, tokens })
            })
            .collect();
        if !days.is_empty() {
            out.daily = Some(days);
        }
    }
    out
}

fn push_codex_snapshot(out: &mut Vec<LimitWindow>, snap: &Value, name: Option<&str>) {
    for slot in ["primary", "secondary"] {
        let w = &snap[slot];
        let Some(pct) = w["usedPercent"].as_f64() else { continue };
        let (key, label) = window_label(w["windowDurationMins"].as_i64());
        let (key, label) = match name {
            Some(n) => (format!("{n}:{key}"), format!("{label} ({n})")),
            None => (key, label),
        };
        out.push(LimitWindow { key, label, used_percent: round1(pct), resets_at: w["resetsAt"].as_i64() });
    }
}

/// Shorter windows first (5-hour before weekly), then by label.
fn sort_windows(w: &mut [LimitWindow]) {
    fn rank(k: &str) -> u8 {
        let base = k.rsplit(':').next().unwrap_or(k);
        match base {
            "five_hour" => 0,
            "one_day" => 1,
            "seven_day" => 2,
            _ if k.starts_with("seven_day") => 3,
            _ => 4,
        }
    }
    w.sort_by(|a, b| (k_has_prefix(&a.key), rank(&a.key), &a.label).cmp(&(k_has_prefix(&b.key), rank(&b.key), &b.label)));
}

/// Named extra buckets (Codex "name:window", Claude "model:x") go after the main ones.
fn k_has_prefix(k: &str) -> bool {
    k.contains(':')
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

/// Minimal ISO 8601 → unix seconds ("2026-11-24T05:00:00Z", "...+07:00", fractions ok).
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || (b[10] != b'T' && b[10] != b' ') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut rest = &s[19..];
    if let Some(r) = rest.strip_prefix('.') {
        rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        r if (r.starts_with('+') || r.starts_with('-')) && r.len() >= 5 => {
            let sign = if r.starts_with('-') { -1 } else { 1 };
            let hh: i64 = r.get(1..3)?.parse().ok()?;
            let mm: i64 = r.get(r.len() - 2..)?.parse().ok()?;
            sign * (hh * 3600 + mm * 60)
        }
        _ => return None,
    };
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se - offset)
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

// ---------------- reading from the CLIs ----------------

pub async fn read(provider: Provider, env: &[(String, String)]) -> Result<PlanLimits, String> {
    match provider {
        Provider::Claude => match read_claude(env).await {
            // Claude's usage service didn't answer: try once more (a fresh process).
            Err(e) if e == CLAUDE_NO_ANSWER => {
                tokio::time::sleep(RETRY_PAUSE).await;
                read_claude(env).await
            }
            r => r,
        },
        Provider::Codex => read_codex(env).await,
        Provider::Antigravity => Err(
            "Antigravity only shows its quota inside its own /usage screen. Use \"Open /usage\" to see it.".into(),
        ),
    }
}

fn command(provider: Provider, env: &[(String, String)]) -> Result<Command, String> {
    let path = crate::sessions::search_path();
    let bin = which::which_in(provider.binary(), Some(&path), std::env::temp_dir())
        .map_err(|_| format!("The `{}` CLI isn't installed.", provider.binary()))?;
    let mut cmd = Command::new(bin);
    cmd.current_dir(std::env::temp_dir())
        .env("PATH", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    for (k, v) in provider.env() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000);
    Ok(cmd)
}

async fn read_claude(env: &[(String, String)]) -> Result<PlanLimits, String> {
    let mut cmd = command(Provider::Claude, env)?;
    cmd.args([
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--no-session-persistence",
    ]);
    let mut child = cmd.spawn().map_err(|e| format!("Could not start claude: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let req = json!({
        "type": "control_request",
        "request_id": "wren-usage",
        "request": {"subtype": "get_usage", "skip_behaviors": true}
    });
    stdin.write_all(format!("{req}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    stdin.flush().await.map_err(|e| e.to_string())?;

    let answer = tokio::time::timeout(READ_TIMEOUT, async {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            if v["type"] == "control_response" && v["response"]["request_id"] == "wren-usage" {
                return Some(v);
            }
        }
        None
    })
    .await;
    drop(stdin);
    let _ = child.kill().await;
    let v = answer
        .map_err(|_| "Claude Code took too long to answer.".to_string())?
        .ok_or("Claude Code ended without answering. Is it up to date?")?;
    let r = &v["response"];
    if r["subtype"] == "error" {
        return Err(r["error"].as_str().unwrap_or("Claude Code could not read usage.").to_string());
    }
    parse_claude_usage(&r["response"])
}

async fn read_codex(env: &[(String, String)]) -> Result<PlanLimits, String> {
    let mut cmd = command(Provider::Codex, env)?;
    cmd.args(["app-server", "-c", "analytics.enabled=false", "-c", "feedback.enabled=false"]);
    let mut child = cmd.spawn().map_err(|e| format!("Could not start codex: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let msgs = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
               "params": {"clientInfo": {"name": "wren", "title": "Wren", "version": env!("CARGO_PKG_VERSION")}}}),
        json!({"jsonrpc": "2.0", "method": "initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "account/rateLimits/read"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "account/usage/read"}),
    ];
    for m in msgs {
        stdin.write_all(format!("{m}\n").as_bytes()).await.map_err(|e| e.to_string())?;
    }
    stdin.flush().await.map_err(|e| e.to_string())?;

    let answers = tokio::time::timeout(READ_TIMEOUT, async {
        let (mut rate, mut usage): (Option<Value>, Option<Value>) = (None, None);
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
            match v["id"].as_i64() {
                Some(2) => rate = Some(v),
                Some(3) => usage = Some(v),
                _ => {}
            }
            if rate.is_some() && usage.is_some() {
                break;
            }
        }
        (rate, usage)
    })
    .await;
    drop(stdin);
    let _ = child.kill().await;
    let (rate, usage) = answers.map_err(|_| "Codex took too long to answer.".to_string())?;
    let rate = rate.ok_or("Codex ended without answering. Is it up to date?")?;
    if let Some(msg) = rate["error"]["message"].as_str() {
        return Err(if msg.contains("authentication required") {
            "This account isn't signed in to Codex with a ChatGPT plan.".into()
        } else {
            msg.to_string()
        });
    }
    let usage_result = usage.as_ref().map(|u| &u["result"]).filter(|r| r.is_object());
    Ok(parse_codex(&rate["result"], usage_result))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the real CLIs (not signed in) when WREN_REAL_CLI=1.
    #[test]
    fn real_clis_not_signed_in() {
        if std::env::var("WREN_REAL_CLI").is_err() {
            return;
        }
        tauri::async_runtime::block_on(async {
            let tmp = std::env::temp_dir();
            let c = read(Provider::Claude, &[("CLAUDE_CONFIG_DIR".into(), tmp.join("wren-rc").to_string_lossy().into())]).await;
            assert!(c.as_ref().unwrap_err().contains("No plan limits"), "{c:?}");
            let home = std::env::var("HOME").unwrap() + "/wren-cx";
            std::fs::create_dir_all(&home).unwrap();
            let x = read(Provider::Codex, &[("CODEX_HOME".into(), home)]).await;
            assert!(x.as_ref().unwrap_err().contains("isn't signed in"), "{x:?}");
        });
    }

    #[test]
    fn iso_dates() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("2026-11-24T05:00:00.123Z"), Some(1795496400));
        assert_eq!(parse_iso("2026-11-24T12:00:00+07:00"), Some(1795496400));
        assert_eq!(parse_iso("nope"), None);
    }

    #[test]
    fn claude_event_with_windows() {
        let v: Value = serde_json::from_str(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"five_hour","utilization":0.82,"resetsAt":1795496400,"unifiedWindows":{"five_hour":{"utilization":0.82,"resetsAt":1795496400},"seven_day":{"utilization":0.311,"resetsAt":1795900000}}},"uuid":"u","session_id":"s"}"#).unwrap();
        let l = parse_claude_rate_event(&v).unwrap();
        assert_eq!(l.status.as_deref(), Some("allowed_warning"));
        assert_eq!(l.windows.len(), 2);
        assert_eq!(l.windows[0].key, "five_hour");
        assert_eq!(l.windows[0].used_percent, 82.0);
        assert_eq!(l.windows[1].label, "Weekly");
        assert_eq!(l.windows[1].used_percent, 31.1);
    }

    #[test]
    fn claude_event_status_only() {
        let v: Value = serde_json::from_str(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed"}}"#).unwrap();
        let l = parse_claude_rate_event(&v).unwrap();
        assert!(l.windows.is_empty());
        assert_eq!(l.status.as_deref(), Some("allowed"));
    }

    #[test]
    fn claude_get_usage() {
        // Real answer shape (not signed in), captured from Claude Code 2.1.331.
        let none: Value = serde_json::from_str(r#"{"session":{"total_cost_usd":0},"subscription_type":null,"rate_limits_available":false,"rate_limits":null,"behaviors":null}"#).unwrap();
        assert!(parse_claude_usage(&none).is_err());
        let v: Value = serde_json::from_str(r#"{"subscription_type":"max","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":12,"resets_at":"2026-11-24T05:00:00Z"},"seven_day":{"utilization":40.5,"resets_at":"2026-11-28T00:00:00Z"},"seven_day_opus":null,"model_scoped":[{"display_name":"Fable","utilization":7,"resets_at":null}]}}"#).unwrap();
        let l = parse_claude_usage(&v).unwrap();
        assert_eq!(l.plan.as_deref(), Some("max"));
        let keys: Vec<&str> = l.windows.iter().map(|w| w.key.as_str()).collect();
        assert_eq!(keys, ["five_hour", "seven_day", "model:Fable"]);
        assert_eq!(l.windows[0].resets_at, Some(1795496400));
        assert_eq!(l.windows[2].label, "Weekly (Fable)");
    }

    #[test]
    fn claude_get_usage_reasons() {
        // Real answer from a signed-in Pro account on a slow connection (Claude Code 2.1.310).
        let slow: Value = serde_json::from_str(r#"{"subscription_type":"pro","rate_limits_available":true,"rate_limits":null}"#).unwrap();
        assert_eq!(parse_claude_usage(&slow).unwrap_err(), CLAUDE_NO_ANSWER);
        let no_scope: Value = serde_json::from_str(r#"{"subscription_type":"pro","rate_limits_available":false,"rate_limits":null}"#).unwrap();
        assert!(parse_claude_usage(&no_scope).unwrap_err().contains("profile permission"));
    }

    #[test]
    fn claude_real_event_2_1_310() {
        // Exact rate_limit_event from a Pro account (Claude Code 2.1.310, 2026-11-24).
        let v: Value = serde_json::from_str(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","resetsAt":1795515000,"rateLimitType":"five_hour","overageStatus":"rejected","overageDisabledReason":"org_level_disabled","isUsingOverage":false,"unifiedWindows":{"five_hour":{"utilization":0.11,"resetsAt":1795515000,"status":"allowed"},"seven_day":{"utilization":0.18,"resetsAt":1796083200,"status":"allowed"}}},"uuid":"08f1859f-4ed9-43b3-99f5-f3ee4f464194","session_id":"edf789a5-118b-4743-9f8b-a1d8fe43b138"}"#).unwrap();
        let l = parse_claude_rate_event(&v).unwrap();
        let got: Vec<(&str, f64, Option<i64>)> = l.windows.iter().map(|w| (w.key.as_str(), w.used_percent, w.resets_at)).collect();
        assert_eq!(got, [("five_hour", 11.0, Some(1795515000)), ("seven_day", 18.0, Some(1796083200))]);
    }

    #[test]
    fn codex_limits_and_daily() {
        let rate: Value = serde_json::from_str(r#"{"ordinaryUsageAllowed":true,"rateLimits":{"limitId":"codex","limitName":null,"primary":{"usedPercent":23.4,"windowDurationMins":300,"resetsAt":1795496400},"secondary":{"usedPercent":61,"windowDurationMins":10080,"resetsAt":1795900000},"planType":"plus","rateLimitReachedType":null},"rateLimitsByLimitId":{"codex":{},"spark":{"limitId":"spark","limitName":"Spark","primary":{"usedPercent":5,"windowDurationMins":300,"resetsAt":null},"secondary":null}}}"#).unwrap();
        let usage: Value = serde_json::from_str(r#"{"summary":{},"dailyUsageBuckets":[{"startDate":"2026-11-23","tokens":120000},{"startDate":"2026-11-24T00:00:00Z","tokens":"5000"}]}"#).unwrap();
        let l = parse_codex(&rate, Some(&usage));
        assert_eq!(l.plan.as_deref(), Some("plus"));
        assert_eq!(l.status.as_deref(), Some("allowed"));
        let labels: Vec<&str> = l.windows.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, ["5-hour", "Weekly", "5-hour (Spark)"]);
        let d = l.daily.unwrap();
        assert_eq!(d[1], DailyTokens { date: "2026-11-24".into(), tokens: 5000 });
    }

    #[test]
    fn codex_limit_reached() {
        let rate: Value = serde_json::from_str(r#"{"rateLimits":{"primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":1},"secondary":null,"rateLimitReachedType":"rate_limit_reached"},"rateLimitsByLimitId":null}"#).unwrap();
        assert_eq!(parse_codex(&rate, None).status.as_deref(), Some("rejected"));
    }
}
