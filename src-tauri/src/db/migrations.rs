use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: include_str!("../../migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "fts_triggers",
        sql: include_str!("../../migrations/002_fts_triggers.sql"),
    },
    Migration {
        version: 3,
        name: "analysis_tracking",
        sql: include_str!("../../migrations/003_analysis_tracking.sql"),
    },
    Migration {
        version: 4,
        name: "field_expansion",
        sql: include_str!("../../migrations/004_field_expansion.sql"),
    },
    Migration {
        version: 5,
        name: "settings_v16",
        sql: include_str!("../../migrations/005_settings_v16.sql"),
    },
    Migration {
        version: 6,
        name: "user_profile",
        sql: include_str!("../../migrations/006_user_profile.sql"),
    },
    Migration {
        version: 7,
        name: "provenance",
        sql: include_str!("../../migrations/007_provenance.sql"),
    },
    Migration {
        version: 8,
        name: "events_notes",
        sql: include_str!("../../migrations/008_events_notes.sql"),
    },
    Migration {
        version: 9,
        name: "analysis_queue",
        sql: include_str!("../../migrations/009_analysis_queue.sql"),
    },
    Migration {
        version: 10,
        name: "event_reminders",
        sql: include_str!("../../migrations/010_event_reminders.sql"),
    },
    Migration {
        version: 11,
        name: "journal_vectors",
        sql: include_str!("../../migrations/011_journal_vectors.sql"),
    },
    Migration {
        version: 12,
        name: "knowledge_base",
        sql: include_str!("../../migrations/012_knowledge.sql"),
    },
    Migration {
        version: 13,
        name: "knowledge_created_by",
        sql: include_str!("../../migrations/013_knowledge_created_by.sql"),
    },
    Migration {
        version: 14,
        name: "mood_checkins",
        sql: include_str!("../../migrations/014_mood_checkins.sql"),
    },
    Migration {
        version: 15,
        name: "task_archived_status",
        sql: include_str!("../../migrations/015_task_archived_status.sql"),
    },
    Migration {
        version: 16,
        name: "proposed_inbox_refs",
        sql: include_str!("../../migrations/016_proposed_inbox_refs.sql"),
    },
    Migration {
        version: 17,
        name: "ai_message_tool_call_id",
        sql: include_str!("../../migrations/017_ai_message_tool_call_id.sql"),
    },
    Migration {
        version: 18,
        name: "gpu_layers_auto",
        sql: include_str!("../../migrations/018_gpu_layers_auto.sql"),
    },
    Migration {
        version: 19,
        name: "ai_enabled_upgrade",
        sql: include_str!("../../migrations/019_ai_enabled_upgrade.sql"),
    },
];

/// Migrations that rebuild a table with FK dependents. Reserved: SQLite FK
/// references on other tables are rewritten through ALTER TABLE RENAME even
/// when enforcement is off, so table rebuilds need per-table FK restoration.
const REBUILD_MIGRATIONS: &[i64] = &[];

fn is_begin_transaction_line(line: &str) -> bool {
    let before_comment = line.split_once("--").map_or(line, |(left, _)| left);
    matches!(
        before_comment.trim().to_ascii_uppercase().as_str(),
        "BEGIN;"
            | "BEGIN TRANSACTION;"
            | "BEGIN DEFERRED;"
            | "BEGIN IMMEDIATE;"
            | "BEGIN EXCLUSIVE;"
    )
}

fn is_commit_transaction_line(line: &str) -> bool {
    let before_comment = line.split_once("--").map_or(line, |(left, _)| left);
    before_comment.trim().eq_ignore_ascii_case("COMMIT;")
}

fn strip_outer_transaction(sql: &str) -> String {
    let lines: Vec<&str> = sql.lines().collect();
    let begin_index = lines
        .iter()
        .position(|line| is_begin_transaction_line(line));
    let commit_index = lines
        .iter()
        .rposition(|line| is_commit_transaction_line(line));

    match (begin_index, commit_index) {
        (Some(begin), Some(commit)) if begin < commit => lines
            .into_iter()
            .enumerate()
            .filter(|(idx, _)| *idx != begin && *idx != commit)
            .map(|(_, line)| line)
            .collect::<Vec<&str>>()
            .join("\n"),
        _ => sql.to_string(),
    }
}

fn ensure_no_explicit_transactions(sql: &str, migration_name: &str) -> Result<(), AppError> {
    for line in sql.lines() {
        if is_begin_transaction_line(line) || is_commit_transaction_line(line) {
            return Err(AppError::InvalidInput(format!(
                "Migration '{migration_name}' contains explicit BEGIN/COMMIT; migration runner wraps each migration in a rusqlite transaction so SQL files must not contain BEGIN/COMMIT."
            )));
        }
    }
    Ok(())
}

pub fn run_pending(conn: &mut Connection) -> Result<(), AppError> {
    // 1. Ensure migration tracking table exists
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version     INTEGER PRIMARY KEY,
            name        TEXT NOT NULL,
            applied_at  TEXT NOT NULL
        );",
    )?;

    // 2. Fetch applied versions (Fixed E0597: collect to Vec to drop stmt early)
    let applied: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
        let mut versions = Vec::new();
        for row in rows {
            versions.push(row?);
        }
        versions
    };

    // 3. Apply pending migrations atomically
    for m in MIGRATIONS {
        if !applied.contains(&m.version) {
            println!("[DB] Applying migration {}: {}", m.version, m.name);
            let is_rebuild = REBUILD_MIGRATIONS.contains(&m.version);
            // FK enforcement cannot be toggled inside a transaction.
            if is_rebuild {
                conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
            }
            // We use a transaction to ensure D-97 compliance
            let tx = conn.transaction()?;
            let sql = strip_outer_transaction(m.sql);
            ensure_no_explicit_transactions(&sql, m.name)?;
            tx.execute_batch(&sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                params![m.version, m.name, Utc::now().to_rfc3339()],
            )?;
            tx.commit()?;
            if is_rebuild {
                conn.execute_batch("PRAGMA foreign_keys = ON;")?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_outer_transaction_removes_wrapper() {
        let sql = "BEGIN;\nCREATE TABLE x(id INTEGER);\nCOMMIT;";
        assert_eq!(strip_outer_transaction(sql), "CREATE TABLE x(id INTEGER);");
    }

    #[test]
    fn strip_keeps_bare_sql_unchanged() {
        let sql = "CREATE TABLE x(id INTEGER);";
        assert_eq!(strip_outer_transaction(sql), sql);
    }

    #[test]
    fn explicit_transactions_are_rejected() {
        assert!(ensure_no_explicit_transactions("BEGIN;", "t").is_err());
        assert!(ensure_no_explicit_transactions("BEGIN TRANSACTION;", "t").is_err());
        assert!(ensure_no_explicit_transactions("CREATE TABLE x(id INTEGER);", "t").is_ok());
    }

    #[test]
    fn task_archived_migration_adds_flag_column() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn.execute_batch(
            "INSERT INTO tasks (id, parent_task_id, title, status, priority, created_at, updated_at) VALUES
             ('t1', NULL, 'Parent', 'todo', 'high', 'x', 'x'),
             ('t2', 't1', 'Child', 'in_progress', 'low', 'x', 'x');",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO task_dependencies (id, blocked_task_id, blocking_task_id, created_at) VALUES ('d1', 't2', 't1', 'x');
             INSERT INTO task_attachments (id, task_id, file_name, file_path, created_at) VALUES ('a1', 't1', 'f', 'p', 'x');
             INSERT INTO time_logs (id, task_id, started_at, created_at) VALUES ('l1', 't1', 'x', 'x');",
        )
        .unwrap();

        let tx = conn.transaction().unwrap();
        let sql = strip_outer_transaction(include_str!(
            "../../migrations/015_task_archived_status.sql"
        ));
        ensure_no_explicit_transactions(&sql, "015").unwrap();
        tx.execute_batch(&sql).unwrap();
        tx.commit().unwrap();

        // Existing rows kept, dependent FK rows untouched.
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM tasks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 2);
        let dep: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_dependencies", [], |r| r.get(0))
            .unwrap();
        let att: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_attachments", [], |r| r.get(0))
            .unwrap();
        let log: i64 = conn
            .query_row("SELECT COUNT(*) FROM time_logs", [], |r| r.get(0))
            .unwrap();
        assert_eq!((dep, att, log), (1, 1, 1));

        // New column defaults to 0; can be set to 1.
        let arch: i64 = conn
            .query_row("SELECT archived FROM tasks WHERE id='t1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(arch, 0);
        conn.execute("UPDATE tasks SET archived = 1 WHERE id = 't1'", [])
            .unwrap();
        let arch2: i64 = conn
            .query_row("SELECT archived FROM tasks WHERE id='t1'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(arch2, 1);
    }

    #[test]
    fn proposed_inbox_rebuild_drops_task_fk_but_keeps_others() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        let tx003 = conn.transaction().unwrap();
        let sql003 =
            strip_outer_transaction(include_str!("../../migrations/003_analysis_tracking.sql"));
        ensure_no_explicit_transactions(&sql003, "003").unwrap();
        tx003.execute_batch(&sql003).unwrap();
        tx003.commit().unwrap();

        conn.execute(
            "INSERT INTO tasks (id, parent_task_id, title, status, priority, created_at, updated_at)
             VALUES ('t1', NULL, 'P', 'todo', 'medium', 'x', 'x')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, status, created_by, accepted_task_id, created_at)
             VALUES ('p1', 'task', 'T', '{}', 'accepted', 'ai', 't1', 'x')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, status, created_by, accepted_task_id, created_at)
             VALUES ('p2', 'event', 'E', '{}', 'pending', 'ai', NULL, 'x')",
            [],
        )
        .unwrap();

        // Simulate a dangling conversation ref from the era before FK enforcement.
        conn.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        conn.execute(
            "INSERT INTO proposed_inbox (id, conversation_id, item_type, proposed_title, proposed_data, status, created_by, created_at)
             VALUES ('p0', 'ghost-conversation', 'fact', 'F', '{}', 'pending', 'ai', 'x')",
            [],
        )
        .unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();

        // Pre-fix behaviour: an event id in accepted_task_id violates the tasks FK.
        let pre = conn.execute(
            "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, status, created_by, accepted_task_id, created_at)
             VALUES ('p3', 'event', 'E2', '{}', 'accepted', 'ai', 'event-uuid-not-a-task', 'x')",
            [],
        );
        assert!(
            pre.is_err(),
            "pre-016 the tasks FK must reject a non-task accepted_task_id"
        );

        let tx = conn.transaction().unwrap();
        let sql =
            strip_outer_transaction(include_str!("../../migrations/016_proposed_inbox_refs.sql"));
        ensure_no_explicit_transactions(&sql, "016").unwrap();
        tx.execute_batch(&sql).unwrap();
        tx.commit().unwrap();

        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM proposed_inbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 3);
        let accepted: String = conn
            .query_row(
                "SELECT accepted_task_id FROM proposed_inbox WHERE id='p1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(accepted, "t1");
        let ghost: Option<String> = conn
            .query_row(
                "SELECT conversation_id FROM proposed_inbox WHERE id='p0'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            ghost, None,
            "dangling conversation ref must be nulled like ON DELETE SET NULL"
        );

        conn.execute(
            "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, status, created_by, accepted_task_id, created_at)
             VALUES ('p3', 'event', 'E2', '{}', 'accepted', 'ai', 'event-uuid-not-a-task', 'x')",
            [],
        )
        .unwrap();

        let mut stmt = conn
            .prepare("PRAGMA foreign_key_list('proposed_inbox')")
            .unwrap();
        let fk_from: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            !fk_from.contains(&"accepted_task_id".to_string()),
            "accepted_task_id must no longer be an FK; found: {:?}",
            fk_from
        );
        assert!(fk_from.contains(&"conversation_id".to_string()));
        assert!(fk_from.contains(&"source_entry_id".to_string()));

        let old_table: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='proposed_inbox_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(old_table, 0);
        let indexes: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index'
                 AND tbl_name='proposed_inbox'
                 AND name IN ('idx_proposed_inbox_status', 'idx_proposed_inbox_created')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(indexes, 2);
        let inbound: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE sql LIKE '%REFERENCES proposed_inbox%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(inbound, 0, "no table may reference proposed_inbox");
    }

    #[test]
    fn gpu_layers_migration_rewrites_only_the_legacy_pin() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at) VALUES ('llama_ngl', '99', 'x')",
            [],
        )
        .unwrap();

        let run = |conn: &mut Connection| {
            let tx = conn.transaction().unwrap();
            let sql =
                strip_outer_transaction(include_str!("../../migrations/018_gpu_layers_auto.sql"));
            ensure_no_explicit_transactions(&sql, "018").unwrap();
            tx.execute_batch(&sql).unwrap();
            tx.commit().unwrap();
        };

        run(&mut conn);
        let value: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key='llama_ngl'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "auto");

        // A deliberate pin must survive re-running the migration.
        conn.execute(
            "UPDATE app_settings SET value='40' WHERE key='llama_ngl'",
            [],
        )
        .unwrap();
        run(&mut conn);
        let kept: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key='llama_ngl'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, "40");
    }

    #[test]
    fn ai_enabled_migration_opts_in_existing_ai_installs_only() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();

        let run = |conn: &mut Connection| {
            let tx = conn.transaction().unwrap();
            let sql = strip_outer_transaction(include_str!(
                "../../migrations/019_ai_enabled_upgrade.sql"
            ));
            ensure_no_explicit_transactions(&sql, "019").unwrap();
            tx.execute_batch(&sql).unwrap();
            tx.commit().unwrap();
        };

        // Existing install with a configured chat model keeps AI enabled.
        conn.execute(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES ('llama_chat_model', 'model.gguf', 'x')",
            [],
        )
        .unwrap();
        run(&mut conn);
        let enabled: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key='ai_enabled'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(enabled, "true");

        // An explicit choice (including false) is never overwritten.
        conn.execute(
            "UPDATE app_settings SET value='false' WHERE key='ai_enabled'",
            [],
        )
        .unwrap();
        run(&mut conn);
        let kept: String = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key='ai_enabled'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, "false");

        // Fresh install: no model yet, so seed_defaults still supplies false.
        let mut fresh = Connection::open_in_memory().unwrap();
        fresh
            .execute_batch(include_str!("../../migrations/001_initial.sql"))
            .unwrap();
        run(&mut fresh);
        let rows: i64 = fresh
            .query_row(
                "SELECT COUNT(*) FROM app_settings WHERE key='ai_enabled'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rows, 0, "new installs must stay opted out");
    }
}
