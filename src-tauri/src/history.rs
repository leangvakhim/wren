//! Conversation history, stored ONLY on this computer in Wren's app-data
//! folder (`<app_data_dir>/conversations/`). Nothing is uploaded by Wren.
//!
//! Each conversation is one JSON file written by the UI (messages, usage,
//! settings, the CLI's session id). `index.json` keeps a small summary of
//! every conversation so the sidebar can load quickly.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConversationMeta {
    pub id: String,
    pub title: String,
    pub provider: String,
    #[serde(rename = "accountId")]
    pub account_id: String,
    pub cwd: String,
    #[serde(rename = "createdAt")]
    pub created_at: f64,
    #[serde(rename = "updatedAt")]
    pub updated_at: f64,
}

pub struct History {
    dir: PathBuf,
    index: Mutex<Vec<ConversationMeta>>,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

impl History {
    pub fn load(data_dir: &Path) -> Self {
        let dir = data_dir.join("conversations");
        let index = std::fs::read_to_string(dir.join("index.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { dir, index: Mutex::new(index) }
    }

    fn write_atomic(&self, path: &Path, text: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700));
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    fn save_index(&self, idx: &[ConversationMeta]) -> Result<(), String> {
        let text = serde_json::to_string(idx).map_err(|e| e.to_string())?;
        self.write_atomic(&self.dir.join("index.json"), &text)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<ConversationMeta> {
        let mut v = self.index.lock().map(|g| g.clone()).unwrap_or_default();
        v.sort_by(|a, b| b.updated_at.partial_cmp(&a.updated_at).unwrap_or(std::cmp::Ordering::Equal));
        v
    }

    pub fn save(&self, conv: Value) -> Result<ConversationMeta, String> {
        let meta: ConversationMeta = serde_json::from_value(conv.clone()).map_err(|e| format!("bad conversation: {e}"))?;
        if !valid_id(&meta.id) {
            return Err("bad conversation id".into());
        }
        let text = serde_json::to_string(&conv).map_err(|e| e.to_string())?;
        self.write_atomic(&self.dir.join(format!("{}.json", meta.id)), &text)?;
        let mut idx = self.index.lock().map_err(|e| e.to_string())?;
        idx.retain(|m| m.id != meta.id);
        idx.push(meta.clone());
        self.save_index(&idx)?;
        Ok(meta)
    }

    pub fn get(&self, id: &str) -> Result<Value, String> {
        if !valid_id(id) {
            return Err("bad conversation id".into());
        }
        let text = std::fs::read_to_string(self.dir.join(format!("{id}.json")))
            .map_err(|_| "That conversation can't be found.".to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if !valid_id(id) {
            return Err("bad conversation id".into());
        }
        let _ = std::fs::remove_file(self.dir.join(format!("{id}.json")));
        let mut idx = self.index.lock().map_err(|e| e.to_string())?;
        idx.retain(|m| m.id != id);
        self.save_index(&idx)
    }

    /// Every usage record from every saved conversation, for the Usage window.
    /// Each record gets the conversation's id, title and folder. Conversations
    /// saved before per-turn records existed contribute one record from their totals.
    pub fn usage_records(&self) -> Vec<Value> {
        let mut out = vec![];
        for meta in self.list() {
            let Ok(conv) = self.get(&meta.id) else { continue };
            let tag = |mut r: Value| {
                r["conversation"] = Value::String(meta.id.clone());
                r["title"] = Value::String(meta.title.clone());
                r["cwd"] = Value::String(meta.cwd.clone());
                r
            };
            match conv["usage"].as_array() {
                Some(list) if !list.is_empty() => out.extend(list.iter().cloned().map(tag)),
                _ => {
                    let t = &conv["totals"];
                    let input = t["input"].as_f64().unwrap_or(0.0);
                    let output = t["output"].as_f64().unwrap_or(0.0);
                    if input + output > 0.0 {
                        out.push(tag(serde_json::json!({
                            "t": meta.updated_at,
                            "provider": meta.provider,
                            "accountId": meta.account_id,
                            "model": conv["model"].as_str().unwrap_or(""),
                            "input": input,
                            "cached": 0,
                            "output": output,
                            "cost": if t["hasCost"].as_bool().unwrap_or(false) { t["cost"].clone() } else { Value::Null },
                        })));
                    }
                }
            }
        }
        out
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<(), String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("Give the conversation a title.".into());
        }
        let mut conv = self.get(id)?;
        conv["title"] = Value::String(title.chars().take(120).collect());
        self.save(conv).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_list_get_rename_delete() {
        let d = std::env::temp_dir().join(format!("wren-hist-{}", std::process::id()));
        let h = History::load(&d);
        let conv = |id: &str, t: f64| json!({"id": id, "title": "Hi", "provider": "claude", "accountId": "default-claude",
            "cwd": "/x", "createdAt": t, "updatedAt": t, "messages": [{"id": 0, "role": "user", "text": "Hi"}]});
        h.save(conv("a1", 1.0)).unwrap();
        h.save(conv("b2", 2.0)).unwrap();
        h.save(conv("a1", 3.0)).unwrap(); // update moves it to the top
        let l = h.list();
        assert_eq!(l.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["a1", "b2"]);
        assert_eq!(h.get("a1").unwrap()["messages"][0]["text"], "Hi");
        h.rename("b2", "Renamed").unwrap();
        // survives a restart
        let h2 = History::load(&d);
        assert_eq!(h2.list().iter().find(|m| m.id == "b2").unwrap().title, "Renamed");
        // usage records: per-turn list, or one record from totals for old saves
        h2.save(json!({"id": "u1", "title": "U", "provider": "codex", "accountId": "default-codex", "cwd": "/p",
            "createdAt": 4.0, "updatedAt": 4.0, "messages": [],
            "usage": [{"t": 4.0, "provider": "codex", "input": 10, "cached": 5, "output": 2, "cost": 0.01},
                      {"t": 5.0, "provider": "codex", "input": 1, "cached": 0, "output": 1, "cost": null}]})).unwrap();
        h2.save(json!({"id": "o1", "title": "Old", "provider": "claude", "accountId": "default-claude", "cwd": "/q",
            "createdAt": 1.0, "updatedAt": 1.0, "messages": [],
            "totals": {"input": 100, "output": 20, "cost": 0.3, "hasCost": true}})).unwrap();
        let recs = h2.usage_records();
        assert_eq!(recs.iter().filter(|r| r["conversation"] == "u1").count(), 2);
        let old = recs.iter().find(|r| r["conversation"] == "o1").unwrap();
        assert_eq!(old["cost"], 0.3);
        assert_eq!(old["cwd"], "/q");
        h2.delete("a1").unwrap();
        assert!(h2.get("a1").is_err());
        assert!(h2.get("../secret").is_err(), "path tricks are refused");
        std::fs::remove_dir_all(d).ok();
    }
}
