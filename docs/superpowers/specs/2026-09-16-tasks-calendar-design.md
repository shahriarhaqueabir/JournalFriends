# Tasks Tab Calendar — Design

**Date:** 2026-09-16
**Status:** Approved (brainstorm) — pending implementation plan
**Scope:** Tasks tab only. Adds a month-grid **Calendar** view with drag-to-schedule, and migrates the Board's hand-rolled HTML5 drag to SortableJS.

## Objective

The Tasks tab currently offers **List** and **Board** (kanban) views. Add a third **Calendar** view that:

- Shows tasks pinned to their `due_date` in a month grid (same 42-slot layout pattern as the Events tab).
- Lets the user **drag an unscheduled task onto a day** to set its due date.
- Lets the user **drag a day-task onto another day** to reschedule.
- Lets the user **drag a day-task back to the tray** to clear its due date.
- Opens the existing Task Detail panel when a day-task is clicked.

The Board's drag-and-drop is currently hand-rolled HTML5 DnD (`tasks.js:92-122`). **SortableJS 1.15.7 is already vendored** (`src/assets/libs/Sortable.min.js`) but unwired. The Board migration and the Calendar drag-assign both use it, paying off the vendored library in both views.

## Decisions (locked in brainstorm)

| # | Decision |
|---|----------|
| D-129 | Calendar = month grid in the Tasks tab (new `data-view="calendar"` toggle alongside List/Board). |
| D-130 | Interactions = drag-to-schedule (tray→day, day→day, day→tray) + click-to-open-detail. No click-day-to-create, no events overlay. |
| D-131 | SortableJS used for **both** the Board and the Calendar drag-assign. |
| D-132 | Zero backend/schema change — reuse `task_update` and `task_update_status` Tauri commands as-is. |
| D-133 | New module `src/js/tabs/tasks-calendar.js`; `tasks.js` keeps view routing + board rendering. |

## Current ground truth (verified)

- View switching: `tasks.js:55-63` toggles `hidden` on `#task-list-items` / `#task-kanban-board` based on `currentView`. Additive change.
- Board DnD: `tasks.js:92-122` — per-card `dragstart`/`dragend` + per-column `dragover`/`drop`; `drop` reads `col.dataset.status` → `invoke("task_update_status")` → `refreshTasks`.
- Backend: `task_update` (lib.rs:1919) takes a full `Task`; `update_task` (db/tasks.rs:265) writes `due_date` among other fields. `due_date: Option<String>`, persisted as `%Y-%m-%d` (see `calculate_next_due`, db/tasks.rs:185-190). `due_date` has no `sort` column — drag never reorders rows.
- `task_list` returns the full serialized `Task` → the frontend can keep a `tasksById` cache and round-trip `{...task, due_date: "...|null"}`.
- Script loading: index.html loads `chart.umd.min.js` + `lucide.min.js` (lines 18-19) before module scripts. CSP `script-src 'self'` — a vendored `Sortable.min.js` tag is allowed.
- Errors: `ipc.js` `invoke()` wrapper always `.catch()`s to an error toast; the drag handler must additionally roll back (re-render from server truth) because SortableJS mutates the DOM on drop regardless of invoke success.

## Architecture

### Layout (`src/index.html`)

- Add `<button class="view-tab" data-view="calendar">📅 Calendar</button>` next to the existing List/Board tabs (line 115-116).
- Add `#task-calendar` container after `#task-kanban-board`:

```html
<div id="task-calendar" class="calendar-container hidden">
  <div class="calendar-header">
    <button id="cal-prev" class="btn-ghost-sm">‹</button>
    <span id="cal-title"></span>
    <button id="cal-next" class="btn-ghost-sm">›</button>
    <button id="cal-today" class="btn-ghost-sm">Today</button>
  </div>
  <div class="task-calendar-layout">
    <div id="task-calendar-grid"></div>      <!-- 42-slot month grid -->
    <aside id="task-calendar-tray"></aside>  <!-- Unscheduled tray -->
  </div>
</div>
```

- Add `<script src="assets/libs/Sortable.min.js"></script>` **after** lucide, before module scripts.

### `src/js/tabs/tasks.js` (routing + board)

- Extend `currentView` routing in `refreshTasks()` with a `calendar` branch:

```js
} else if (currentView === "calendar") {
  calendarApi.render(tasks);        // from tasks-calendar.js (module export)
  listContainer.classList.add("hidden");
  boardContainer.classList.add("hidden");
  calendarContainer.classList.remove("hidden");
}
```

Where `calendarApi = initTasksCalendar({ onEditTask: openDetail })`, initialized once in `initTasks()` alongside the other element refs. `tasks-calendar.js` exports `initTasksCalendar`, not a bare `renderCalendar`.

- **Remove** the per-card/per-column DnD wiring (lines 92-122) so `renderKanban` only builds cards.
- **Instantiate Sortable once** on the three `.column-items` containers (guarded by `typeof Sortable !== "undefined"`), since `renderKanban` rebuilds children each refresh and Sortable picks new children up automatically:

```js
columnItems.forEach((col) => {
  new Sortable(col, {
    group: { name: "board", pull: true, put: true },
    animation: 150,
    onEnd: async (evt) => {
      const status = evt.to?.dataset.status;
      if (status && status !== evt.from?.dataset.status) {
        await invoke("task_update_status", { id: evt.item.dataset.id, status });
      }
    },
  });
});
```

### `src/js/tabs/tasks-calendar.js` (new module)

```
export function initTasksCalendar({ onEditTask }) → { render }
```

Inner state: `{ y, m }` view cursor, `tasksById: Map`, `sortables: []`.

`render(tasks, query)` — bound into `calendarApi`; `tasks.js` calls it as `calendarApi.render(tasks)` without the query arg (the filtered task set already reflects search/sub-filters). Render steps:
1. Filter: drop `cancelled`; if `query`, case-insensitive match on `title`.
2. Build `tasksById` and `byDue: Map<dayKey, Task[]>` (empty/`null` `due_date` → unscheduled tray).
3. Build 42-slot grid from **local** date math — first weekday offset + days-in-month, other-month filler cells (mirror `events.js` `renderCalendar` counting), each `.day-cell` carrying `data-day="YYYY-MM-DD"`.
4. Per cell: render up to 4 `.task-chip` (priority border, `done` strike, `overdue` tint) then `+N more` inline-expand toggler; `done` tasks included but struck; `today` cell highlighted.
5. Tray: header `Unscheduled (n)` + sortable list of undated chips ordered by priority DESC.
6. **Sortable lifecycle:** `sortables.forEach(s => s.destroy())` before re-render; then `new Sortable(cell, { group: "tasks-calendar" })` for each `.day-cell` + the tray (single shared group so tray↔day and day↔day all work).
7. Bit listeners: month nav (`cal-prev`/`cal-next`/`cal-today`/`cal-title`), chip click → `onEditTask(id)`.

Drag `onEnd(evt)`:
```js
const id = evt.item.dataset.id;
const newDue = evt.to?.dataset.day ?? null;          // tray has no data-day → clears
const oldDue = tasksById.get(id)?.due_date ?? null;
if (newDue === oldDue) return;
const task = { ...tasksById.get(id), due_date: newDue };
try { await invoke("task_update", { task }); }
catch { /* ipc.js toasts; rollback below re-syncs DOM */ }
refresh();   // re-fetch task_list via refreshTasks → re-render (also rolls back on failure)
```

### CSS (`src/styles/components.css`)

- `.task-calendar-layout`: grid + fixed-width tray (`~220px`), reusing the app's grid primitives; scrollable grid area.
- `.task-chip`: rounded pill, left accent by `priority`, truncation, `cursor: grab`; `is-done` strikethrough + dim; `is-overdue` red tint; `is-today` outline.
- `.unscheduled-tray` header/chip list; reuse existing `.calendar-container`, `.calendar-day`, `.other-month`, `.today` classes from the Events grid.

## Data flow summary

```
user drags chip
  → SortableJS onEnd
  → compute due_date from evt.to.dataset.day (null = tray)
  → invoke("task_update", { task: {...cached, due_date} })
  → refreshTasks() → re-fetch task_list → re-render current view
    (on invoke failure: ipc.js toasts; re-render restores server truth)
board drag
  → SortableJS onEnd
  → invoke("task_update_status", { id, status })
  → refreshTasks()
```

## Edge cases

- **Timezone:** day cells always use **local** `YYYY-MM-DD` built from `(y, m, d)` parts. Never `toISOString()` (shifts a day across negative-offset timezones).
- **Drop back on the same day / same tray:** early-return (identical `due_date`).
- **Empty day cell or empty tray:** valid Sortable drop targets (empty container lists).
- **Esc / cancel drag:** handled by SortableJS.
- **Recurring tasks:** rescheduling edits `due_date` only; next-occurrence spawning is triggered by `status == "done"` in `update_task_status` — unaffected.
- **Overdue definition:** `due_date < today` (local) and `status !== "done"`.
- **Search + pending/completed sub-filter:** fall out naturally — `refreshTasks` passes the filtered set to `renderCalendar`.
- **Chips overflow:** max 4 per day + `+N more` inline expand; never opens a new panel.
- **Board stale-instance risk:** Sortable containers are initialized once; `renderKanban` rebuilds children only — no `destroy()` needed on the board side. Calendar instances **must** be destroyed pre-render (see lifecycle).

## Testing (test suite intentionally removed)

- `node --check` on all touched JS.
- `cargo check` sanity (backend untouched).
- Manual smoke:
  1. Board: card todo→in_progress→done (and back); reorder within a column; verify detail panel reflects status.
  2. Calendar: tray→day sets due date; day→day reschedules; day→tray clears; chip click opens detail; prev/next/today nav; overdue tint for a past-due undone task; done strike; search + sub-filter apply to calendar.
  3. Persistence: relaunch app → due dates survive.

## Files touched

- `src/index.html` — view-tab, `#task-calendar` container, `Sortable.min.js` script tag.
- `src/js/tabs/tasks.js` — calendar view routing; board DnD replaced by one-time Sortable init.
- `src/js/tabs/tasks-calendar.js` — **new** module (grid + tray + sortable lifecycle + drag-assign).
- `src/styles/components.css` — chip/tray/layout styles.
- `CHANGELOG.md`, `AGENTS.md` — entries + D-129..D-133 + file-structure line.

## Out of scope / follow-ups

- Click-day-to-create (declined in brainstorm).
- Overlaying Events on the Tasks calendar (Events tab keeps its own grid).
- Shared calendar-grid component extraction (brainstorm Approach B — separate refactor).
- Custom task sort ordering (no `sort` column exists in the schema).