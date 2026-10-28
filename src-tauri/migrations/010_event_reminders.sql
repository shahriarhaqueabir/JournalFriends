-- Add reminder fields to events for notifications
ALTER TABLE events ADD COLUMN reminder_at TEXT;
ALTER TABLE events ADD COLUMN reminder_fired INTEGER NOT NULL DEFAULT 0;
