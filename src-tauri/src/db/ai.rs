#![allow(dead_code)]

use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AiConversation {
    pub id: String,
    pub title: Option<String>,
    pub model: String,
    pub source: String,
    pub source_entry_id: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_deleted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AiMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub tool_result: Option<String>,
    pub tool_call_id: Option<String>,
    pub confirmed: Option<bool>,
    pub model: Option<String>,
    pub tokens_in: Option<i32>,
    pub tokens_out: Option<i32>,
    pub created_at: String,
    pub source_entry_id: Option<String>,
    pub created_by: Option<String>,
    pub last_reinforced: Option<String>,
}

pub fn upsert_conversation(
    conn: &Connection,
    id: &str,
    title: Option<&str>,
    model: &str,
    source: &str,
    source_entry_id: Option<&str>,
    created_by: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO ai_conversations (id, title, model, source, source_entry_id, created_by, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(id) DO UPDATE SET
            title = COALESCE(ai_conversations.title, excluded.title),
            model = excluded.model,
            source = excluded.source,
            source_entry_id = COALESCE(ai_conversations.source_entry_id, excluded.source_entry_id),
            created_by = COALESCE(ai_conversations.created_by, excluded.created_by),
            updated_at = excluded.updated_at,
            is_deleted = 0",
        params![id, title, model, source, source_entry_id, created_by, now],
    )?;

    Ok(())
}

pub fn insert_message(conn: &Connection, message: &AiMessage) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO ai_messages (
            id, conversation_id, role, content, tool_name, tool_args, tool_result, tool_call_id,
            confirmed, model, tokens_in, tokens_out, created_at, source_entry_id, created_by, last_reinforced
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            message.id,
            message.conversation_id,
            message.role,
            message.content,
            message.tool_name,
            message.tool_args,
            message.tool_result,
            message.tool_call_id,
            message.confirmed.map(|v| if v { 1 } else { 0 }),
            message.model,
            message.tokens_in,
            message.tokens_out,
            message.created_at,
            message.source_entry_id,
            message.created_by,
            message.last_reinforced,
        ],
    )?;

    conn.execute(
        "UPDATE ai_conversations SET updated_at = ?1 WHERE id = ?2",
        params![message.created_at, message.conversation_id],
    )?;

    Ok(())
}

pub fn list_conversations(conn: &Connection, limit: u32) -> Result<Vec<AiConversation>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, model, source, source_entry_id, created_at, updated_at, is_deleted, created_by
         FROM ai_conversations
         WHERE is_deleted = 0
         ORDER BY updated_at DESC
         LIMIT ?1",
    )?;

    let rows = stmt.query_map(params![limit], map_conversation)?;
    let mut conversations = Vec::new();
    for row in rows {
        conversations.push(row?);
    }
    Ok(conversations)
}

pub fn list_messages(conn: &Connection, conversation_id: &str) -> Result<Vec<AiMessage>, AppError> {
    let mut stmt = conn.prepare(
    "SELECT id, conversation_id, role, content, tool_name, tool_args, tool_result, tool_call_id,
        confirmed, model, tokens_in, tokens_out, created_at, source_entry_id, created_by, last_reinforced
     FROM ai_messages
     WHERE conversation_id = ?1
     ORDER BY created_at ASC, rowid ASC",
    )?;

    let rows = stmt.query_map(params![conversation_id], map_message)?;
    let mut messages = Vec::new();
    for row in rows {
        messages.push(row?);
    }
    Ok(messages)
}

/// Most recent messages for a conversation, oldest-first. The requested
/// `limit` bounds only user/assistant turns; the `tool` messages that directly
/// follow a selected assistant message are always included so a replayed
/// history keeps each tool call paired with its result (an assistant tool_calls
/// message without the matching tool result is rejected by many templates).
pub fn recent_messages(
    conn: &Connection,
    conversation_id: &str,
    limit: u32,
) -> Result<Vec<AiMessage>, AppError> {
    // Bound the scan at SQL level: find the (created_at, rowid) of the Nth
    // newest user/assistant turn, then fetch only rows at or after it. Without
    // this the whole conversation was loaded and filtered in memory. Tool rows
    // belonging to a kept assistant turn are chronologically >= that turn, so
    // this window still contains them.
    let boundary: Option<(String, i64)> = conn
        .query_row(
            "SELECT created_at, rowid FROM ai_messages
             WHERE conversation_id = ?1 AND role IN ('user','assistant')
             ORDER BY created_at DESC, rowid DESC
             LIMIT 1 OFFSET ?2",
            params![conversation_id, limit.saturating_sub(1)],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .optional()?;

    let all: Vec<AiMessage> = {
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, tool_name, tool_args, tool_result, tool_call_id,
                confirmed, model, tokens_in, tokens_out, created_at, source_entry_id, created_by, last_reinforced
             FROM ai_messages
             WHERE conversation_id = ?1
               AND (?2 IS NULL
                    OR created_at > ?2
                    OR (created_at = ?2 AND rowid >= ?3))
             ORDER BY created_at DESC, rowid DESC",
        )?;
        let (b_created, b_rowid): (Option<String>, Option<i64>) = match &boundary {
            Some((c, r)) => (Some(c.clone()), Some(*r)),
            None => (None, None),
        };
        let rows = stmt.query_map(params![conversation_id, b_created, b_rowid], map_message)?;
        let mut v = Vec::new();
        for row in rows {
            v.push(row?);
        }
        v.reverse(); // oldest-first
        v
    };

    // Keep only the last `limit` user/assistant turns, then include the tool
    // messages whose tool_call_id belongs to a kept assistant message.
    let turn_idxs: Vec<usize> = all
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user" || m.role == "assistant")
        .map(|(i, _)| i)
        .collect();
    let keep_from = turn_idxs.len().saturating_sub(limit as usize);
    let kept_turns: std::collections::HashSet<usize> =
        turn_idxs[keep_from..].iter().copied().collect();
    let kept_call_ids: std::collections::HashSet<String> = kept_turns
        .iter()
        .filter_map(|&i| all[i].tool_call_id.clone())
        .filter(|id| !id.is_empty())
        .collect();

    Ok(all
        .into_iter()
        .enumerate()
        .filter(|(i, m)| {
            kept_turns.contains(i)
                || (m.role == "tool"
                    && m.tool_call_id
                        .as_ref()
                        .is_some_and(|id| kept_call_ids.contains(id)))
        })
        .map(|(_, m)| m)
        .collect())
}

/// Rebuild the LLM message array from persisted messages, preserving tool-call
/// round-trips: an assistant `tool_calls` entry is emitted for assistant
/// messages carrying a tool call, and a matching `role:"tool"` message with its
/// `tool_call_id` is emitted for each tool result.
pub fn replay_messages(messages: &[AiMessage]) -> Vec<serde_json::Value> {
    // Only replay IDs represented by both an assistant tool call and its tool
    // result. Inconsistent/legacy rows must not create an orphaned tool message
    // or an assistant tool_calls entry that templates reject.
    let call_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter(|m| m.role == "assistant")
        .filter(|m| m.tool_name.as_ref().is_some_and(|name| !name.is_empty()))
        .filter_map(|m| m.tool_call_id.clone())
        .filter(|s| !s.is_empty())
        .collect();
    let result_ids: std::collections::HashSet<String> = messages
        .iter()
        .filter(|m| m.role == "tool")
        .filter(|m| m.tool_result.is_some())
        .filter_map(|m| m.tool_call_id.clone())
        .filter(|s| !s.is_empty())
        .collect();
    let paired_ids: std::collections::HashSet<String> =
        call_ids.intersection(&result_ids).cloned().collect();

    messages
        .iter()
        .filter_map(|m| match m.role.as_str() {
            "assistant" => {
                if let (Some(name), Some(call_id)) = (m.tool_name.as_ref(), m.tool_call_id.as_ref())
                {
                    if !name.is_empty() && !call_id.is_empty() {
                        if !paired_ids.contains(call_id) {
                            // Preserve actual assistant text, but never replay an
                            // empty shell for a tool call that has no result.
                            return (!m.content.trim().is_empty()).then(
                                || serde_json::json!({ "role": "assistant", "content": m.content }),
                            );
                        }
                        let arguments = m
                            .tool_args
                            .clone()
                            .filter(|s| !s.is_empty())
                            .unwrap_or_else(|| "{}".to_string());
                        return Some(serde_json::json!({
                            "role": "assistant",
                            "content": m.content,
                            "tool_calls": [{
                                "id": call_id,
                                "type": "function",
                                "function": { "name": name, "arguments": arguments }
                            }],
                        }));
                    }
                }
                Some(serde_json::json!({ "role": "assistant", "content": m.content }))
            }
            "tool" => {
                let id = m.tool_call_id.clone().unwrap_or_default();
                if id.is_empty() || !paired_ids.contains(&id) {
                    None
                } else {
                    Some(serde_json::json!({
                        "role": "tool",
                        "tool_call_id": id,
                        "content": m.tool_result.clone().unwrap_or_default(),
                    }))
                }
            }
            role => Some(serde_json::json!({ "role": role, "content": m.content })),
        })
        .collect()
}

fn map_conversation(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiConversation> {
    Ok(AiConversation {
        id: row.get(0)?,
        title: row.get(1)?,
        model: row.get(2)?,
        source: row.get(3)?,
        source_entry_id: row.get(4)?,
        created_by: row.get(8).ok(),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        is_deleted: row.get::<_, i32>(7)? != 0,
    })
}

fn map_message(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiMessage> {
    Ok(AiMessage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: row.get(2)?,
        content: row.get(3)?,
        tool_name: row.get(4)?,
        tool_args: row.get(5)?,
        tool_result: row.get(6)?,
        tool_call_id: row.get(7).ok(),
        confirmed: row.get::<_, Option<i32>>(8)?.map(|v| v != 0),
        model: row.get(9)?,
        tokens_in: row.get(10)?,
        tokens_out: row.get(11)?,
        created_at: row.get(12)?,
        source_entry_id: row.get(13)?,
        created_by: row.get(14)?,
        last_reinforced: row.get(15)?,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct ProposedInboxItem {
    pub id: String,
    pub conversation_id: Option<String>,
    pub source_entry_id: Option<String>,
    pub item_type: String,
    pub proposed_title: String,
    pub proposed_data: String,
    pub source_text: Option<String>,
    pub status: String,
    pub confidence: Option<f64>,
    pub created_by: Option<String>,
    pub accepted_task_id: Option<String>,
    pub accepted_by: Option<String>,
    pub accepted_at: Option<String>,
    pub last_reinforced: Option<String>,
    pub created_at: String,
}

pub fn ensure_proposed_inbox_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS proposed_inbox (
            id TEXT PRIMARY KEY,
            conversation_id TEXT,
            source_entry_id TEXT,
            item_type TEXT NOT NULL,
            proposed_title TEXT NOT NULL,
            proposed_data TEXT NOT NULL,
            source_text TEXT,
            status TEXT NOT NULL DEFAULT 'pending',
            confidence REAL,
            created_by TEXT,
            accepted_task_id TEXT,
            accepted_by TEXT,
            accepted_at TEXT,
            last_reinforced TEXT,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_proposed_item(
    conn: &Connection,
    id: &str,
    conversation_id: Option<&str>,
    source_entry_id: Option<&str>,
    item_type: &str,
    proposed_title: &str,
    proposed_data: &str,
    source_text: Option<&str>,
    confidence: Option<f64>,
    created_by: Option<&str>,
    created_at: &str,
) -> Result<(), AppError> {
    ensure_proposed_inbox_table(conn)?;
    conn.execute(
        "INSERT INTO proposed_inbox (
            id, conversation_id, source_entry_id, item_type, proposed_title,
            proposed_data, source_text, status, confidence, created_by, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, ?10)",
        params![
            id,
            conversation_id,
            source_entry_id,
            item_type,
            proposed_title,
            proposed_data,
            source_text,
            confidence,
            created_by,
            created_at,
        ],
    )?;
    Ok(())
}

pub fn list_proposed(
    conn: &Connection,
    status: Option<&str>,
    limit: u32,
) -> Result<Vec<ProposedInboxItem>, AppError> {
    ensure_proposed_inbox_table(conn)?;

    let sql = if status.is_some() {
        "SELECT id, conversation_id, source_entry_id, item_type, proposed_title, proposed_data, source_text, status, confidence, created_by, accepted_task_id, accepted_by, accepted_at, last_reinforced, created_at FROM proposed_inbox WHERE status = ?1 ORDER BY created_at DESC LIMIT ?2"
    } else {
        "SELECT id, conversation_id, source_entry_id, item_type, proposed_title, proposed_data, source_text, status, confidence, created_by, accepted_task_id, accepted_by, accepted_at, last_reinforced, created_at FROM proposed_inbox ORDER BY created_at DESC LIMIT ?1"
    };

    let mut stmt = conn.prepare(sql)?;

    let mut items = Vec::new();
    if let Some(s) = status {
        let mut rows = stmt.query(params![s, limit])?;
        while let Some(row) = rows.next()? {
            items.push(map_proposed(row));
        }
    } else {
        let mut rows = stmt.query(params![limit])?;
        while let Some(row) = rows.next()? {
            items.push(map_proposed(row));
        }
    }
    Ok(items)
}

pub fn get_proposal(conn: &Connection, id: &str) -> Result<Option<ProposedInboxItem>, AppError> {
    ensure_proposed_inbox_table(conn)?;
    let mut stmt = conn.prepare("SELECT id, conversation_id, source_entry_id, item_type, proposed_title, proposed_data, source_text, status, confidence, created_by, accepted_task_id, accepted_by, accepted_at, last_reinforced, created_at FROM proposed_inbox WHERE id = ?1 LIMIT 1")?;
    let row = stmt.query_row(params![id], |row| Ok(map_proposed(row)));
    match row {
        Ok(item) => Ok(Some(item)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

fn map_proposed(row: &rusqlite::Row<'_>) -> ProposedInboxItem {
    ProposedInboxItem {
        id: row.get(0).unwrap_or_default(),
        conversation_id: row.get(1).ok(),
        source_entry_id: row.get(2).ok(),
        item_type: row.get(3).unwrap_or_default(),
        proposed_title: row.get(4).unwrap_or_default(),
        proposed_data: row.get(5).unwrap_or_default(),
        source_text: row.get(6).ok(),
        status: row.get(7).unwrap_or_else(|_| "pending".into()),
        confidence: row.get(8).ok(),
        created_by: row.get(9).ok(),
        accepted_task_id: row.get(10).ok(),
        accepted_by: row.get(11).ok(),
        accepted_at: row.get(12).ok(),
        last_reinforced: row.get(13).ok(),
        created_at: row.get(14).unwrap_or_default(),
    }
}

pub fn update_proposal_status(
    conn: &Connection,
    id: &str,
    status: &str,
    accepted_task_id: Option<&str>,
    accepted_by: Option<&str>,
    accepted_at: Option<&str>,
) -> Result<(), AppError> {
    ensure_proposed_inbox_table(conn)?;
    conn.execute(
        "UPDATE proposed_inbox SET status = ?1, accepted_task_id = ?2, accepted_by = ?3, accepted_at = ?4 WHERE id = ?5",
        params![status, accepted_task_id, accepted_by, accepted_at, id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(id: &str, role: &str, content: &str, created_at: &str) -> AiMessage {
        AiMessage {
            id: id.into(),
            conversation_id: "conv-1".into(),
            role: role.into(),
            content: content.into(),
            tool_name: None,
            tool_args: None,
            tool_result: None,
            tool_call_id: None,
            confirmed: None,
            model: None,
            tokens_in: None,
            tokens_out: None,
            created_at: created_at.into(),
            source_entry_id: None,
            created_by: None,
            last_reinforced: None,
        }
    }

    #[test]
    fn replay_pairs_assistant_tool_call_with_tool_message() {
        let mut assistant = msg(
            "a1",
            "assistant",
            "Checking your schedule now.",
            "2026-01-01T00:00:01Z",
        );
        assistant.tool_name = Some("list_tasks".into());
        assistant.tool_args = Some("{\"status\":\"todo\"}".into());
        assistant.tool_call_id = Some("call_1".into());

        let mut tool = msg("t1", "tool", "", "2026-01-01T00:00:02Z");
        tool.tool_call_id = Some("call_1".into());
        tool.tool_result = Some("{\"success\":true}".into());

        let replayed = replay_messages(&[
            msg("u1", "user", "hi", "2026-01-01T00:00:00Z"),
            assistant,
            tool,
        ]);

        assert_eq!(replayed[1]["role"], "assistant");
        assert_eq!(replayed[1]["content"], "Checking your schedule now.");
        assert_eq!(replayed[1]["tool_calls"][0]["id"], "call_1");
        assert_eq!(
            replayed[1]["tool_calls"][0]["function"]["name"],
            "list_tasks"
        );
        assert_eq!(replayed[2]["role"], "tool");
        assert_eq!(replayed[2]["tool_call_id"], "call_1");
        assert_eq!(replayed[2]["content"], "{\"success\":true}");
    }

    #[test]
    fn replay_plain_assistant_has_no_tool_calls() {
        let replayed = replay_messages(&[msg("a1", "assistant", "hello", "2026-01-01T00:00:01Z")]);
        assert!(replayed[0].get("tool_calls").is_none());
        assert_eq!(replayed[0]["content"], "hello");
    }

    #[test]
    fn replay_drops_unpaired_tool_calls_and_tool_results() {
        let mut assistant = msg("a1", "assistant", "", "2026-01-01T00:00:01Z");
        assistant.tool_name = Some("list_tasks".into());
        assistant.tool_args = Some("{}".into());
        assistant.tool_call_id = Some("call_without_result".into());

        let mut tool = msg("t1", "tool", "", "2026-01-01T00:00:02Z");
        tool.tool_call_id = Some("result_without_call".into());
        tool.tool_result = Some("{}".into());

        let replayed = replay_messages(&[assistant, tool]);
        assert!(
            replayed.is_empty(),
            "unpaired tool history must not be replayed: {replayed:?}"
        );
    }

    fn test_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run_pending(&mut conn).unwrap();
        crate::db::ai::upsert_conversation(
            &conn,
            "conv-1",
            Some("t"),
            "m",
            "sidebar",
            None,
            Some("user"),
        )
        .unwrap();
        conn
    }

    #[test]
    fn recent_messages_keeps_tool_pairs_and_bounds_turns() {
        let conn = test_conn();

        let mut u1 = msg("u1", "user", "first question", "2026-01-01T00:00:00Z");
        u1.conversation_id = "conv-1".into();
        insert_message(&conn, &u1).unwrap();

        let mut a1 = msg("a1", "assistant", "", "2026-01-01T00:00:01Z");
        a1.conversation_id = "conv-1".into();
        a1.tool_name = Some("list_tasks".into());
        a1.tool_args = Some("{}".into());
        a1.tool_call_id = Some("call_1".into());
        insert_message(&conn, &a1).unwrap();

        let mut t1 = msg("t1", "tool", "", "2026-01-01T00:00:02Z");
        t1.conversation_id = "conv-1".into();
        t1.tool_call_id = Some("call_1".into());
        t1.tool_result = Some("{\"success\":true}".into());
        insert_message(&conn, &t1).unwrap();

        let mut a2 = msg("a2", "assistant", "done", "2026-01-01T00:00:03Z");
        a2.conversation_id = "conv-1".into();
        insert_message(&conn, &a2).unwrap();

        let rows = recent_messages(&conn, "conv-1", 20).unwrap();
        let roles: Vec<&str> = rows.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["user", "assistant", "tool", "assistant"]);

        // A single-turn window keeps the tool result but drops the older user.
        let rows = recent_messages(&conn, "conv-1", 1).unwrap();
        let roles: Vec<&str> = rows.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["assistant"]);
    }

    #[test]
    fn recent_messages_window_excludes_older_turns() {
        let conn = test_conn();
        // Three plain turns, no tools.
        for (i, role) in [(0, "user"), (1, "assistant"), (2, "user")].iter() {
            let mut m = msg(
                &format!("m{i}"),
                role,
                "text",
                &format!("2026-01-01T00:00:0{i}Z"),
            );
            m.conversation_id = "conv-1".into();
            insert_message(&conn, &m).unwrap();
        }
        let rows = recent_messages(&conn, "conv-1", 2).unwrap();
        let roles: Vec<&str> = rows.iter().map(|m| m.role.as_str()).collect();
        // Last two turns only: assistant + user, the first user is outside.
        assert_eq!(roles, vec!["assistant", "user"]);
    }

    #[test]
    fn list_messages_orders_tool_pair_by_insertion_when_timestamps_tie() {
        let conn = test_conn();
        // Assistant tool-call and its tool result share a timestamp; rowid must
        // break the tie so the call always precedes its result.
        let ts = "2026-01-01T00:00:01Z";
        let mut a = msg("a1", "assistant", "", ts);
        a.conversation_id = "conv-1".into();
        a.tool_name = Some("list_tasks".into());
        a.tool_args = Some("{}".into());
        a.tool_call_id = Some("call_1".into());
        insert_message(&conn, &a).unwrap();

        let mut t = msg("t1", "tool", "", ts);
        t.conversation_id = "conv-1".into();
        t.tool_call_id = Some("call_1".into());
        t.tool_result = Some("{}".into());
        insert_message(&conn, &t).unwrap();

        let rows = list_messages(&conn, "conv-1").unwrap();
        let roles: Vec<&str> = rows.iter().map(|m| m.role.as_str()).collect();
        assert_eq!(roles, vec!["assistant", "tool"]);
    }
}
