#![allow(dead_code)]

use crate::db::pool;
use crate::error::AppError;
use crate::AppState;
use chrono::{Datelike, Local, Utc};
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;

pub async fn generate_weekly_review(handle: &tauri::AppHandle) -> Result<String, AppError> {
    let state = handle.state::<AppState>();
    let pool = state.db()?;
    let handle_clone = handle.clone();

    let (report, now) = pool::with_conn(pool, move |conn| {
        let report = crate::db::reports::get_weekly_review(conn)?;
        let now = Utc::now().to_rfc3339();

        // Store the run timestamp
        let _ = conn.execute(
            "INSERT OR REPLACE INTO app_settings (key, value, updated_at) VALUES ('weekly_review_last_run', ?1, ?2)",
            rusqlite::params![now, now],
        );
        Ok((report, now))
    }).await?;

    // Send a desktop notification
    let _ = handle_clone
        .notification()
        .builder()
        .title("Weekly Review")
        .body(format!(
            "{} entries, {} tasks done, {} pending",
            report.journal_entries, report.tasks_completed, report.tasks_pending
        ))
        .show();

    let summary = format!(
        "Week {} – {}: {} entries, {} words, {} tasks completed, {} pending. Mood: {:?}",
        Local::now().iso_week().week(),
        report.week_start,
        report.journal_entries,
        report.total_words,
        report.tasks_completed,
        report.tasks_pending,
        report.dominant_mood,
    );

    // Emit log event for frontend visibility
    crate::events::emit(
        &handle_clone,
        crate::events::AppEvent::LogEvent {
            timestamp: now,
            level: "info".into(),
            source: "weekly_report".into(),
            message: summary.clone(),
        },
    );

    Ok(summary)
}

pub async fn should_run_weekly_review(handle: &tauri::AppHandle) -> bool {
    let now_date = Local::now().date_naive();

    // Only run on Mondays
    if Local::now().format("%u").to_string() != "1" {
        return false;
    }

    let state = handle.state::<AppState>();
    let pool = match state.db() {
        Ok(p) => p,
        Err(_) => return false,
    };

    let last_run = pool::with_conn(pool, move |conn| {
        let res: Result<Option<String>, AppError> = conn
            .query_row(
                "SELECT value FROM app_settings WHERE key = 'weekly_review_last_run'",
                [],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|e| {
                if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                    Ok(None)
                } else {
                    Err(AppError::Database(e.to_string()))
                }
            });
        res
    })
    .await
    .unwrap_or(None);

    match last_run {
        None => true,
        Some(last) => {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&last) {
                dt.with_timezone(&chrono::Local).date_naive() != now_date
            } else {
                true
            }
        }
    }
}

pub fn spawn_weekly_review_worker(handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
        loop {
            interval.tick().await;
            if should_run_weekly_review(&handle).await {
                if let Err(e) = generate_weekly_review(&handle).await {
                    tracing::warn!(target: "weekly_report", error = %e, "failed to generate weekly review");
                }
            }
        }
    });
}
