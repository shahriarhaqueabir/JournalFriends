use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};

pub fn ensure_table(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS analysis_queue (
            entry_id TEXT PRIMARY KEY,
            status TEXT NOT NULL DEFAULT 'queued',
            queued_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_analysis_queue_status_queued ON analysis_queue (status, queued_at);
        ",
    )?;
    Ok(())
}

pub fn enqueue(conn: &Connection, entry_id: &str) -> Result<(), AppError> {
    ensure_table(conn)?;
    let now = Utc::now().to_rfc3339();
    // Upsert to queued atomically (single statement avoids races)
    conn.execute(
        "INSERT INTO analysis_queue (entry_id, status, queued_at, updated_at, attempts) VALUES (?1, 'queued', ?2, ?2, 0)
         ON CONFLICT(entry_id) DO UPDATE SET status = 'queued', queued_at = excluded.queued_at, updated_at = excluded.queued_at, attempts = 0",
        params![entry_id, now],
    )?;
    Ok(())
}

pub fn list_queued(conn: &Connection) -> Result<Vec<String>, AppError> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare("SELECT entry_id FROM analysis_queue WHERE status IN ('queued','processing') ORDER BY queued_at ASC")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut ids = Vec::new();
    for r in rows {
        ids.push(r?);
    }
    Ok(ids)
}

pub fn mark_processing(conn: &Connection, entry_id: &str) -> Result<(), AppError> {
    ensure_table(conn)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE analysis_queue SET status = 'processing', attempts = attempts + 1, updated_at = ?2 WHERE entry_id = ?1",
        params![entry_id, now],
    )?;
    Ok(())
}

pub fn mark_done(conn: &Connection, entry_id: &str) -> Result<(), AppError> {
    ensure_table(conn)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE analysis_queue SET status = 'done', updated_at = ?2 WHERE entry_id = ?1",
        params![entry_id, now],
    )?;
    Ok(())
}

pub fn mark_failed(conn: &Connection, entry_id: &str) -> Result<(), AppError> {
    ensure_table(conn)?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE analysis_queue SET status = 'failed', updated_at = ?2 WHERE entry_id = ?1",
        params![entry_id, now],
    )?;
    Ok(())
}

/// One row of `analysis_queue` as shown in the System tab (snake_case wire, D-148).
#[derive(serde::Serialize)]
pub struct QueueRow {
    pub entry_id: String,
    pub status: String,
    pub queued_at: String,
    pub updated_at: String,
    pub attempts: i64,
}

/// E-2: total persisted queue rows. This is the single source shared by the
/// System tab's "Analysis queue" runtime cell and the queue card so the two
/// numbers on the same screen always agree (the in-memory deque drains as
/// items complete and is not a stable user-facing count).
pub fn count_all(conn: &Connection) -> Result<i64, AppError> {
    ensure_table(conn)?;
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM analysis_queue", [], |r| r.get(0))?;
    Ok(n)
}

/// All queue rows: failures first (serves the Retry-failed control), then newest first.
pub fn list_all(conn: &Connection) -> Result<Vec<QueueRow>, AppError> {
    ensure_table(conn)?;
    let mut stmt = conn.prepare(
        "SELECT entry_id, status, queued_at, updated_at, attempts
         FROM analysis_queue
         ORDER BY CASE status WHEN 'failed' THEN 0 ELSE 1 END, queued_at DESC, entry_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(QueueRow {
            entry_id: row.get(0)?,
            status: row.get(1)?,
            queued_at: row.get(2)?,
            updated_at: row.get(3)?,
            attempts: row.get(4)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Flip failed → queued. Returns `false` when the row is not failed — the
/// caller maps that to `AppError::NotFound` (spec 2.4, item 3).
pub fn reset_failed(conn: &Connection, entry_id: &str) -> Result<bool, AppError> {
    ensure_table(conn)?;
    let now = Utc::now().to_rfc3339();
    let n = conn.execute(
        "UPDATE analysis_queue SET status = 'queued', updated_at = ?2
         WHERE entry_id = ?1 AND status = 'failed'",
        params![entry_id, now],
    )?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::migrations::run_pending;

    fn test_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        run_pending(&mut conn).unwrap();
        conn
    }

    #[test]
    fn list_all_orders_failures_first_then_newest() {
        let conn = test_db();
        enqueue(&conn, "e1").unwrap();
        enqueue(&conn, "e2").unwrap();
        mark_processing(&conn, "e1").unwrap();
        mark_done(&conn, "e1").unwrap();
        mark_failed(&conn, "e2").unwrap();
        // Pin timestamps so the DESC ordering is deterministic.
        conn.execute(
            "UPDATE analysis_queue SET queued_at = '2026-01-03T00:00:00Z' WHERE entry_id = 'e1'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE analysis_queue SET queued_at = '2026-01-02T00:00:00Z' WHERE entry_id = 'e2'",
            [],
        )
        .unwrap();

        let rows = list_all(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].entry_id, "e2"); // failed floats to the top
        assert_eq!(rows[0].status, "failed");
        assert_eq!(rows[1].entry_id, "e1");
        assert_eq!(rows[1].status, "done");
        assert_eq!(rows[1].attempts, 1); // mark_processing bumped it
    }

    #[test]
    fn reset_failed_requeues_only_failed_rows() {
        let conn = test_db();
        enqueue(&conn, "e1").unwrap();
        enqueue(&conn, "e2").unwrap();
        mark_done(&conn, "e1").unwrap();
        mark_failed(&conn, "e2").unwrap();

        assert!(reset_failed(&conn, "e2").unwrap());
        assert_eq!(list_queued(&conn).unwrap(), vec!["e2".to_string()]);
        assert!(!reset_failed(&conn, "e1").unwrap()); // done rows are not resettable
        assert!(!reset_failed(&conn, "nope").unwrap()); // unknown ids neither
    }

    // E-2 regression: the runtime cell and the queue card must read the same
    // persisted store, so the count must include every terminal row, not just
    // the in-flight ones.
    #[test]
    fn count_all_counts_every_persisted_row() {
        let conn = test_db();
        assert_eq!(count_all(&conn).unwrap(), 0);
        enqueue(&conn, "e1").unwrap();
        enqueue(&conn, "e2").unwrap();
        mark_processing(&conn, "e1").unwrap();
        mark_done(&conn, "e1").unwrap();
        mark_failed(&conn, "e2").unwrap();

        // 2 rows total (one done, one failed) — matching list_all's length.
        assert_eq!(count_all(&conn).unwrap(), 2);
        assert_eq!(
            count_all(&conn).unwrap() as usize,
            list_all(&conn).unwrap().len()
        );
    }
}
