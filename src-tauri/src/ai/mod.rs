pub mod analysis;
pub mod client;
pub mod engine;
pub mod probe;
pub mod keywords;
pub mod models;
pub mod ports;
pub mod prompt;
pub mod sse;
pub mod stream;
pub mod tools;
pub mod vault;

use serde::{Deserialize, Serialize};

pub const CHAT_NO_FINAL_RESPONSE: &str =
    "I couldn't produce a final reply this time. Please try again.";
pub const CHAT_TOOL_LIMIT_RESPONSE: &str =
    "I couldn't finish after several tool calls. Please ask me to try again.";

/// Select only user-visible chat text for persistence. Reasoning tokens are
/// intentionally excluded by callers; empty final turns receive a safe,
/// user-facing fallback instead of persisting internal reasoning.
pub fn terminal_chat_response(visible_text: &str, tool_limit_reached: bool) -> String {
    if !visible_text.trim().is_empty() {
        visible_text.to_string()
    } else if tool_limit_reached {
        CHAT_TOOL_LIMIT_RESPONSE.to_string()
    } else {
        CHAT_NO_FINAL_RESPONSE.to_string()
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub enum AnalysisStatus {
    Queued,
    Processing,
    Done,
    Failed,
}

#[derive(Debug, Deserialize, Default)]
pub struct RawAnalysis {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub mood: String,
    #[serde(default)]
    pub emotions: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_analysis_tasks")]
    pub tasks: Option<Vec<RawProposedTask>>,
    #[serde(default)]
    pub insights: Option<Vec<String>>,
    #[serde(default)]
    pub triplets: Option<Vec<(String, String, String)>>,
    #[serde(default)]
    pub facts: Option<Vec<RawProposedFact>>,
    #[serde(default)]
    pub events: Option<Vec<RawProposedEvent>>,
    #[serde(default)]
    pub notes: Option<Vec<RawProposedNote>>,
}

#[derive(Debug, Deserialize, Default, Clone, PartialEq)]
pub struct RawProposedTask {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
}

/// Tolerates both the legacy string form (`"tasks": ["Buy milk"]`) and the
/// structured object form (`"tasks": [{"title": "...", "due_date": "..."}]`).
fn de_analysis_tasks<'de, D>(d: D) -> Result<Option<Vec<RawProposedTask>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TaskOr {
        S(String),
        Obj(RawProposedTask),
    }
    let elems: Option<Vec<TaskOr>> = Option::deserialize(d)?;
    Ok(elems.map(|v| {
        v.into_iter()
            .map(|e| match e {
                TaskOr::S(s) => RawProposedTask {
                    title: s,
                    ..Default::default()
                },
                TaskOr::Obj(o) => o,
            })
            .collect()
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct RawProposedFact {
    #[serde(default)]
    pub fact_key: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub category: String,
}

#[derive(Debug, Deserialize, Default)]
pub struct RawProposedEvent {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub start_at: String, // ISO 8601
    #[serde(default)]
    pub location: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct RawProposedNote {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

/// Trims markdown code fences (e.g. ```json ... ```) and extracts the outer JSON object `{...}`.
pub fn clean_json_string(s: &str) -> &str {
    let trimmed = s.trim();
    let stripped = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let stripped = stripped.strip_suffix("```").unwrap_or(stripped).trim();
    if let (Some(start), Some(end)) = (stripped.find('{'), stripped.rfind('}')) {
        if start <= end {
            return &stripped[start..=end];
        }
    }
    stripped
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProposedFact {
    pub fact_key: String,
    pub content: String,
    pub category: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProposedEvent {
    pub title: String,
    pub start_at: String,
    pub location: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProposedNote {
    pub title: Option<String>,
    pub content: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProposedTask {
    pub title: String,
    pub due_date: Option<String>,
    pub priority: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnalysisResult {
    pub id: String,
    pub summary: String,
    pub mood: String,
    pub emotions: Vec<String>,
    pub tasks: Vec<ProposedTask>,
    pub insights: Vec<String>,
    pub triplets: Vec<(String, String, String)>,
    pub facts: Vec<ProposedFact>,
    pub events: Vec<ProposedEvent>,
    pub notes: Vec<ProposedNote>,
}

impl AnalysisResult {
    pub fn from_raw(raw: RawAnalysis, id: String) -> Self {
        Self {
            id,
            summary: clean(raw.summary),
            mood: clean(raw.mood),
            emotions: raw.emotions.unwrap_or_default(),
            tasks: raw
                .tasks
                .unwrap_or_default()
                .into_iter()
                .map(|t| ProposedTask {
                    title: clean(t.title),
                    due_date: t.due_date,
                    priority: t.priority,
                })
                .collect(),
            insights: raw.insights.unwrap_or_default(),
            triplets: raw.triplets.unwrap_or_default(),
            facts: raw
                .facts
                .unwrap_or_default()
                .into_iter()
                .map(|f| ProposedFact {
                    fact_key: f.fact_key,
                    content: f.content,
                    category: f.category,
                })
                .collect(),
            events: raw
                .events
                .unwrap_or_default()
                .into_iter()
                .map(|e| ProposedEvent {
                    title: e.title,
                    start_at: e.start_at,
                    location: e.location,
                })
                .collect(),
            notes: raw
                .notes
                .unwrap_or_default()
                .into_iter()
                .map(|n| ProposedNote {
                    title: n.title,
                    content: n.content,
                    tags: n.tags.unwrap_or_default(),
                })
                .collect(),
        }
    }
}

fn clean(s: String) -> String {
    s.trim().replace('\n', " ").chars().take(200).collect()
}

/// Decode `%XX` URL-style escapes in text. Only `%` followed by two hex digits
/// is decoded; a lone `%` (e.g. "100%") is left untouched.
pub fn percent_decode_str(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = hex_val(bytes[i + 1]);
            let lo = hex_val(bytes[i + 2]);
            if let (Some(h), Some(l)) = (hi, lo) {
                out.push((h << 4) | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Assemble the LLM message array: the system prompt followed by the replayed
/// history **verbatim**. The replayed entries must not be field-filtered — an
/// assistant turn carrying `tool_calls` and its `role:"tool"` result must keep
/// their ids, or the next turn is rejected by tool-aware templates.
pub fn build_chat_messages(
    system_prompt: &str,
    replayed: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let mut msgs = Vec::with_capacity(replayed.len() + 1);
    msgs.push(serde_json::json!({
        "role": "system",
        "content": system_prompt,
    }));
    msgs.extend(replayed.iter().cloned());
    msgs
}

/// Recursively percent-decode every string leaf in a JSON `Value`.
pub fn percent_decode_value(v: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match v {
        Value::String(s) => Value::String(percent_decode_str(&s)),
        Value::Array(a) => Value::Array(a.into_iter().map(percent_decode_value).collect()),
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, val)| (k, percent_decode_value(val)))
                .collect(),
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_decode_str_handles_escapes_and_plain() {
        assert_eq!(
            percent_decode_str("Title%3A%20second%20(due%3A%2024th)"),
            "Title: second (due: 24th)"
        );
        assert_eq!(percent_decode_str("100% done"), "100% done");
        assert_eq!(percent_decode_str("plain title"), "plain title");
        assert_eq!(percent_decode_str("%ZZ"), "%ZZ");
        assert_eq!(percent_decode_str("Buy milk and eggs"), "Buy milk and eggs");
    }

    #[test]
    fn percent_decode_value_walks_nested_values() {
        let v = serde_json::json!({
            "title": "Fix%20the%20faucet",
            "due_date": "2026-10-05",
            "tags": ["a%20b", "plain"],
            "nested": {"x": "y%20z"},
            "num": 3
        });
        let d = percent_decode_value(v);
        assert_eq!(d["title"], "Fix the faucet");
        assert_eq!(d["due_date"], "2026-10-05");
        assert_eq!(d["tags"][0], "a b");
        assert_eq!(d["nested"]["x"], "y z");
        assert_eq!(d["num"], 3);
    }

    #[test]
    fn de_analysis_tasks_accepts_strings_and_objects() {
        let raw: RawAnalysis = serde_json::from_str(
            r#"{"tasks":["plain task", {"title":"Structured%3A%20task", "due_date":"2026-10-05", "priority":"high"}]}"#,
        )
        .unwrap();
        let tasks = raw.tasks.unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "plain task");
        assert_eq!(tasks[0].due_date, None);
        assert_eq!(tasks[1].title, "Structured%3A%20task");
        assert_eq!(tasks[1].due_date.as_deref(), Some("2026-10-05"));
        assert_eq!(tasks[1].priority.as_deref(), Some("high"));
    }

    #[test]
    fn de_analysis_tasks_defaults_when_absent() {
        let raw: RawAnalysis = serde_json::from_str(r#"{"summary":"hi"}"#).unwrap();
        assert!(raw.tasks.is_none());
    }

    #[test]
    fn build_chat_messages_preserves_tool_calls_and_ids() {
        // Regression: replaying history must NOT strip tool_calls/tool_call_id,
        // or the assembled request contains a `role:"tool"` message with no id.
        let replayed = vec![
            serde_json::json!({"role": "user", "content": "hi"}),
            serde_json::json!({
                "role": "assistant",
                "content": "",
                "tool_calls": [{
                    "id": "call_1",
                    "type": "function",
                    "function": {"name": "list_tasks", "arguments": "{}"}
                }]
            }),
            serde_json::json!({
                "role": "tool",
                "tool_call_id": "call_1",
                "content": "{\"success\":true}"
            }),
        ];
        let msgs = build_chat_messages("SYS", &replayed);
        assert_eq!(msgs[0]["role"], "system");
        assert_eq!(msgs[0]["content"], "SYS");
        assert_eq!(msgs[2]["tool_calls"][0]["id"], "call_1");
        assert_eq!(msgs[3]["tool_call_id"], "call_1");
    }

    #[test]
    fn terminal_chat_response_uses_only_visible_text_or_safe_fallback() {
        assert_eq!(
            terminal_chat_response("Visible answer", false),
            "Visible answer"
        );
        assert_eq!(terminal_chat_response("", false), CHAT_NO_FINAL_RESPONSE);
        assert_eq!(
            terminal_chat_response("   ", true),
            CHAT_TOOL_LIMIT_RESPONSE
        );
    }
}
