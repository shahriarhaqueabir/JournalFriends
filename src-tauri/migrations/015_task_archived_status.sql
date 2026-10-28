-- 015_task_archived_status.sql
-- Support the Tasks board's "Archived" column without expanding the strict
-- status CHECK (which SQLite cannot ALTER and a rebuild would imperil the FK
-- references in task_dependencies / task_attachments / time_logs /
-- proposed_inbox). Archiving is modelled as an additive boolean flag: an
-- archived task keeps status='done' (so list/completed filters stay correct)
-- while the `archived` flag drives the board's Completed/archive column.
-- `db::tasks::update_task_status` maps the 'archived' pseudo-status.

ALTER TABLE tasks ADD COLUMN archived INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_tasks_archived ON tasks(archived);