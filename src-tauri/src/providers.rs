//! CLI adapters: how to launch each official CLI in its headless JSON mode,
//! and how to turn its output lines into `UiEvent`s.
//!
//! Wren only ever runs the unmodified, official binaries and never reads
//! their login tokens.
//!
//! - Claude Code: one long-lived process per conversation
//!   (`claude -p --input-format stream-json --output-format stream-json`).
//! - Antigravity CLI: one long-lived process per conversation
//!   (`agy --input-format stream-json --output-format stream-json`).
//! - Codex CLI: one process per turn (`codex exec --json`, then
//!   `codex exec --json resume <thread_id>`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::attachments::{human_size, Outgoing};
use crate::events::{truncate, UiEvent, Usage};
use crate::link::{Link, Pending};

const TOOL_OUTPUT_MAX: usize = 4000;

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Claude,
    Codex,
    Antigravity,
}

/// How much the agent may do without asking (like Claude Desktop's
/// Manual / Auto). Older saved values "ask" and "edits" still load.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// The AI asks before anything risky; the user allows or denies each
    /// request on a card (Claude Code, Codex). Antigravity can't ask in
    /// headless mode, so its requests are refused with "Allow & retry".
    #[serde(alias = "ask")]
    Manual,
    /// The AI's own auto mode decides (Claude: auto-mode classifier; Codex:
    /// its auto reviewer; Antigravity: auto mode). Nothing is asked.
    #[serde(alias = "edits")]
    Auto,
    /// Research and propose a plan, change nothing.
    Plan,
    /// Everything allowed, including commands, with no checks. Dangerous.
    Full,
}

/// Codex app-server thread settings for a permission level:
/// (approvalPolicy, sandbox, approvalsReviewer).
pub fn codex_policy(perm: Permission) -> (&'static str, &'static str, Option<&'static str>) {
    match perm {
        // Asks for anything that isn't a known-safe read; edits and commands need a yes.
        Permission::Manual => ("untrusted", "read-only", Some("user")),
        // Works in the project folder; anything more goes to Codex's own auto reviewer.
        Permission::Auto => ("on-request", "workspace-write", Some("auto_review")),
        Permission::Plan => ("never", "read-only", None),
        Permission::Full => ("never", "danger-full-access", None),
    }
}

/// A special way of working, chosen with the "+" menu in the message box.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Tutor: explain step by step, hints before answers, check understanding.
    Study,
    /// Search the web widely and write a cited report.
    Research,
}

pub const STUDY_PROMPT: &str = "Study mode is on. Act as a patient tutor, not just a problem solver:\n\
- First find out (or infer) what the learner already knows, and pitch the explanation at that level.\n\
- Explain step by step in plain language, with a small example for each new idea.\n\
- When the learner asks for the answer to an exercise or a bug fix, give a hint or a guiding question first, and give the full answer only if they ask again or are stuck.\n\
- For code, explain why it works, point out the key concept, and invite the learner to write small parts themselves.\n\
- End each answer with one short question that checks understanding or suggests the next thing to try.\n\
- Keep answers focused; never invent facts. If unsure, say so.";

pub const RESEARCH_PROMPT: &str = "Deep research mode is on. Treat the request as a research task:\n\
1. Plan: restate the question and list the sub-questions you need to answer.\n\
2. Search widely: use web search and fetch many independent, reliable sources (official docs, papers, primary sources, reputable news). Prefer recent sources; note dates. Where you can, research sub-questions in parallel with sub-agents.\n\
3. Check: compare sources against each other, note where they disagree, and never state something no source supports.\n\
4. Write the final answer as a complete Markdown report: start with a '# ' title line, then a short summary, then sections with findings, then 'Open questions' if any, then a numbered 'Sources' list with title and URL. Cite sources inline as [1], [2].\n\
Do not create or edit files for the report: Wren saves your final answer as a Markdown file in the project folder automatically. Local files in the project may be read when they are relevant.";

impl Mode {
    pub fn prompt(self) -> &'static str {
        match self {
            Mode::Study => STUDY_PROMPT,
            Mode::Research => RESEARCH_PROMPT,
        }
    }

    /// For CLIs without a system-prompt flag (Antigravity): the instructions
    /// go in front of each message.
    pub fn wrap_message(self, text: &str) -> String {
        format!("<wren_mode>\n{}\n</wren_mode>\n\n{text}", self.prompt())
    }
}

impl Provider {
    pub fn binary(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Codex => "codex",
            Provider::Antigravity => "agy",
        }
    }

    /// Name shown to people.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude Code",
            Provider::Codex => "Codex",
            Provider::Antigravity => "Antigravity",
        }
    }

    /// Command-line arguments. `resume` is the CLI's own session id (Claude,
    /// Antigravity; Codex resumes through its startup messages).
    /// Every CLI runs as one long-lived process per conversation and reads
    /// each new user message from stdin.
    pub fn args(
        self,
        perm: Permission,
        resume: Option<&str>,
        model: Option<&str>,
        effort: Option<&str>,
        mode: Option<Mode>,
    ) -> Vec<String> {
        let model = model.filter(|m| !m.is_empty());
        // Deep research thinks harder by default (the user's choice wins).
        let effort = effort
            .filter(|e| !e.is_empty())
            .or((mode == Some(Mode::Research) && self != Provider::Antigravity).then_some("high"));
        let mut a: Vec<String> = Vec::new();
        let mut push = |s: &str| a.push(s.to_string());
        match self {
            Provider::Claude => {
                for s in [
                    "-p",
                    "--input-format",
                    "stream-json",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                ] {
                    push(s);
                }
                // Manual / Plan: Claude Code sends Wren a `can_use_tool` request
                // for anything that needs approval, and the user answers it on a
                // card. Auto / Full: nothing is asked; anything that would ask is
                // refused at once instead of waiting.
                let (pmode, prompts) = match perm {
                    Permission::Manual => (Some("default"), "stdio"),
                    Permission::Plan => (Some("plan"), "stdio"),
                    Permission::Auto => (Some("auto"), "none"),
                    Permission::Full => (None, "none"),
                };
                match pmode {
                    Some(m) => {
                        push("--permission-mode");
                        push(m);
                    }
                    None => push("--dangerously-skip-permissions"),
                }
                if prompts == "stdio" {
                    push("--permission-prompt-tool");
                    push("stdio");
                } else {
                    push("--permission-prompts");
                    push("none");
                }
                if let Some(id) = resume {
                    push("--resume");
                    push(id);
                }
                if let Some(m) = model {
                    push("--model");
                    push(m);
                }
                if let Some(e) = effort {
                    push("--effort");
                    push(e);
                }
                match mode {
                    Some(Mode::Study) => {
                        // Claude Code's own built-in "Learning" output style.
                        push("--settings");
                        push(r#"{"outputStyle":"Learning"}"#);
                        push("--append-system-prompt");
                        push(STUDY_PROMPT);
                    }
                    Some(Mode::Research) => {
                        push("--append-system-prompt");
                        push(RESEARCH_PROMPT);
                        // Web tools and sub-agents may run without asking,
                        // even in Read only / Plan.
                        push("--allowedTools");
                        push("WebSearch WebFetch Agent");
                    }
                    None => {}
                }
            }
            Provider::Antigravity => {
                for s in ["--input-format", "stream-json", "--output-format", "stream-json"] {
                    push(s);
                }
                // Antigravity can't ask in headless mode: in Manual ("default",
                // request-review) anything needing approval is refused, and
                // Wren offers "Allow & retry".
                match perm {
                    Permission::Manual => {
                        push("--mode");
                        push("default")
                    }
                    Permission::Plan => {
                        push("--mode");
                        push("plan")
                    }
                    Permission::Auto => {
                        push("--mode");
                        push("auto")
                    }
                    Permission::Full => push("--dangerously-skip-permissions"),
                }
                if let Some(id) = resume {
                    push("--conversation");
                    push(id);
                }
                if let Some(m) = model {
                    push("--model");
                    push(m);
                }
                if let Some(e) = effort {
                    push("--effort");
                    push(e);
                }
            }
            Provider::Codex => {
                // Codex app-server (JSON-RPC over stdio): the only Codex
                // interface that can ask the user to approve actions.
                push("app-server");
                // Privacy: no client analytics, no feedback uploads.
                for s in ["-c", "analytics.enabled=false", "-c", "feedback.enabled=false"] {
                    push(s);
                }
                // -c values are TOML; a JSON string is also a valid TOML string.
                let toml = |x: &str| serde_json::to_string(x).unwrap_or_default();
                if let Some(m) = model {
                    push("-c");
                    push(&format!("model={}", toml(m)));
                }
                if let Some(e) = effort {
                    push("-c");
                    push(&format!("model_reasoning_effort={}", toml(e)));
                }
                if let Some(m) = mode {
                    push("-c");
                    push(&format!("developer_instructions={}", toml(m.prompt())));
                    if m == Mode::Research {
                        push("-c");
                        push("web_search=\"live\"");
                    }
                }
            }
        }
        a
    }

    /// Extra environment variables. Privacy: turn off non-essential traffic
    /// (telemetry, error reporting, surveys) where the CLI supports it.
    pub fn env(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Provider::Claude => vec![
                ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1"),
                ("DISABLE_TELEMETRY", "1"),
                ("DISABLE_ERROR_REPORTING", "1"),
                ("CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY", "1"),
            ],
            // Codex: analytics/feedback are turned off with -c flags (see args).
            // Antigravity: telemetry is a setting inside agy (/config ->
            // Enable Telemetry); Wren's Privacy window explains how.
            Provider::Codex | Provider::Antigravity => vec![],
        }
    }

    /// Whether the CLI needs a startup handshake before it accepts messages.
    pub fn needs_handshake(self) -> bool {
        matches!(self, Provider::Codex)
    }

    /// Lines written to stdin right after the CLI starts (Codex: initialize,
    /// then start or resume the thread). The thread answer has id "thread".
    pub fn startup_messages(self, perm: Permission, cwd: &str, resume: Option<&str>) -> Vec<String> {
        if self != Provider::Codex {
            return vec![];
        }
        let mut out = vec![
            json!({"jsonrpc": "2.0", "id": "init", "method": "initialize", "params": {
                "clientInfo": {"name": "wren", "title": "Wren", "version": env!("CARGO_PKG_VERSION")},
                "capabilities": {"experimentalApi": false, "requestAttestation": false}
            }}),
            json!({"jsonrpc": "2.0", "method": "initialized"}),
        ];
        out.push(match resume {
            Some(id) => codex_thread_request("thread/resume", perm, cwd, Some(id)),
            None => codex_thread_request("thread/start", perm, cwd, None),
        });
        out.into_iter().map(|v| format!("{v}\n")).collect()
    }

    /// One user message on stdin, with any attached files.
    /// Codex needs its thread id and a request id.
    pub fn stdin_message(self, text: &str, files: &[Outgoing], thread_id: Option<&str>, request_no: u64) -> String {
        let v = match self {
            Provider::Claude => {
                // Pictures and PDFs inline (first, as in Claude's own apps); other files by path.
                let mut content: Vec<Value> = vec![];
                for f in files {
                    if f.is_inline_image() {
                        content.push(json!({"type": "image", "source": {"type": "base64", "media_type": f.mime, "data": f.inline_base64}}));
                    } else if f.is_inline_pdf() {
                        content.push(json!({"type": "document", "source": {"type": "base64", "media_type": "application/pdf", "data": f.inline_base64}, "title": f.name}));
                    }
                }
                let by_path: Vec<&Outgoing> = files.iter().filter(|f| !f.is_inline_image() && !f.is_inline_pdf()).collect();
                let text = with_file_list(text, &by_path);
                if !text.is_empty() || content.is_empty() {
                    content.push(json!({"type": "text", "text": text}));
                }
                json!({"type": "user", "message": {"role": "user", "content": content}})
            }
            Provider::Antigravity => {
                // No file input in headless mode: `@path` pulls a file into context.
                let text = if files.is_empty() {
                    text.to_string()
                } else {
                    let refs: Vec<String> = files.iter().map(|f| format!("@{}", f.path)).collect();
                    let n = files.len();
                    format!("{text}\n\nAttached file{}: {}", if n == 1 { "" } else { "s" }, refs.join(" ")).trim_start().to_string()
                };
                json!({"event": "user", "message": {"content": text}})
            }
            Provider::Codex => {
                let mut input: Vec<Value> = files
                    .iter()
                    .filter(|f| f.is_inline_image())
                    .map(|f| json!({"type": "localImage", "path": f.path}))
                    .collect();
                let by_path: Vec<&Outgoing> = files.iter().filter(|f| !f.is_inline_image()).collect();
                let text = with_file_list(text, &by_path);
                if !text.is_empty() || input.is_empty() {
                    input.push(json!({"type": "text", "text": text, "text_elements": []}));
                }
                json!({"jsonrpc": "2.0", "id": format!("turn-{request_no}"), "method": "turn/start", "params": {
                    "threadId": thread_id.unwrap_or_default(),
                    "input": input
                }})
            }
        };
        format!("{v}\n")
    }
}

/// The message text plus a list of attached files the AI opens with its own tools.
fn with_file_list(text: &str, files: &[&Outgoing]) -> String {
    if files.is_empty() {
        return text.to_string();
    }
    let list: Vec<String> = files.iter().map(|f| format!("- {} ({}, {})", f.path, human_size(f.size), f.mime)).collect();
    format!(
        "{text}\n\n[Attached file{} (copies saved on this computer; open them with your tools):\n{}]",
        if files.len() == 1 { "" } else { "s" },
        list.join("\n")
    )
    .trim_start()
    .to_string()
}

/// `thread/start` or `thread/resume` with the pane's folder and permission.
pub fn codex_thread_request(method: &str, perm: Permission, cwd: &str, thread_id: Option<&str>) -> Value {
    let (approval, sandbox, reviewer) = codex_policy(perm);
    let mut params = json!({"cwd": cwd, "approvalPolicy": approval, "sandbox": sandbox});
    if let Some(r) = reviewer {
        params["approvalsReviewer"] = json!(r);
    }
    if let Some(id) = thread_id {
        params["threadId"] = json!(id);
    }
    json!({"jsonrpc": "2.0", "id": "thread", "method": method, "params": params})
}

/// Turns one provider's output lines into UiEvents.
pub struct Parser {
    provider: Provider,
    /// Shared with the session so it can resume later.
    session_id: Arc<Mutex<Option<String>>>,
    /// Antigravity reports cumulative usage per session; we keep the last
    /// value to turn it into per-turn numbers.
    last_cumulative: Usage,
    /// Answers Wren must write back to the CLI's stdin (requests it can't
    /// show, queued messages), so the CLI never waits on Wren.
    replies: Vec<String>,
    /// Shared with the session: approval requests waiting for the user,
    /// and messages held back until the CLI is ready.
    link: Link,
    /// Codex: if resuming the old thread fails, start a new one with this.
    fallback_start: Option<String>,
    /// Codex: what each running item is (for approval cards).
    codex_items: HashMap<String, String>,
    /// Codex: latest cumulative token usage of the thread.
    codex_total: Usage,
}

impl Parser {
    pub fn new(provider: Provider, session_id: Arc<Mutex<Option<String>>>, link: Link, fallback_start: Option<String>) -> Self {
        Self {
            provider,
            session_id,
            last_cumulative: Usage::default(),
            replies: vec![],
            link,
            fallback_start,
            codex_items: HashMap::new(),
            codex_total: Usage::default(),
        }
    }

    /// Show an approval card and remember what the CLI is waiting for.
    fn ask(&mut self, id: String, pending: Pending, tool: &str, title: String, detail: Option<String>) -> UiEvent {
        let allow_session = match &pending {
            Pending::Claude { .. } => true,
            Pending::Codex { session_ok, .. } => *session_ok,
        };
        if let Ok(mut l) = self.link.lock() {
            l.pending.insert(id.clone(), pending);
        }
        UiEvent::Approval { id, tool: tool.to_string(), title: truncate(&title, 600), detail: detail.map(|d| truncate(&d, 4000)), allow_session }
    }

    /// Lines to write to the CLI's stdin, produced by the last `parse` calls.
    pub fn take_replies(&mut self) -> Vec<String> {
        std::mem::take(&mut self.replies)
    }

    fn set_session(&self, id: &str) {
        if let Ok(mut g) = self.session_id.lock() {
            *g = Some(id.to_string());
        }
    }

    pub fn parse(&mut self, line: &str) -> Vec<UiEvent> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            // Not JSON: show it as a log line rather than dropping it.
            return vec![UiEvent::Log { line: line.to_string() }];
        };
        match self.provider {
            Provider::Claude => self.parse_claude(&v),
            Provider::Codex => self.parse_codex(&v),
            Provider::Antigravity => self.parse_agy(&v),
        }
    }

    // ---------------- Claude Code ----------------
    fn parse_claude(&mut self, v: &Value) -> Vec<UiEvent> {
        let mut out = vec![];
        match v["type"].as_str().unwrap_or_default() {
            "system" if v["subtype"] == "init" => {
                if let Some(id) = v["session_id"].as_str() {
                    self.set_session(id);
                    out.push(UiEvent::SessionStarted {
                        session_id: id.to_string(),
                        model: v["model"].as_str().map(String::from),
                    });
                }
            }
            "stream_event" => {
                let ev = &v["event"];
                if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta"
                    && let Some(t) = ev["delta"]["text"].as_str() {
                        out.push(UiEvent::TextDelta { text: t.to_string() });
                    }
                if ev["type"] == "content_block_start"
                    && matches!(ev["content_block"]["type"].as_str(), Some("thinking" | "redacted_thinking"))
                {
                    out.push(UiEvent::Thinking);
                }
            }
            // Claude Code asks the program that started it (Wren) something.
            // Tool approvals become cards; anything else is answered at once so
            // the CLI never waits forever.
            "control_request" => {
                let id = v["request_id"].as_str().unwrap_or_default().to_string();
                let req = &v["request"];
                let sub = req["subtype"].as_str().unwrap_or("unknown");
                if sub == "can_use_tool" {
                    let tool = req["tool_name"].as_str().unwrap_or("tool").to_string();
                    let input = req["input"].clone();
                    if tool == "AskUserQuestion" {
                        // Wren can't show Claude's question forms: ask in the reply instead.
                        self.replies.push(format!(
                            "{}\n",
                            json!({"type": "control_response", "response": {"subtype": "success", "request_id": id,
                                "response": {"behavior": "deny", "message": "This app can't show question forms. Ask the user your questions in plain text in your reply instead."}}})
                        ));
                        return out;
                    }
                    let (title, detail) = if tool == "ExitPlanMode" {
                        ("Claude's plan is ready. Start making changes?".to_string(), input["plan"].as_str().map(String::from))
                    } else {
                        let what = summarize_input(&input);
                        let why = req["description"].as_str().or(req["title"].as_str()).map(String::from);
                        (if what.is_empty() { format!("Use {tool}") } else { what }, why.filter(|w| !w.is_empty()))
                    };
                    let display = req["display_name"].as_str().unwrap_or(&tool).to_string();
                    let suggestions = req["permission_suggestions"].as_array().cloned().unwrap_or_default();
                    let pending = Pending::Claude { request_id: id.clone(), tool: tool.clone(), input, suggestions };
                    out.push(self.ask(id, pending, &display, title, detail));
                } else {
                    self.replies.push(format!(
                        "{}\n",
                        json!({"type": "control_response", "response": {"subtype": "error", "request_id": id,
                            "error": format!("Wren does not support the '{sub}' request")}})
                    ));
                    out.push(UiEvent::Notice { text: format!("Claude Code asked Wren for \"{sub}\", which Wren doesn't support, so it was declined.") });
                }
            }
            "assistant" => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    match block["type"].as_str().unwrap_or_default() {
                        "text" => out.push(UiEvent::TextEnd {
                            full: block["text"].as_str().map(String::from),
                        }),
                        "tool_use" => out.push(UiEvent::ToolUse {
                            name: block["name"].as_str().unwrap_or("tool").to_string(),
                            summary: summarize_input(&block["input"]),
                        }),
                        _ => {}
                    }
                }
            }
            "user" => {
                for block in v["message"]["content"].as_array().into_iter().flatten() {
                    if block["type"] == "tool_result" {
                        out.push(UiEvent::ToolResult {
                            output: truncate(&content_to_text(&block["content"]), TOOL_OUTPUT_MAX),
                            is_error: block["is_error"].as_bool().unwrap_or(false),
                        });
                    }
                }
            }
            "rate_limit_event" => {
                if let Some(limits) = crate::limits::parse_claude_rate_event(v) {
                    out.push(UiEvent::PlanLimits { limits });
                }
            }
            "result" => {
                let u = &v["usage"];
                let is_error = v["is_error"].as_bool().unwrap_or(false);
                out.push(UiEvent::TurnDone {
                    usage: Usage {
                        input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                        output_tokens: u["output_tokens"].as_u64().unwrap_or(0),
                        cache_read_tokens: u["cache_read_input_tokens"].as_u64().unwrap_or(0),
                        cache_write_tokens: u["cache_creation_input_tokens"].as_u64().unwrap_or(0),
                    },
                    cost_usd: v["total_cost_usd"].as_f64(),
                    error: is_error.then(|| {
                        v["result"]
                            .as_str()
                            .or(v["subtype"].as_str())
                            .unwrap_or("error")
                            .to_string()
                    }),
                });
            }
            _ => {}
        }
        out
    }

    // ---------------- Codex CLI (app-server, JSON-RPC) ----------------
    fn parse_codex(&mut self, v: &Value) -> Vec<UiEvent> {
        let mut out = vec![];
        let method = v["method"].as_str();
        let has_id = !v["id"].is_null();
        match (method, has_id) {
            // Answers to Wren's own requests.
            (None, true) => {
                let id = v["id"].as_str().unwrap_or_default();
                if id == "thread" {
                    if let Some(tid) = v["result"]["thread"]["id"].as_str() {
                        self.set_session(tid);
                        out.push(UiEvent::SessionStarted {
                            session_id: tid.to_string(),
                            model: v["result"]["model"].as_str().map(String::from),
                        });
                        // Ready: send the messages that were waiting for the thread.
                        if let Ok(mut l) = self.link.lock() {
                            l.ready = true;
                            let queued = std::mem::take(&mut l.queued);
                            for (text, images) in queued {
                                l.next_id += 1;
                                self.replies.push(Provider::Codex.stdin_message(&text, &images, Some(tid), l.next_id));
                            }
                        }
                    } else if let Some(start) = self.fallback_start.take() {
                        // The old conversation couldn't be reopened: start a new one.
                        self.replies.push(start);
                        out.push(UiEvent::Notice { text: "Codex couldn't reopen the earlier conversation, so a new one was started.".into() });
                    } else {
                        out.push(UiEvent::Error { message: rpc_error(v, "Codex could not start a conversation.") });
                    }
                } else if id.starts_with("turn-") && !v["error"].is_null() {
                    out.push(UiEvent::TurnDone { usage: Usage::default(), cost_usd: None, error: Some(rpc_error(v, "Codex refused the message.")) });
                } else if id == "init" && !v["error"].is_null() {
                    out.push(UiEvent::Error { message: rpc_error(v, "Codex could not start.") });
                }
            }
            // Codex asks Wren (the user) something.
            (Some(m), true) => {
                let rpc_id = v["id"].clone();
                let p = &v["params"];
                let wid = format!("codex-{}", rpc_id.to_string().trim_matches('"'));
                // "For this conversation" only when Codex offers it for this request
                // (it sometimes offers only a permanent rule instead).
                let session_ok = p["availableDecisions"]
                    .as_array()
                    .map(|a| a.iter().any(|d| d == "acceptForSession"))
                    .unwrap_or(true);
                match m {
                    "item/commandExecution/requestApproval" | "execCommandApproval" => {
                        let item = p["itemId"].as_str().and_then(|i| self.codex_items.get(i)).cloned();
                        let cmd = p["command"].as_str().map(String::from).or(item).unwrap_or_else(|| "a command".into());
                        let reason = p["reason"].as_str().map(String::from);
                        out.push(self.ask(wid, Pending::Codex { rpc_id, session_ok }, "Shell", cmd, reason));
                    }
                    "item/fileChange/requestApproval" | "applyPatchApproval" => {
                        let item = p["itemId"].as_str().and_then(|i| self.codex_items.get(i)).cloned();
                        let what = item.unwrap_or_else(|| "Edit files".into());
                        let reason = p["reason"].as_str().map(String::from);
                        out.push(self.ask(wid, Pending::Codex { rpc_id, session_ok }, "Edit", what, reason));
                    }
                    other => {
                        self.replies.push(format!(
                            "{}\n",
                            json!({"jsonrpc": "2.0", "id": rpc_id, "error": {"code": -32601, "message": format!("Wren can't answer '{other}'")}})
                        ));
                        out.push(UiEvent::Notice { text: format!("Codex asked Wren for \"{other}\", which Wren doesn't support, so it was declined.") });
                    }
                }
            }
            // Notifications.
            (Some(m), false) => {
                let p = &v["params"];
                match m {
                    "item/agentMessage/delta" => {
                        if let Some(t) = p["delta"].as_str() {
                            out.push(UiEvent::TextDelta { text: t.to_string() });
                        }
                    }
                    "item/started" => {
                        let item = &p["item"];
                        let id = item["id"].as_str().unwrap_or_default().to_string();
                        match item["type"].as_str().unwrap_or_default() {
                            "reasoning" => out.push(UiEvent::Thinking),
                            "commandExecution" => {
                                let cmd = item["command"].as_str().unwrap_or_default().to_string();
                                self.codex_items.insert(id, cmd.clone());
                                out.push(UiEvent::ToolUse { name: "shell".into(), summary: cmd });
                            }
                            "fileChange" => {
                                let paths = codex_changes(item);
                                self.codex_items.insert(id, paths.clone());
                                out.push(UiEvent::ToolUse { name: "edit".into(), summary: paths });
                            }
                            "mcpToolCall" => out.push(UiEvent::ToolUse {
                                name: format!("{}.{}", item["server"].as_str().unwrap_or("mcp"), item["tool"].as_str().unwrap_or("tool")),
                                summary: summarize_input(&item["arguments"]),
                            }),
                            "webSearch" => out.push(UiEvent::ToolUse {
                                name: "web_search".into(),
                                summary: item["query"].as_str().unwrap_or_default().to_string(),
                            }),
                            _ => {}
                        }
                    }
                    "item/completed" => {
                        let item = &p["item"];
                        let status = item["status"].as_str().unwrap_or("completed");
                        let failed = matches!(status, "failed" | "declined");
                        match item["type"].as_str().unwrap_or_default() {
                            "agentMessage" => out.push(UiEvent::TextEnd { full: item["text"].as_str().map(String::from) }),
                            "commandExecution" => out.push(UiEvent::ToolResult {
                                output: truncate(
                                    &match (item["aggregatedOutput"].as_str(), status) {
                                        (_, "declined") => "Declined.".to_string(),
                                        (Some(o), _) => o.to_string(),
                                        (None, _) => String::new(),
                                    },
                                    TOOL_OUTPUT_MAX,
                                ),
                                is_error: failed || item["exitCode"].as_i64().map(|c| c != 0).unwrap_or(false),
                            }),
                            "fileChange" => out.push(UiEvent::ToolResult {
                                output: if status == "declined" { "Declined.".into() } else { codex_changes(item) },
                                is_error: failed,
                            }),
                            "mcpToolCall" | "webSearch" => out.push(UiEvent::ToolResult { output: String::new(), is_error: failed }),
                            _ => {}
                        }
                    }
                    "thread/tokenUsage/updated" => {
                        let t = &p["tokenUsage"]["total"];
                        self.codex_total = Usage {
                            input_tokens: t["inputTokens"].as_u64().unwrap_or(0),
                            output_tokens: t["outputTokens"].as_u64().unwrap_or(0),
                            cache_read_tokens: t["cachedInputTokens"].as_u64().unwrap_or(0),
                            cache_write_tokens: 0,
                        };
                    }
                    "turn/completed" => {
                        // Totals are cumulative for the thread; report this turn's part.
                        // OpenAI counts cached tokens inside input; split them.
                        let (cum, last) = (self.codex_total.clone(), self.last_cumulative.clone());
                        let cached = cum.cache_read_tokens.saturating_sub(last.cache_read_tokens);
                        let usage = Usage {
                            input_tokens: cum.input_tokens.saturating_sub(last.input_tokens).saturating_sub(cached),
                            output_tokens: cum.output_tokens.saturating_sub(last.output_tokens),
                            cache_read_tokens: cached,
                            cache_write_tokens: 0,
                        };
                        self.last_cumulative = cum;
                        let turn = &p["turn"];
                        let error = (turn["status"] == "failed")
                            .then(|| turn["error"]["message"].as_str().unwrap_or("Codex turn failed").to_string());
                        out.push(UiEvent::TurnDone { usage, cost_usd: None, error });
                    }
                    "error" => {
                        if p["willRetry"] != true {
                            out.push(UiEvent::Error { message: p["error"]["message"].as_str().unwrap_or("Codex error").to_string() });
                        }
                    }
                    "account/rateLimits/updated" => {
                        let limits = crate::limits::parse_codex(&json!({"rateLimits": p["rateLimits"]}), None);
                        if !limits.windows.is_empty() {
                            out.push(UiEvent::PlanLimits { limits });
                        }
                    }
                    _ => {}
                }
            }
            (None, false) => {}
        }
        out
    }

    // ---------------- Antigravity CLI ----------------
    fn parse_agy(&mut self, v: &Value) -> Vec<UiEvent> {
        let mut out = vec![];
        match v["event"].as_str().unwrap_or_default() {
            "init" => {
                if let Some(id) = v["conversation_id"].as_str() {
                    self.set_session(id);
                    out.push(UiEvent::SessionStarted {
                        session_id: id.to_string(),
                        model: v["init"]["model"].as_str().map(String::from),
                    });
                }
            }
            "step_update" => {
                let s = &v["step_update"];
                let done = s["state"] == "DONE";
                match s["step_type"].as_str().unwrap_or_default() {
                    "agent_response" => {
                        if let Some(t) = s["text_delta"].as_str()
                            && !t.is_empty() {
                                out.push(UiEvent::TextDelta { text: t.to_string() });
                            }
                        if done {
                            out.push(UiEvent::TextEnd { full: None });
                        }
                    }
                    "tool" if done => {
                        let info = &s["tool_info"];
                        out.push(UiEvent::ToolUse {
                            name: s["tool_name"].as_str().unwrap_or("tool").to_string(),
                            summary: summarize_input(&info["parameters"]),
                        });
                        let err = info["error"]["message"].as_str();
                        out.push(UiEvent::ToolResult {
                            output: truncate(
                                err.unwrap_or_else(|| info["output"].as_str().unwrap_or_default()),
                                TOOL_OUTPUT_MAX,
                            ),
                            is_error: err.is_some(),
                        });
                    }
                    _ => {}
                }
            }
            "result" => {
                let r = &v["result"];
                if let Some(id) = r["conversation_id"].as_str() {
                    self.set_session(id);
                }
                let u = &r["usage"];
                let cum = Usage {
                    input_tokens: u["input_tokens"].as_u64().unwrap_or(0),
                    output_tokens: u["output_tokens"].as_u64().unwrap_or(0)
                        + u["thinking_tokens"].as_u64().unwrap_or(0),
                    cache_read_tokens: u["cache_read_tokens"].as_u64().unwrap_or(0),
                    cache_write_tokens: 0,
                };
                // In stream-input sessions the counters are cumulative.
                // Gemini counts cached tokens inside input_tokens; split them.
                let turn_cached = cum.cache_read_tokens.saturating_sub(self.last_cumulative.cache_read_tokens);
                let turn = Usage {
                    input_tokens: cum
                        .input_tokens
                        .saturating_sub(self.last_cumulative.input_tokens)
                        .saturating_sub(turn_cached),
                    output_tokens: cum.output_tokens.saturating_sub(self.last_cumulative.output_tokens),
                    cache_read_tokens: cum
                        .cache_read_tokens
                        .saturating_sub(self.last_cumulative.cache_read_tokens),
                    cache_write_tokens: 0,
                };
                self.last_cumulative = cum;
                let status = r["status"].as_str().unwrap_or("SUCCESS");
                out.push(UiEvent::TurnDone {
                    usage: turn,
                    cost_usd: None,
                    error: (status != "SUCCESS").then(|| {
                        r["error"]["message"]
                            .as_str()
                            .or(r["error"].as_str())
                            .unwrap_or(status)
                            .to_string()
                    }),
                });
            }
            _ => {}
        }
        out
    }
}

/// A short, human-readable line for a tool call's input.
fn summarize_input(input: &Value) -> String {
    for key in [
        "command", "CommandLine", "file_path", "path", "pattern", "url", "query", "description",
        "prompt",
    ] {
        if let Some(s) = input[key].as_str() {
            return truncate(s, 300);
        }
    }
    if input.is_null() {
        return String::new();
    }
    truncate(&input.to_string(), 300)
}

/// "add src/a.rs, update src/b.rs" for a Codex fileChange item.
fn codex_changes(item: &Value) -> String {
    item["changes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let kind = c["kind"].as_str().or(c["kind"]["type"].as_str()).unwrap_or("edit");
            format!("{kind} {}", c["path"].as_str().unwrap_or_default())
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// The message of a JSON-RPC error answer.
fn rpc_error(v: &Value, fallback: &str) -> String {
    v["error"]["message"].as_str().unwrap_or(fallback).to_string()
}

/// Tool results can be a string or a list of content blocks.
fn content_to_text(c: &Value) -> String {
    match c {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser(p: Provider) -> (Parser, Arc<Mutex<Option<String>>>) {
        let id = Arc::new(Mutex::new(None));
        (Parser::new(p, id.clone(), crate::link::new_link(true), None), id)
    }

    #[test]
    fn claude_stream() {
        let (mut p, id) = parser(Provider::Claude);
        let ev = p.parse(r#"{"type":"system","subtype":"init","session_id":"s1","model":"claude-opus-5-5"}"#);
        assert_eq!(ev, vec![UiEvent::SessionStarted { session_id: "s1".into(), model: Some("claude-opus-5-5".into()) }]);
        assert_eq!(id.lock().unwrap().as_deref(), Some("s1"));
        let ev = p.parse(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}}"#);
        assert_eq!(ev, vec![UiEvent::TextDelta { text: "Hi".into() }]);
        let ev = p.parse(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Hi there"},{"type":"tool_use","name":"Bash","input":{"command":"ls"}}]}}"#);
        assert_eq!(ev.len(), 2);
        assert_eq!(ev[1], UiEvent::ToolUse { name: "Bash".into(), summary: "ls".into() });
        let ev = p.parse(r#"{"type":"user","message":{"content":[{"type":"tool_result","content":[{"type":"text","text":"a.txt"}],"is_error":false}]}}"#);
        assert_eq!(ev, vec![UiEvent::ToolResult { output: "a.txt".into(), is_error: false }]);
        let ev = p.parse(r#"{"type":"result","subtype":"success","is_error":false,"total_cost_usd":0.012,"usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":7},"result":"Hi there"}"#);
        match &ev[0] {
            UiEvent::TurnDone { usage, cost_usd, error } => {
                assert_eq!(usage.input_tokens, 10);
                assert_eq!(usage.cache_write_tokens, 7);
                assert_eq!(*cost_usd, Some(0.012));
                assert!(error.is_none());
            }
            e => panic!("unexpected {e:?}"),
        }
    }

    #[test]
    fn codex_app_server_stream() {
        let id = Arc::new(Mutex::new(None));
        let link = crate::link::new_link(false);
        let mut p = Parser::new(Provider::Codex, id.clone(), link.clone(), None);
        // A message typed before the thread exists waits, then goes out.
        link.lock().unwrap().queued.push(("hello".into(), vec![]));
        let ev = p.parse(r#"{"id":"thread","result":{"thread":{"id":"t-1"},"model":"gpt-6.2-sol"}}"#);
        assert_eq!(ev, vec![UiEvent::SessionStarted { session_id: "t-1".into(), model: Some("gpt-6.2-sol".into()) }]);
        assert_eq!(id.lock().unwrap().as_deref(), Some("t-1"));
        assert!(link.lock().unwrap().ready);
        let r = p.take_replies();
        let v: Value = serde_json::from_str(r[0].trim()).unwrap();
        assert_eq!(v["method"], "turn/start");
        assert_eq!(v["params"]["threadId"], "t-1");
        assert_eq!(v["params"]["input"][0]["text"], "hello");

        let ev = p.parse(r#"{"method":"item/started","params":{"item":{"type":"commandExecution","id":"i1","command":"ls -la","status":"inProgress"}}}"#);
        assert_eq!(ev, vec![UiEvent::ToolUse { name: "shell".into(), summary: "ls -la".into() }]);
        // Codex asks for approval of that command -> a card.
        let ev = p.parse(r#"{"id":5,"method":"item/commandExecution/requestApproval","params":{"itemId":"i1","reason":"needs to list files"}}"#);
        assert!(matches!(&ev[0], UiEvent::Approval { id, tool, title, detail, allow_session: true } if id == "codex-5" && tool == "Shell" && title == "ls -la" && detail.as_deref() == Some("needs to list files")));
        // Real Codex 0.172 request: no "for this session" option offered -> hidden.
        let ev = p.parse(r#"{"id":6,"method":"item/commandExecution/requestApproval","params":{"itemId":"i1","command":"touch x","availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["touch","x"]}},"cancel"]}}"#);
        assert!(matches!(&ev[0], UiEvent::Approval { allow_session: false, .. }));
        assert!(link.lock().unwrap().pending.contains_key("codex-5"));
        let ev = p.parse(r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"i1","command":"ls","status":"completed","aggregatedOutput":"a.txt","exitCode":0}}}"#);
        assert_eq!(ev, vec![UiEvent::ToolResult { output: "a.txt".into(), is_error: false }]);
        let ev = p.parse(r#"{"method":"item/agentMessage/delta","params":{"delta":"Do"}}"#);
        assert_eq!(ev, vec![UiEvent::TextDelta { text: "Do".into() }]);
        let ev = p.parse(r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"m","text":"Done"}}}"#);
        assert_eq!(ev, vec![UiEvent::TextEnd { full: Some("Done".into()) }]);
        p.parse(r#"{"method":"thread/tokenUsage/updated","params":{"tokenUsage":{"total":{"inputTokens":20,"cachedInputTokens":4,"outputTokens":9},"last":{}}}}"#);
        let ev = p.parse(r#"{"method":"turn/completed","params":{"turn":{"id":"u1","status":"completed","error":null}}}"#);
        assert!(matches!(&ev[0], UiEvent::TurnDone { usage, error: None, .. } if usage.input_tokens == 16 && usage.cache_read_tokens == 4 && usage.output_tokens == 9));
        // Second turn: only the difference counts.
        p.parse(r#"{"method":"thread/tokenUsage/updated","params":{"tokenUsage":{"total":{"inputTokens":50,"cachedInputTokens":4,"outputTokens":19}}}}"#);
        let ev = p.parse(r#"{"method":"turn/completed","params":{"turn":{"status":"failed","error":{"message":"boom"}}}}"#);
        assert!(matches!(&ev[0], UiEvent::TurnDone { usage, error: Some(e), .. } if usage.input_tokens == 30 && usage.output_tokens == 10 && e == "boom"));
        // Requests Wren can't show are declined at once.
        let ev = p.parse(r#"{"id":9,"method":"item/tool/requestUserInput","params":{}}"#);
        assert!(matches!(&ev[0], UiEvent::Notice { .. }));
        let v: Value = serde_json::from_str(p.take_replies()[0].trim()).unwrap();
        assert_eq!(v["id"], 9);
        assert!(v["error"].is_object());
        // Live plan limits.
        let ev = p.parse(r#"{"method":"account/rateLimits/updated","params":{"rateLimits":{"primary":{"usedPercent":40,"windowDurationMins":300,"resetsAt":1}}}}"#);
        assert!(matches!(&ev[0], UiEvent::PlanLimits { limits } if limits.windows[0].used_percent == 40.0));
    }

    #[test]
    fn codex_resume_falls_back_to_new_thread() {
        let link = crate::link::new_link(false);
        let mut p = Parser::new(Provider::Codex, Arc::new(Mutex::new(None)), link, Some("START\n".into()));
        let ev = p.parse(r#"{"id":"thread","error":{"code":-1,"message":"no rollout"}}"#);
        assert!(matches!(&ev[0], UiEvent::Notice { .. }));
        assert_eq!(p.take_replies(), vec!["START\n".to_string()]);
    }

    #[test]
    fn agy_stream_and_cumulative_usage() {
        let (mut p, id) = parser(Provider::Antigravity);
        p.parse(r#"{"event":"init","conversation_id":"c1","init":{"cwd":"/x","model":"Gemini"}}"#);
        assert_eq!(id.lock().unwrap().as_deref(), Some("c1"));
        let ev = p.parse(r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":2,"state":"ACTIVE","step_type":"agent_response","text_delta":"app"}}"#);
        assert_eq!(ev, vec![UiEvent::TextDelta { text: "app".into() }]);
        let ev = p.parse(r#"{"event":"step_update","step_update":{"conversation_id":"c1","step_index":4,"state":"DONE","step_type":"tool","tool_name":"run_command","tool_info":{"name":"run_command","parameters":{"CommandLine":"echo hi"},"output":"hi"}}}"#);
        assert_eq!(ev[0], UiEvent::ToolUse { name: "run_command".into(), summary: "echo hi".into() });
        let r1 = p.parse(r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","usage":{"input_tokens":100,"output_tokens":10,"thinking_tokens":5,"cache_read_tokens":0}}}"#);
        let r2 = p.parse(r#"{"event":"result","result":{"conversation_id":"c1","status":"SUCCESS","usage":{"input_tokens":250,"output_tokens":30,"thinking_tokens":5,"cache_read_tokens":0}}}"#);
        assert!(matches!(&r1[0], UiEvent::TurnDone { usage, .. } if usage.input_tokens == 100 && usage.output_tokens == 15));
        assert!(matches!(&r2[0], UiEvent::TurnDone { usage, .. } if usage.input_tokens == 150 && usage.output_tokens == 20));
    }

    #[test]
    fn args_per_permission() {
        let a = Provider::Codex.args(Permission::Manual, None, Some("gpt-6.2-sol"), Some("high"), None);
        assert_eq!(
            a,
            ["app-server", "-c", "analytics.enabled=false", "-c", "feedback.enabled=false", "-c", "model=\"gpt-6.2-sol\"", "-c", "model_reasoning_effort=\"high\""]
        );
        let start: Value = serde_json::from_str(Provider::Codex.startup_messages(Permission::Manual, "/p", None)[2].trim()).unwrap();
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["approvalPolicy"], "untrusted");
        assert_eq!(start["params"]["sandbox"], "read-only");
        let resume: Value = serde_json::from_str(Provider::Codex.startup_messages(Permission::Auto, "/p", Some("t-1"))[2].trim()).unwrap();
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "t-1");
        assert_eq!(resume["params"]["approvalsReviewer"], "auto_review");
        assert!(Provider::Claude.startup_messages(Permission::Manual, "/p", None).is_empty());

        let c = Provider::Claude.args(Permission::Manual, Some("s1"), Some("opus"), Some("xhigh"), None);
        assert!(c.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "default"));
        assert!(c.windows(2).any(|w| w[0] == "--permission-prompt-tool" && w[1] == "stdio"), "Manual asks Wren");
        assert!(c.ends_with(&["--resume".into(), "s1".into(), "--model".into(), "opus".into(), "--effort".into(), "xhigh".into()]));
        let auto = Provider::Claude.args(Permission::Auto, None, None, None, None);
        assert!(auto.windows(2).any(|w| w[0] == "--permission-mode" && w[1] == "auto"));
        assert!(auto.windows(2).any(|w| w[0] == "--permission-prompts" && w[1] == "none"), "Auto never waits");
        let full = Provider::Claude.args(Permission::Full, None, None, None, None);
        assert!(full.contains(&"--dangerously-skip-permissions".to_string()));
        let g = Provider::Antigravity.args(Permission::Manual, None, Some(""), None, None);
        assert!(g.windows(2).any(|w| w[0] == "--mode" && w[1] == "default"));
        assert!(!g.contains(&"--model".to_string()), "empty model = CLI default");
        // Old saved values still load.
        let old: Permission = serde_json::from_str("\"ask\"").unwrap();
        assert_eq!(old, Permission::Manual);
        let old: Permission = serde_json::from_str("\"edits\"").unwrap();
        assert_eq!(old, Permission::Auto);
    }

    #[test]
    fn mode_flags() {
        let c = Provider::Claude.args(Permission::Manual, None, None, None, Some(Mode::Study));
        let i = c.iter().position(|x| x == "--settings").unwrap();
        assert_eq!(c[i + 1], r#"{"outputStyle":"Learning"}"#);
        assert!(c.contains(&"--append-system-prompt".to_string()));
        assert!(!c.contains(&"--effort".to_string()), "study keeps the default effort");

        let r = Provider::Claude.args(Permission::Manual, None, None, None, Some(Mode::Research));
        let i = r.iter().position(|x| x == "--allowedTools").unwrap();
        assert_eq!(r[i + 1], "WebSearch WebFetch Agent");
        let i = r.iter().position(|x| x == "--effort").unwrap();
        assert_eq!(r[i + 1], "high");
        let r2 = Provider::Claude.args(Permission::Manual, None, None, Some("low"), Some(Mode::Research));
        assert!(r2.windows(2).any(|w| w[0] == "--effort" && w[1] == "low"), "the user's effort wins");

        let x = Provider::Codex.args(Permission::Manual, None, None, None, Some(Mode::Research));
        assert!(x.contains(&"web_search=\"live\"".to_string()));
        assert!(x.contains(&"model_reasoning_effort=\"high\"".to_string()));
        let d = x.iter().find(|a| a.starts_with("developer_instructions=")).unwrap();
        // the value must parse back to the exact prompt
        let v: String = serde_json::from_str(d.trim_start_matches("developer_instructions=")).unwrap();
        assert_eq!(v, RESEARCH_PROMPT);

        let g = Provider::Antigravity.args(Permission::Manual, None, None, None, Some(Mode::Research));
        assert!(!g.contains(&"--effort".to_string()));
        assert!(Mode::Study.wrap_message("why?").ends_with("</wren_mode>\n\nwhy?"));
    }

    #[test]
    fn claude_rate_limit_event_becomes_plan_limits() {
        let (mut p, _) = parser(Provider::Claude);
        let ev = p.parse(r#"{"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":{"five_hour":{"utilization":0.5,"resetsAt":100}}},"uuid":"u","session_id":"s"}"#);
        assert!(matches!(&ev[0], UiEvent::PlanLimits { limits } if limits.windows[0].used_percent == 50.0));
    }

    #[test]
    fn claude_approvals_become_cards() {
        let link = crate::link::new_link(true);
        let mut p = Parser::new(Provider::Claude, Arc::new(Mutex::new(None)), link.clone(), None);
        // Real request shape from Claude Code 2.1.331 (--permission-prompt-tool stdio).
        let ev = p.parse(r#"{"type":"control_request","request_id":"r1","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"touch /tmp/proj/made.txt","description":"make file"},"description":"make file","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"touch /tmp/proj/made.txt"}],"behavior":"allow","destination":"localSettings"}],"tool_use_id":"tu1"}}"#);
        assert_eq!(ev, vec![UiEvent::Approval { id: "r1".into(), tool: "Bash".into(), title: "touch /tmp/proj/made.txt".into(), detail: Some("make file".into()), allow_session: true }]);
        assert!(p.take_replies().is_empty(), "waits for the user");
        assert!(matches!(link.lock().unwrap().pending.get("r1"), Some(Pending::Claude { .. })));

        // Question forms can't be shown: declined with a hint at once.
        let ev = p.parse(r#"{"type":"control_request","request_id":"r2","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","input":{}}}"#);
        assert!(ev.is_empty());
        let v: Value = serde_json::from_str(p.take_replies()[0].trim()).unwrap();
        assert_eq!(v["response"]["response"]["behavior"], "deny");

        // Anything else is declined at once so the CLI never waits.
        let ev = p.parse(r#"{"type":"control_request","request_id":"r3","request":{"subtype":"elicitation"}}"#);
        assert!(matches!(&ev[0], UiEvent::Notice { .. }));
        let v: Value = serde_json::from_str(p.take_replies()[0].trim()).unwrap();
        assert_eq!(v["response"]["subtype"], "error");

        let ev = p.parse(r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#);
        assert_eq!(ev, vec![UiEvent::Thinking]);
    }

    #[test]
    fn attachments_per_cli() {
        let img = Outgoing { path: "/data/attachments/a1/shot.png".into(), name: "shot.png".into(), size: 4, mime: "image/png".into(), inline_base64: Some("AAAA".into()) };
        let pdf = Outgoing { path: "/data/attachments/a2/paper.pdf".into(), name: "paper.pdf".into(), size: 9, mime: "application/pdf".into(), inline_base64: Some("JVBE".into()) };
        let csv = Outgoing { path: "/data/attachments/a3/sales.csv".into(), name: "sales.csv".into(), size: 2048, mime: "text/csv".into(), inline_base64: None };
        let all = [img.clone(), pdf.clone(), csv.clone()];

        let v: Value = serde_json::from_str(Provider::Claude.stdin_message("compare these", &all, None, 0).trim()).unwrap();
        let c = &v["message"]["content"];
        assert_eq!(c[0]["type"], "image");
        assert_eq!(c[1]["type"], "document");
        assert_eq!(c[1]["title"], "paper.pdf");
        let t = c[2]["text"].as_str().unwrap();
        assert!(t.starts_with("compare these") && t.contains("- /data/attachments/a3/sales.csv (2 KB, text/csv)") && !t.contains("shot.png"), "{t}");
        let only: Value = serde_json::from_str(Provider::Claude.stdin_message("", std::slice::from_ref(&img), None, 0).trim()).unwrap();
        assert_eq!(only["message"]["content"].as_array().unwrap().len(), 1, "no empty text block");

        let v: Value = serde_json::from_str(Provider::Codex.stdin_message("fix this", &all, Some("t1"), 3).trim()).unwrap();
        assert_eq!(v["params"]["input"][0], json!({"type": "localImage", "path": "/data/attachments/a1/shot.png"}));
        let t = v["params"]["input"][1]["text"].as_str().unwrap();
        assert!(t.contains("paper.pdf") && t.contains("sales.csv"), "PDF by path for Codex: {t}");

        let v: Value = serde_json::from_str(Provider::Antigravity.stdin_message("look", &[img, csv], None, 0).trim()).unwrap();
        assert_eq!(v["message"]["content"], "look\n\nAttached files: @/data/attachments/a1/shot.png @/data/attachments/a3/sales.csv");
    }

    #[test]
    fn non_json_becomes_log() {
        let (mut p, _) = parser(Provider::Claude);
        assert_eq!(p.parse("hello"), vec![UiEvent::Log { line: "hello".into() }]);
    }
}
