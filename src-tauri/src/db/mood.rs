use crate::error::AppError;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

pub const VALID_MOODS: [&str; 7] = [
    "calm",
    "focused",
    "joyful",
    "anxious",
    "frustrated",
    "sad",
    "energetic",
];

pub fn validate_mood(v: &str) -> bool {
    VALID_MOODS.contains(&v)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MoodDay {
    pub date: String,
    pub mood: String,
    pub source: String,
    #[serde(default)]
    pub note: Option<String>,
}

pub fn checkin(
    conn: &Connection,
    id: &str,
    mood: &str,
    note: &str,
    date: &str,
    now: &str,
) -> Result<(), AppError> {
    conn.execute(
        "INSERT INTO mood_checkins (id, mood, note, checkin_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(checkin_date) DO UPDATE SET
           mood = excluded.mood,
           note = excluded.note,
           created_at = excluded.created_at",
        params![id, mood, note, date, now],
    )?;
    Ok(())
}

pub fn manual_days(conn: &Connection, start: &str, end: &str) -> Result<Vec<MoodDay>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT checkin_date, mood, note FROM mood_checkins
         WHERE checkin_date >= ?1 AND checkin_date <= ?2
         ORDER BY checkin_date ASC",
    )?;
    let rows = stmt.query_map(params![start, end], |row| {
        Ok(MoodDay {
            date: row.get(0)?,
            mood: row.get(1)?,
            source: "manual".into(),
            note: row.get(2)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn inferred_days(conn: &Connection, start: &str, end: &str) -> Result<Vec<MoodDay>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT date, mood FROM (
           SELECT date(created_at) AS date,
                  json_each.value AS mood,
                  COUNT(*) AS c,
                  ROW_NUMBER() OVER (PARTITION BY date(created_at) ORDER BY COUNT(*) DESC) AS rn
           FROM journal_entries, json_each(emotions)
           WHERE is_deleted = 0 AND date(created_at) >= ?1 AND date(created_at) <= ?2
           GROUP BY date, mood
         ) WHERE rn = 1
         ORDER BY date ASC",
    )?;
    let rows = stmt.query_map(params![start, end], |row| {
        Ok(MoodDay {
            date: row.get(0)?,
            mood: row.get(1)?,
            source: "ai".into(),
            note: None,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn delete(conn: &Connection, date: &str) -> Result<(), AppError> {
    conn.execute(
        "DELETE FROM mood_checkins WHERE checkin_date = ?1",
        params![date],
    )?;
    Ok(())
}

pub fn merge_days(manual: Vec<MoodDay>, inferred: Vec<MoodDay>) -> Vec<MoodDay> {
    let mut by_date: std::collections::HashMap<String, MoodDay> = std::collections::HashMap::new();
    for d in manual {
        by_date.insert(d.date.clone(), d);
    }
    for d in inferred {
        if !by_date.contains_key(&d.date) {
            by_date.insert(d.date.clone(), d);
        }
    }
    let mut days: Vec<MoodDay> = by_date.into_values().collect();
    days.sort_by(|a, b| a.date.cmp(&b.date));
    days
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manual(date: &str, mood: &str) -> MoodDay {
        MoodDay {
            date: date.into(),
            mood: mood.into(),
            source: "manual".into(),
            note: None,
        }
    }

    fn ai(date: &str, mood: &str) -> MoodDay {
        MoodDay {
            date: date.into(),
            mood: mood.into(),
            source: "ai".into(),
            note: None,
        }
    }

    #[test]
    fn merge_manual_wins_and_sorts() {
        let merged = merge_days(
            vec![manual("2026-09-21", "anxious")],
            vec![ai("2026-09-20", "calm"), ai("2026-09-21", "joyful")],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].date, "2026-09-20");
        assert_eq!(merged[0].source, "ai");
        assert_eq!(merged[1].date, "2026-09-21");
        assert_eq!(merged[1].source, "manual");
        assert_eq!(merged[1].mood, "anxious");
    }

    #[test]
    fn only_valid_moods_pass() {
        assert!(validate_mood("calm"));
        assert!(!validate_mood("euphoric"));
    }

    fn test_conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(include_str!("../../migrations/014_mood_checkins.sql"))
            .unwrap();
        c
    }

    #[test]
    fn checkin_upserts_by_date() {
        let conn = test_conn();
        checkin(
            &conn,
            "id1",
            "calm",
            "note",
            "2026-09-21",
            "2026-09-21T08:00:00Z",
        )
        .unwrap();
        checkin(
            &conn,
            "id2",
            "focused",
            "",
            "2026-09-21",
            "2026-09-21T20:00:00Z",
        )
        .unwrap();
        let days = manual_days(&conn, "2026-09-01", "2026-09-30").unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].mood, "focused");
        assert_eq!(days[0].source, "manual");
    }

    #[test]
    fn delete_removes_only_target_date() {
        let conn = test_conn();
        checkin(&conn, "a", "calm", "", "2026-09-21", "t").unwrap();
        checkin(&conn, "b", "sad", "", "2026-09-22", "t").unwrap();
        delete(&conn, "2026-09-21").unwrap();
        let days = manual_days(&conn, "2026-09-01", "2026-09-30").unwrap();
        assert_eq!(days.len(), 1);
        assert_eq!(days[0].date, "2026-09-22");
    }
}
