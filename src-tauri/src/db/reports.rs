#![allow(dead_code)]

use crate::error::AppError;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ActivityReportRow {
    pub date: String,
    pub journal_count: i64,
    pub task_completed: i64,
    pub total_words: i64,
}

#[derive(Debug, Serialize)]
pub struct TaskCompletionReportRow {
    pub period: String,
    pub total: i64,
    pub completed: i64,
    pub completion_rate: f64,
}

#[derive(Debug, Serialize)]
pub struct MoodTrendReportRow {
    pub date: String,
    pub mood: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct ProductivityReportRow {
    pub period: String,
    pub tasks_done: i64,
    pub hours_logged: f64,
    pub tasks_per_day: f64,
}

#[derive(Debug, Serialize)]
pub struct WeeklyReviewReport {
    pub week_start: String,
    pub week_end: String,
    pub journal_entries: i64,
    pub tasks_completed: i64,
    pub tasks_created: i64,
    pub tasks_pending: i64,
    pub dominant_mood: Option<String>,
    pub total_words: i64,
}

#[derive(Debug, Serialize)]
pub struct TagReportRow {
    pub tag: String,
    pub count: i64,
}

// ── Handlers ─────────────────────────────────────────────────────────

/// Activity Report — journal entries by day in a date range.
pub fn get_activity_report(
    conn: &Connection,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<ActivityReportRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT date(created_at) as day,
                COUNT(*) as journal_count,
                COALESCE(SUM(word_count), 0) as total_words,
                0 as task_completed
         FROM journal_entries
         WHERE is_deleted = 0
           AND date(created_at) >= ?1
           AND date(created_at) <= ?2
         GROUP BY day
         ORDER BY day ASC",
    )?;

    let rows = stmt.query_map(params![start_date, end_date], |row| {
        Ok(ActivityReportRow {
            date: row.get(0)?,
            journal_count: row.get(1)?,
            total_words: row.get(2)?,
            task_completed: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Task Completion Report — completion rate grouped by period.
pub fn get_task_completion_report(
    conn: &Connection,
    period: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<TaskCompletionReportRow>, AppError> {
    let period_expr = match period {
        "weekly" => "strftime('%Y-W%W', created_at)",
        "monthly" => "strftime('%Y-%m', created_at)",
        _ => "date(created_at)", // daily
    };

    let sql = format!(
        "SELECT {} as period,
                COUNT(*) as total,
                SUM(CASE WHEN status = 'done' THEN 1 ELSE 0 END) as completed,
                ROUND(AVG(CASE WHEN status = 'done' THEN 1.0 ELSE 0.0 END) * 100, 1) as completion_rate
         FROM tasks
         WHERE is_deleted = 0
           AND created_at >= ?1
           AND created_at <= ?2
         GROUP BY period
         ORDER BY period ASC",
        period_expr
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![start_date, end_date], |row| {
        Ok(TaskCompletionReportRow {
            period: row.get(0)?,
            total: row.get(1)?,
            completed: row.get(2)?,
            completion_rate: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Mood Trend Report — mood frequency across journal entries in a date range.
pub fn get_mood_trend_report(
    conn: &Connection,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<MoodTrendReportRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT date(created_at) as date,
                json_each.value as mood,
                COUNT(*) as count
         FROM journal_entries, json_each(emotions)
         WHERE is_deleted = 0
           AND date(created_at) >= ?1
           AND date(created_at) <= ?2
         GROUP BY date, mood
         ORDER BY date ASC, count DESC",
    )?;

    let rows = stmt.query_map(params![start_date, end_date], |row| {
        Ok(MoodTrendReportRow {
            date: row.get(0)?,
            mood: row.get(1)?,
            count: row.get(2)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Productivity Report — tasks done vs time logged per period.
pub fn get_productivity_report(
    conn: &Connection,
    start_date: &str,
    end_date: &str,
) -> Result<Vec<ProductivityReportRow>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT strftime('%Y-%m-%d', t.created_at) as period,
                COUNT(DISTINCT t.id) FILTER (WHERE t.status = 'done') as tasks_done,
                COALESCE(SUM(tl.duration), 0) / 3600.0 as hours_logged,
                ROUND(
                  COUNT(DISTINCT t.id) FILTER (WHERE t.status = 'done') * 1.0 /
                  MAX(COUNT(DISTINCT date(tl.started_at)), 1), 1
                ) as tasks_per_day
         FROM tasks t
         LEFT JOIN time_logs tl ON t.id = tl.task_id
         WHERE t.is_deleted = 0
           AND t.created_at >= ?1
           AND t.created_at <= ?2
         GROUP BY period
         ORDER BY period ASC",
    )?;

    let rows = stmt.query_map(params![start_date, end_date], |row| {
        Ok(ProductivityReportRow {
            period: row.get(0)?,
            tasks_done: row.get(1)?,
            hours_logged: row.get(2)?,
            tasks_per_day: row.get(3)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Weekly Review — comprehensive snapshot of the past 7 days.
pub fn get_weekly_review(conn: &Connection) -> Result<WeeklyReviewReport, AppError> {
    let now = Utc::now();
    let week_end = now.format("%Y-%m-%d").to_string();
    let week_start = (now - chrono::Duration::days(6))
        .format("%Y-%m-%d")
        .to_string();

    let journal_entries: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal_entries
         WHERE is_deleted = 0 AND date(created_at) >= ?1 AND date(created_at) <= ?2",
        params![&week_start, &week_end],
        |row| row.get(0),
    )?;

    let total_words: i64 = conn.query_row(
        "SELECT COALESCE(SUM(word_count), 0) FROM journal_entries
         WHERE is_deleted = 0 AND date(created_at) >= ?1 AND date(created_at) <= ?2",
        params![&week_start, &week_end],
        |row| row.get(0),
    )?;

    let tasks_completed: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE is_deleted = 0 AND status = 'done'
           AND date(completed_at) >= ?1 AND date(completed_at) <= ?2",
        params![&week_start, &week_end],
        |row| row.get(0),
    )?;

    let tasks_created: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE is_deleted = 0 AND date(created_at) >= ?1 AND date(created_at) <= ?2",
        params![&week_start, &week_end],
        |row| row.get(0),
    )?;

    let tasks_pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks
         WHERE is_deleted = 0 AND status IN ('todo', 'in_progress')",
        params![],
        |row| row.get(0),
    )?;

    let dominant_mood: Option<String> = conn
        .query_row(
            "SELECT json_each.value as mood
             FROM journal_entries, json_each(emotions)
             WHERE is_deleted = 0
               AND date(created_at) >= ?1 AND date(created_at) <= ?2
             GROUP BY mood
             ORDER BY COUNT(*) DESC
             LIMIT 1",
            params![&week_start, &week_end],
            |row| row.get(0),
        )
        .ok();

    Ok(WeeklyReviewReport {
        week_start,
        week_end,
        journal_entries,
        tasks_completed,
        tasks_created,
        tasks_pending,
        dominant_mood,
        total_words,
    })
}

/// Tag Report — tag frequency across journal entries or tasks.
pub fn get_tag_report(conn: &Connection, entity: &str) -> Result<Vec<TagReportRow>, AppError> {
    let (table, _id_col) = match entity {
        "journal" => ("journal_entries", "id"),
        "tasks" => ("tasks", "id"),
        _ => {
            return Err(AppError::InvalidInput(format!(
                "Unknown entity type for tag report: {}",
                entity
            )))
        }
    };

    let sql = format!(
        "SELECT json_each.value as tag, COUNT(*) as count
         FROM {}, json_each(tags)
         WHERE is_deleted = 0
         GROUP BY tag
         ORDER BY count DESC
         LIMIT 50",
        table
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok(TagReportRow {
            tag: row.get(0)?,
            count: row.get(1)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}
