//! The provider-neutral events Wren sends from Rust to the UI.
//! Every CLI adapter (Claude Code, Codex, Antigravity) turns its own JSON
//! output into these events, so the UI only has to understand one format.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

/// One subscription usage window (e.g. the 5-hour session or the week),
/// as the AI's own CLI reports it.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct LimitWindow {
    /// Stable key: "five_hour", "seven_day", "seven_day_opus", "model:<name>", ...
    pub key: String,
    /// Short label for the UI ("5-hour", "Weekly", "Weekly (Opus)", ...).
    pub label: String,
    /// Share of the window used, 0-100 (can go above 100).
    pub used_percent: f64,
    /// When the window resets (unix seconds), if known.
    pub resets_at: Option<i64>,
}

/// Tokens the AI company counted for this account on one day.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct DailyTokens {
    /// YYYY-MM-DD
    pub date: String,
    pub tokens: u64,
}

/// The plan limits of one account. Read from the CLI; Wren never reads tokens.
#[derive(Serialize, Clone, Debug, PartialEq, Default)]
pub struct PlanLimits {
    pub windows: Vec<LimitWindow>,
    /// Only Codex reports this (account-wide, all devices).
    pub daily: Option<Vec<DailyTokens>>,
    /// "pro", "max", "plus", ... when the CLI says.
    pub plan: Option<String>,
    /// "allowed" | "allowed_warning" | "rejected" when the CLI says.
    pub status: Option<String>,
}

/// Key and label for a window from its length in minutes (Codex reports lengths).
pub fn window_label(mins: Option<i64>) -> (String, String) {
    match mins {
        Some(300) => ("five_hour".into(), "5-hour".into()),
        Some(1440) => ("one_day".into(), "Daily".into()),
        Some(10080) => ("seven_day".into(), "Weekly".into()),
        Some(m) if m > 0 && m % 1440 == 0 => (format!("{}d", m / 1440), format!("{}-day", m / 1440)),
        Some(m) if m > 0 && m % 60 == 0 => (format!("{}h", m / 60), format!("{}-hour", m / 60)),
        Some(m) => (format!("{m}m"), format!("{m}-minute")),
        None => ("window".into(), "Limit".into()),
    }
}

/// Label for a Claude window key.
pub fn claude_label(key: &str) -> String {
    match key {
        "five_hour" => "5-hour".into(),
        "seven_day" => "Weekly".into(),
        "seven_day_opus" => "Weekly (Opus)".into(),
        "seven_day_sonnet" => "Weekly (Sonnet)".into(),
        "seven_day_oauth_apps" => "Weekly (apps)".into(),
        "seven_day_overage_included" => "Weekly (included models)".into(),
        "overage" => "Extra usage".into(),
        other => other.replace('_', " "),
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    /// The CLI reported new plan-limit numbers for the pane's account.
    PlanLimits { limits: PlanLimits },
    /// The model started thinking (shown as "Thinking…" until text arrives).
    Thinking,
    /// The AI asks to do something (Manual mode). The user answers on a card
    /// with Allow / Allow for this chat / Deny (`answer_approval`).
    /// `allow_session` is false when the AI doesn't offer "for this
    /// conversation" for this request (the card then hides that button).
    Approval { id: String, tool: String, title: String, detail: Option<String>, allow_session: bool },
    /// Something the user should know about this turn (shown as a note).
    Notice { text: String },
    /// The CLI reported its own session/conversation id (used for resuming).
    SessionStarted {
        session_id: String,
        model: Option<String>,
    },
    /// A piece of streaming assistant text.
    TextDelta { text: String },
    /// The current assistant text block is finished. `full` (when present)
    /// is the complete text and replaces whatever was streamed.
    TextEnd { full: Option<String> },
    /// The agent called a tool (ran a command, edited a file, ...).
    ToolUse { name: String, summary: String },
    /// Output of a tool call (truncated).
    ToolResult { output: String, is_error: bool },
    /// A turn finished. `cost_usd` is only filled when the CLI reports it
    /// (Claude Code); it is an API-price estimate on subscription plans.
    TurnDone {
        usage: Usage,
        cost_usd: Option<f64>,
        error: Option<String>,
    },
    /// Something went wrong (spawn failure, CLI error event, ...).
    Error { message: String },
    /// A line the CLI wrote to stderr.
    Log { line: String },
    /// The CLI process ended.
    Exited { code: Option<i32> },
}

pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{cut}\n… (truncated)")
}
