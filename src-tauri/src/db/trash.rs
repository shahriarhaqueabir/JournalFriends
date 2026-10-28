#![allow(dead_code)]

use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct TrashItem {
    pub id: String,
    pub entity_type: String,
    pub title: Option<String>,
    pub deleted_at: String,
}

/// List all soft-deleted items across journal entries, tasks, notes, and events.
pub fn list_trash(conn: &Connection) -> Result<Vec<TrashItem>, AppError> {
    let mut items = Vec::new();

    // Journal entries
    {
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(title, content) as title, updated_at
             FROM journal_entries WHERE is_deleted = 1
             ORDER BY updated_at DESC LIMIT 100",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                entity_type: "journal".into(),
                title: row.get::<_, Option<String>>(1)?,
                deleted_at: row.get(2)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
    }

    // Tasks
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, updated_at
             FROM tasks WHERE is_deleted = 1
             ORDER BY updated_at DESC LIMIT 100",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                entity_type: "task".into(),
                title: row.get::<_, Option<String>>(1)?,
                deleted_at: row.get(2)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
    }

    // Notes
    {
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(title, substr(content, 1, 80)), updated_at
             FROM notes WHERE is_deleted = 1
             ORDER BY updated_at DESC LIMIT 100",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                entity_type: "note".into(),
                title: row.get::<_, Option<String>>(1)?,
                deleted_at: row.get(2)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
    }

    // Events
    {
        let mut stmt = conn.prepare(
            "SELECT id, title, updated_at
             FROM events WHERE is_deleted = 1
             ORDER BY updated_at DESC LIMIT 100",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(TrashItem {
                id: row.get(0)?,
                entity_type: "event".into(),
                title: row.get::<_, Option<String>>(1)?,
                deleted_at: row.get(2)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
    }

    // Sort all by deleted_at DESC, limit total
    items.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
    items.truncate(100);
    Ok(items)
}

/// Permanently delete all trash items (true purge). Returns total count removed.
pub fn empty_trash(conn: &Connection) -> Result<u64, AppError> {
    let mut total = 0u64;
    total += conn.execute("DELETE FROM journal_entries WHERE is_deleted = 1", [])? as u64;
    total += conn.execute("DELETE FROM tasks WHERE is_deleted = 1", [])? as u64;
    total += conn.execute("DELETE FROM notes WHERE is_deleted = 1", [])? as u64;
    total += conn.execute("DELETE FROM events WHERE is_deleted = 1", [])? as u64;
    Ok(total)
}

/// Restore a single trash item by entity type and id.
pub fn restore_item(conn: &Connection, entity_type: &str, id: &str) -> Result<bool, AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let sql = match entity_type {
        "journal" => "UPDATE journal_entries SET is_deleted = 0, updated_at = ?1 WHERE id = ?2",
        "task" => "UPDATE tasks SET is_deleted = 0, updated_at = ?1 WHERE id = ?2",
        "note" => "UPDATE notes SET is_deleted = 0, updated_at = ?1 WHERE id = ?2",
        "event" => "UPDATE events SET is_deleted = 0, updated_at = ?1 WHERE id = ?2",
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unknown entity type: {}",
                entity_type
            )))
        }
    };
    let rows = conn.execute(sql, params![now, id])?;
    Ok(rows > 0)
}

/// Permanently delete a single trash item by entity type and id.
pub fn purge_item(conn: &Connection, entity_type: &str, id: &str) -> Result<bool, AppError> {
    let sql = match entity_type {
        "journal" => "DELETE FROM journal_entries WHERE id = ?1",
        "task" => "DELETE FROM tasks WHERE id = ?1",
        "note" => "DELETE FROM notes WHERE id = ?1",
        "event" => "DELETE FROM events WHERE id = ?1",
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unknown entity type: {}",
                entity_type
            )))
        }
    };
    let rows = conn.execute(sql, params![id])?;
    Ok(rows > 0)
}
