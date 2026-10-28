use serde_json::{json, Value};
use std::sync::Arc;

use crate::db::pool;

pub struct ToolExecutor {
    db_pool: Arc<pool::DbPool>,
    // T-05: decrypted in-memory mirror of journal entries. When present,
    // search_journal runs against it — the on-disk `content` column holds
    // AES-GCM ciphertext that never matches a LIKE query.
    mem_db: Option<Arc<tokio::sync::Mutex<rusqlite::Connection>>>,
}

/// Upper bound for a tool response sent back to the model. Read-only tools can
/// return long lists; without a cap a single journal search or event listing
/// can consume the whole context window.
const MAX_TOOL_MESSAGE_CHARS: usize = 8_000;

fn literal_like_pattern(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

pub struct ToolResult {
    pub success: bool,
    pub message: String,
    pub data: Value,
    pub proposal_id: Option<String>,
}

impl ToolResult {
    /// Serialize the full result for the model's `tool` message. The model
    /// needs `data` (IDs, rows, snippets) to continue the turn, not just the
    /// human-readable summary.
    pub fn to_model_content(&self) -> String {
        let envelope = json!({
            "success": self.success,
            "message": self.message,
            "data": self.data,
        });
        let serialized = envelope.to_string();
        if serialized.chars().count() <= MAX_TOOL_MESSAGE_CHARS {
            return serialized;
        }

        let preview: String = serialized
            .chars()
            .take(MAX_TOOL_MESSAGE_CHARS.saturating_sub(512))
            .collect();
        let fallback = json!({
            "success": self.success,
            "message": self.message,
            "data": {"truncated": true, "preview": preview},
        })
        .to_string();
        if fallback.chars().count() <= MAX_TOOL_MESSAGE_CHARS {
            fallback
        } else {
            fallback.chars().take(MAX_TOOL_MESSAGE_CHARS).collect()
        }
    }
}

impl ToolExecutor {
    pub fn new(db_pool: Arc<pool::DbPool>) -> Self {
        Self {
            db_pool,
            mem_db: None,
        }
    }

    /// Construct with the vault's decrypted in-memory journal mirror so
    /// search_journal returns real text instead of ciphertext.
    pub fn new_with_vault(
        db_pool: Arc<pool::DbPool>,
        mem_db: Arc<tokio::sync::Mutex<rusqlite::Connection>>,
    ) -> Self {
        Self {
            db_pool,
            mem_db: Some(mem_db),
        }
    }

    pub async fn execute(&self, name: &str, args: Value) -> ToolResult {
        match name {
            "list_tasks" => self.exec_list_tasks(args).await,
            "list_events" => self.exec_list_events(args).await,
            "list_notes" => self.exec_list_notes(args).await,
            "search_journal" => self.exec_search_journal(args).await,
            "create_task" => self.propose_create_task(args).await,
            "update_task" => self.propose_update_task(args).await,
            "complete_task" => self.propose_complete_task(args).await,
            "create_event" => self.propose_event_action(args, "create").await,
            "update_event" => self.propose_event_action(args, "update").await,
            "delete_event" => self.propose_event_action(args, "delete").await,
            "create_note" => self.propose_note_action(args, "create").await,
            "update_note" => self.propose_note_action(args, "update").await,
            "delete_note" => self.propose_note_action(args, "delete").await,
            "list_memory" => self.exec_list_memory(args).await,
            "create_memory" => self.propose_memory_action(args, "create").await,
            "update_memory" => self.propose_memory_action(args, "update").await,
            "delete_memory" => self.propose_memory_action(args, "delete").await,
            "link_memory" => self.propose_memory_action(args, "link").await,
            _ => ToolResult {
                success: false,
                message: format!("Unknown tool: {}", name),
                data: json!(null),
                proposal_id: None,
            },
        }
    }

    async fn exec_list_tasks(&self, args: Value) -> ToolResult {
        let status = args
            .get("status")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let all = pool::with_conn(self.db_pool.clone(), |conn| {
            crate::db::tasks::list_tasks(conn, false)
        })
        .await
        .unwrap_or_default();

        let filtered: Vec<_> = if let Some(s) = status {
            all.iter().filter(|t| t.status == s).collect()
        } else {
            all.iter().collect()
        };

        let summaries: Vec<Value> = filtered
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "title": t.title,
                    "status": t.status,
                    "priority": t.priority,
                    "due_date": t.due_date,
                })
            })
            .collect();

        ToolResult {
            success: true,
            message: format!("Found {} task(s).", summaries.len()),
            data: json!({"tasks": summaries, "count": summaries.len()}),
            proposal_id: None,
        }
    }

    async fn exec_list_events(&self, args: Value) -> ToolResult {
        let start_after = args
            .get("start_after")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());
        let limit = list_events_limit(&args);

        let events = pool::with_conn(self.db_pool.clone(), move |conn| {
            crate::db::events::list_events(conn, Some(&start_after), Some(limit))
        })
        .await
        .unwrap_or_default();

        let summaries: Vec<Value> = events
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "start_at": e.start_at,
                    "location": e.location,
                })
            })
            .collect();

        ToolResult {
            success: true,
            message: format!("Found {} event(s).", summaries.len()),
            data: json!({"events": summaries, "count": summaries.len()}),
            proposal_id: None,
        }
    }

    async fn exec_list_notes(&self, args: Value) -> ToolResult {
        let limit = args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, 50) as u32)
            .unwrap_or(20);

        let notes = pool::with_conn(self.db_pool.clone(), move |conn| {
            crate::db::events::list_notes(conn, Some(limit))
        })
        .await
        .unwrap_or_default();

        let summaries: Vec<Value> = notes
            .iter()
            .map(|n| {
                json!({
                    "id": n.id,
                    "title": n.title,
                    "content": n.content.chars().take(200).collect::<String>(),
                    "tags": n.tags,
                    "updated_at": n.updated_at,
                })
            })
            .collect();

        ToolResult {
            success: true,
            message: format!("Found {} note(s).", summaries.len()),
            data: json!({"notes": summaries, "count": summaries.len()}),
            proposal_id: None,
        }
    }

    async fn propose_event_action(&self, args: Value, action: &str) -> ToolResult {
        let needs_title = action == "create";
        if needs_title
            && args
                .get("title")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
        {
            return ToolResult {
                success: false,
                message: "create_event requires a non-empty 'title'.".into(),
                data: json!({"error": "missing_required", "field": "title"}),
                proposal_id: None,
            };
        }
        if needs_title
            && args
                .get("start_at")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
        {
            return ToolResult {
                success: false,
                message: "create_event requires a non-empty 'start_at' ISO 8601 timestamp.".into(),
                data: json!({"error": "missing_required", "field": "start_at"}),
                proposal_id: None,
            };
        }
        if !needs_title
            && args
                .get("id")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
        {
            return ToolResult {
                success: false,
                message: format!(
                    "{}_event requires a real event 'id' (use list_events first).",
                    action
                ),
                data: json!({"error": "missing_required", "field": "id"}),
                proposal_id: None,
            };
        }

        let proposal_id = uuid::Uuid::new_v4().to_string();
        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!(format!("event_{}", action)));
        let data_str = Value::Object(data_obj).to_string();

        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let summary = match action {
            "create" => format!("Create event: {}", title),
            "delete" => format!(
                "Delete event {}",
                args.get("id").and_then(|v| v.as_str()).unwrap_or("")
            ),
            _ => format!(
                "Update event {}",
                args.get("id").and_then(|v| v.as_str()).unwrap_or("")
            ),
        };

        if let Err(error) = self
            .insert_ai_proposal(proposal_id.clone(), "event", &summary, data_str)
            .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not create event proposal: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: "Proposed event change. User must confirm in the Review Inbox.".into(),
            data: json!({"proposal_id": proposal_id}),
            proposal_id: Some(proposal_id),
        }
    }

    async fn propose_note_action(&self, args: Value, action: &str) -> ToolResult {
        if action == "create" {
            if args
                .get("content")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
            {
                return ToolResult {
                    success: false,
                    message: "create_note requires non-empty 'content'.".into(),
                    data: json!({"error": "missing_required", "field": "content"}),
                    proposal_id: None,
                };
            }
        } else if args
            .get("id")
            .and_then(|v| v.as_str())
            .is_none_or(|s| s.trim().is_empty())
        {
            return ToolResult {
                success: false,
                message: format!(
                    "{}_note requires a real note 'id' (use list_notes first).",
                    action
                ),
                data: json!({"error": "missing_required", "field": "id"}),
                proposal_id: None,
            };
        }

        let proposal_id = uuid::Uuid::new_v4().to_string();
        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!(format!("note_{}", action)));
        let data_str = Value::Object(data_obj).to_string();

        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let summary = match action {
            "create" => format!("Create note: {}", title),
            "delete" => format!(
                "Delete note {}",
                args.get("id").and_then(|v| v.as_str()).unwrap_or("")
            ),
            _ => format!(
                "Update note {}",
                args.get("id").and_then(|v| v.as_str()).unwrap_or("")
            ),
        };

        if let Err(error) = self
            .insert_ai_proposal(proposal_id.clone(), "note", &summary, data_str)
            .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not create note proposal: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: "Proposed note change. User must confirm in the Review Inbox.".into(),
            data: json!({"proposal_id": proposal_id}),
            proposal_id: Some(proposal_id),
        }
    }

    async fn insert_ai_proposal(
        &self,
        proposal_id: String,
        item_type: &str,
        summary: &str,
        data: String,
    ) -> Result<(), crate::error::AppError> {
        let pool = self.db_pool.clone();
        let item_type = item_type.to_string();
        let summary = summary.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        pool::with_conn(pool, move |conn| {
            crate::db::ai::insert_proposed_item(
                conn,
                &proposal_id,
                None,
                None,
                &item_type,
                &summary,
                &data,
                None,
                None,
                Some("ai"),
                &now,
            )
        })
        .await
    }

    async fn exec_search_journal(&self, args: Value) -> ToolResult {
        let query = args
            .get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if query.is_empty() {
            return ToolResult {
                success: false,
                message: "No search query provided.".into(),
                data: json!(null),
                proposal_id: None,
            };
        }

        // T-05: prefer the decrypted in-memory mirror (vault unlocked).
        // Fall back to the disk query only when no vault mirror is attached.
        let snippets: Vec<crate::db::journal::JournalContextSnippet> =
            if let Some(mem_db) = &self.mem_db {
                let mem = mem_db.lock().await;
                let search_term = literal_like_pattern(&query);
                let mut stmt = match mem.prepare(
                    "SELECT id, title, substr(content, 1, 280) as snippet, created_at
                         FROM mem_journal_entries
                         WHERE content LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
                         LIMIT 10",
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        return ToolResult {
                            success: false,
                            message: format!("Journal search failed: {}", e),
                            data: json!(null),
                            proposal_id: None,
                        };
                    }
                };
                let rows: Vec<crate::db::journal::JournalContextSnippet> = stmt
                    .query_map(rusqlite::params![search_term], |row| {
                        Ok(crate::db::journal::JournalContextSnippet {
                            id: row.get(0)?,
                            title: row.get(1)?,
                            snippet: row.get(2)?,
                            created_at: row.get(3)?,
                        })
                    })
                    .map(|rows| rows.filter_map(|r| r.ok()).collect())
                    .unwrap_or_default();
                rows
            } else {
                let query_clone = query.clone();
                pool::with_conn(self.db_pool.clone(), move |conn| {
                    crate::db::journal::search_context_snippets(conn, &query_clone, 10)
                })
                .await
                .unwrap_or_default()
            };

        let results: Vec<Value> = snippets
            .iter()
            .map(|s| {
                json!({
                    "id": s.id,
                    "title": s.title,
                    "snippet": s.snippet,
                    "created_at": s.created_at,
                })
            })
            .collect();

        ToolResult {
            success: true,
            message: format!("Found {} journal entry/entries.", results.len()),
            data: json!({"results": results, "count": results.len()}),
            proposal_id: None,
        }
    }

    async fn propose_create_task(&self, args: Value) -> ToolResult {
        let title = match args.get("title").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            Some(_) => {
                return ToolResult {
                    success: false,
                    message: "create_task requires a non-empty 'title'.".into(),
                    data: json!({"error": "missing_required", "field": "title"}),
                    proposal_id: None,
                };
            }
            None => {
                return ToolResult {
                    success: false,
                    message: "create_task requires a non-empty 'title'.".into(),
                    data: json!({"error": "missing_required", "field": "title"}),
                    proposal_id: None,
                };
            }
        };
        let now = chrono::Utc::now().to_rfc3339();
        let proposal_id = uuid::Uuid::new_v4().to_string();
        let proposal_id_clone = proposal_id.clone();
        let title_clone = title.clone();

        // Ensure "type": "task" is in the data for proposed_action parser
        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!("task"));
        let data_str = Value::Object(data_obj).to_string();

        let pool = self.db_pool.clone();
        if let Err(error) = pool::with_conn(pool, move |conn| {
            crate::db::ai::insert_proposed_item(
                conn,
                &proposal_id_clone,
                None,
                None,
                "task", // Aligned with proposed_action "task" branch
                &title_clone,
                &data_str,
                None,
                None,
                Some("ai"),
                &now,
            )
        })
        .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not create task proposal: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: format!(
                "Proposed creating task: \"{}\". User must confirm in Inbox.",
                title
            ),
            data: json!({"proposal_id": proposal_id, "title": title}),
            proposal_id: Some(proposal_id),
        }
    }

    async fn propose_update_task(&self, args: Value) -> ToolResult {
        let task_id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => {
                return ToolResult {
                    success: false,
                    message: "update_task requires a task 'id' (use list_tasks first).".into(),
                    data: json!({"error": "missing_required", "field": "id"}),
                    proposal_id: None,
                };
            }
        };
        let now = chrono::Utc::now().to_rfc3339();
        let proposal_id = uuid::Uuid::new_v4().to_string();
        let proposal_id_clone = proposal_id.clone();
        let summary = format!("Update task {}", task_id);

        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!("task_update"));
        let data_str = Value::Object(data_obj).to_string();

        let pool = self.db_pool.clone();
        if let Err(error) = pool::with_conn(pool, move |conn| {
            crate::db::ai::insert_proposed_item(
                conn,
                &proposal_id_clone,
                None,
                None,
                "task",
                &summary,
                &data_str,
                None,
                None,
                Some("ai"),
                &now,
            )
        })
        .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not create task update proposal: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: "Proposed task update. User must confirm in Inbox.".into(),
            data: json!({"proposal_id": proposal_id}),
            proposal_id: Some(proposal_id),
        }
    }

    async fn propose_complete_task(&self, args: Value) -> ToolResult {
        let task_id = match args.get("id").and_then(|v| v.as_str()) {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => {
                return ToolResult {
                    success: false,
                    message: "complete_task requires a task 'id' (use list_tasks first).".into(),
                    data: json!({"error": "missing_required", "field": "id"}),
                    proposal_id: None,
                };
            }
        };
        let now = chrono::Utc::now().to_rfc3339();
        let proposal_id = uuid::Uuid::new_v4().to_string();
        let proposal_id_clone = proposal_id.clone();
        let summary = format!("Complete task {}", task_id);

        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!("task_complete"));
        let data_str = Value::Object(data_obj).to_string();

        let pool = self.db_pool.clone();
        if let Err(error) = pool::with_conn(pool, move |conn| {
            crate::db::ai::insert_proposed_item(
                conn,
                &proposal_id_clone,
                None,
                None,
                "task",
                &summary,
                &data_str,
                None,
                None,
                Some("ai"),
                &now,
            )
        })
        .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not create task completion proposal: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: "Proposed marking task as complete. User must confirm in Inbox.".into(),
            data: json!({"proposal_id": proposal_id}),
            proposal_id: Some(proposal_id),
        }
    }

    async fn exec_list_memory(&self, args: Value) -> ToolResult {
        let search = args
            .get("search")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let items = pool::with_conn(self.db_pool.clone(), move |conn| {
            crate::db::knowledge::list_items(conn, None, None, search.as_deref())
        })
        .await
        .unwrap_or_default();

        let summaries: Vec<Value> = items
            .iter()
            .map(|i| {
                json!({
                    "id": i.id,
                    "title": i.title,
                    "kind": i.kind,
                    "category": i.category,
                    "content": i.content,
                    "tags": i.tags,
                })
            })
            .collect();

        ToolResult {
            success: true,
            message: format!("Found {} memory item(s).", summaries.len()),
            data: json!({"items": summaries, "count": summaries.len()}),
            proposal_id: None,
        }
    }

    async fn propose_memory_action(&self, args: Value, action: &str) -> ToolResult {
        if action != "create" {
            if args
                .get("item_id")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
            {
                return ToolResult {
                    success: false,
                    message: format!("{}_memory requires a non-empty 'item_id'.", action).into(),
                    data: json!({"error": "missing_required", "field": "item_id"}),
                    proposal_id: None,
                };
            }
        }
        if action == "create" {
            if args
                .get("title")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
                || args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| s.trim().is_empty())
            {
                return ToolResult {
                    success: false,
                    message: "create_memory requires 'title' and 'content'.".into(),
                    data: json!({"error": "missing_required"}),
                    proposal_id: None,
                };
            }
        }
        if action == "link" {
            if args
                .get("source_id")
                .and_then(|v| v.as_str())
                .is_none_or(|s| s.trim().is_empty())
                || args
                    .get("target_id")
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| s.trim().is_empty())
                || args
                    .get("relation")
                    .and_then(|v| v.as_str())
                    .is_none_or(|s| s.trim().is_empty())
            {
                return ToolResult {
                    success: false,
                    message: "link_memory requires 'source_id', 'target_id', and 'relation'."
                        .into(),
                    data: json!({"error": "missing_required"}),
                    proposal_id: None,
                };
            }
        }
        let now = chrono::Utc::now().to_rfc3339();
        let proposal_id = uuid::Uuid::new_v4().to_string();
        let proposal_id_clone = proposal_id.clone();

        let mut data_obj = args.as_object().cloned().unwrap_or_default();
        data_obj.insert("type".to_string(), json!("memory"));
        data_obj.insert("memory_action".to_string(), json!(action));
        let title = args
            .get("title")
            .and_then(|v| v.as_str())
            .or_else(|| args.get("item_id").and_then(|v| v.as_str()))
            .unwrap_or("memory")
            .to_string();
        let data_str = Value::Object(data_obj).to_string();

        let title_suffix = if title.is_empty() {
            String::new()
        } else {
            format!(": {}", title)
        };
        let summary = format!("{} memory item{}", action, title_suffix);

        let pool = self.db_pool.clone();
        if let Err(error) = pool::with_conn(pool, move |conn| {
            crate::db::ai::insert_proposed_item(
                conn,
                &proposal_id_clone,
                None,
                None,
                "memory",
                &summary,
                &data_str,
                None,
                None,
                Some("ai"),
                &now,
            )
        })
        .await
        {
            return ToolResult {
                success: false,
                message: format!("Could not propose memory action: {error}"),
                data: json!(null),
                proposal_id: None,
            };
        }

        ToolResult {
            success: true,
            message: "Proposed memory update. User must confirm in Inbox.".into(),
            data: json!({"proposal_id": proposal_id}),
            proposal_id: Some(proposal_id),
        }
    }
}

fn list_events_limit(args: &Value) -> u32 {
    args.get("limit")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as u32)
        .unwrap_or(20)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result_with(data: Value) -> ToolResult {
        ToolResult {
            success: true,
            message: "Found 1 task(s).".into(),
            data,
            proposal_id: None,
        }
    }

    #[test]
    fn tool_result_content_should_include_message_and_data() {
        let content = result_with(json!({
            "tasks": [{"id": "task-abc", "title": "Fix faucet"}],
            "count": 1
        }))
        .to_model_content();

        assert!(content.contains("Found 1 task(s)."));
        assert!(
            content.contains("task-abc"),
            "tool result must expose IDs to the model: {content}"
        );
        assert!(content.contains("\"success\":true"));
    }

    #[test]
    fn tool_result_content_should_mark_failures() {
        let content = ToolResult {
            success: false,
            message: "No search query provided.".into(),
            data: json!(null),
            proposal_id: None,
        }
        .to_model_content();

        assert!(content.contains("\"success\":false"));
        assert!(content.contains("No search query provided."));
    }

    #[test]
    fn tool_result_content_should_stay_bounded_when_data_is_huge() {
        let content = result_with(json!({"blob": "x".repeat(20_000)})).to_model_content();
        assert!(
            content.chars().count() <= MAX_TOOL_MESSAGE_CHARS,
            "tool result exceeded the context budget: {} chars",
            content.chars().count()
        );
        assert!(content.contains("\"truncated\":true"));
    }

    #[test]
    fn list_events_limit_defaults_and_clamps_without_integer_wrap() {
        assert_eq!(list_events_limit(&json!({})), 20);
        assert_eq!(list_events_limit(&json!({"limit": 0})), 1);
        assert_eq!(list_events_limit(&json!({"limit": 25})), 25);
        assert_eq!(list_events_limit(&json!({"limit": 101})), 100);
        assert_eq!(list_events_limit(&json!({"limit": u64::MAX})), 100);
    }

    #[test]
    fn task_and_event_tool_schemas_include_supported_fields() {
        let tools = get_tools();
        let function = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["function"]["name"] == name)
                .unwrap()
        };

        let status_values = function("update_task")["function"]["parameters"]["properties"]
            ["status"]["enum"]
            .as_array()
            .unwrap();
        assert!(status_values.contains(&json!("archived")));
        assert_eq!(
            function("list_events")["function"]["parameters"]["properties"]["limit"]["type"],
            "integer"
        );
    }

    #[test]
    fn ai_tool_schema_has_no_external_fetch_tool() {
        assert!(!get_tools()
            .iter()
            .any(|tool| tool["function"]["name"] == "fetch_url"));
    }

    #[test]
    fn event_and_note_tool_schemas_include_supported_fields() {
        let tools = get_tools();
        let function = |name: &str| {
            tools
                .iter()
                .find(|tool| tool["function"]["name"] == name)
                .unwrap_or_else(|| panic!("missing tool schema: {name}"))
        };

        assert_eq!(
            function("list_notes")["function"]["parameters"]["properties"]["limit"]["type"],
            "integer"
        );
        for name in [
            "create_event",
            "update_event",
            "delete_event",
            "create_note",
            "update_note",
            "delete_note",
        ] {
            let description = function(name)["function"]["description"]
                .as_str()
                .unwrap_or_default();
            assert!(
                description.contains("Review Inbox"),
                "{name} must describe the confirmation gate: {description}"
            );
        }
        assert_eq!(
            function("create_event")["function"]["parameters"]["required"],
            json!(["title", "start_at"])
        );
        assert_eq!(
            function("create_note")["function"]["parameters"]["required"],
            json!(["content"])
        );
        assert_eq!(
            function("delete_note")["function"]["parameters"]["required"],
            json!(["id"])
        );
    }

    #[test]
    fn literal_like_pattern_matches_percent_underscore_and_backslash_as_text() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let literal = r"100%_\done";
        let pattern = literal_like_pattern(literal);
        let matches_literal: bool = conn
            .query_row(
                "SELECT ?1 LIKE ?2 ESCAPE '\\'",
                rusqlite::params![literal, pattern],
                |row| row.get(0),
            )
            .unwrap();
        let matches_wildcard_expansion: bool = conn
            .query_row(
                "SELECT ?1 LIKE ?2 ESCAPE '\\'",
                rusqlite::params!["100XX\\done", literal_like_pattern(literal)],
                |row| row.get(0),
            )
            .unwrap();

        assert!(matches_literal);
        assert!(!matches_wildcard_expansion);
    }
}

pub fn get_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "create_task",
                "description": "Create a new task in the user's task manager. Use this when the user explicitly asks to add or remember something to do.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "The title of the task" },
                        "priority": { "type": "string", "description": "Priority level: low, medium, high, urgent", "enum": ["low", "medium", "high", "urgent"] },
                        "due_date": { "type": "string", "description": "Optional due date in YYYY-MM-DD format" },
                        "description": { "type": "string", "description": "Optional detailed description of the task" }
                    },
                    "required": ["title"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "update_task",
                "description": "Update an existing task. Use this to change priority, status, or title of a task. Requires the task ID (use list_tasks first if unknown).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the task to update" },
                        "title": { "type": "string", "description": "New title for the task" },
                        "status": { "type": "string", "description": "New status: todo, in_progress, done, archived", "enum": ["todo", "in_progress", "done", "archived"] },
                        "priority": { "type": "string", "description": "New priority: low, medium, high, urgent", "enum": ["low", "medium", "high", "urgent"] }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "complete_task",
                "description": "Mark a specific task as done. Requires the task ID.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the task to mark as completed" }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_tasks",
                "description": "List existing tasks. Use this to find task IDs or see current workload.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "status": { "type": "string", "description": "Filter by status: todo, in_progress, done. Omit for all pending tasks." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "search_journal",
                "description": "Search the user's past journal entries. Use this to answer questions about the past or find specific information previously recorded.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query or keyword" }
                    },
                    "required": ["query"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_events",
                "description": "List scheduled events and appointments. Use this to see the user's schedule or check for conflicts.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "start_after": { "type": "string", "description": "ISO8601 timestamp to list events after. Defaults to now." },
                        "limit": { "type": "integer", "description": "Max number of events to return (1-100, default 20)." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_notes",
                "description": "List the user's notes. Read-only; use this to find a real note ID before proposing an update or delete.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "limit": { "type": "integer", "description": "Max number of notes to return (1-50, default 20)." }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_event",
                "description": "Propose creating a calendar event. The user confirms in the Review Inbox before it is saved.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Event title" },
                        "start_at": { "type": "string", "description": "Start time as an ISO 8601 timestamp with timezone, e.g. 2026-11-02T09:00:00+00:00" },
                        "end_at": { "type": "string", "description": "Optional end time as an ISO 8601 timestamp" },
                        "description": { "type": "string", "description": "Optional details" },
                        "location": { "type": "string", "description": "Optional location" },
                        "is_all_day": { "type": "boolean", "description": "True for an all-day event" },
                        "reminder_at": { "type": "string", "description": "Optional reminder time as an ISO 8601 timestamp" }
                    },
                    "required": ["title", "start_at"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "update_event",
                "description": "Propose editing an existing calendar event. Requires a real event ID from list_events. The user confirms in the Review Inbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the event to update" },
                        "title": { "type": "string" },
                        "start_at": { "type": "string", "description": "New ISO 8601 start timestamp" },
                        "end_at": { "type": "string" },
                        "description": { "type": "string" },
                        "location": { "type": "string" },
                        "is_all_day": { "type": "boolean" },
                        "reminder_at": { "type": "string" }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "delete_event",
                "description": "Propose deleting an existing calendar event. Requires a real event ID from list_events. The user confirms in the Review Inbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the event to delete" }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_note",
                "description": "Propose creating a note. The user confirms in the Review Inbox before it is saved.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Optional short title" },
                        "content": { "type": "string", "description": "The note body" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional tags" }
                    },
                    "required": ["content"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "update_note",
                "description": "Propose editing an existing note. Requires a real note ID from list_notes. The user confirms in the Review Inbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the note to update" },
                        "title": { "type": "string" },
                        "content": { "type": "string" },
                        "tags": { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "delete_note",
                "description": "Propose deleting an existing note. Requires a real note ID from list_notes. The user confirms in the Review Inbox.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string", "description": "The ID of the note to delete" }
                    },
                    "required": ["id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "list_memory",
                "description": "Search the user's memory/knowledge base (memories, information, knowledge about the user). Read-only.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "search": { "type": "string", "description": "Optional keyword to filter memory items by title or content" }
                    }
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "create_memory",
                "description": "Propose adding a memory item to the user's knowledge base. The user confirms in the Review Inbox before it is saved.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "Short label for the item" },
                        "content": { "type": "string", "description": "The factual content to remember" },
                        "kind": { "type": "string", "enum": ["memory", "information", "knowledge"], "description": "memory = dated personal memory, information = fact about user, knowledge = learned concept" },
                        "category": { "type": "string", "enum": ["preference", "habit", "constraint", "person", "goal", "other"] },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "Optional tags" },
                        "item_date": { "type": "string", "description": "Optional YYYY-MM-DD date if this is a dated memory" }
                    },
                    "required": ["title", "content", "kind"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "update_memory",
                "description": "Propose editing an existing memory item (title, content, kind, category, tags, date, confidence). Requires the item ID (use list_memory first).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "item_id": { "type": "string", "description": "The ID of the memory item to edit" },
                        "title": { "type": "string" },
                        "content": { "type": "string" },
                        "kind": { "type": "string", "enum": ["memory", "information", "knowledge"] },
                        "category": { "type": "string", "enum": ["preference", "habit", "constraint", "person", "goal", "other"] },
                        "tags": { "type": "array", "items": { "type": "string" } },
                        "item_date": { "type": "string" },
                        "confidence": { "type": "number" }
                    },
                    "required": ["item_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "delete_memory",
                "description": "Propose deleting a memory item permanently. Requires the item ID.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "item_id": { "type": "string", "description": "The ID of the memory item to delete" }
                    },
                    "required": ["item_id"]
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "link_memory",
                "description": "Propose linking two memory items with a typed relation. Requires the source and target item IDs and one of the preset relations.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "source_id": { "type": "string", "description": "Source item ID" },
                        "target_id": { "type": "string", "description": "Target item ID" },
                        "relation": { "type": "string", "enum": ["related-to", "part-of", "contradicts", "cause-of", "reminds-of"] }
                    },
                    "required": ["source_id", "target_id", "relation"]
                }
            }
        }),
    ]
}
