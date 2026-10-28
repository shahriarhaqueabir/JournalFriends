use crate::db::events::get_event;
use crate::db::pool;
use crate::db::tasks::get_task;
use crate::AppState;
use chrono::Utc;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

pub fn spawn_reminder_worker(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;

            let state = handle.state::<AppState>();
            let pool = match state.db() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let handle_clone = handle.clone();

            let _ = pool::with_conn(pool, move |conn| {
                let now = Utc::now().to_rfc3339();

                // Find tasks with pending reminders
                let mut stmt = conn.prepare(
                    "SELECT id FROM tasks WHERE reminder_at <= ?1 AND reminder_fired = 0 AND is_deleted = 0 AND status != 'done'"
                ).map_err(|e| crate::error::AppError::Database(e.to_string()))?;

                let ids: Vec<String> = stmt.query_map([&now], |row| row.get(0))
                    .map_err(|e| crate::error::AppError::Database(e.to_string()))?
                    .filter_map(|r| r.ok())
                    .collect();

                for id in ids {
                    if let Ok(Some(task)) = get_task(conn, &id) {
                        // Send notification
                        handle_clone.notification()
                            .builder()
                            .title("Task Reminder")
                            .body(&task.title)
                            .show()
                            .unwrap_or_default();

                        // Mark as fired
                        let _ = conn.execute(
                            "UPDATE tasks SET reminder_fired = 1 WHERE id = ?1",
                            [&id]
                        );
                    }
                }

                // Find events with pending reminders
                let mut stmt_ev = conn.prepare(
                    "SELECT id FROM events WHERE reminder_at <= ?1 AND reminder_fired = 0 AND is_deleted = 0"
                ).map_err(|e| crate::error::AppError::Database(e.to_string()))?;

                let ev_ids: Vec<String> = stmt_ev.query_map([&now], |row| row.get(0))
                    .map_err(|e| crate::error::AppError::Database(e.to_string()))?
                    .filter_map(|r| r.ok())
                    .collect();

                for id in ev_ids {
                    if let Ok(Some(ev)) = get_event(conn, &id) {
                        handle_clone.notification()
                            .builder()
                            .title("Event Reminder")
                            .body(&ev.title)
                            .show()
                            .unwrap_or_default();

                        let _ = conn.execute(
                            "UPDATE events SET reminder_fired = 1 WHERE id = ?1",
                            [&id]
                        );
                    }
                }
                Ok(())
            }).await;
        }
    });
}
