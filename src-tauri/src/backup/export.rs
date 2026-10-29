#![allow(dead_code)]

use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportData {
    pub exported_at: String,
    pub journal_entries: Vec<ExportJournalEntry>,
    pub tasks: Vec<ExportTask>,
    pub facts: Vec<ExportFact>,
    pub events: Vec<ExportEvent>,
    pub notes: Vec<ExportNote>,
    pub conversations: Vec<ExportConversation>,
    pub messages: Vec<ExportMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportJournalEntry {
    pub id: String,
    pub notebook_id: Option<String>,
    pub title: Option<String>,
    pub content: String,
    pub emotions: String,
    pub tags: String,
    pub word_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportTask {
    pub id: String,
    pub parent_task_id: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub priority: String,
    pub due_date: Option<String>,
    pub tags: String,
    pub category: Option<String>,
    pub project: Option<String>,
    pub notes: Option<String>,
    pub recurrence: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportFact {
    pub id: String,
    pub fact_key: String,
    pub content: String,
    pub category: String,
    pub source: Option<String>,
    pub confidence: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportEvent {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_at: String,
    pub end_at: Option<String>,
    pub is_all_day: bool,
    pub location: Option<String>,
    pub recurrence_rule: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportNote {
    pub id: String,
    pub title: Option<String>,
    pub content: String,
    pub tags: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportConversation {
    pub id: String,
    pub title: Option<String>,
    pub model: String,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExportMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub tool_name: Option<String>,
    pub tool_args: Option<String>,
    pub tool_result: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub journal_entries: u64,
    pub tasks: u64,
    pub facts: u64,
    pub events: u64,
    pub notes: u64,
}

// ── Export ───────────────────────────────────────────────────────────

/// Export all user data as a pretty-printed JSON string.
pub fn export_json(conn: &Connection) -> Result<String, AppError> {
    let data = collect_export_data(conn)?;
    let json = serde_json::to_string_pretty(&data).map_err(|e| AppError::Io(e.to_string()))?;
    Ok(json)
}

/// Export all data as a ZIP archive containing:
/// - `arnold_export.json` - full data export
/// - `metadata.json` — version, export date, and schema info
pub fn export_zip(conn: &Connection, output_path: &Path) -> Result<PathBuf, AppError> {
    let json = export_json(conn)?;
    let metadata = serde_json::json!({
        "version": "0.1.0",
        "exported_at": chrono::Utc::now().to_rfc3339(),
        "app": "Arnold",
    });
    let metadata_json =
        serde_json::to_string_pretty(&metadata).map_err(|e| AppError::Io(e.to_string()))?;

    let file = std::fs::File::create(output_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    zip.start_file("arnold_export.json", options)
        .map_err(|e| AppError::Io(e.to_string()))?;
    zip.write_all(json.as_bytes())?;

    zip.start_file("metadata.json", options)
        .map_err(|e| AppError::Io(e.to_string()))?;
    zip.write_all(metadata_json.as_bytes())?;

    zip.finish().map_err(|e| AppError::Io(e.to_string()))?;
    Ok(output_path.to_path_buf())
}

/// Collect all export data from the database.
fn collect_export_data(conn: &Connection) -> Result<ExportData, AppError> {
    let exported_at = chrono::Utc::now().to_rfc3339();

    let journal_entries = {
        let mut stmt = conn.prepare(
            "SELECT id, notebook_id, title, content, emotions, tags, word_count,
                    created_at, updated_at
             FROM journal_entries WHERE is_deleted = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportJournalEntry {
                id: row.get(0)?,
                notebook_id: row.get(1)?,
                title: row.get(2)?,
                content: row.get(3)?,
                emotions: row.get(4)?,
                tags: row.get(5)?,
                word_count: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let tasks = {
        let mut stmt = conn.prepare(
            "SELECT id, parent_task_id, title, description, status, priority,
                    due_date, tags, category, project, notes, recurrence,
                    created_at, updated_at, completed_at
             FROM tasks WHERE is_deleted = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportTask {
                id: row.get(0)?,
                parent_task_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                status: row.get(4)?,
                priority: row.get(5)?,
                due_date: row.get(6)?,
                tags: row.get(7)?,
                category: row.get(8)?,
                project: row.get(9)?,
                notes: row.get(10)?,
                recurrence: row.get(11)?,
                created_at: row.get(12)?,
                updated_at: row.get(13)?,
                completed_at: row.get(14)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let facts = {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, category, confidence, created_at, updated_at
             FROM knowledge_items",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportFact {
                id: row.get(0)?,
                fact_key: row.get(1)?,
                content: row.get(2)?,
                category: row.get(3)?,
                source: None,
                confidence: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let events = {
        let mut stmt = conn.prepare(
            "SELECT id, title, description, start_at, end_at, is_all_day,
                    location, recurrence_rule, created_at, updated_at
             FROM events WHERE is_deleted = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportEvent {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                start_at: row.get(3)?,
                end_at: row.get(4)?,
                is_all_day: row.get::<_, i32>(5)? != 0,
                location: row.get(6)?,
                recurrence_rule: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let notes = {
        let mut stmt = conn.prepare(
            "SELECT id, title, content, tags, created_at, updated_at
             FROM notes WHERE is_deleted = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportNote {
                id: row.get(0)?,
                title: row.get(1)?,
                content: row.get(2)?,
                tags: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let (conversations, messages) = {
        let mut convs = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, title, model, source, created_at, updated_at
             FROM ai_conversations WHERE is_deleted = 0",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportConversation {
                id: row.get(0)?,
                title: row.get(1)?,
                model: row.get(2)?,
                source: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        for row in rows {
            convs.push(row?);
        }

        let mut msgs = Vec::new();
        let mut stmt = conn.prepare(
            "SELECT id, conversation_id, role, content, tool_name, tool_args,
                    tool_result, created_at
             FROM ai_messages ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportMessage {
                id: row.get(0)?,
                conversation_id: row.get(1)?,
                role: row.get(2)?,
                content: row.get(3)?,
                tool_name: row.get(4)?,
                tool_args: row.get(5)?,
                tool_result: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?;
        for row in rows {
            msgs.push(row?);
        }

        (convs, msgs)
    };

    Ok(ExportData {
        exported_at,
        journal_entries,
        tasks,
        facts,
        events,
        notes,
        conversations,
        messages,
    })
}

// ── Import ───────────────────────────────────────────────────────────

/// Import data from a JSON export string.
///
/// Uses INSERT OR IGNORE for idempotent re-import.
pub fn import_json(conn: &Connection, json_data: &str) -> Result<ImportResult, AppError> {
    let data: ExportData =
        serde_json::from_str(json_data).map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let mut result = ImportResult {
        journal_entries: 0,
        tasks: 0,
        facts: 0,
        events: 0,
        notes: 0,
    };

    for entry in &data.journal_entries {
        let count = conn.execute(
            "INSERT OR IGNORE INTO journal_entries
             (id, notebook_id, title, content, emotions, tags, word_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.id,
                entry.notebook_id,
                entry.title,
                entry.content,
                entry.emotions,
                entry.tags,
                entry.word_count,
                entry.created_at,
                entry.updated_at,
            ],
        )?;
        result.journal_entries += count as u64;
    }

    for task in &data.tasks {
        let count = conn.execute(
            "INSERT OR IGNORE INTO tasks
             (id, parent_task_id, title, description, status, priority, due_date,
              tags, category, project, notes, recurrence,
              created_at, updated_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![
                task.id,
                task.parent_task_id,
                task.title,
                task.description,
                task.status,
                task.priority,
                task.due_date,
                task.tags,
                task.category,
                task.project,
                task.notes,
                task.recurrence,
                task.created_at,
                task.updated_at,
                task.completed_at,
            ],
        )?;
        result.tasks += count as u64;
    }

    for fact in &data.facts {
        let count = conn.execute(
            "INSERT OR IGNORE INTO knowledge_items
             (id, title, content, kind, category, tags, item_date, confidence, provenance, created_by, source_entry_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'information', ?4, '[]', NULL, ?5, 'ai', 'ai', NULL, ?6, ?7)",
            params![
                fact.id,
                fact.fact_key,
                fact.content,
                fact.category,
                fact.confidence,
                fact.created_at,
                fact.updated_at,
            ],
        )?;
        result.facts += count as u64;
    }

    for event in &data.events {
        let count = conn.execute(
            "INSERT OR IGNORE INTO events
             (id, title, description, start_at, end_at, is_all_day, location,
              recurrence_rule, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.id,
                event.title,
                event.description,
                event.start_at,
                event.end_at,
                event.is_all_day as i32,
                event.location,
                event.recurrence_rule,
                event.created_at,
                event.updated_at,
            ],
        )?;
        result.events += count as u64;
    }

    for note in &data.notes {
        let count = conn.execute(
            "INSERT OR IGNORE INTO notes
             (id, title, content, tags, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                note.id,
                note.title,
                note.content,
                note.tags,
                note.created_at,
                note.updated_at,
            ],
        )?;
        result.notes += count as u64;
    }

    Ok(result)
}
