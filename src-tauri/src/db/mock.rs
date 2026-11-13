//! Mock-data end-to-end store tests over a fully-migrated in-memory schema (D-97 runner).
#![cfg(test)]

use rusqlite::{params, Connection};

use crate::db::journal::{list_entries, soft_delete, upsert_entry, JournalEntry};
use crate::db::migrations::run_pending;
use crate::db::mood::{checkin, delete, inferred_days, manual_days, merge_days};
use crate::db::tasks::{create_task, get_task, list_tasks, update_task_status, Task};

fn test_db() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    run_pending(&mut conn).unwrap();
    conn
}

fn seed_journal(conn: &Connection, id: &str, content: &str, emotions: &str, ts: &str) {
    upsert_entry(
        conn,
        &JournalEntry {
            id: id.into(),
            title: Some(id.into()),
            content: content.into(),
            word_count: content.split_whitespace().count() as i64,
            created_at: ts.into(),
            updated_at: ts.into(),
        },
    )
    .unwrap();
    conn.execute(
        "UPDATE journal_entries SET emotions = ?1 WHERE id = ?2",
        params![emotions, id],
    )
    .unwrap();
}

#[test]
fn full_migration_stack_applies_cleanly() {
    let conn = test_db();
    let applied: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(applied, 19);

    let expected_tables = [
        "notebooks",
        "journal_entries",
        "tasks",
        "task_dependencies",
        "task_attachments",
        "time_logs",
        "ai_conversations",
        "ai_messages",
        "app_settings",
        "audit_log",
        "proposed_task_log",
        "journal_fts",
        "proposed_inbox",
        "analysis_queue",
        "events",
        "notes",
        "journal_vectors",
        "knowledge_items",
        "knowledge_links",
        "mood_checkins",
    ];
    for t in expected_tables {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = ?1)",
                [t],
                |r| r.get(0),
            )
            .unwrap();
        assert!(exists, "missing table: {t}");
    }

    let fts_triggers: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'tr_journal_%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fts_triggers, 5, "FTS sync triggers missing");
}

#[test]
fn run_pending_is_idempotent_on_second_pass() {
    let mut conn = Connection::open_in_memory().unwrap();
    run_pending(&mut conn).unwrap();
    let before: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    run_pending(&mut conn).unwrap();
    let after: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, 19);
    assert_eq!(after, before);
}

#[test]
fn dual_source_mood_merge_over_mock_week() {
    let conn = test_db();
    seed_journal(
        &conn,
        "j01",
        "Woke up tense.",
        "[\"anxious\"]",
        "2026-09-15T08:00:00Z",
    );
    seed_journal(
        &conn,
        "j02",
        "Deep work block one.",
        "[\"focused\"]",
        "2026-09-16T09:00:00Z",
    );
    seed_journal(
        &conn,
        "j03",
        "Deep work block two.",
        "[\"focused\"]",
        "2026-09-16T14:00:00Z",
    );
    seed_journal(
        &conn,
        "j04",
        "Slow evening, calm.",
        "[\"calm\"]",
        "2026-09-16T19:00:00Z",
    );
    seed_journal(
        &conn,
        "j05",
        "Walk after lunch, lovely.",
        "[\"joyful\"]",
        "2026-09-18T12:00:00Z",
    );
    seed_journal(
        &conn,
        "j06",
        "Call with friend.",
        "[\"joyful\"]",
        "2026-09-18T16:00:00Z",
    );

    checkin(
        &conn,
        "m1",
        "energetic",
        "gym double session",
        "2026-09-16",
        "2026-09-16T20:00:00Z",
    )
    .unwrap();
    checkin(&conn, "m2", "sad", "", "2026-09-17", "2026-09-17T21:00:00Z").unwrap();

    let inferred = inferred_days(&conn, "2026-09-14", "2026-09-20").unwrap();
    assert_eq!(inferred.len(), 3);
    assert_eq!(inferred[0].mood, "anxious");
    assert_eq!(
        inferred[1].mood, "focused",
        "dominant must be count-based: focused appears twice"
    );
    assert_eq!(inferred[2].mood, "joyful");

    let manual = manual_days(&conn, "2026-09-14", "2026-09-20").unwrap();
    assert_eq!(manual.len(), 2);
    assert_eq!(manual[0].date, "2026-09-16");
    assert_eq!(manual[1].date, "2026-09-17");

    let merged = merge_days(manual, inferred);
    let shape: Vec<(&str, &str, &str)> = merged
        .iter()
        .map(|d| (d.date.as_str(), d.mood.as_str(), d.source.as_str()))
        .collect();
    assert_eq!(
        shape,
        vec![
            ("2026-09-15", "anxious", "ai"),
            ("2026-09-16", "energetic", "manual"),
            ("2026-09-17", "sad", "manual"),
            ("2026-09-18", "joyful", "ai"),
        ],
        "manual must win on shared days; ai fills un-checked days"
    );
}

#[test]
fn mood_delete_removes_only_mock_day() {
    let conn = test_db();
    checkin(&conn, "m1", "calm", "", "2026-09-16", "t").unwrap();
    checkin(&conn, "m2", "joyful", "", "2026-09-17", "t").unwrap();
    delete(&conn, "2026-09-16").unwrap();
    let days = manual_days(&conn, "2026-09-01", "2026-09-30").unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].date, "2026-09-17");
    let inferred = inferred_days(&conn, "2026-09-01", "2026-09-30").unwrap();
    assert!(inferred.is_empty());
}

#[test]
fn journal_keyset_pagination_and_soft_delete() {
    let conn = test_db();
    seed_journal(&conn, "j1", "entry one", "[]", "2026-09-01T09:00:00Z");
    seed_journal(&conn, "j2", "entry two", "[]", "2026-09-02T10:00:00Z");
    seed_journal(&conn, "j3", "entry three", "[]", "2026-09-03T11:00:00Z");
    seed_journal(&conn, "j4", "entry four", "[]", "2026-09-04T12:00:00Z");
    seed_journal(&conn, "j5", "entry five", "[]", "2026-09-05T13:00:00Z");

    let page1 = list_entries(&conn, None, 4).unwrap();
    let ids1: Vec<&str> = page1.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids1, vec!["j5", "j4", "j3", "j2"], "newest-first ordering");

    let last = &page1[3];
    let cursor = format!("{{\"ts\":\"{}\",\"id\":\"{}\"}}", last.created_at, last.id);
    let page2 = list_entries(&conn, Some(&cursor), 4).unwrap();
    let ids2: Vec<&str> = page2.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids2,
        vec!["j1"],
        "keyset cursor resumes after page 1 (D-104)"
    );

    assert!(soft_delete(&conn, "j3").unwrap());
    let remaining = list_entries(&conn, None, 10).unwrap();
    let ids3: Vec<&str> = remaining.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        ids3,
        vec!["j5", "j4", "j2", "j1"],
        "soft-deleted row excluded"
    );
}

#[test]
fn task_crud_and_status_flow_over_mock_tasks() {
    let conn = test_db();
    let mk = |id: &str, title: &str, status: &str, priority: &str, due: Option<&str>| Task {
        id: id.into(),
        parent_task_id: None,
        title: title.into(),
        description: Some("mock".into()),
        status: status.into(),
        priority: priority.into(),
        due_date: due.map(|d| d.into()),
        due_time: None,
        reminder_at: None,
        reminder_fired: false,
        time_estimate: None,
        time_logged: 0,
        tags: "[]".into(),
        labels: "[]".into(),
        category: None,
        project: None,
        energy_level: None,
        context_tag: None,
        linked_url: None,
        ai_created: false,
        recurrence_rule: None,
        is_blocked: false,
        created_at: "2026-09-20T08:00:00Z".into(),
        updated_at: "2026-09-20T08:00:00Z".into(),
        completed_at: None,
        archived: false,
    };

    create_task(
        &conn,
        &mk("t1", "Ship MVP", "todo", "urgent", Some("2026-09-25")),
    )
    .unwrap();
    create_task(
        &conn,
        &mk(
            "t2",
            "Mood check",
            "in_progress",
            "medium",
            Some("2026-09-21"),
        ),
    )
    .unwrap();
    create_task(&conn, &mk("t3", "Read a book", "done", "low", None)).unwrap();
    create_task(&conn, &mk("t4", "Old sprint task", "done", "urgent", None)).unwrap();

    let open = list_tasks(&conn, false).unwrap();
    let open_ids: Vec<&str> = open.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        open_ids,
        vec!["t1", "t2"],
        "done/cancelled hidden; priority DESC first"
    );

    let all = list_tasks(&conn, true).unwrap();
    assert_eq!(all.len(), 4);

    update_task_status(&conn, "t2", "done").unwrap();
    let open2 = list_tasks(&conn, false).unwrap();
    let open2_ids: Vec<&str> = open2.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(open2_ids, vec!["t1"]);

    // Archive keeps status=done but sets the flag; restore clears it.
    update_task_status(&conn, "t3", "archived").unwrap();
    let t3 = get_task(&conn, "t3").unwrap().unwrap();
    assert!(t3.archived);
    assert_eq!(t3.status, "done");
    let open3 = list_tasks(&conn, false).unwrap();
    assert_eq!(open3.len(), 1, "archived tasks stay out of the open list");
    update_task_status(&conn, "t3", "todo").unwrap();
    let t3b = get_task(&conn, "t3").unwrap().unwrap();
    assert!(!t3b.archived);
    assert_eq!(t3b.status, "todo");
    let open3b = list_tasks(&conn, false).unwrap();
    assert_eq!(open3b.len(), 2, "restored task returns to the open list");
}

#[test]
fn proposal_inbox_lifecycle_over_mock_items() {
    let conn = test_db();
    conn.execute(
        "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, confidence, status, created_at)
         VALUES ('p1','task','Call dentist','{\"title\":\"Call dentist\",\"priority\":\"medium\"}',0.8,'pending','2026-09-20T08:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, source_text, confidence, status, created_at)
         VALUES ('p2','fact','Prefers mornings','{}','noted habit',0.6,'pending','2026-09-20T09:00:00Z')",
        [],
    )
    .unwrap();
    const PENDING: &str = "SELECT COUNT(*) FROM proposed_inbox WHERE status='pending'";
    let pending: i64 = conn.query_row(PENDING, [], |r| r.get(0)).unwrap();
    assert_eq!(pending, 2);

    let invalid = conn.execute(
        "INSERT INTO proposed_inbox (id, item_type, proposed_title, proposed_data, status, created_at)
         VALUES ('p3','bogus','x','{}','pending','2026-09-20T10:00:00Z')",
        [],
    );
    assert!(
        invalid.is_err(),
        "item_type CHECK must reject unknown kinds"
    );

    conn.execute(
        "UPDATE proposed_inbox SET status='accepted', accepted_at='2026-09-20T10:00:00Z' WHERE id='p1'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE proposed_inbox SET status='dismissed' WHERE id='p2'",
        [],
    )
    .unwrap();
    let pending_after: i64 = conn.query_row(PENDING, [], |r| r.get(0)).unwrap();
    assert_eq!(pending_after, 0);
}

#[test]
fn analysis_queue_enqueue_is_deduplicated() {
    let conn = test_db();
    crate::db::analysis_queue::enqueue(&conn, "entry-a").unwrap();
    crate::db::analysis_queue::enqueue(&conn, "entry-a").unwrap();
    crate::db::analysis_queue::enqueue(&conn, "entry-b").unwrap();
    let pending = crate::db::analysis_queue::list_queued(&conn).unwrap();
    assert_eq!(pending.len(), 2, "re-enqueue must stay one row per entry");
    assert!(pending.contains(&"entry-a".to_string()));
    assert!(pending.contains(&"entry-b".to_string()));
}
