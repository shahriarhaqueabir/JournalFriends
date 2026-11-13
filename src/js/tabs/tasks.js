import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { showToast } from "../notifications.js";
import { initTasksCalendar } from "./tasks-calendar.js";
import { BOARD_LAYOUTS, columnStatusForTask, dropStatusFor } from "./board-model.js";

// Set inside initTasks so the global app_event router can refresh the list
// without reaching into the closure.
let refreshFromAi = null;

const TASK_CHANGE_LABELS = {
  created: "Task created",
  updated: "Task updated",
  completed: "Task completed",
  deleted: "Task deleted",
};

/** D-166: react to a backend `task_changed` event. */
export function handleTaskChanged(payload) {
  const label = TASK_CHANGE_LABELS[payload?.action];
  if (label) showToast(label, { variant: "success", timeoutMs: 3000 });
  refreshFromAi?.();
}

export function initTasks() {
  const listEl = document.getElementById("task-list-items");
  const searchInput = document.getElementById("task-search");
  const newTaskBtn = document.getElementById("btn-new-task");
  const taskTabs = document.querySelectorAll(".task-tab");
  const boardTabs = document.querySelectorAll(".board-tab");
  const viewTabs = document.querySelectorAll(".task-view-toggle .view-tab");
  const listContainer = document.getElementById("task-list-items");
  const boardContainer = document.getElementById("task-kanban-board");
  const calendarContainer = document.getElementById("task-calendar");

  let activeBoard = "inprogress";
  let activeView = "board";
  let allTasks = [];
  let calendarApi = null;
  
  // Detail Panel Elements
  const detailPanel = document.getElementById("task-detail-panel");
  const closeDetailBtn = document.getElementById("btn-close-task-detail");
  const saveTaskBtn = document.getElementById("btn-save-task");
  const deleteTaskBtn = document.getElementById("btn-delete-task");
  
  const detailId = document.getElementById("detail-task-id");
  const detailTitle = document.getElementById("detail-task-title");
  const detailDesc = document.getElementById("detail-task-desc");
  const detailPriority = document.getElementById("detail-task-priority");
  const detailStatus = document.getElementById("detail-task-status");
  const detailDate = document.getElementById("detail-task-date");
  const detailTag = document.getElementById("detail-task-tag");
  const detailRecurrence = document.getElementById("detail-task-recurrence");

  window.loadTask = async (id) => {
    const tasksTab = document.querySelector('[data-tab="tasks"]');
    if (tasksTab) tasksTab.click();
    await openDetail(id);
  };

  if (!listEl) return;

  refreshFromAi = () => refreshTasks(searchInput.value);

  let currentFilter = "all";

  // --- REFRESH / LIST ---
  async function refreshTasks(query = "") {
    try {
      const fetched = query.trim()
        ? await invoke("task_search", { query })
        : await invoke("task_list", { includeCompleted: true });

      allTasks = fetched;

      // Client-side filtering for sub-tabs
      let tasks = fetched;
      if (currentFilter === "pending") {
        tasks = tasks.filter(t => !t.archived && t.status !== 'done' && t.status !== 'cancelled');
      } else if (currentFilter === "completed") {
        tasks = tasks.filter(t => t.status === 'done' || t.archived);
      }

      renderKanban(tasks, activeBoard);
      renderTasks(tasks);
      calendarApi?.render(tasks);
      applyView();
    } catch (err) {
      console.error("Failed to load tasks:", err);
    }
  }

  function applyView() {
    listContainer.classList.toggle("hidden", activeView !== "list");
    boardContainer.classList.toggle("hidden", activeView !== "board");
    calendarContainer.classList.toggle("hidden", activeView !== "calendar");
    document.querySelectorAll(".view-toggler").forEach((el) => {
      el.classList.toggle("hidden", activeView !== "board");
    });
  }

  function renderKanban(tasks, board) {
    const layout = BOARD_LAYOUTS[board] || BOARD_LAYOUTS.inprogress;

    boardContainer.innerHTML = layout.map(col => `
      <div class="kanban-column" data-status="${col.status}">
        <div class="column-header">${col.label} <span class="column-count">0</span></div>
        <div class="column-items" data-status="${col.status}"></div>
      </div>
    `).join("");
    boardContainer.classList.toggle("boards-single", layout.length === 1);

    tasks.forEach(task => {
      const colStatus = columnStatusForTask(task);
      if (!colStatus) return; // cancelled (or unknown) hidden

      const col = boardContainer.querySelector(`.kanban-column[data-status="${colStatus}"] .column-items`);
      if (!col) return;

      const card = document.createElement("article");
      const stateClass = task.archived ? ' is-archived'
        : task.status === 'done' ? ' is-done'
        : task.status === 'in_progress' ? ' is-in-progress' : '';
      card.className = `task-card priority-${task.priority}${stateClass}`;
      card.dataset.id = task.id;

      const subtasks = tasks.filter(t => t.parent_task_id === task.id).length;
      card.innerHTML = `
        <button type="button" class="task-card-title">${escapeHtml(task.title)}</button>
        <div class="task-card-meta">
          ${task.due_date ? `<span>Due ${escapeHtml(task.due_date)}</span>` : ""}
          ${subtasks > 0 ? `<span>${subtasks} subtask${subtasks > 1 ? "s" : ""}</span>` : ""}
          <span class="priority-label">${escapeHtml(task.priority)}</span>
        </div>
        ${task.is_blocked ? '<span class="card-flag">Blocked</span>' : ""}
        ${task.parent_task_id ? '<span class="subtag">Subtask</span>' : ""}
        <button type="button" class="icon-btn task-card-menu" aria-label="Task actions" aria-haspopup="menu" aria-expanded="false">
          <hj-icon name="more-horizontal" size="16" aria-hidden="true"></hj-icon>
        </button>
      `;

      card.querySelector(".task-card-title").addEventListener("click", () => openDetail(task.id));
      card.querySelector(".task-card-menu").addEventListener("click", (e) => {
        e.stopPropagation();
        openTaskMenu(task.id, e.currentTarget);
      });

      col.appendChild(card);
    });

    boardContainer.querySelectorAll(".kanban-column").forEach(col => {
      col.querySelector(".column-count").textContent =
        col.querySelectorAll(".task-card").length;
    });

    window.renderIcons?.();

    // forceFallback avoids WebView2's broken native HTML5 drag path; never set card.draggable.
    // SortableJS — re-created on every render (columns are rebuilt)
    if (typeof Sortable !== "undefined") {
      boardContainer.querySelectorAll(".column-items").forEach((col) => {
        new Sortable(col, {
          group: { name: "board", pull: true, put: true },
          forceFallback: true,
          fallbackOnBody: true,
          animation: 150,
          filter: ".task-card-menu",
          onEnd: async (evt) => {
            const newColumn = evt.to?.dataset.status;
            const oldColumn = evt.from?.dataset.status;
            if (!newColumn || newColumn === oldColumn) return;
            const newStatus = dropStatusFor(newColumn);
            try {
              await invoke("task_update_status", { id: evt.item.dataset.id, status: newStatus });
              refreshTasks(searchInput.value);
            } catch (err) {
              console.error("Status update failed:", err);
            }
          },
        });
      });
    }
  }

  async function setTaskStatus(id, status) {
    try {
      await invoke("task_update_status", { id, status });
      await refreshTasks(searchInput.value);
    } catch (err) {
      console.error("Status update failed:", err);
    }
  }

  function openTaskMenu(taskId, anchor) {
    document.querySelectorAll(".menu.task-menu").forEach((m) => m.remove());
    const task = allTasks.find((t) => t.id === taskId);
    if (!task) return;
    const items = task.archived
      ? [["Restore", () => setTaskStatus(taskId, "todo")]]
      : [["To Do", () => setTaskStatus(taskId, "todo")],
         ["In Progress", () => setTaskStatus(taskId, "in_progress")],
         ["Done", () => setTaskStatus(taskId, "done")],
         ["Archive", () => setTaskStatus(taskId, "archived")]];
    const menu = document.createElement("div");
    menu.className = "menu task-menu";
    menu.setAttribute("role", "menu");
    for (const [label, fn] of items) {
      const b = document.createElement("button");
      b.type = "button";
      b.className = "menu-item";
      b.setAttribute("role", "menuitem");
      b.textContent = label;
      b.addEventListener("click", () => { fn(); close(); });
      menu.appendChild(b);
    }
    document.body.appendChild(menu);
    const r = anchor.getBoundingClientRect();
    menu.style.position = "fixed";
    menu.style.top = `${r.bottom + 4}px`;
    menu.style.left = `${Math.max(8, Math.min(r.left, window.innerWidth - 200))}px`;
    anchor.setAttribute("aria-expanded", "true");
    menu.querySelector("button")?.focus();
    const close = (e) => {
      if (e) {
        if (e.type === "keydown" && e.key !== "Escape") return;
        if (e.type === "click" && menu.contains(e.target)) return;
      }
      menu.remove();
      anchor.setAttribute("aria-expanded", "false");
      anchor.focus();
      document.removeEventListener("keydown", close);
      document.removeEventListener("click", close, true);
    };
    document.addEventListener("keydown", close);
    document.addEventListener("click", close, true);
  }

  function renderTasks(tasks) {
    if (!tasks || tasks.length === 0) {
      listEl.innerHTML = '<div class="list-empty">No tasks found.</div>';
      return;
    }

    // Sort/Organize tasks into a flat list with children following parents
    const taskMap = new Map(tasks.map(t => [t.id, { ...t, children: [] }]));
    const roots = [];
    
    taskMap.forEach(task => {
      if (task.parent_task_id && taskMap.has(task.parent_task_id)) {
        taskMap.get(task.parent_task_id).children.push(task);
      } else {
        roots.push(task);
      }
    });

    const flattened = [];
    function flatten(nodes) {
      nodes.forEach(n => {
        flattened.push(n);
        if (n.children.length > 0) flatten(n.children);
      });
    }
    flatten(roots);

    function taskItemHtml(task) {
      const priorityClass = `priority-${task.priority}`;
      const isCompleted = task.status === 'done';
      const isSubtask = task.parent_task_id !== null;
      const checked = isCompleted ? 'checked' : '';
      const tag = (task.project || "").trim();
      return `
        <div class="task-item ${priorityClass} ${isCompleted ? 'completed' : ''} ${isSubtask ? 'is-subtask' : ''} ${task.is_blocked ? 'is-blocked' : ''}" data-id="${task.id}">
          <input type="checkbox" ${checked} class="task-toggle" title="Mark as done" aria-label="Mark ${escapeHtml(task.title)} as done" />
          <div class="task-content">
            <div class="task-title">
              ${task.is_blocked ? '<span class="task-blocker-icon">Blocked</span>' : ''}
              ${escapeHtml(task.title)}
            </div>
            <div class="task-meta">
               ${tag ? `<span class="task-tag-badge">${escapeHtml(tag)}</span>` : ""}
               ${task.due_date ? `<span class="task-due">Due ${escapeHtml(task.due_date)}</span>` : ""}
               ${task.energy_level ? `<span class="task-energy">${escapeHtml(task.energy_level.replace('_', ' '))}</span>` : ""}
               <span class="task-badge">${escapeHtml(task.priority)}</span>
            </div>
          </div>
        </div>
      `;
    }

    // Group by tag (project) when any tagged tasks exist; otherwise flat list.
    const tagMap = new Map();
    const untagged = [];
    flattened.forEach((t) => {
      const tag = (t.project || "").trim();
      if (!tag) untagged.push(t);
      else {
        if (!tagMap.has(tag)) tagMap.set(tag, []);
        tagMap.get(tag).push(t);
      }
    });
    const hasTags = tagMap.size > 0;

    if (!hasTags) {
      listEl.innerHTML = flattened.map(taskItemHtml).join("");
    } else {
      const groups = [];
      const sortedTags = Array.from(tagMap.keys()).sort((a, b) => a.localeCompare(b));
      sortedTags.forEach((tag) => {
        const items = tagMap.get(tag).map(taskItemHtml).join("");
        groups.push(`
          <section class="task-tag-group">
            <header class="tag-group-header">
              <span class="tag-group-title">
                <span class="tag-pill">${escapeHtml(tag)}</span>
              </span>
              <span class="tag-group-count">${tagMap.get(tag).length}</span>
            </header>
            <div class="tag-group-items">${items}</div>
          </section>
        `);
      });
      if (untagged.length) {
        const items = untagged.map(taskItemHtml).join("");
        groups.push(`
          <section class="task-tag-group">
            <header class="tag-group-header">
              <span class="tag-group-title">Untagged</span>
              <span class="tag-group-count">${untagged.length}</span>
            </header>
            <div class="tag-group-items">${items}</div>
          </section>
        `);
      }
      listEl.innerHTML = groups.join("");
    }

    // Toggle listener
    listEl.querySelectorAll(".task-toggle").forEach(cb => {
      cb.addEventListener("click", (e) => e.stopPropagation()); // Don't open panel when toggling
      cb.addEventListener("change", async (e) => {
        const id = e.target.closest(".task-item").dataset.id;
        const newStatus = e.target.checked ? "done" : "todo";
        await invoke("task_update_status", { id, status: newStatus });
        refreshTasks(searchInput.value);
      });
    });

    // Item click -> Open Detail
    listEl.querySelectorAll(".task-item").forEach(item => {
      item.addEventListener("click", () => openDetail(item.dataset.id));
    });
  }

  // --- DETAIL PANEL ---
  async function openDetail(id) {
    try {
      const task = await invoke("task_get", { id });
      if (!task) return;

      detailId.value = task.id;
      detailTitle.value = task.title;
      detailDesc.value = task.description || "";
      detailPriority.value = task.priority;
      detailStatus.value = task.status;
      detailDate.value = task.due_date || "";
      if (detailTag) detailTag.value = task.project || "";
      detailRecurrence.value = task.recurrence_rule || "";

      // Hide subtask section if this is already a level-2 subtask
      const subtaskSection = document.getElementById("subtasks-section");
      if (task.parent_task_id) {
        subtaskSection.classList.add("hidden");
      } else {
        subtaskSection.classList.remove("hidden");
        // Load subtasks for this task
        const allTasks = await invoke("task_list");
        const subtasks = allTasks.filter(t => t.parent_task_id === task.id);
        renderSubtaskMiniList(subtasks);
      }

      // Load dependencies
      const deps = await invoke("task_get_dependencies", { taskId: task.id });
      renderDependencyList(deps, task.id);

      detailPanel.classList.remove("hidden");
    } catch (err) {
      console.error("Failed to get task details:", err);
    }
  }

  function renderDependencyList(deps, currentTaskId) {
    const container = document.getElementById("dependency-list");
    container.innerHTML = deps.map(d => `
      <div class="dependency-chip">
        <span>${escapeHtml(d.title)}</span>
        <span class="remove-dep" data-blocked="${currentTaskId}" data-blocking="${d.id}">×</span>
      </div>
    `).join("");

    container.querySelectorAll(".remove-dep").forEach(btn => {
      btn.addEventListener("click", async () => {
        const { blocked, blocking } = btn.dataset;
        await invoke("task_remove_dependency", { blockedId: blocked, blockingId: blocking });
        const newDeps = await invoke("task_get_dependencies", { taskId: blocked });
        renderDependencyList(newDeps, blocked);
        refreshTasks(searchInput.value);
      });
    });
  }

  function renderSubtaskMiniList(subtasks) {
    const container = document.getElementById("subtask-list");
    if (!subtasks || subtasks.length === 0) {
      container.innerHTML = "";
      return;
    }

    container.innerHTML = subtasks.map(s => `
      <div class="subtask-mini-item">
        <input type="checkbox" ${s.status === 'done' ? 'checked' : ''} disabled />
        <span>${escapeHtml(s.title)}</span>
      </div>
    `).join("");
  }

  function closeDetail() {
    detailPanel.classList.add("hidden");
  }

  // --- ACTIONS ---
  searchInput.addEventListener("input", (e) => {
    refreshTasks(e.target.value);
  });

  newTaskBtn.addEventListener("click", async () => {
    try {
      const id = await invoke("task_create", { title: "New Task" });
      refreshTasks(searchInput.value);
      openDetail(id);
    } catch (err) {
      console.error("Failed to create task:", err);
    }
  });

  document.getElementById("btn-add-subtask").addEventListener("click", async () => {
    try {
      const parentId = detailId.value;
      const id = await invoke("task_create", { 
        title: "New Subtask", 
        parentTaskId: parentId 
      });
      // Refresh both
      refreshTasks(searchInput.value);
      openDetail(parentId);
    } catch (err) {
      // showError will be called if it exceeds limit
      console.error("Failed to create subtask:", err);
      alert(err);
    }
  });

  saveTaskBtn.addEventListener("click", async () => {
    const task = {
      id: detailId.value,
      title: detailTitle.value,
      description: detailDesc.value || null,
      priority: detailPriority.value,
      status: detailStatus.value,
      due_date: detailDate.value || null,
      due_time: null,
      reminder_at: null,
      reminder_fired: false,
      time_estimate: null,
      time_logged: 0,
      tags: "[]",
      labels: "[]",
      category: null,
      project: (detailTag ? detailTag.value.trim() : null) || null,
      notes: null,
      recurrence_rule: detailRecurrence.value || null,
      next_occurrence: null,
      energy_level: null,
      context_tag: null,
      linked_url: null,
      ai_created: false,
      is_blocked: false, // Calculated by backend on read
      created_at: "", 
      updated_at: "",
      completed_at: null
    };

    try {
      await invoke("task_update", { task });
      closeDetail();
      refreshTasks(searchInput.value);
    } catch (err) {
      console.error("Failed to update task:", err);
    }
  });

  deleteTaskBtn.addEventListener("click", async () => {
    if (!confirm("Are you sure you want to delete this task?")) return;
    try {
      await invoke("task_delete", { id: detailId.value });
      closeDetail();
      refreshTasks(searchInput.value);
    } catch (err) {
      console.error("Failed to delete task:", err);
    }
  });

  closeDetailBtn.addEventListener("click", closeDetail);

  // --- TAB FILTERING ---
  taskTabs.forEach(tab => {
    tab.addEventListener("click", () => {
      taskTabs.forEach(t => { t.classList.remove("active"); t.setAttribute("aria-pressed", "false"); });
      tab.classList.add("active");
      tab.setAttribute("aria-pressed", "true");
      currentFilter = tab.dataset.filter;
      refreshTasks(searchInput.value);
    });
  });

  // --- DEPENDENCY SEARCH ---
  const depSearchInput = document.getElementById("dependency-search");
  const depSearchResults = document.getElementById("dependency-search-results");

  depSearchInput.addEventListener("input", async (e) => {
    const query = e.target.value.trim();
    if (query.length < 2) {
      depSearchResults.classList.add("hidden");
      return;
    }

    try {
      const results = await invoke("task_search", { query });
      const currentId = detailId.value;
      // Filter out self and already added dependencies? 
      // For now just self.
      const filtered = results.filter(r => r.id !== currentId).slice(0, 5);
      
      if (filtered.length === 0) {
        depSearchResults.innerHTML = '<div class="mini-search-item">No tasks found</div>';
      } else {
        depSearchResults.innerHTML = filtered.map(r => `
          <div class="mini-search-item" data-id="${escapeHtml(r.id)}">${escapeHtml(r.title)}</div>
        `).join("");
      }
      depSearchResults.classList.remove("hidden");

      depSearchResults.querySelectorAll(".mini-search-item").forEach(item => {
        item.addEventListener("click", async () => {
          const blockingId = item.dataset.id;
          const blockedId = detailId.value;
          await invoke("task_add_dependency", { blockedId, blockingId });
          
          depSearchInput.value = "";
          depSearchResults.classList.add("hidden");
          
          const newDeps = await invoke("task_get_dependencies", { taskId: blockedId });
          renderDependencyList(newDeps, blockedId);
          refreshTasks(searchInput.value);
        });
      });
    } catch (err) {
      console.error("Dependency search failed:", err);
    }
  });

  // Close search results when clicking outside
  document.addEventListener("click", (e) => {
    if (!depSearchInput.contains(e.target) && !depSearchResults.contains(e.target)) {
      depSearchResults.classList.add("hidden");
    }
  });

  // --- VIEW SWITCHING ---
  viewTabs.forEach(tab => {
    tab.addEventListener("click", () => {
      viewTabs.forEach(t => { t.classList.remove("active"); t.setAttribute("aria-pressed", "false"); });
      tab.classList.add("active");
      tab.setAttribute("aria-pressed", "true");
      activeView = tab.dataset.view;
      applyView();
      if (activeView === "calendar") calendarApi?.render(allTasks);
    });
  });

  // --- BOARD SWITCHING ---
  boardTabs.forEach(tab => {
    tab.addEventListener("click", () => {
      boardTabs.forEach(t => { t.classList.remove("active"); t.setAttribute("aria-pressed", "false"); });
      tab.classList.add("active");
      tab.setAttribute("aria-pressed", "true");
      activeBoard = tab.dataset.board;
      refreshTasks(searchInput.value);
    });
  });

  // --- CALENDAR VIEW ---
  calendarApi = initTasksCalendar({
    onEditTask: (id) => openDetail(id),
    onChanged: () => refreshTasks(searchInput.value),
  });

  // Initial load — gate on vault status (startup runs before unlock, when
  // task_list fails with VaultLocked) and refetch on vault:unlocked.
  invoke("vault_status", {}, { silent: true })
    .then((s) => {
      if (s.unlocked) refreshTasks();
    })
    .catch(() => {});
  document.addEventListener("vault:unlocked", () => refreshTasks());
}
