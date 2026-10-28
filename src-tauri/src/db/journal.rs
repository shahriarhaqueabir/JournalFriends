#![allow(dead_code)]

use crate::error::AppError;
use chrono::Utc;
use rusqlite::{named_params, params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
    pub word_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

pub fn upsert_entry(conn: &Connection, entry: &JournalEntry) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO journal_entries (id, title, content, word_count, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            content = excluded.content,
            word_count = excluded.word_count,
            updated_at = excluded.updated_at",
        params![
            entry.id,
            entry.title,
            entry.content,
            entry.word_count,
            entry.created_at,
            entry.updated_at
        ],
    )?;
    Ok(())
}

pub fn get_entry(conn: &Connection, id: &str) -> Result<Option<JournalEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, word_count, created_at, updated_at
         FROM journal_entries
         WHERE id = ?1 AND is_deleted = 0",
    )?;

    let entry = stmt.query_row(params![id], |row| {
        Ok(JournalEntry {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            word_count: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    });

    match entry {
        Ok(e) => Ok(Some(e)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

pub fn get_entry_by_id(conn: &Connection, id: &str) -> Result<Option<JournalEntry>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, content, word_count, created_at, updated_at 
         FROM journal_entries WHERE id = ?1 AND is_deleted = 0",
    )?;
    let entry = stmt.query_row(params![id], |row| {
        Ok(JournalEntry {
            id: row.get(0)?,
            title: row.get(1)?,
            content: row.get(2)?,
            word_count: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        })
    });

    match entry {
        Ok(e) => Ok(Some(e)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JournalEntrySummary {
    pub id: String,
    pub title: Option<String>,
    pub word_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JournalContextSnippet {
    pub id: String,
    pub title: Option<String>,
    pub snippet: String,
    pub created_at: String,
}

pub fn list_entries(
    conn: &Connection,
    cursor: Option<&str>,
    limit: u32,
) -> Result<Vec<JournalEntrySummary>, AppError> {
    let mut result = Vec::new();

    if let Some(c) = cursor {
        #[derive(serde::Deserialize)]
        struct Cursor {
            ts: String,
            id: String,
        }

        let parsed: Cursor = serde_json::from_str(c)
            .map_err(|e| AppError::InvalidInput(format!("bad journal cursor: {e}")))?;

        let mut stmt = conn.prepare(
            "SELECT id, title, word_count, created_at, updated_at
             FROM journal_entries
             WHERE is_deleted = 0
               AND (created_at < :cursor_ts
                    OR (created_at = :cursor_ts AND id < :cursor_id))
             ORDER BY created_at DESC, id DESC
             LIMIT :limit",
        )?;

        let entries = stmt.query_map(
            named_params! { ":cursor_ts": parsed.ts, ":cursor_id": parsed.id, ":limit": limit },
            |row| {
                Ok(JournalEntrySummary {
                    id: row.get(0)?,
                    title: row.get(1)?,
                    word_count: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )?;

        for entry in entries {
            result.push(entry?);
        }
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, title, word_count, created_at, updated_at
             FROM journal_entries
             WHERE is_deleted = 0
             ORDER BY created_at DESC, id DESC
             LIMIT :limit",
        )?;

        let entries = stmt.query_map(named_params! { ":limit": limit }, |row| {
            Ok(JournalEntrySummary {
                id: row.get(0)?,
                title: row.get(1)?,
                word_count: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;

        for entry in entries {
            result.push(entry?);
        }
    }

    Ok(result)
}

pub fn soft_delete(conn: &Connection, id: &str) -> Result<bool, AppError> {
    let result = conn.execute(
        "UPDATE journal_entries SET is_deleted = 1, updated_at = ?1 WHERE id = ?2 AND is_deleted = 0",
        params![Utc::now().to_rfc3339(), id],
    )?;

    Ok(result > 0)
}

pub fn search_context_snippets(
    conn: &Connection,
    query: &str,
    limit: u32,
) -> Result<Vec<JournalContextSnippet>, AppError> {
    let terms: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .map(|term| term.trim().to_lowercase())
        .filter(|term| term.len() > 3)
        .take(5)
        .collect();

    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let conditions = terms
        .iter()
        .map(|_| "(lower(content) LIKE ? ESCAPE '\\' OR lower(title) LIKE ? ESCAPE '\\')")
        .collect::<Vec<_>>()
        .join(" OR ");
    let sql = format!(
        "SELECT id, title,
                substr(replace(content, char(10), ' '), 1, 280) AS snippet,
                created_at
         FROM journal_entries
         WHERE is_deleted = 0 AND ({})
         ORDER BY updated_at DESC
         LIMIT ?",
        conditions
    );

    let mut params = Vec::new();
    for term in terms {
        let like_query = format!("%{}%", term.replace('%', "\\%").replace('_', "\\_"));
        params.push(like_query.clone());
        params.push(like_query);
    }
    params.push(limit.to_string());

    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok(JournalContextSnippet {
            id: row.get(0)?,
            title: row.get(1)?,
            snippet: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;

    let mut snippets = Vec::new();
    for row in rows {
        snippets.push(row?);
    }
    Ok(snippets)
}
