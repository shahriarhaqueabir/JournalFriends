#![allow(dead_code)]

use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Event {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_at: String,
    pub end_at: Option<String>,
    pub is_all_day: bool,
    pub location: Option<String>,
    pub recurrence_rule: Option<String>,
    pub reminder_at: Option<String>,
    pub reminder_fired: bool,
    pub status: Option<String>,
    pub created_by: Option<String>,
    pub source_entry_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_deleted: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub struct Note {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
    pub tags: String, // JSON array
    pub source_entry_id: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub is_deleted: bool,
}

pub fn ensure_events_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS events (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT,
            start_at TEXT NOT NULL,
            end_at TEXT,
            is_all_day INTEGER NOT NULL DEFAULT 0,
            location TEXT,
            recurrence_rule TEXT,
            reminder_at TEXT,
            reminder_fired INTEGER NOT NULL DEFAULT 0,
            status TEXT DEFAULT 'confirmed',
            created_by TEXT,
            source_entry_id TEXT REFERENCES journal_entries(id) ON DELETE SET NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;
    Ok(())
}

pub fn ensure_notes_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS notes (
            id TEXT PRIMARY KEY,
            title TEXT,
            content TEXT NOT NULL,
            tags TEXT DEFAULT '[]',
            source_entry_id TEXT REFERENCES journal_entries(id) ON DELETE SET NULL,
            created_by TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            is_deleted INTEGER NOT NULL DEFAULT 0
        )",
        [],
    )?;
    Ok(())
}

pub fn create_event(conn: &Connection, event: &Event) -> Result<String, AppError> {
    ensure_events_table(conn)?;
    conn.execute(
        "INSERT INTO events (
            id, title, description, start_at, end_at, is_all_day, location, recurrence_rule, reminder_at, reminder_fired, status,
            created_by, source_entry_id, created_at, updated_at, is_deleted
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            event.id,
            event.title,
            event.description,
            event.start_at,
            event.end_at,
            if event.is_all_day { 1 } else { 0 },
            event.location,
            event.recurrence_rule,
            event.reminder_at,
            if event.reminder_fired { 1 } else { 0 },
            event.status,
            event.created_by,
            event.source_entry_id,
            event.created_at,
            event.updated_at,
            if event.is_deleted { 1 } else { 0 },
        ],
    )?;
    Ok(event.id.clone())
}

pub fn update_event(conn: &Connection, event: &Event) -> Result<(), AppError> {
    ensure_events_table(conn)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE events SET
            title = ?1, description = ?2, start_at = ?3, end_at = ?4, is_all_day = ?5,
            location = ?6, recurrence_rule = ?7, reminder_at = ?8, status = ?9, updated_at = ?10
         WHERE id = ?11",
        params![
            event.title,
            event.description,
            event.start_at,
            event.end_at,
            if event.is_all_day { 1 } else { 0 },
            event.location,
            event.recurrence_rule,
            event.reminder_at,
            event.status,
            now,
            event.id,
        ],
    )?;
    Ok(())
}

pub fn get_event(conn: &Connection, id: &str) -> Result<Option<Event>, AppError> {
    ensure_events_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, title, description, start_at, end_at, is_all_day, location, recurrence_rule, reminder_at, reminder_fired, status, created_by, source_entry_id, created_at, updated_at, is_deleted
         FROM events WHERE id = ?1 AND is_deleted = 0",
    )?;
    let res = stmt.query_row(params![id], |row| {
        Ok(Event {
            id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            start_at: row.get(3)?,
            end_at: row.get(4)?,
            is_all_day: row.get::<_, i32>(5)? != 0,
            location: row.get(6)?,
            recurrence_rule: row.get(7)?,
            reminder_at: row.get(8)?,
            reminder_fired: row.get::<_, i32>(9)? != 0,
            status: row.get(10)?,
            created_by: row.get(11)?,
            source_entry_id: row.get(12)?,
            created_at: row.get(13)?,
            updated_at: row.get(14)?,
            is_deleted: row.get::<_, i32>(15)? != 0,
        })
    });

    match res {
        Ok(e) => Ok(Some(e)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

pub fn list_events(
    conn: &Connection,
    start_after: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<Event>, AppError> {
    ensure_events_table(conn)?;
    let lim = limit.unwrap_or(50) as i64;

    let mut items = Vec::new();
    if let Some(sa) = start_after {
        let sql = "SELECT id, title, description, start_at, end_at, is_all_day, location, recurrence_rule, reminder_at, reminder_fired, status, created_by, source_entry_id, created_at, updated_at, is_deleted FROM events WHERE is_deleted = 0 AND start_at >= ?1 ORDER BY start_at ASC LIMIT ?2";
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(params![sa, lim])?;
        while let Some(row) = rows.next()? {
            items.push(Event {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                start_at: row.get(3)?,
                end_at: row.get(4)?,
                is_all_day: row.get::<_, i32>(5)? != 0,
                location: row.get(6)?,
                recurrence_rule: row.get(7)?,
                reminder_at: row.get(8)?,
                reminder_fired: row.get::<_, i32>(9)? != 0,
                status: row.get(10)?,
                created_by: row.get(11)?,
                source_entry_id: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
                is_deleted: row.get::<_, i32>(15)? != 0,
            });
        }
    } else {
        let sql = "SELECT id, title, description, start_at, end_at, is_all_day, location, recurrence_rule, reminder_at, reminder_fired, status, created_by, source_entry_id, created_at, updated_at, is_deleted FROM events WHERE is_deleted = 0 ORDER BY start_at ASC LIMIT ?1";
        let mut stmt = conn.prepare(sql)?;
        let mut rows = stmt.query(params![lim])?;
        while let Some(row) = rows.next()? {
            items.push(Event {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                start_at: row.get(3)?,
                end_at: row.get(4)?,
                is_all_day: row.get::<_, i32>(5)? != 0,
                location: row.get(6)?,
                recurrence_rule: row.get(7)?,
                reminder_at: row.get(8)?,
                reminder_fired: row.get::<_, i32>(9)? != 0,
                status: row.get(10)?,
                created_by: row.get(11)?,
                source_entry_id: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
                is_deleted: row.get::<_, i32>(15)? != 0,
            });
        }
    }
    Ok(items)
}

pub fn soft_delete_event(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let now = Utc::now().to_rfc3339();
    let res = conn.execute(
        "UPDATE events SET is_deleted = 1, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(res > 0)
}

// -------------------- Notes --------------------

pub fn create_note(conn: &Connection, note: &Note) -> Result<String, AppError> {
    ensure_notes_table(conn)?;
    conn.execute(
        "INSERT INTO notes (id, title, content, tags, source_entry_id, created_by, created_at, updated_at, is_deleted) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            note.id,
            note.title,
            note.content,
            note.tags,
            note.source_entry_id,
            note.created_by,
            note.created_at,
            note.updated_at,
            if note.is_deleted { 1 } else { 0 },
        ],
    )?;
    Ok(note.id.clone())
}

pub fn update_note(conn: &Connection, note: &Note) -> Result<(), AppError> {
    ensure_notes_table(conn)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, tags = ?3, updated_at = ?4 WHERE id = ?5",
        params![note.title, note.content, note.tags, now, note.id],
    )?;
    Ok(())
}

pub fn get_note(conn: &Connection, id: &str) -> Result<Option<Note>, AppError> {
    ensure_notes_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, title, content, tags, source_entry_id, created_by, created_at, updated_at, is_deleted FROM notes WHERE id = ?1 AND is_deleted = 0",
    )?;
    let res = stmt.query_row(params![id], |row| {
        Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            tags: row.get(3)?,
            source_entry_id: row.get(4)?,
            created_by: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            is_deleted: row.get::<_, i32>(8)? != 0,
        })
    });

    match res {
        Ok(n) => Ok(Some(n)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

pub fn list_notes(conn: &Connection, limit: Option<u32>) -> Result<Vec<Note>, AppError> {
    ensure_notes_table(conn)?;
    let lim = limit.unwrap_or(50) as i64;
    let mut stmt = conn.prepare("SELECT id, title, content, tags, source_entry_id, created_by, created_at, updated_at, is_deleted FROM notes WHERE is_deleted = 0 ORDER BY updated_at DESC LIMIT ?1")?;
    let rows = stmt.query_map(params![lim], |row| {
        Ok(Note {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            tags: row.get(3)?,
            source_entry_id: row.get(4)?,
            created_by: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            is_deleted: row.get::<_, i32>(8)? != 0,
        })
    })?;

    let mut notes = Vec::new();
    for r in rows {
        notes.push(r?);
    }
    Ok(notes)
}

pub fn soft_delete_note(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let now = Utc::now().to_rfc3339();
    let res = conn.execute(
        "UPDATE notes SET is_deleted = 1, updated_at = ?1 WHERE id = ?2",
        params![now, id],
    )?;
    Ok(res > 0)
}

// -------------------- AI proposal application --------------------

fn required_string(data: &serde_json::Value, key: &str, action: &str) -> Result<String, AppError> {
    match data.get(key).and_then(|v| v.as_str()) {
        Some(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        _ => Err(AppError::InvalidInput(format!(
            "{} proposal requires a non-empty '{}'.",
            action, key
        ))),
    }
}

fn optional_string(value: &serde_json::Value) -> Result<Option<String>, AppError> {
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) if s.trim().is_empty() => Ok(None),
        serde_json::Value::String(s) => Ok(Some(s.clone())),
        _ => Err(AppError::InvalidInput(
            "Expected a string or null.".to_string(),
        )),
    }
}

fn validate_timestamp(value: &str, field: &str) -> Result<(), AppError> {
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| {
        AppError::InvalidInput(format!(
            "'{}' must be an ISO 8601 timestamp with timezone (e.g. 2026-11-02T09:00:00+00:00).",
            field
        ))
    })?;
    Ok(())
}

fn tags_to_string(value: Option<&serde_json::Value>) -> String {
    match value {
        None | Some(serde_json::Value::Null) => "[]".to_string(),
        Some(v @ serde_json::Value::Array(_)) => v.to_string(),
        Some(serde_json::Value::String(s)) => {
            let trimmed = s.trim();
            if trimmed.starts_with('[')
                && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
            {
                trimmed.to_string()
            } else if trimmed.is_empty() {
                "[]".to_string()
            } else {
                serde_json::Value::Array(vec![serde_json::Value::String(trimmed.to_string())])
                    .to_string()
            }
        }
        Some(_) => "[]".to_string(),
    }
}

/// Apply an accepted AI event proposal. `action` is `create`, `update`, or
/// `delete`; returns the affected record ID. Missing records fail with
/// `NotFound` so a stale proposal cannot silently create a duplicate.
pub fn apply_event_proposal(
    conn: &Connection,
    action: &str,
    data: &serde_json::Value,
    source_entry_id: Option<&str>,
) -> Result<String, AppError> {
    match action {
        "create" => {
            let title = required_string(data, "title", "Event create")?;
            let start_at = required_string(data, "start_at", "Event create")?;
            validate_timestamp(&start_at, "start_at")?;
            if let Some(end) = data.get("end_at").and_then(|v| v.as_str()) {
                validate_timestamp(end, "end_at")?;
            }
            if let Some(reminder) = data.get("reminder_at").and_then(|v| v.as_str()) {
                validate_timestamp(reminder, "reminder_at")?;
            }

            let now = Utc::now().to_rfc3339();
            let event = Event {
                id: uuid::Uuid::new_v4().to_string(),
                title,
                description: data
                    .get("description")
                    .map(|v| optional_string(v))
                    .transpose()?
                    .flatten(),
                start_at,
                end_at: data
                    .get("end_at")
                    .map(|v| optional_string(v))
                    .transpose()?
                    .flatten(),
                is_all_day: data
                    .get("is_all_day")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                location: data
                    .get("location")
                    .map(|v| optional_string(v))
                    .transpose()?
                    .flatten(),
                recurrence_rule: data
                    .get("recurrence_rule")
                    .map(|v| optional_string(v))
                    .transpose()?
                    .flatten(),
                reminder_at: data
                    .get("reminder_at")
                    .map(|v| optional_string(v))
                    .transpose()?
                    .flatten(),
                reminder_fired: false,
                status: Some("confirmed".to_string()),
                created_by: Some("ai".to_string()),
                source_entry_id: source_entry_id.map(|s| s.to_string()),
                created_at: now.clone(),
                updated_at: now,
                is_deleted: false,
            };
            create_event(conn, &event)
        }
        "update" => {
            let id = required_string(data, "id", "Event update")?;
            let mut event = get_event(conn, &id)?
                .ok_or_else(|| AppError::NotFound(format!("Event {} not found", id)))?;

            if let Some(value) = data.get("title") {
                event.title = match optional_string(value)? {
                    Some(title) => title,
                    None => {
                        return Err(AppError::InvalidInput(
                            "Event title cannot be empty.".to_string(),
                        ))
                    }
                };
            }
            if let Some(value) = data.get("description") {
                event.description = optional_string(value)?;
            }
            if let Some(value) = data.get("start_at") {
                let start = optional_string(value)?.ok_or_else(|| {
                    AppError::InvalidInput("Event start_at cannot be empty.".to_string())
                })?;
                validate_timestamp(&start, "start_at")?;
                event.start_at = start;
            }
            if let Some(value) = data.get("end_at") {
                if let Some(end) = optional_string(value)? {
                    validate_timestamp(&end, "end_at")?;
                    event.end_at = Some(end);
                } else {
                    event.end_at = None;
                }
            }
            if let Some(value) = data.get("is_all_day") {
                event.is_all_day = value.as_bool().ok_or_else(|| {
                    AppError::InvalidInput("is_all_day must be a boolean.".to_string())
                })?;
            }
            if let Some(value) = data.get("location") {
                event.location = optional_string(value)?;
            }
            if let Some(value) = data.get("recurrence_rule") {
                event.recurrence_rule = optional_string(value)?;
            }
            if let Some(value) = data.get("reminder_at") {
                if let Some(reminder) = optional_string(value)? {
                    validate_timestamp(&reminder, "reminder_at")?;
                    event.reminder_at = Some(reminder);
                } else {
                    event.reminder_at = None;
                }
            }
            if let Some(value) = data.get("status") {
                event.status = optional_string(value)?;
            }
            update_event(conn, &event)?;
            Ok(id)
        }
        "delete" => {
            let id = required_string(data, "id", "Event delete")?;
            get_event(conn, &id)?
                .ok_or_else(|| AppError::NotFound(format!("Event {} not found", id)))?;
            if !soft_delete_event(conn, &id)? {
                return Err(AppError::NotFound(format!("Event {} not found", id)));
            }
            Ok(id)
        }
        other => Err(AppError::InvalidInput(format!(
            "Unknown event proposal action: {}",
            other
        ))),
    }
}

/// Apply an accepted AI note proposal. `action` is `create`, `update`, or
/// `delete`; returns the affected record ID.
pub fn apply_note_proposal(
    conn: &Connection,
    action: &str,
    data: &serde_json::Value,
    source_entry_id: Option<&str>,
) -> Result<String, AppError> {
    match action {
        "create" => {
            let content = required_string(data, "content", "Note create")?;
            let now = Utc::now().to_rfc3339();
            let note = Note {
                id: uuid::Uuid::new_v4().to_string(),
                title: data
                    .get("title")
                    .map(|v| optional_string(v))
                    .transpose()?
                    .flatten(),
                content,
                tags: tags_to_string(data.get("tags")),
                source_entry_id: source_entry_id.map(|s| s.to_string()),
                created_by: Some("ai".to_string()),
                created_at: now.clone(),
                updated_at: now,
                is_deleted: false,
            };
            create_note(conn, &note)
        }
        "update" => {
            let id = required_string(data, "id", "Note update")?;
            let mut note = get_note(conn, &id)?
                .ok_or_else(|| AppError::NotFound(format!("Note {} not found", id)))?;

            if let Some(value) = data.get("title") {
                note.title = optional_string(value)?;
            }
            if let Some(value) = data.get("content") {
                note.content = optional_string(value)?.ok_or_else(|| {
                    AppError::InvalidInput("Note content cannot be empty.".to_string())
                })?;
            }
            if let Some(value) = data.get("tags") {
                note.tags = tags_to_string(Some(value));
            }
            update_note(conn, &note)?;
            Ok(id)
        }
        "delete" => {
            let id = required_string(data, "id", "Note delete")?;
            get_note(conn, &id)?
                .ok_or_else(|| AppError::NotFound(format!("Note {} not found", id)))?;
            if !soft_delete_note(conn, &id)? {
                return Err(AppError::NotFound(format!("Note {} not found", id)));
            }
            Ok(id)
        }
        other => Err(AppError::InvalidInput(format!(
            "Unknown note proposal action: {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        // The source_entry_id columns reference journal_entries, so the stub
        // table must exist before the events/notes DDL runs.
        conn.execute("CREATE TABLE journal_entries (id TEXT PRIMARY KEY)", [])
            .unwrap();
        for id in ["entry-1", "entry-2"] {
            conn.execute("INSERT INTO journal_entries (id) VALUES (?1)", [id])
                .unwrap();
        }
        ensure_events_table(&conn).unwrap();
        ensure_notes_table(&conn).unwrap();
        conn
    }

    #[test]
    fn apply_event_proposal_creates_updates_and_soft_deletes() {
        let conn = conn();

        let created = apply_event_proposal(
            &conn,
            "create",
            &json!({
                "title": "Dentist",
                "start_at": "2026-11-02T09:00:00+00:00",
                "location": "Clinic",
            }),
            Some("entry-1"),
        )
        .unwrap();

        let event = get_event(&conn, &created).unwrap().unwrap();
        assert_eq!(event.title, "Dentist");
        assert_eq!(event.location.as_deref(), Some("Clinic"));
        assert_eq!(event.source_entry_id.as_deref(), Some("entry-1"));
        assert_eq!(event.created_by.as_deref(), Some("ai"));

        let updated = apply_event_proposal(
            &conn,
            "update",
            &json!({"id": created, "title": "Dentist (moved)"}),
            None,
        )
        .unwrap();
        assert_eq!(updated, created);

        let event = get_event(&conn, &created).unwrap().unwrap();
        assert_eq!(event.title, "Dentist (moved)");
        assert_eq!(
            event.start_at, "2026-11-02T09:00:00+00:00",
            "omitted fields must be preserved on update"
        );
        assert_eq!(
            event.location.as_deref(),
            Some("Clinic"),
            "omitted optional fields must be preserved on update"
        );

        let deleted = apply_event_proposal(&conn, "delete", &json!({"id": created}), None).unwrap();
        assert_eq!(deleted, created);
        assert!(
            get_event(&conn, &created).unwrap().is_none(),
            "soft-deleted events must disappear from reads"
        );
        assert!(list_events(&conn, None, None).unwrap().is_empty());
    }

    #[test]
    fn apply_event_proposal_clears_optional_fields_when_explicitly_null() {
        let conn = conn();
        let created = apply_event_proposal(
            &conn,
            "create",
            &json!({
                "title": "Call",
                "start_at": "2026-11-03T10:00:00+00:00",
                "location": "Office",
            }),
            None,
        )
        .unwrap();

        apply_event_proposal(
            &conn,
            "update",
            &json!({"id": created, "location": null}),
            None,
        )
        .unwrap();

        let event = get_event(&conn, &created).unwrap().unwrap();
        assert!(event.location.is_none());
    }

    #[test]
    fn apply_event_proposal_rejects_missing_fields_and_unknown_ids() {
        let conn = conn();

        let err =
            apply_event_proposal(&conn, "create", &json!({"title": "No date"}), None).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)), "{err:?}");

        let err = apply_event_proposal(
            &conn,
            "create",
            &json!({"title": "Bad date", "start_at": "tomorrow"}),
            None,
        )
        .unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)), "{err:?}");

        let err =
            apply_event_proposal(&conn, "update", &json!({"id": "missing"}), None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");

        let err =
            apply_event_proposal(&conn, "delete", &json!({"id": "missing"}), None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn apply_note_proposal_creates_updates_and_soft_deletes() {
        let conn = conn();

        let created = apply_note_proposal(
            &conn,
            "create",
            &json!({
                "title": "Idea",
                "content": "Try morning pages",
                "tags": ["writing", "habit"],
            }),
            Some("entry-2"),
        )
        .unwrap();

        let note = get_note(&conn, &created).unwrap().unwrap();
        assert_eq!(note.title.as_deref(), Some("Idea"));
        assert_eq!(note.content, "Try morning pages");
        assert_eq!(note.tags, "[\"writing\",\"habit\"]");

        apply_note_proposal(
            &conn,
            "update",
            &json!({"id": created, "content": "Try evening pages"}),
            None,
        )
        .unwrap();

        let note = get_note(&conn, &created).unwrap().unwrap();
        assert_eq!(note.content, "Try evening pages");
        assert_eq!(
            note.title.as_deref(),
            Some("Idea"),
            "omitted fields must be preserved on update"
        );
        assert_eq!(note.tags, "[\"writing\",\"habit\"]");

        apply_note_proposal(&conn, "delete", &json!({"id": created}), None).unwrap();
        assert!(get_note(&conn, &created).unwrap().is_none());
        assert!(list_notes(&conn, None).unwrap().is_empty());
    }

    #[test]
    fn apply_note_proposal_rejects_missing_content_and_unknown_ids() {
        let conn = conn();

        let err =
            apply_note_proposal(&conn, "create", &json!({"title": "No body"}), None).unwrap_err();
        assert!(matches!(err, AppError::InvalidInput(_)), "{err:?}");

        let err =
            apply_note_proposal(&conn, "update", &json!({"id": "missing"}), None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");

        let err =
            apply_note_proposal(&conn, "delete", &json!({"id": "missing"}), None).unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }
}
