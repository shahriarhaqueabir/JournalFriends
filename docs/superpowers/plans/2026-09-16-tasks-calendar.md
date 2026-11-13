# Tasks Tab Calendar Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a month-grid **Calendar** view to the Tasks tab with drag-to-schedule (unscheduled tray → day, day → day, day → tray), click-to-open the task detail panel, and migrate the Board's hand-rolled HTML5 drag to SortableJS.

**Architecture:** Frontend-only. A third `view-tab` (`data-view="calendar"`) routes into a new module `src/js/tabs/tasks-calendar.js` that self-renders a `.calendar-view` shell (header + 42-slot grid + unscheduled tray) into an empty `#task-calendar` container — the same pattern `events.js` uses for the Events calendar. SortableJS 1.15.7 (already vendored, unwired) drives both the three Board column drops and the calendar tray↔day drag-assign. Reuses the existing `task_update` / `task_update_status` Tauri commands — zero backend change.

**Tech Stack:** Vanilla ES modules, SortableJS 1.15.7 (global UMD `window.Sortable`), existing `ipc.js` invoke wrapper + `sanitize.js` `escapeHtml`, existing `calendar.css` / `tasks.css` class system, CSP `script-src 'self'`.

## Global Constraints

- **No backend changes.** No Rust, no schema, no new Tauri commands. All persistence goes through `invoke("task_update", { task })` and `invoke("task_update_status", { id, status })`.
- **All `invoke()` calls go through `../ipc.js`** — it always `.catch()`es to the error toast; handlers do NOT swallow failures without re-rendering.
- **Every dynamic string inserted via `innerHTML` goes through `escapeHtml`** from `../sanitize.js` (stored-XSS-safe; repo-wide security rule from the 2026-09-15 XSS pass).
- **Day keys are LOCAL `YYYY-MM-DD`** built from `(y, m, d)` parts — never `toISOString().slice(0, 10)` (UTC shift across timezones). Matches DB `due_date` format (`%Y-%m-%d`, see `calculate_next_due`, `db/tasks.rs:185`).
- **CSP `script-src 'self'`** — Sortable is loaded from `src/assets/libs/Sortable.min.js` (already vendored), global name `window.Sortable`; guard all usage with `typeof Sortable !== "undefined"`.
- **View switch pattern** (`tasks.js:418-426`): click `.view-tab` → set `currentView` → `refreshTasks(searchInput.value)`. New views are additive.
- **No automated test suite** (removed intentionally). Verification = `node --check` + manual smoke per task + final full smoke.
- **Commit style:** small focused commits, conventional messages, `wip/ollama-to-llamacpp-migration` branch, no backticks in messages.
- Docs (`CHANGELOG.md`, `AGENTS.md`) are gitignored by repo convention — update on disk in Task 7, do not `git add` them.

---

### Task 1: Load SortableJS + add Calendar view shell (`src/index.html`)

**Files:**
- Modify: `src/index.html`

**Interfaces:**
- Produces: `window.Sortable` global (loaded); `#task-calendar` empty container (consumed by Task 4/5); `.view-tab[data-view="calendar"]` button (consumed by Task 4).

- [ ] **Step 1: Add the SortableJS script tag after the lucide tag**

In `src/index.html`, after line 19 (`<script src="assets/libs/lucide.min.js"></script>`), add:

```html
<script src="assets/libs/Sortable.min.js"></script>
```

- [ ] **Step 2: Add the Calendar view-tab button**

In `src/index.html`, lines 115-116, change:

```html
<button class="view-tab active" data-view="list">☰ List</button>
<button class="view-tab" data-view="board">📋 Board</button>
```

to:

```html
<button class="view-tab active" data-view="list">☰ List</button>
<button class="view-tab" data-view="board">📋 Board</button>
<button class="view-tab" data-view="calendar">📅 Calendar</button>
```

- [ ] **Step 3: Add the empty calendar shell container**

In `src/index.html`, immediately after the `#task-kanban-board` closing div (line 145), before the closing `</div>` of the tasks tab content (line 146), add:

```html
<div id="task-calendar" class="calendar-container hidden"></div>
```

> The module in Task 3/5 renders the full `.calendar-view` shell into this empty container, exactly like `events.js` renders into `#events-calendar`.

- [ ] **Step 4: Verify markup + JSON integrity**

Run: `node -e "const fs=require('fs');const s=fs.readFileSync('src/index.html','utf8');for(const id of ['task-calendar','Sortable.min.js','data-view=\"calendar\"'])if(!s.includes(id))throw new Error('missing '+id);console.log('index.html OK')"`
Expected: `index.html OK`

- [ ] **Step 5: Commit**

```bash
git add src/index.html
git commit -m "feat(tasks): add SortableJS script tag and Calendar view shell"
```

---

### Task 2: Migrate Board drag to SortableJS (`src/js/tabs/tasks.js`)

**Files:**
- Modify: `src/js/tabs/tasks.js:92-122` (hand-rolled DnD), `:418-426` (view wiring block end)

**Interfaces:**
- Consumes: `window.Sortable` (Task 1), `invoke` from `../ipc.js` (already imported at line 1)
- Produces: Board `.column-items` sortables with status-assign on drop; a documented insertion point (end of `initTasks`) for the calendar API (Task 4).

- [ ] **Step 1: Remove the hand-rolled card drag + column drop wiring**

In `src/js/tabs/tasks.js`, in `renderKanban`, delete the `card.addEventListener("dragstart", ...)` / `card.addEventListener("dragend", ...)` block (currently lines 92-98) and the entire `// Setup drop zones` forEach block (currently lines 104-122), leaving:

```js
        col.appendChild(card);
      }
    });
  }
```

so `renderKanban` only builds and appends cards (they must keep `card.dataset.id = task.id`, which Task 5's `evt.item.dataset.id` reads).

- [ ] **Step 2: Add one-time Board sortable init at the end of `initTasks`**

In `src/js/tabs/tasks.js`, after the `// --- VIEW SWITCHING ---` block (which ends at line 426) and before the `// Initial load` comment (line 428), add:

```js
  // --- BOARD DRAG-AND-DROP (SortableJS, once) ---
  if (typeof Sortable !== "undefined" && boardContainer) {
    boardContainer.querySelectorAll(".column-items").forEach((col) => {
      new Sortable(col, {
        group: { name: "board", pull: true, put: true },
        animation: 150,
        onEnd: async (evt) => {
          const newStatus = evt.to?.dataset.status;
          const oldStatus = evt.from?.dataset.status;
          if (newStatus && newStatus !== oldStatus) {
            await invoke("task_update_status", { id: evt.item.dataset.id, status: newStatus });
            refreshTasks(searchInput.value);
          }
        },
      });
    });
  }
```

> `.column-items` containers are initialized once; `renderKanban` later rebuilds only their children, which SortableJS picks up automatically. No `destroy()` needed on the board side.

- [ ] **Step 3: Syntax check**

Run: `node --check src/js/tabs/tasks.js`
Expected: no output (pass)

- [ ] **Step 4: Manual smoke — Board still drags**

Rebuild the app (`cargo build` in `src-tauri`, or wait for Task 7's full rebuild) and verify: drag a card To Do→In Progress→Done (status persists in detail panel after reopen), drag within one column (no status change), Esc cancels drag.
Expected: no flood of `drag-over` styling, card ghost follows cursor, status updates once per column move.

- [ ] **Step 5: Commit**

```bash
git add src/js/tabs/tasks.js
git commit -m "refactor(tasks): replace hand-rolled board DnD with SortableJS"
```

---

### Task 3: Create `tasks-calendar.js` — grid render + navigation + chips (no drag yet)

**Files:**
- Create: `src/js/tabs/tasks-calendar.js`

**Interfaces:**
- Consumes: `invoke` (`../ipc.js`), `escapeHtml` (`../sanitize.js`), `window.Sortable` (used in Task 5), callbacks provided by `initTasksCalendar(options)`.
- Produces:
  - `export function initTasksCalendar({ onEditTask, onChanged })` → `{ render }` where `render(tasks)` renders the full `.calendar-view` (header, `#task-calendar-grid`, `#task-calendar-tray`) into `#task-calendar`, tracks month cursor + `lastTasks`, and re-binds nav/chips per render.
  - Internal `localDateKey(date) → "YYYY-MM-DD"` (local).
  - Internal `PRIORITY_RANK = { urgent: 0, high: 1, medium: 2, low: 3 }`.
  - DOM refs: `#cal-prev`, `#cal-next`, `#cal-today`, `#cal-title`, `#task-calendar-grid`, `#task-calendar-tray-list`. Chip buttons carry `data-id`. `onEditTask(id)` is called on chip click; `onChanged()` is called after any mutation.

- [ ] **Step 1: Write the module**

Create `src/js/tabs/tasks-calendar.js`:

```js
// Tasks tab Calendar view — month grid + unscheduled tray.
// Rendered self-contained into #task-calendar like events.js renders #events-calendar.
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";

const PRIORITY_RANK = { urgent: 0, high: 1, medium: 2, low: 3 };
const MAX_CHIPS = 4;

function localDateKey(date) {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function makeKey(year, monthIndex, day) {
  return `${year}-${String(monthIndex + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

export function initTasksCalendar({ onEditTask, onChanged }) {
  const container = document.getElementById("task-calendar");
  const cursor = new Date();
  cursor.setDate(1);
  cursor.setHours(0, 0, 0, 0);
  let lastTasks = [];
  let tasksById = new Map();
  let sortables = [];

  function destroySortables() {
    sortables.forEach((s) => s && s.destroy());
    sortables = [];
  }

  function chipHtml(task, extraClass = "") {
    const done = task.status === "done";
    const overdue = !done && task.due_date && task.due_date < localDateKey(new Date());
    const cls = ["task-chip", `priority-${task.priority || "medium"}`, extraClass, done ? "is-done" : "", overdue ? "is-overdue" : ""]
      .filter(Boolean)
      .join(" ");
    return `<button type="button" class="${cls}" data-id="${escapeHtml(task.id)}" title="${escapeHtml(task.title)}">${escapeHtml(task.title)}</button>`;
  }

  function render(tasks) {
    if (!container) return;
    destroySortables();
    lastTasks = tasks || [];
    tasksById.clear();
    lastTasks.forEach((t) => tasksById.set(t.id, t));

    const year = cursor.getFullYear();
    const month = cursor.getMonth();
    const firstOffset = new Date(year, month, 1).getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const todayKey = localDateKey(new Date());

    const byDue = new Map();
    const unscheduled = [];
    lastTasks.forEach((t) => {
      if (t.status === "cancelled") return;
      if (t.due_date) {
        const list = byDue.get(t.due_date) || [];
        list.push(t);
        byDue.set(t.due_date, list);
      } else {
        unscheduled.push(t);
      }
    });

    // Shell (rebuilt each render, mirroring events.js)
    container.innerHTML = `
      <div class="calendar-view">
        <div class="calendar-header">
          <h3>Tasks — <span id="cal-title"></span></h3>
          <div class="calendar-nav">
            <button type="button" id="cal-prev" class="btn-ghost-sm" title="Previous month">‹</button>
            <button type="button" id="cal-today" class="btn-ghost-sm">Today</button>
            <button type="button" id="cal-next" class="btn-ghost-sm" title="Next month">›</button>
          </div>
        </div>
        <div class="task-calendar-layout">
          <div id="task-calendar-grid" class="calendar-grid"></div>
          <aside class="unscheduled-tray">
            <h4>Unscheduled <span id="tray-count"></span></h4>
            <div id="task-calendar-tray-list"></div>
          </aside>
        </div>
      </div>
    `;

    document.getElementById("cal-title").textContent =
      `${new Date(year, month, 1).toLocaleString(undefined, { month: "long" })} ${year}`;

    // Grid: 42 fixed slots (6 rows x 7 cols)
    let html = "";
    for (let slot = 0; slot < 42; slot++) {
      const dayNum = slot - firstOffset + 1;
      const inMonth = dayNum >= 1 && dayNum <= daysInMonth;
      const key = inMonth ? makeKey(year, month, dayNum) : "";
      const dayTasks = inMonth ? (byDue.get(key) || []) : [];
      const isToday = key === todayKey;
      const chips = dayTasks.map((t, i) => chipHtml(t, i >= MAX_CHIPS ? "overflow" : "")).join("");
      const more = dayTasks.length > MAX_CHIPS
        ? `<button type="button" class="more-chip">+${dayTasks.length - MAX_CHIPS} more</button>`
        : "";
      html += `
        <div class="calendar-day ${inMonth ? "" : "other-month"} ${isToday ? "today" : ""}" data-day="${key}">
          ${inMonth ? `<span class="calendar-day-number">${dayNum}</span>` : ""}
          <div class="day-tasks">${chips}${more}</div>
        </div>
      `;
    }
    const grid = document.getElementById("task-calendar-grid");
    grid.innerHTML = html;

    // Tray (priority-ordered, undated tasks)
    unscheduled.sort((a, b) => (PRIORITY_RANK[a.priority] ?? 2) - (PRIORITY_RANK[b.priority] ?? 2));
    const trayList = document.getElementById("task-calendar-tray-list");
    document.getElementById("tray-count").textContent = `(${unscheduled.length})`;
    trayList.innerHTML = unscheduled.length
      ? unscheduled.map((t) => chipHtml(t)).join("")
      : '<div class="tray-empty">All tasks scheduled</div>';

    // Navigation
    document.getElementById("cal-prev").addEventListener("click", () => {
      cursor.setMonth(cursor.getMonth() - 1);
      render(lastTasks);
    });
    document.getElementById("cal-next").addEventListener("click", () => {
      cursor.setMonth(cursor.getMonth() + 1);
      render(lastTasks);
    });
    document.getElementById("cal-today").addEventListener("click", () => {
      const now = new Date();
      cursor.setFullYear(now.getFullYear(), now.getMonth(), 1);
      cursor.setHours(0, 0, 0, 0);
      render(lastTasks);
    });

    // +N more toggle
    grid.querySelectorAll(".more-chip").forEach((btn) => {
      btn.addEventListener("click", () => {
        const dayTasks = btn.closest(".day-tasks");
        dayTasks.classList.add("expanded");
        btn.remove();
      });
    });

    // Chip → open task detail
    grid.addEventListener("click", (e) => {
      const chip = e.target.closest(".task-chip");
      if (chip) onEditTask(chip.dataset.id);
    });
    trayList.addEventListener("click", (e) => {
      const chip = e.target.closest(".task-chip");
      if (chip) onEditTask(chip.dataset.id);
    });

    // sortables wired in Task 5
  }

  return { render };
}
```

- [ ] **Step 2: Syntax check**

Run: `node --check src/js/tabs/tasks-calendar.js`
Expected: no output (pass)

- [ ] **Step 3: Commit**

```bash
git add src/js/tabs/tasks-calendar.js
git commit -m "feat(tasks): calendar module - month grid, tray, navigation, chips"
```

---

### Task 4: Route the Calendar view in `tasks.js`

**Files:**
- Modify: `src/js/tabs/tasks.js` (imports line 1-2, `refreshTasks` lines 42-67, `initTasks` declarations)

**Interfaces:**
- Consumes: `initTasksCalendar({ onEditTask, onChanged })` from Task 3; `#task-calendar` + `.view-tab[data-view="calendar"]` from Task 1.
- Produces: `currentView === "calendar"` branch that calls `calendarApi.render(tasks)`; `calendarApi` init inside `initTasks` wiring `onEditTask` → `openDetail` and `onChanged` → `refreshTasks(searchInput.value)`.

- [ ] **Step 1: Import the module**

In `src/js/tabs/tasks.js`, change:

```js
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
```

to:

```js
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { initTasksCalendar } from "./tasks-calendar.js";
```

- [ ] **Step 2: Declare the calendar container + API**

In `src/js/tabs/tasks.js`, after line 11 (`const boardContainer = document.getElementById("task-kanban-board");`), add:

```js
  const calendarContainer = document.getElementById("task-calendar");
```

- [ ] **Step 3: Make `refreshTasks` three-way**

In `src/js/tabs/tasks.js`, replace the view block inside `refreshTasks` (currently lines 55-63):

```js
      if (currentView === "list") {
        renderTasks(tasks);
        listContainer.classList.remove("hidden");
        boardContainer.classList.add("hidden");
      } else {
        renderKanban(tasks);
        listContainer.classList.add("hidden");
        boardContainer.classList.remove("hidden");
      }
```

with:

```js
      if (currentView === "list") {
        renderTasks(tasks);
        listContainer.classList.remove("hidden");
        boardContainer.classList.add("hidden");
        calendarContainer.classList.add("hidden");
      } else if (currentView === "calendar") {
        calendarApi.render(tasks);
        listContainer.classList.add("hidden");
        boardContainer.classList.add("hidden");
        calendarContainer.classList.remove("hidden");
      } else {
        renderKanban(tasks);
        listContainer.classList.add("hidden");
        boardContainer.classList.remove("hidden");
        calendarContainer.classList.add("hidden");
      }
```

- [ ] **Step 4: Initialize `calendarApi` after the Board sortables**

In `src/js/tabs/tasks.js`, inside `initTasks`, directly after the `// --- BOARD DRAG-AND-DROP` block added in Task 2 (still before `// Initial load`), add:

```js
  // --- CALENDAR VIEW ---
  const calendarApi = initTasksCalendar({
    onEditTask: (id) => openDetail(id),
    onChanged: () => refreshTasks(searchInput.value),
  });
```

> `openDetail` and `refreshTasks` are function declarations inside `initTasks`, so they are hoisted and available here.

- [ ] **Step 5: Syntax check**

Run: `node --check src/js/tabs/tasks.js`
Expected: no output (pass)

- [ ] **Step 6: Manual smoke — Calendar renders**

Rebuild + relaunch, go to Tasks → Calendar. Expected: header "Tasks — <Month> <Year>", 42-slot grid, tray shows undated tasks (or "All tasks scheduled"), day chips for dated tasks, prev/next/today work, chip click opens the Task Detail panel, search box + pending/completed sub-filters narrow the calendar.

- [ ] **Step 7: Commit**

```bash
git add src/js/tabs/tasks.js
git commit -m "feat(tasks): route Calendar view into tasks-calendar module"
```

---

### Task 5: Calendar drag-to-schedule with SortableJS

**Files:**
- Modify: `src/js/tabs/tasks-calendar.js` (render tail, new `onEnd`)

**Interfaces:**
- Consumes: `window.Sortable` (Task 1); `invoke`, `escapeHtml`, `tasksById`, `onChanged` (defined in this module).
- Produces: Sortable instances over every `.day-tasks` + the tray list in shared sortable group `"tasks-calendar"`; `onEnd(evt)` resolves the drop target's day key and calls `invoke("task_update")` then `onChanged()`.

- [ ] **Step 1: Add `onEnd` and wire sortables inside `render`**

In `src/js/tabs/tasks-calendar.js`, replace the line at the end of `render` that reads:

```js
    // sortables wired in Task 5
```

with:

```js
    // Drag-to-schedule (SortableJS, rebuilt per render so instances never go stale)
    if (typeof Sortable !== "undefined") {
      sortables.push(new Sortable(trayList, {
        group: { name: "tasks-calendar", pull: true, put: true },
        animation: 150,
        filter: ".more-chip",
        onEnd,
      }));
      grid.querySelectorAll(".day-tasks").forEach((dayTasks) => {
        if (dayTasks.closest(".other-month")) return;
        sortables.push(new Sortable(dayTasks, {
          group: { name: "tasks-calendar", pull: true, put: true },
          animation: 150,
          filter: ".more-chip",
          onEnd,
        }));
      });
    }
  }

  async function onEnd(evt) {
    const id = evt.item?.dataset?.id;
    if (!id) return;
    const task = tasksById.get(id);
    if (!task) return;
    const newDue = evt.to?.closest?.(".calendar-day")?.dataset?.day ?? null;
    const oldDue = task.due_date ?? null;
    if (newDue === oldDue) return;
    try {
      await invoke("task_update", { task: { ...task, due_date: newDue } });
    } catch {
      // ipc.js already toasts; onChanged() re-fetches and rolls the DOM back to server truth
    }
    if (typeof onChanged === "function") onChanged();
  }
```

> Note: `.day-tasks` (the Sortable container) sits inside `.calendar-day[data-day]`, so `onEnd` resolves the day via `evt.to.closest(".calendar-day")`. Dragging to the tray (no `.calendar-day` ancestor) yields `null` → clears `due_date`. Other-month cells are explicitly excluded so chips can't land on filler slots.

- [ ] **Step 2: Syntax check**

Run: `node --check src/js/tabs/tasks-calendar.js`
Expected: no output (pass)

- [ ] **Step 3: Manual smoke — drag-to-schedule**

Rebuild + relaunch, Tasks → Calendar:
1. Tray → a day: chip appears on that day; task's due date = that day (verify in detail panel → Due Date).
2. Day → another day: chip moves, due date updates.
3. Day → tray: chip returns to tray; due date cleared.
4. Drag back onto the same day: no invoke (no visible change).
5. `+N more` chip is not draggable; month nav after a drop does not resurrect stale drop zones; a forced failure path (stop engines or close DB) toasts and re-renders.

- [ ] **Step 4: Commit**

```bash
git add src/js/tabs/tasks-calendar.js
git commit -m "feat(tasks): drag-to-schedule on calendar via SortableJS"
```

---

### Task 6: Calendar + chip styling

**Files:**
- Modify: `src/styles/calendar.css` (layout additions), `src/styles/tasks.css` (chip styles)

**Interfaces:**
- Consumes: CSS custom properties `--color-bg/surface/surface-soft/border/border-soft/text/muted/accent/accent-soft/error/error-soft/warn/success` from `base.css`; existing `.calendar-grid`, `.calendar-day`, `.calendar-day-number`, `.other-month`, `.today`, `.calendar-header` from `calendar.css`; `.priority-*` color classes from `tasks.css`.
- Produces: `.task-calendar-layout`, `.unscheduled-tray`, `.day-tasks`, `.task-chip` (+ `.overflow`, `.is-done`, `.is-overdue`, `.expanded`), `.more-chip`, `.tray-empty`, `.calendar-nav`.

- [ ] **Step 1: Append layout styles to `src/styles/calendar.css`**

```css
/* ── TASKS CALENDAR (drag-to-schedule) ─────────── */
.task-calendar-layout {
  display: grid;
  grid-template-columns: 1fr 220px;
  flex: 1;
  min-height: 0;
}

#task-calendar-grid.calendar-grid {
  overflow: auto;
}

.calendar-nav {
  display: flex;
  gap: 6px;
  align-items: center;
}

.day-tasks {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-height: 18px;
}

.day-tasks .task-chip.overflow { display: none; }
.day-tasks.expanded .task-chip.overflow { display: inline-flex; }

.more-chip {
  background: none;
  border: none;
  color: var(--color-muted);
  font-size: 10px;
  padding: 0;
  cursor: pointer;
  text-align: left;
}

.unscheduled-tray {
  border-left: 1px solid var(--color-border);
  padding: 12px;
  overflow-y: auto;
  background: var(--color-surface-soft);
}

.unscheduled-tray h4 {
  font-size: 12px;
  color: var(--color-muted);
  margin: 0 0 8px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

#task-calendar-tray-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tray-empty {
  font-size: 11px;
  color: var(--color-muted);
  padding: 8px 2px;
}
```

- [ ] **Step 2: Append chip styles to `src/styles/tasks.css`**

```css
/* ── CALENDAR TASK CHIPS ───────────────────────── */
.task-chip {
  display: inline-flex;
  align-items: center;
  max-width: 100%;
  padding: 2px 7px;
  font-size: 11px;
  line-height: 1.35;
  color: var(--color-text);
  background: var(--color-surface);
  border: 1px solid var(--color-border-soft);
  border-left-width: 3px;
  border-radius: var(--radius-sm);
  text-align: left;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  cursor: grab;
  transition: background-color 0.15s ease-out, transform 0.1s ease;
}

.task-chip:hover {
  background: var(--color-surface-soft);
}

.task-chip:active {
  cursor: grabbing;
}

.task-chip.priority-urgent { border-left-color: var(--color-error); }
.task-chip.priority-high   { border-left-color: var(--color-warn); }
.task-chip.priority-medium { border-left-color: var(--color-accent); }
.task-chip.priority-low    { border-left-color: var(--color-border); }

.task-chip.is-done {
  text-decoration: line-through;
  opacity: 0.55;
}

.task-chip.is-overdue {
  background: var(--color-error-soft);
  border-color: var(--color-error);
}

.sortable-ghost {
  opacity: 0.4;
  border: 1px dashed var(--color-accent);
}
```

- [ ] **Step 3: Visual check**

Rebuild + relaunch, Tasks → Calendar. Expected: grid fills the tab, tray column on the right with header + count, chips render with priority border color, `done` struck + dimmed, overdue tinted, `+N more` expands the cell, drag ghost is the `.sortable-ghost` dashed style, other-month filler cells dimmed per existing `.calendar-day.other-month`.

- [ ] **Step 4: Commit**

```bash
git add src/styles/calendar.css src/styles/tasks.css
git commit -m "style(tasks): calendar layout, tray, and task-chip styles"
```

---

### Task 7: Docs, full smoke, final commit

**Files:**
- Modify: `CHANGELOG.md`, `AGENTS.md` (on disk only — gitignored by repo convention, do NOT `git add`)

**Interfaces:**
- Consumes: everything from Tasks 1-6.
- Produces: verified working feature + recorded D-129..D-133 decisions.

- [ ] **Step 1: Rebuild + relaunch the app**

Run: `cargo build` in `src-tauri` (0 errors, 6 pre-existing warnings), then launch `src-tauri/target/debug/arnold-journal-ai.exe`.

- [ ] **Step 2: Full end-to-end smoke**

1. Journal tab renders; unlock vault.
2. Tasks → List: renders, search + pending/completed filters work, checkbox toggles status, Add Task + detail edit persists.
3. Tasks → Board: card drags across columns persist (SortableJS), reorder within column is smooth, Esc cancels.
4. Tasks → Calendar: header month/year correct; nav prev/next/today; tray lists undated tasks by priority; dated chips render; done struck; overdue tinted; `+N more` expands; chip click opens detail and the Due Date field reflects the task's date.
5. Drag tray→day → detail panel Due Date = target; day→day reschedules; day→tray clears (Due Date empty); monthly nav survives drops; swap to Board/List and back — view state consistent.
6. Relaunch app → all dragged due dates persisted.
7. AI sidebar + chat still function (regression on shared tasks backend).

- [ ] **Step 3: Update `CHANGELOG.md`**

Under `## [Unreleased]`, append to `### Added`:

```markdown
- **Tasks Calendar view (D-129..D-133):** third Tasks view toggle (List/Board/Calendar) with a month grid + Unscheduled tray. Drag-to-schedule via SortableJS: tray→day sets `due_date`, day→day reschedules, day→tray clears. Chip click opens the Task Detail panel. Board DnD migrated from hand-rolled HTML5 drag to SortableJS 1.15.7 (vendored, previously unwired). Zero backend change — reuses `task_update` / `task_update_status`.
```

- [ ] **Step 4: Update `AGENTS.md`**

1. In `Current Build Status`, set the top line to: `**Last built:** Sprint 9 + Tasks Calendar (2026-09-16): ... calendar/tray/drag-to-schedule, board SortableJS ...`.
2. Add to `Known Decisions` (after D-128):

```markdown
 D-129    | Tasks calendar = month grid view (new toggle) | Mirrors Events tab grid pattern; self-rendered by tasks-calendar.js |
 D-130    | Calendar interactions = drag-assign + click-open | No click-day-to-create, no events overlay |
 D-131    | SortableJS for Board AND Calendar DnD | Vendored 1.15.7 finally wired; one DnD lib everywhere |
 D-132    | Zero backend/schema change for calendar | Reuses task_update / task_update_status |
 D-133    | Day keys are local YYYY-MM-DD | toISOString would shift days across timezones |
```

3. Add `src/js/tabs/tasks-calendar.js` to the File Structure `tabs/` listing.

- [ ] **Step 5: Final commit**

```bash
git add src/index.html src/js/tabs/tasks.js src/js/tabs/tasks-calendar.js src/styles/calendar.css src/styles/tasks.css CHANGELOG.md AGENTS.md
git commit -m "feat(tasks): tasks calendar view with drag-to-schedule"
```

> If this commit complains about the ignored `CHANGELOG.md`/`AGENTS.md` paths, run it WITHOUT those two paths (they stay on disk untracked per repo convention) and commit the six versioned files only.

---

## Self-review notes (already checked)

- **Spec coverage:** grid-only month view (D-129) → Task 4; drag-assign tray→day/day→day/day→tray (D-130, D-131) → Task 5; zero backend (D-132) → all tasks use existing commands; local date keys (D-133) → `makeKey`/`localDateKey`; click-open detail → Task 3/4 `onEditTask`; board SortableJS migration → Task 2; CSP `script-src 'self'` → vendored local tag (Task 1); search/sub-filter fallthrough → Task 4 Step 6; `+N more` overflow → Task 3 + CSS; error rollback → Task 5 `onChanged()`.
- **Type/signature consistency:** `render(tasks)`, `onEditTask(id)`, `onChanged()`, `localDateKey(date)`, `makeKey(year, monthIndex, day)`, `chipHtml(task, extraClass)` all used identically across tasks. `evt.to.closest(".calendar-day").dataset.day` matches the markup built in Task 3 (`data-day` on `.calendar-day`).
- **No placeholders:** every step has full code and a command.