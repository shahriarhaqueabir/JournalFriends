use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn seed_defaults(conn: &Connection) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    let defaults = [
        ("theme", "dark"),
        ("weekly_review_last_run", ""), // D-109 tracking
        ("onboarding_complete", ""),    // P-04: set to RFC3339 date when first-run wizard finishes
        ("llama_exe_path", ""),
        ("llama_chat_model", ""),
        ("llama_embed_model", ""),
        ("llama_chat_port", "8080"),
        ("llama_embed_port", "8081"),
        ("llama_ngl", "auto"),
        ("llama_ctx_size", "16384"),
        ("ai_voice", ""),
        ("llama_api_key", ""),
    ];

    for (key, value) in defaults {
        conn.execute(
            "INSERT OR IGNORE INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
            params![key, value, now],
        )?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, AppError> {
    let mut stmt = conn.prepare("SELECT value FROM app_settings WHERE key = ?1")?;
    let res = stmt.query_row(params![key], |row| row.get(0));
    match res {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(AppError::Database(e.to_string())),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![key, value, now],
    )?;
    Ok(())
}
