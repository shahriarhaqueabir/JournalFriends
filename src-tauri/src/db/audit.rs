#![allow(dead_code)]

use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AuditEntry {
    pub id: i64,
    pub action: String,
    pub entity: String,
    pub entity_id: String,
    pub actor: String,
    pub changes: Option<String>,
    pub ai_conv_id: Option<String>,
    pub created_at: String,
}

/// Write an entry to the audit log.
///
/// `action`: "create" | "update" | "delete" | "restore"
/// `actor`: "user" | "ai"
pub fn write(
    conn: &Connection,
    action: &str,
    entity: &str,
    entity_id: &str,
    actor: &str,
    changes: Option<&str>,
    ai_conv_id: Option<&str>,
) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO audit_log (action, entity, entity_id, actor, changes, ai_conv_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![action, entity, entity_id, actor, changes, ai_conv_id, now],
    )?;
    Ok(())
}

/// List recent audit entries, optionally filtered by entity type.
pub fn list_entries(
    conn: &Connection,
    entity: Option<&str>,
    limit: u32,
) -> Result<Vec<AuditEntry>, AppError> {
    let mut items = Vec::new();

    if let Some(e) = entity {
        let mut stmt = conn.prepare(
            "SELECT id, action, entity, entity_id, actor, changes, ai_conv_id, created_at
             FROM audit_log WHERE entity = ?1
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![e, limit], |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                action: row.get(1)?,
                entity: row.get(2)?,
                entity_id: row.get(3)?,
                actor: row.get(4)?,
                changes: row.get(5)?,
                ai_conv_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, action, entity, entity_id, actor, changes, ai_conv_id, created_at
             FROM audit_log
             ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(AuditEntry {
                id: row.get(0)?,
                action: row.get(1)?,
                entity: row.get(2)?,
                entity_id: row.get(3)?,
                actor: row.get(4)?,
                changes: row.get(5)?,
                ai_conv_id: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        for row in rows {
            items.push(row?);
        }
    }

    Ok(items)
}

/// Remove audit entries older than `older_than_days` days. Returns count deleted.
pub fn archive_old_entries(conn: &Connection, older_than_days: i64) -> Result<u64, AppError> {
    let cutoff = (Utc::now() - chrono::Duration::days(older_than_days)).to_rfc3339();
    let deleted = conn.execute(
        "DELETE FROM audit_log WHERE created_at < ?1",
        params![cutoff],
    )? as u64;
    Ok(deleted)
}
