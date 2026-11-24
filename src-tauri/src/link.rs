//! State shared between a pane's session (which writes to the CLI) and its
//! output parser (which reads from it): approval requests waiting for the
//! user, and messages held back until the CLI is ready (Codex handshake).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use serde_json::{json, Value};

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
    }
}
