// Pure status/column mapping for the two-board Tasks kanban.
export const BOARD_LAYOUTS = {
  inprogress: [
    { status: "todo", label: "To Do" },
    { status: "in_progress", label: "In Progress" },
    { status: "done", label: "Done" },
    { status: "archived", label: "Archived" },
  ],
  completed: [{ status: "archived", label: "Archived" }],
};

// Maps a task (object; also accepts a bare status string) to its board column.
// The `archived` flag wins over status so archived tasks (status stays 'done')
// land in the Archived column and disappear from Done.
export function columnStatusForTask(task) {
  const t = typeof task === "string" ? { status: task } : task;
  if (t.archived || t.status === "archived") return "archived";
  if (t.status === "done") return "done";
  if (t.status === "in_progress") return "in_progress";
  if (t.status === "cancelled") return null;
  return "todo";
}

export function dropStatusFor(columnStatus) {
  if (columnStatus === "done") return "done";
  if (columnStatus === "archived") return "archived";
  if (columnStatus === "in_progress") return "in_progress";
  return "todo";
}
