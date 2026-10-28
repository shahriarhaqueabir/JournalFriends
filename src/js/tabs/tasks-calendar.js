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
    const cls = ["task-chip", `priority-${task.priority || "medium"}`, done ? "is-done" : "", overdue ? "is-overdue" : ""]
      .filter(Boolean)
      .join(" ");
    return `
      <div class="task-chip-row ${extraClass}" data-id="${escapeHtml(task.id)}">
        <button type="button" class="${cls}" data-id="${escapeHtml(task.id)}" title="${escapeHtml(task.title)}">${escapeHtml(task.title)}</button>
        <button type="button" class="icon-btn task-chip-menu" aria-label="Task actions for ${escapeHtml(task.title)}" aria-haspopup="menu" aria-expanded="false">
          <hj-icon name="more-horizontal" size="16" aria-hidden="true"></hj-icon>
        </button>
      </div>`;
  }

  function closeMenus() {
    document.querySelectorAll(".menu.task-menu").forEach((m) => m.remove());
  }

  function openChipMenu(task, anchor) {
    closeMenus();
    if (!task) return;

    const menu = document.createElement("div");
    menu.className = "menu task-menu";
    menu.setAttribute("role", "menu");

    const schedule = document.createElement("button");
    schedule.type = "button";
    schedule.className = "menu-item";
    schedule.setAttribute("role", "menuitem");
    schedule.textContent = "Schedule…";

    const field = document.createElement("div");
    field.className = "menu-field hidden";
    const input = document.createElement("input");
    input.type = "date";
    input.className = "text-input-control";
    input.value = task.due_date || localDateKey(new Date());
    input.setAttribute("aria-label", "Due date");
    field.appendChild(input);

    const clear = document.createElement("button");
    clear.type = "button";
    clear.className = "menu-item";
    clear.setAttribute("role", "menuitem");
    clear.textContent = "Clear date";
    clear.disabled = !task.due_date;

    let dismiss = () => {};

    async function updateDue(value) {
      try {
        await invoke("task_update", { task: { ...task, due_date: value } });
        const detailId = document.getElementById("detail-task-id");
        const detailDate = document.getElementById("detail-task-date");
        if (detailId && detailId.value === task.id && detailDate) {
          detailDate.value = value || "";
        }
      } catch (err) {
        console.error("Due date update failed:", err);
      }
      closeMenus();
      dismiss();
      if (typeof onChanged === "function") onChanged();
    }

    schedule.addEventListener("click", () => {
      field.classList.toggle("hidden");
      if (!field.classList.contains("hidden")) input.focus();
    });
    input.addEventListener("change", () => {
      if (input.value) updateDue(input.value);
    });
    clear.addEventListener("click", () => updateDue(null));

    menu.append(schedule, field, clear);
    document.body.appendChild(menu);

    const r = anchor.getBoundingClientRect();
    menu.style.position = "fixed";
    menu.style.top = `${r.bottom + 4}px`;
    menu.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - 220))}px`;
    anchor.setAttribute("aria-expanded", "true");
    schedule.focus();

    const close = (event) => {
      if (event) {
        if (event.type === "keydown" && event.key !== "Escape") return;
        if (event.type === "click" && menu.contains(event.target)) return;
      }
      menu.remove();
      anchor.setAttribute("aria-expanded", "false");
      document.removeEventListener("keydown", close);
      document.removeEventListener("click", close, true);
    };
    dismiss = close;
    document.addEventListener("keydown", close);
    document.addEventListener("click", close, true);
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
            <button type="button" id="cal-prev" class="icon-btn" aria-label="Previous month"><hj-icon name="chevron-left" size="16" aria-hidden="true"></hj-icon></button>
            <button type="button" id="cal-today" class="btn-secondary">Today</button>
            <button type="button" id="cal-next" class="icon-btn" aria-label="Next month"><hj-icon name="chevron-right" size="16" aria-hidden="true"></hj-icon></button>
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

    // Grid: whole weeks only (5 or 6 rows)
    let html = "";
    const cells = Math.ceil((firstOffset + daysInMonth) / 7) * 7;
    for (let slot = 0; slot < cells; slot++) {
      const dayNum = slot - firstOffset + 1;
      const inMonth = dayNum >= 1 && dayNum <= daysInMonth;
      const key = inMonth ? makeKey(year, month, dayNum) : "";
      const dayTasks = inMonth ? (byDue.get(key) || []) : [];
      const isToday = key === todayKey;
      const chips = dayTasks.map((t, i) => chipHtml(t, i >= MAX_CHIPS ? "overflow" : "")).join("");
      const more = dayTasks.length > MAX_CHIPS
        ? `<button type="button" class="more-chip">+${dayTasks.length - MAX_CHIPS} more</button>`
        : "";
      const slotDate = new Date(year, month, slot - firstOffset + 1);
      html += `
        <div class="calendar-day ${inMonth ? "" : "other-month"} ${isToday ? "today" : ""}" data-day="${key}">
          <span class="calendar-day-number">${slotDate.getDate()}</span>
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

    window.renderIcons?.();

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
        btn.closest(".calendar-day")?.classList.add("expanded");
        btn.remove();
      });
    });

    // Chip → open task detail; ⋯ → status/date menu
    function dayClick(e) {
      const menuBtn = e.target.closest(".task-chip-menu");
      if (menuBtn) {
        e.stopPropagation();
        const id = menuBtn.closest(".task-chip-row")?.dataset.id;
        openChipMenu(tasksById.get(id), menuBtn);
        return;
      }
      const chip = e.target.closest(".task-chip");
      if (chip) onEditTask(chip.dataset.id);
    }
    grid.addEventListener("click", dayClick);
    trayList.addEventListener("click", dayClick);

    // Drag-to-schedule (SortableJS, rebuilt per render so instances never go stale)
    // forceFallback + fallbackOnBody required for WebView2 compatibility
    if (typeof Sortable !== "undefined") {
      sortables.push(new Sortable(trayList, {
        group: { name: "tasks-calendar", pull: true, put: true },
        forceFallback: true,
        fallbackOnBody: true,
        animation: 150,
        filter: ".more-chip, .task-chip-menu",
        onEnd,
      }));
      grid.querySelectorAll(".day-tasks").forEach((dayTasks) => {
        if (dayTasks.closest(".other-month")) return;
        sortables.push(new Sortable(dayTasks, {
          group: { name: "tasks-calendar", pull: true, put: true },
          forceFallback: true,
          fallbackOnBody: true,
          animation: 150,
          filter: ".more-chip, .task-chip-menu",
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
      const detailId = document.getElementById("detail-task-id");
      const detailDate = document.getElementById("detail-task-date");
      if (detailId && detailId.value === id && detailDate) {
        detailDate.value = newDue || "";
      }
    } catch {
      // ipc.js already toasts; onChanged() re-fetches and rolls the DOM back to server truth
    }
    if (typeof onChanged === "function") onChanged();
  }

  return { render };
}