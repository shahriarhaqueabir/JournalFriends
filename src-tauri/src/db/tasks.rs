#![allow(dead_code)]

use crate::error::AppError;
use chrono::{Datelike, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const IS_BLOCKED_COLUMN_INDEX: usize = 31;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Task {
    pub id: String,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub due_time: Option<String>,
    pub reminder_at: Option<String>,
    pub reminder_fired: bool,
    pub time_estimate: Option<i32>,
    pub time_logged: i32,
    pub tags: String,   // JSON array
    pub labels: String, // JSON array
    pub category: Option<String>,
    pub project: Option<String>,
    pub energy_level: Option<String>,
    pub context_tag: Option<String>,
    pub linked_url: Option<String>,
    pub ai_created: bool,
    pub recurrence_rule: Option<String>,
    pub is_blocked: bool,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TimeLog {
    pub id: String,
    pub task_id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration: Option<i32>,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct TaskAttachment {
    pub id: String,
    pub task_id: String,
    pub file_name: String,
    pub file_path: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i32>,
    pub file_missing: bool,
    pub created_at: String,
}

const VALID_PRIORITIES: [&str; 4] = ["low", "medium", "high", "urgent"];

/// The 001 CHECK only accepts low/medium/high/urgent, but AI proposals pass
/// free-form strings through unwrap_or("medium") only when the key is absent.
/// Normalizing at the write choke points (create_task/update_task) keeps the
/// CHECK from rejecting the whole save; unknown values fall back to "medium".
pub fn normalize_priority(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    if VALID_PRIORITIES.contains(&trimmed.as_str()) {
        trimmed
    } else {
        tracing::warn!(value = %raw, "unknown task priority; falling back to \"medium\"");
        "medium".to_string()
    }
}

pub fn create_task(conn: &Connection, task: &Task) -> Result<String, AppError> {
    conn.execute(
        "INSERT INTO tasks (
            id, parent_task_id, title, description, status, priority, 
            due_date, due_time, reminder_at, time_estimate, time_logged,
            tags, labels, category, project, energy_level, context_tag, 
            linked_url, ai_created, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21)",
        params![
            task.id,
            task.parent_task_id,
            task.title,
            task.description,
            task.status,
            normalize_priority(&task.priority),
            task.due_date,
            task.due_time,
            task.reminder_at,
            task.time_estimate,
            task.time_logged,
            task.tags,
            task.labels,
            task.category,
            task.project,
            task.energy_level,
            task.context_tag,
            task.linked_url,
            task.ai_created as i32,
            task.created_at,
            task.updated_at,
        ],
    )?;
    Ok(task.id.clone())
}

pub fn list_tasks(conn: &Connection, include_completed: bool) -> Result<Vec<Task>, AppError> {
    let query = if include_completed {
        "SELECT *, 
         (SELECT COUNT(*) FROM task_dependencies d JOIN tasks bt ON d.blocking_task_id = bt.id WHERE d.blocked_task_id = tasks.id AND bt.status != 'done') > 0 as is_blocked
         FROM tasks WHERE is_deleted = 0 ORDER BY priority DESC, due_date ASC"
    } else {
        "SELECT *, 
         (SELECT COUNT(*) FROM task_dependencies d JOIN tasks bt ON d.blocking_task_id = bt.id WHERE d.blocked_task_id = tasks.id AND bt.status != 'done') > 0 as is_blocked
         FROM tasks WHERE is_deleted = 0 AND status != 'done' AND status != 'cancelled' ORDER BY priority DESC, due_date ASC"
    };

    let mut stmt = conn.prepare(query)?;
    let rows = stmt.query_map([], |row| {
        Ok(Task {
            id: row.get(0)?,
            parent_task_id: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            status: row.get(4)?,
            priority: row.get(5)?,
            due_date: row.get(6)?,
            due_time: row.get(7)?,
            reminder_at: row.get(8)?,
            reminder_fired: row.get::<_, i32>(9)? != 0,
            time_estimate: row.get(10)?,
            time_logged: row.get(11)?,
            tags: row.get(13)?,
            labels: row.get(14)?,
            category: row.get(15)?,
            project: row.get(16)?,
            recurrence_rule: row.get(18)?,
            energy_level: row.get(20)?,
            context_tag: row.get(21)?,
            linked_url: row.get(22)?,
            ai_created: row.get::<_, i32>(24)? != 0,
            is_blocked: row.get::<_, i32>(IS_BLOCKED_COLUMN_INDEX)? != 0,
            created_at: row.get(26)?,
            updated_at: row.get(27)?,
            completed_at: row.get(28)?,
            archived: row.get::<_, i64>(30)? != 0,
        })
    })?;

    let mut tasks = Vec::new();
    for task in rows {
        tasks.push(task?);
    }
    Ok(tasks)
}

pub fn update_task_status(conn: &Connection, id: &str, status: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    // The board's "archived" column drag maps to status 'done' plus the
    // `archived` flag (the tasks status CHECK only allows todo/in_progress/
    // done/cancelled). Any other transition clears the flag (un-archive).
    let (db_status, archived, completed_at) = match status {
        "archived" => ("done", true, Some(now.clone())),
        "done" => ("done", false, Some(now.clone())),
        other => (other, false, None),
    };

    conn.execute(
        "UPDATE tasks SET status = ?1, updated_at = ?2, completed_at = ?3, archived = ?4 WHERE id = ?5",
        params![db_status, now, completed_at, archived as i32, id],
    )?;

    // Handle Recurrence (D-48)
    if status == "done" {
        if let Some(task) = get_task(conn, id)? {
            if let Some(rule) = task.recurrence_rule.clone() {
                let next_due = calculate_next_due(&task.due_date, &rule);
                if let Some(due) = next_due {
                    let mut new_task = task.clone();
                    new_task.id = Uuid::new_v4().to_string();
                    new_task.status = "todo".to_string();
                    new_task.due_date = Some(due);
                    new_task.completed_at = None;
                    new_task.created_at = now.clone();
                    new_task.updated_at = now;
                    create_task(conn, &new_task)?;
                }
            }
        }
    }

    Ok(())
}

fn calculate_next_due(current_due: &Option<String>, rule: &str) -> Option<String> {
    let base_date = if let Some(d) = current_due {
        chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()?
    } else {
        chrono::Local::now().date_naive()
    };

    let next = match rule.to_lowercase().as_str() {
        "daily" => base_date + chrono::Duration::days(1),
        "weekly" => base_date + chrono::Duration::weeks(1),
        "monthly" => {
            let month = base_date.month();
            let year = base_date.year();
            if month == 12 {
                chrono::NaiveDate::from_ymd_opt(year + 1, 1, base_date.day())
                    .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(year + 1, 1, 28).unwrap())
            } else {
                chrono::NaiveDate::from_ymd_opt(year, month + 1, base_date.day()).unwrap_or_else(
                    || chrono::NaiveDate::from_ymd_opt(year, month + 1, 28).unwrap(),
                )
            }
        }
        _ => return None,
    };

    Some(next.format("%Y-%m-%d").to_string())
}

/// Apply an AI `update_task`/`complete_task` proposal to an existing task.
/// `kind` is the proposal's `data.type` ("task_update" or "task_complete") and
/// `data` its decoded payload. Returns the task id that was mutated so callers
/// can record it on the proposal.
pub fn apply_task_proposal(
    conn: &Connection,
    kind: &str,
    data: &serde_json::Value,
) -> Result<String, AppError> {
    let task_id = data
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if task_id.is_empty() {
        return Err(AppError::InvalidInput(
            "task update proposal missing id".into(),
        ));
    }
    let mut task =
        get_task(conn, &task_id)?.ok_or_else(|| AppError::NotFound(format!("Task {}", task_id)))?;

    if kind == "task_complete" {
        update_task_status(conn, &task_id, "done")?;
        return Ok(task_id);
    }

    if let Some(t) = data.get("title").and_then(|v| v.as_str()) {
        if !t.trim().is_empty() {
            task.title = t.to_string();
        }
    }
    if data.get("description").is_some() {
        task.description = data
            .get("description")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
    }
    if let Some(p) = data.get("priority").and_then(|v| v.as_str()) {
        if !p.trim().is_empty() {
            task.priority = p.to_string();
        }
    }
    if data.get("due_date").is_some() {
        task.due_date = data
            .get("due_date")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
    }

    // Persist non-status fields first, then route any status change through the
    // dedicated choke point so 'archived' maps to done+flag and recurrence fires.
    update_task(conn, &task)?;
    if let Some(s) = data.get("status").and_then(|v| v.as_str()) {
        if !s.trim().is_empty() {
            update_task_status(conn, &task_id, s)?;
        }
    }
    Ok(task_id)
}

pub fn soft_delete(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let now = Utc::now().to_rfc3339();
    let result = conn.execute(
        "UPDATE tasks SET is_deleted = 1, updated_at = ?1 WHERE id = ?2 OR parent_task_id = ?2",
        params![now, id],
    )?;
    Ok(result > 0)
}

pub fn get_task(conn: &Connection, id: &str) -> Result<Option<Task>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT *, 
         (SELECT COUNT(*) FROM task_dependencies d JOIN tasks bt ON d.blocking_task_id = bt.id WHERE d.blocked_task_id = tasks.id AND bt.status != 'done') > 0 as is_blocked
         FROM tasks WHERE id = ?1 AND is_deleted = 0"
    )?;
    let task = stmt.query_row(params![id], |row| {
        Ok(Task {
            id: row.get(0)?,
            parent_task_id: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            status: row.get(4)?,
            priority: row.get(5)?,
            due_date: row.get(6)?,
            due_time: row.get(7)?,
            reminder_at: row.get(8)?,
            reminder_fired: row.get::<_, i32>(9)? != 0,
            time_estimate: row.get(10)?,
            time_logged: row.get(11)?,
            tags: row.get(13)?,
            labels: row.get(14)?,
            category: row.get(15)?,
            project: row.get(16)?,
            recurrence_rule: row.get(18)?,
            energy_level: row.get(20)?,
            context_tag: row.get(21)?,
            linked_url: row.get(22)?,
            ai_created: row.get::<_, i32>(24)? != 0,
            is_blocked: row.get::<_, i32>(IS_BLOCKED_COLUMN_INDEX)? != 0,
            created_at: row.get(26)?,
            updated_at: row.get(27)?,
            completed_at: row.get(28)?,
            archived: row.get::<_, i64>(30)? != 0,
        })
    });

    match task {
        Ok(t) => Ok(Some(t)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

pub fn update_task(conn: &Connection, task: &Task) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE tasks SET 
            title = ?1, description = ?2, status = ?3, priority = ?4,
            due_date = ?5, due_time = ?6, reminder_at = ?7, time_estimate = ?8,
            category = ?9, project = ?10, energy_level = ?11, context_tag = ?12,
            linked_url = ?13, updated_at = ?14
         WHERE id = ?15",
        params![
            task.title,
            task.description,
            task.status,
            normalize_priority(&task.priority),
            task.due_date,
            task.due_time,
            task.reminder_at,
            task.time_estimate,
            task.category,
            task.project,
            task.energy_level,
            task.context_tag,
            task.linked_url,
            now,
            task.id,
        ],
    )?;
    Ok(())
}

pub fn search_tasks(conn: &Connection, query: &str) -> Result<Vec<Task>, AppError> {
    let sql_query = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT *, 
         (SELECT COUNT(*) FROM task_dependencies d JOIN tasks bt ON d.blocking_task_id = bt.id WHERE d.blocked_task_id = tasks.id AND bt.status != 'done') > 0 as is_blocked
         FROM tasks 
         WHERE is_deleted = 0 AND (title LIKE ?1 OR description LIKE ?1 OR project LIKE ?1)
         ORDER BY priority DESC, created_at DESC"
    )?;

    let rows = stmt.query_map(params![sql_query], |row| {
        Ok(Task {
            id: row.get(0)?,
            parent_task_id: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            status: row.get(4)?,
            priority: row.get(5)?,
            due_date: row.get(6)?,
            due_time: row.get(7)?,
            reminder_at: row.get(8)?,
            reminder_fired: row.get::<_, i32>(9)? != 0,
            time_estimate: row.get(10)?,
            time_logged: row.get(11)?,
            tags: row.get(13)?,
            labels: row.get(14)?,
            category: row.get(15)?,
            project: row.get(16)?,
            recurrence_rule: row.get(18)?,
            energy_level: row.get(20)?,
            context_tag: row.get(21)?,
            linked_url: row.get(22)?,
            ai_created: row.get::<_, i32>(24)? != 0,
            is_blocked: row.get::<_, i32>(IS_BLOCKED_COLUMN_INDEX)? != 0,
            created_at: row.get(26)?,
            updated_at: row.get(27)?,
            completed_at: row.get(28)?,
            archived: row.get::<_, i64>(30)? != 0,
        })
    })?;

    let mut tasks = Vec::new();
    for task in rows {
        tasks.push(task?);
    }
    Ok(tasks)
}
pub fn add_dependency(
    conn: &Connection,
    blocked_id: &str,
    blocking_id: &str,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO task_dependencies (id, blocked_task_id, blocking_task_id, created_at) 
         VALUES (?1, ?2, ?3, ?4)",
        params![Uuid::new_v4().to_string(), blocked_id, blocking_id, now],
    )?;
    Ok(())
}

pub fn remove_dependency(
    conn: &Connection,
    blocked_id: &str,
    blocking_id: &str,
) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM task_dependencies WHERE blocked_task_id = ?1 AND blocking_task_id = ?2",
        params![blocked_id, blocking_id],
    )?;
    Ok(())
}

pub fn get_dependencies(conn: &Connection, task_id: &str) -> Result<Vec<Task>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT t.*, 
         (SELECT COUNT(*) FROM task_dependencies d2 JOIN tasks bt ON d2.blocking_task_id = bt.id WHERE d2.blocked_task_id = t.id AND bt.status != 'done') > 0 as is_blocked
         FROM tasks t
         JOIN task_dependencies d ON t.id = d.blocking_task_id
         WHERE d.blocked_task_id = ?1 AND t.is_deleted = 0"
    )?;

    let rows = stmt.query_map(params![task_id], |row| {
        Ok(Task {
            id: row.get(0)?,
            parent_task_id: row.get(1)?,
            title: row.get(2)?,
            description: row.get(3)?,
            status: row.get(4)?,
            priority: row.get(5)?,
            due_date: row.get(6)?,
            due_time: row.get(7)?,
            reminder_at: row.get(8)?,
            reminder_fired: row.get::<_, i32>(9)? != 0,
            time_estimate: row.get(10)?,
            time_logged: row.get(11)?,
            tags: row.get(13)?,
            labels: row.get(14)?,
            category: row.get(15)?,
            project: row.get(16)?,
            recurrence_rule: row.get(18)?,
            energy_level: row.get(20)?,
            context_tag: row.get(21)?,
            linked_url: row.get(22)?,
            ai_created: row.get::<_, i32>(24)? != 0,
            is_blocked: row.get::<_, i32>(IS_BLOCKED_COLUMN_INDEX)? != 0,
            created_at: row.get(26)?,
            updated_at: row.get(27)?,
            completed_at: row.get(28)?,
            archived: row.get::<_, i64>(30)? != 0,
        })
    })?;

    let mut tasks = Vec::new();
    for task in rows {
        tasks.push(task?);
    }
    Ok(tasks)
}

// ==========================================
// Phase 2 Placeholders: Timers
// ==========================================

pub fn timer_start(conn: &Connection, task_id: &str) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO time_logs (id, task_id, started_at, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, task_id, now, now],
    )?;
    Ok(id)
}

pub fn timer_stop(
    conn: &Connection,
    log_id: &str,
    duration: i32,
    note: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();

    // Update the time_log
    conn.execute(
        "UPDATE time_logs SET ended_at = ?1, duration = ?2, note = ?3 WHERE id = ?4",
        params![now, duration, note, log_id],
    )?;

    // Also update the task time_logged
    conn.execute(
        "UPDATE tasks SET time_logged = time_logged + ?1 WHERE id = (SELECT task_id FROM time_logs WHERE id = ?2)",
        params![duration, log_id],
    )?;

    Ok(())
}

pub fn timer_get_logs(conn: &Connection, task_id: &str) -> Result<Vec<TimeLog>, AppError> {
    let mut stmt =
        conn.prepare("SELECT * FROM time_logs WHERE task_id = ?1 ORDER BY started_at DESC")?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(TimeLog {
            id: row.get(0)?,
            task_id: row.get(1)?,
            started_at: row.get(2)?,
            ended_at: row.get(3)?,
            duration: row.get(4)?,
            note: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;

    let mut logs = Vec::new();
    for row in rows {
        logs.push(row?);
    }
    Ok(logs)
}

// ==========================================
// Phase 2 Placeholders: Attachments
// ==========================================

pub fn attachment_add(
    conn: &Connection,
    task_id: &str,
    file_name: &str,
    file_path: &str,
    mime_type: Option<&str>,
    size_bytes: Option<i32>,
) -> Result<String, AppError> {
    let now = Utc::now().to_rfc3339();
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO task_attachments (id, task_id, file_name, file_path, mime_type, size_bytes, file_missing, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
        params![id, task_id, file_name, file_path, mime_type, size_bytes, now],
    )?;
    Ok(id)
}

pub fn attachment_remove(conn: &Connection, attachment_id: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM task_attachments WHERE id = ?1",
        params![attachment_id],
    )?;
    Ok(())
}

pub fn attachment_list(conn: &Connection, task_id: &str) -> Result<Vec<TaskAttachment>, AppError> {
    let mut stmt =
        conn.prepare("SELECT * FROM task_attachments WHERE task_id = ?1 ORDER BY created_at DESC")?;
    let rows = stmt.query_map(params![task_id], |row| {
        Ok(TaskAttachment {
            id: row.get(0)?,
            task_id: row.get(1)?,
            file_name: row.get(2)?,
            file_path: row.get(3)?,
            mime_type: row.get(4)?,
            size_bytes: row.get(5)?,
            file_missing: row.get::<_, i32>(6)? != 0,
            created_at: row.get(7)?,
        })
    })?;

    let mut attachments = Vec::new();
    for row in rows {
        attachments.push(row?);
    }
    Ok(attachments)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task_json_without_archived() -> serde_json::Value {
        serde_json::json!({
            "id": "t1",
            "title": "Test task",
            "status": "todo",
            "priority": "medium",
            "reminder_fired": false,
            "time_logged": 0,
            "tags": "[]",
            "labels": "[]",
            "ai_created": false,
            "is_blocked": false,
            "created_at": "2026-10-06T00:00:00Z",
            "updated_at": "2026-10-06T00:00:00Z"
        })
    }

    #[test]
    fn task_deserializes_when_archived_missing() {
        let task: Task = serde_json::from_value(task_json_without_archived())
            .expect("Task must deserialize when archived is absent (task_update payload omits it)");
        assert!(!task.archived);
    }

    #[test]
    fn task_deserializes_with_archived_true() {
        let mut json = task_json_without_archived();
        json["archived"] = serde_json::json!(true);
        let task: Task =
            serde_json::from_value(json).expect("Task must deserialize with archived present");
        assert!(task.archived);
    }

    #[test]
    fn normalize_priority_accepts_valid_set() {
        for p in ["low", "medium", "high", "urgent"] {
            assert_eq!(normalize_priority(p), p);
        }
        assert_eq!(normalize_priority("  HIGH  "), "high");
        assert_eq!(normalize_priority("Urgent"), "urgent");
        assert_eq!(normalize_priority("Low"), "low");
    }

    #[test]
    fn normalize_priority_falls_back_to_medium() {
        for p in ["", "  ", "critical", "p1", "asap", "MEDIUM-HIGH"] {
            assert_eq!(normalize_priority(p), "medium", "input: {:?}", p);
        }
    }

    #[test]
    fn create_task_stores_normalized_priority() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        let mut task: Task = serde_json::from_value(task_json_without_archived()).unwrap();
        task.priority = "Critical".to_string();
        create_task(&conn, &task).unwrap();
        let stored: String = conn
            .query_row(
                "SELECT priority FROM tasks WHERE id = ?1",
                params![task.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "medium");
    }

    fn seeded_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn.execute_batch("ALTER TABLE tasks ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;")
            .unwrap();
        let task: Task = serde_json::from_value(task_json_without_archived()).unwrap();
        create_task(&conn, &task).unwrap();
        conn
    }

    #[test]
    fn apply_task_update_proposal_mutates_existing_task_not_create_new() {
        let conn = seeded_conn();
        let data = serde_json::json!({
            "type": "task_update",
            "id": "t1",
            "title": "Renamed",
            "priority": "high"
        });
        let id = apply_task_proposal(&conn, "task_update", &data).unwrap();
        assert_eq!(id, "t1");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "update must not create a second task");
        let (title, priority): (String, String) = conn
            .query_row(
                "SELECT title, priority FROM tasks WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "Renamed");
        assert_eq!(priority, "high");
    }

    #[test]
    fn apply_task_update_proposal_routes_archived_to_done_and_flag() {
        let conn = seeded_conn();
        let data = serde_json::json!({"type": "task_update", "id": "t1", "status": "archived"});
        apply_task_proposal(&conn, "task_update", &data).unwrap();
        let (status, archived): (String, i64) = conn
            .query_row(
                "SELECT status, archived FROM tasks WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(status, "done");
        assert_eq!(archived, 1);
    }

    #[test]
    fn apply_task_complete_proposal_marks_done() {
        let conn = seeded_conn();
        let data = serde_json::json!({"type": "task_complete", "id": "t1"});
        let id = apply_task_proposal(&conn, "task_complete", &data).unwrap();
        assert_eq!(id, "t1");
        let status: String = conn
            .query_row("SELECT status FROM tasks WHERE id = 't1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(status, "done");
    }

    #[test]
    fn apply_task_update_proposal_rejects_missing_id() {
        let conn = seeded_conn();
        let data = serde_json::json!({"type": "task_update", "title": "x"});
        assert!(apply_task_proposal(&conn, "task_update", &data).is_err());
    }
}
