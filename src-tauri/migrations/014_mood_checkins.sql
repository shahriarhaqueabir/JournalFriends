CREATE TABLE IF NOT EXISTS mood_checkins (
  id TEXT PRIMARY KEY,
  mood TEXT NOT NULL,
  note TEXT DEFAULT '',
  checkin_date TEXT NOT NULL,
  created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mood_checkins_date ON mood_checkins(checkin_date);