//! State shared between a pane's session (which writes to the CLI) and its
//! output parser (which reads from it): approval requests waiting for the
//! user, and messages held back until the CLI is ready (Codex handshake).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

use crate::providers::Permission;

/// What the user decided on an approval card.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// Allow this one action.
    Allow,
    /// Allow this kind of action for the rest of the conversation.
    AllowSession,
    Deny,
}

/// An approval request the CLI is waiting on.
#[derive(Clone, Debug)]
pub enum Pending {
    /// Claude Code `can_use_tool` control request.
    Claude { request_id: String, tool: String, input: Value, suggestions: Vec<Value> },
    /// Codex app-server approval request (command or file change).
    /// `session_ok`: Codex offered "accept for this session".
    Codex { rpc_id: Value, session_ok: bool },
    /// Claude Code AskUserQuestion (`can_use_tool`); `input` is sent back with the answers.
    ClaudeQuestion { request_id: String, input: Value },
    /// Codex app-server `item/tool/requestUserInput`.
    CodexQuestion { rpc_id: Value },
    /// Codex asking to run an MCP tool (e.g. the browser tool): an
    /// `mcpServer/elicitation/request` with `codex_approval_kind` = "mcp_tool_call".
    CodexMcpTool { rpc_id: Value },
}

impl Pending {
    /// A question form (not an approval).
    pub fn is_question(&self) -> bool {
        matches!(self, Pending::ClaudeQuestion { .. } | Pending::CodexQuestion { .. })
    }
}

/// Allow a waiting request because the user switched to Auto / Full access.
/// Claude's plan card also tells Claude to go on in that mode (`claude_mode`),
/// so leaving plan mode doesn't undo the switch.
pub fn reply_switched(p: &Pending, claude_mode: &str) -> String {
    match p {
        Pending::Claude { request_id, tool, input, .. } if tool == "ExitPlanMode" => {
            let response = json!({"behavior": "allow", "updatedInput": input,
                "updatedPermissions": [{"type": "setMode", "mode": claude_mode, "destination": "session"}]});
            format!("{}\n", json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}}))
        }
        _ => reply(p, Decision::Allow),
    }
}

/// The user's answers on a question form, by question id. `None` = skipped.
pub type Answers = Option<HashMap<String, Vec<String>>>;

/// The line to write to the CLI's stdin for answers to a question form.
pub fn reply_answers(p: &Pending, answers: &Answers) -> Option<String> {
    let v = match p {
        Pending::ClaudeQuestion { request_id, input } => {
            let response = match answers {
                Some(a) => {
                    // Claude's format: question text -> one answer string.
                    let map: serde_json::Map<String, Value> = a
                        .iter()
                        .filter(|(_, v)| !v.is_empty())
                        .map(|(k, v)| (k.clone(), json!(v.join(", "))))
                        .collect();
                    let mut updated = input.clone();
                    updated["answers"] = Value::Object(map);
                    json!({"behavior": "allow", "updatedInput": updated})
                }
                None => json!({"behavior": "deny", "message": "The user skipped these questions. Continue with your best judgement, or ask again in plain text if you really need the answer."}),
            };
            json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})
        }
        Pending::CodexQuestion { rpc_id } => {
            let map: serde_json::Map<String, Value> = answers
                .iter()
                .flatten()
                .map(|(k, v)| (k.clone(), json!({"answers": v})))
                .collect();
            json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"answers": map}})
        }
        _ => return None,
    };
    Some(format!("{v}\n"))
}

#[derive(Default)]
pub struct LinkState {
    /// The CLI finished its startup handshake and accepts messages.
    pub ready: bool,
    /// User messages (text + images) waiting for `ready`.
    pub queued: Vec<(String, Vec<crate::attachments::Outgoing>)>,
    /// Approval requests by Wren's id (shown on the cards).
    pub pending: HashMap<String, Pending>,
    /// Counter for JSON-RPC request ids Wren sends.
    pub next_id: u64,
    /// Set when the user changes the permission while this process runs
    /// (the process was started with another one). The parser answers new
    /// requests by it; the next process is started with it.
    pub permission: Option<Permission>,
}

pub type Link = Arc<Mutex<LinkState>>;

pub fn new_link(ready: bool) -> Link {
    Arc::new(Mutex::new(LinkState { ready, ..Default::default() }))
}

/// The line to write to the CLI's stdin for this decision.
pub fn reply(p: &Pending, d: Decision) -> String {
    let v = match p {
        Pending::Claude { request_id, tool, input, suggestions } => {
            let response = match d {
                Decision::Deny => json!({"behavior": "deny", "message": "The user denied this action."}),
                Decision::Allow => json!({"behavior": "allow", "updatedInput": input}),
                Decision::AllowSession => {
                    // Claude's own suggested "always allow" rules, kept for this conversation only.
                    let mut rules: Vec<Value> = suggestions
                        .iter()
                        .filter(|s| s["type"] == "addRules")
                        .map(|s| {
                            let mut s = s.clone();
                            s["destination"] = json!("session");
                            s
                        })
                        .collect();
                    if rules.is_empty() {
                        rules.push(json!({"type": "addRules", "rules": [{"toolName": tool}], "behavior": "allow", "destination": "session"}));
                    }
                    json!({"behavior": "allow", "updatedInput": input, "updatedPermissions": rules})
                }
            };
            json!({"type": "control_response", "response": {"subtype": "success", "request_id": request_id, "response": response}})
        }
        Pending::Codex { rpc_id, session_ok } => {
            let decision = match d {
                Decision::Allow => "accept",
                Decision::AllowSession if *session_ok => "acceptForSession",
                // Not offered for this request: allow just this once.
                Decision::AllowSession => "accept",
                Decision::Deny => "decline",
            };
            json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"decision": decision}})
        }
        // "For this chat" isn't offered (Codex's way to remember it isn't documented).
        Pending::CodexMcpTool { rpc_id } => match d {
            Decision::Allow | Decision::AllowSession => json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"action": "accept", "content": {}}}),
            Decision::Deny => json!({"jsonrpc": "2.0", "id": rpc_id, "result": {"action": "decline"}}),
        },
        // A question form answered with an approval button: treat as skipped.
        Pending::ClaudeQuestion { .. } | Pending::CodexQuestion { .. } => {
            return reply_answers(p, &None).unwrap_or_default();
        }
    };
    format!("{v}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_replies() {
        let p = Pending::Claude {
            request_id: "r1".into(),
            tool: "Bash".into(),
            input: json!({"command": "touch a"}),
            suggestions: vec![
                json!({"type": "addRules", "rules": [{"toolName": "Bash", "ruleContent": "touch a"}], "behavior": "allow", "destination": "localSettings"}),
                json!({"type": "setMode", "mode": "acceptEdits", "destination": "session"}),
            ],
        };
        let v: Value = serde_json::from_str(reply(&p, Decision::Allow).trim()).unwrap();
        assert_eq!(v["response"]["request_id"], "r1");
        assert_eq!(v["response"]["response"]["behavior"], "allow");
        assert_eq!(v["response"]["response"]["updatedInput"]["command"], "touch a");
        let v: Value = serde_json::from_str(reply(&p, Decision::AllowSession).trim()).unwrap();
        let rules = v["response"]["response"]["updatedPermissions"].as_array().unwrap();
        assert_eq!(rules.len(), 1, "only rules, not mode changes");
        assert_eq!(rules[0]["destination"], "session", "never saved to the project's settings");
        let v: Value = serde_json::from_str(reply(&p, Decision::Deny).trim()).unwrap();
        assert_eq!(v["response"]["response"]["behavior"], "deny");
    }

    #[test]
    fn question_replies() {
        let p = Pending::ClaudeQuestion {
            request_id: "q1".into(),
            input: json!({"questions": [{"question": "Which DB?", "header": "DB", "options": [], "multiSelect": true}]}),
        };
        let mut a = HashMap::new();
        a.insert("Which DB?".to_string(), vec!["Postgres".to_string(), "SQLite".to_string()]);
        let v: Value = serde_json::from_str(reply_answers(&p, &Some(a.clone())).unwrap().trim()).unwrap();
        let r = &v["response"]["response"];
        assert_eq!(r["behavior"], "allow");
        assert_eq!(r["updatedInput"]["answers"]["Which DB?"], "Postgres, SQLite");
        assert_eq!(r["updatedInput"]["questions"][0]["header"], "DB", "questions are sent back too");
        let v: Value = serde_json::from_str(reply_answers(&p, &None).unwrap().trim()).unwrap();
        assert_eq!(v["response"]["response"]["behavior"], "deny");

        let c = Pending::CodexQuestion { rpc_id: json!(12) };
        let mut a = HashMap::new();
        a.insert("db".to_string(), vec!["Postgres".to_string()]);
        let v: Value = serde_json::from_str(reply_answers(&c, &Some(a)).unwrap().trim()).unwrap();
        assert_eq!(v["id"], 12);
        assert_eq!(v["result"]["answers"]["db"]["answers"][0], "Postgres");
        assert!(reply_answers(&Pending::Codex { rpc_id: json!(1), session_ok: true }, &None).is_none());
    }

    #[test]
    fn switched_replies() {
        let plan = Pending::Claude { request_id: "p1".into(), tool: "ExitPlanMode".into(), input: json!({"plan": "x"}), suggestions: vec![] };
        let v: Value = serde_json::from_str(reply_switched(&plan, "bypassPermissions").trim()).unwrap();
        assert_eq!(v["response"]["response"]["behavior"], "allow");
        assert_eq!(v["response"]["response"]["updatedPermissions"][0]["mode"], "bypassPermissions");
        let bash = Pending::Claude { request_id: "b1".into(), tool: "Bash".into(), input: json!({"command": "ls"}), suggestions: vec![] };
        assert_eq!(reply_switched(&bash, "auto"), reply(&bash, Decision::Allow));
        let codex = Pending::Codex { rpc_id: json!(3), session_ok: true };
        let v: Value = serde_json::from_str(reply_switched(&codex, "auto").trim()).unwrap();
        assert_eq!(v["result"]["decision"], "accept");
        assert!(Pending::CodexQuestion { rpc_id: json!(1) }.is_question());
        assert!(!codex.is_question());
    }

    #[test]
    fn codex_replies() {
        let p = Pending::Codex { rpc_id: json!(7), session_ok: true };
        let v: Value = serde_json::from_str(reply(&p, Decision::AllowSession).trim()).unwrap();
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["decision"], "acceptForSession");
        let once = Pending::Codex { rpc_id: json!(8), session_ok: false };
        let v: Value = serde_json::from_str(reply(&once, Decision::AllowSession).trim()).unwrap();
        assert_eq!(v["result"]["decision"], "accept", "never a permanent rule");
        let v: Value = serde_json::from_str(reply(&p, Decision::Deny).trim()).unwrap();
        assert_eq!(v["result"]["decision"], "decline");
        let mcp = Pending::CodexMcpTool { rpc_id: json!(9) };
        let v: Value = serde_json::from_str(reply(&mcp, Decision::Allow).trim()).unwrap();
        assert_eq!((v["id"].clone(), v["result"]["action"].clone()), (json!(9), json!("accept")));
        let v: Value = serde_json::from_str(reply(&mcp, Decision::Deny).trim()).unwrap();
        assert_eq!(v["result"]["action"], "decline");
    }
}
