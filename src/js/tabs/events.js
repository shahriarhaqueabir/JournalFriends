import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { recordDiagnostic } from "../diagnostics.js";

let currentView = "events"; // events, notes, calendar
let calendarDate = new Date();

export async function initEvents() {
    const tabEl = document.getElementById("tab-events");
    if (!tabEl) return;

    // View switcher
    tabEl.querySelectorAll(".switcher-btn").forEach(btn => {
        btn.addEventListener("click", () => {
            tabEl.querySelectorAll(".switcher-btn").forEach(b => b.classList.remove("active"));
            btn.classList.add("active");
            currentView = btn.dataset.view;
            refreshView();
        });
    });

    // New item buttons
    document.getElementById("btn-new-event")?.addEventListener("click", () => openEventDetail());
    document.getElementById("btn-new-note")?.addEventListener("click", () => openNoteDetail());

    // Detail panel close
    document.getElementById("btn-close-event-detail")?.addEventListener("click", () => closePanel("event-detail-panel"));
    document.getElementById("btn-close-note-detail")?.addEventListener("click", () => closePanel("note-detail-panel"));

    // Save buttons
    document.getElementById("btn-save-event")?.addEventListener("click", saveEvent);
    document.getElementById("btn-save-note")?.addEventListener("click", saveNote);

    // Delete buttons
    document.getElementById("btn-delete-event")?.addEventListener("click", deleteEvent);
    document.getElementById("btn-delete-note")?.addEventListener("click", deleteNote);

    refreshView();
}

window.loadEvent = async (id) => {
    try {
        const event = await invoke("event_get", { id });
        if (event) {
            const eventsTab = document.querySelector('[data-tab="events"]');
            if (eventsTab) eventsTab.click();

            // Switch to events view if not already there
            const switcher = document.querySelector('.switcher-btn[data-view="events"]');
            if (switcher) switcher.click();

            openEventDetail(event);
        }
    } catch (e) {
        console.error("Failed to load event:", e);
    }
};

async function refreshView() {
    const listContainer = document.getElementById("events-list");
    const calendarContainer = document.getElementById("events-calendar");

    if (currentView === "calendar") {
        listContainer.classList.add("hidden");
        calendarContainer.classList.remove("hidden");
        renderCalendar();
    } else {
        listContainer.classList.remove("hidden");
        calendarContainer.classList.add("hidden");
        if (currentView === "events") {
            await loadEvents();
        } else {
            await loadNotes();
        }
    }
}

async function loadEvents() {
    const itemsEl = document.getElementById("events-list-items");
    itemsEl.innerHTML = "Loading events...";
    try {
        const events = await invoke("event_list", { limit: 100 });
        renderEvents(events);
    } catch (e) {
        itemsEl.innerHTML = `<div class="error">Failed to load events: ${e}</div>`;
    }
}

function renderEvents(events) {
    const itemsEl = document.getElementById("events-list-items");
    if (!events || events.length === 0) {
        itemsEl.innerHTML = '<div class="list-empty">No upcoming events.</div>';
        return;
    }

    itemsEl.innerHTML = events.map(ev => `
        <div class="task-item event-item" data-id="${escapeHtml(ev.id)}">
            <div class="task-item-main">
                <div class="task-item-title">🗓️ ${escapeHtml(ev.title)}</div>
                <div class="task-item-meta">
                    <span>${escapeHtml(new Date(ev.start_at).toLocaleString())}</span>
                    ${ev.location ? `<span>📍 ${escapeHtml(ev.location)}</span>` : ''}
                </div>
            </div>
        </div>
    `).join("");

    itemsEl.querySelectorAll(".event-item").forEach(el => {
        el.addEventListener("click", () => openEventDetail(events.find(e => e.id === el.dataset.id)));
    });
}

async function loadNotes() {
    const itemsEl = document.getElementById("events-list-items");
    itemsEl.innerHTML = "Loading notes...";
    try {
        const notes = await invoke("note_list", { limit: 100 });
        renderNotes(notes);
    } catch (e) {
        itemsEl.innerHTML = `<div class="error">Failed to load notes: ${e}</div>`;
    }
}

function renderNotes(notes) {
    const itemsEl = document.getElementById("events-list-items");
    if (!notes || notes.length === 0) {
        itemsEl.innerHTML = '<div class="list-empty">No notes yet.</div>';
        return;
    }

    itemsEl.innerHTML = notes.map(n => `
        <div class="task-item note-item" data-id="${escapeHtml(n.id)}">
            <div class="task-item-main">
                <div class="task-item-title">📝 ${escapeHtml(n.title || 'Untitled Note')}</div>
                <div class="task-item-meta">
                    <span>Last updated: ${escapeHtml(new Date(n.updated_at).toLocaleDateString())}</span>
                </div>
            </div>
        </div>
    `).join("");

    itemsEl.querySelectorAll(".note-item").forEach(el => {
        el.addEventListener("click", () => openNoteDetail(notes.find(n => n.id === el.dataset.id)));
    });
}

function openEventDetail(ev = null) {
    const panel = document.getElementById("event-detail-panel");
    panel.classList.remove("hidden");

    document.getElementById("detail-event-id").value = ev?.id || "";
    document.getElementById("detail-event-title").value = ev?.title || "";
    document.getElementById("detail-event-desc").value = ev?.description || "";
    document.getElementById("detail-event-start").value = ev?.start_at ? ev.start_at.slice(0, 16) : "";
    document.getElementById("detail-event-end").value = ev?.end_at ? ev.end_at.slice(0, 16) : "";
    document.getElementById("detail-event-all-day").checked = ev?.is_all_day || false;
    document.getElementById("detail-event-location").value = ev?.location || "";
    document.getElementById("detail-event-reminder").value = ev?.reminder_at ? ev.reminder_at.slice(0, 16) : "";
}

function openNoteDetail(note = null) {
    const panel = document.getElementById("note-detail-panel");
    panel.classList.remove("hidden");

    document.getElementById("detail-note-id").value = note?.id || "";
    document.getElementById("detail-note-title").value = note?.title || "";
    document.getElementById("detail-note-content").value = note?.content || "";
    document.getElementById("detail-note-tags").value = note?.tags || "[]";
}

async function saveEvent() {
    const event = {
        id: document.getElementById("detail-event-id").value,
        title: document.getElementById("detail-event-title").value,
        description: document.getElementById("detail-event-desc").value || null,
        start_at: new Date(document.getElementById("detail-event-start").value).toISOString(),
        end_at: document.getElementById("detail-event-end").value ? new Date(document.getElementById("detail-event-end").value).toISOString() : null,
        is_all_day: document.getElementById("detail-event-all-day").checked,
        location: document.getElementById("detail-event-location").value || null,
        recurrence_rule: null,
        reminder_at: document.getElementById("detail-event-reminder").value ? new Date(document.getElementById("detail-event-reminder").value).toISOString() : null,
        reminder_fired: false,
        status: "confirmed",
        created_by: "user",
        source_entry_id: null,
        created_at: "",
        updated_at: "",
        is_deleted: false
    };

    try {
        await invoke("event_save", { event });
        closePanel("event-detail-panel");
        refreshView();
    } catch (e) {
        alert("Failed to save event: " + e);
    }
}

async function saveNote() {
    const note = {
        id: document.getElementById("detail-note-id").value,
        title: document.getElementById("detail-note-title").value || null,
        content: document.getElementById("detail-note-content").value,
        tags: document.getElementById("detail-note-tags").value || "[]",
        source_entry_id: null,
        created_by: "user",
        created_at: "",
        updated_at: "",
        is_deleted: false
    };

    try {
        await invoke("note_save", { note });
        closePanel("note-detail-panel");
        refreshView();
    } catch (e) {
        alert("Failed to save note: " + e);
    }
}

async function deleteEvent() {
    const id = document.getElementById("detail-event-id").value;
    if (!id || !confirm("Delete this event?")) return;
    try {
        await invoke("event_delete", { id });
        closePanel("event-detail-panel");
        refreshView();
    } catch (e) {
        alert("Failed to delete event: " + e);
    }
}

async function deleteNote() {
    const id = document.getElementById("detail-note-id").value;
    if (!id || !confirm("Delete this note?")) return;
    try {
        await invoke("note_delete", { id });
        closePanel("note-detail-panel");
        refreshView();
    } catch (e) {
        alert("Failed to delete note: " + e);
    }
}

function closePanel(id) {
    document.getElementById(id).classList.add("hidden");
}

async function renderCalendar() {
    const container = document.getElementById("events-calendar");
    const year = calendarDate.getFullYear();
    const month = calendarDate.getMonth();

    // Fetch events for the month
    // We'll just fetch a broad range for now
    let events = [];
    try {
        events = await invoke("event_list", { limit: 200 });
    } catch (e) {
        console.error("Failed to fetch events for calendar", e);
    }

    const firstDay = new Date(year, month, 1).getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const prevMonthDays = new Date(year, month, 0).getDate();

    let html = `
        <div class="calendar-view">
            <div class="calendar-header">
                <button id="btn-prev-month" class="btn-ghost-sm">❮</button>
                <h3>${calendarDate.toLocaleString('default', { month: 'long', year: 'numeric' })}</h3>
                <button id="btn-next-month" class="btn-ghost-sm">❯</button>
            </div>
            <div class="calendar-grid">
                <div class="calendar-day-label">Sun</div>
                <div class="calendar-day-label">Mon</div>
                <div class="calendar-day-label">Tue</div>
                <div class="calendar-day-label">Wed</div>
                <div class="calendar-day-label">Thu</div>
                <div class="calendar-day-label">Fri</div>
                <div class="calendar-day-label">Sat</div>
    `;

    // Previous month days
    for (let i = firstDay - 1; i >= 0; i--) {
        html += `<div class="calendar-day other-month"><span class="calendar-day-number">${prevMonthDays - i}</span></div>`;
    }

    // Current month days
    const today = new Date();
    today.setHours(0, 0, 0, 0);

    for (let day = 1; day <= daysInMonth; day++) {
        const currentLocalDate = new Date(year, month, day);
        const isToday = currentLocalDate.getTime() === today.getTime();

        // Match events that start on this day OR are currently occurring (multi-day)
        const dayEvents = events.filter(e => {
            if (!e.start_at) return false;
            const start = new Date(e.start_at);
            start.setHours(0, 0, 0, 0);

            if (e.end_at) {
                const end = new Date(e.end_at);
                end.setHours(23, 59, 59, 999);
                return currentLocalDate >= start && currentLocalDate <= end;
            }
            return currentLocalDate.getTime() === start.getTime();
        });

        html += `
            <div class="calendar-day ${isToday ? 'today' : ''}">
                <span class="calendar-day-number">${day}</span>
                <div class="calendar-events">
                    ${dayEvents.map(e => {
                        const isAi = e.created_by === 'ai';
                        const hasReminder = e.reminder_at && !e.reminder_fired;
                        const typeClass = isAi ? 'ai-proposed' : 'user-created';
                        const reminderClass = hasReminder ? 'has-reminder' : '';

                        return `
                            <div class="calendar-event-pill ${typeClass} ${reminderClass}"
                                 data-id="${e.id}"
                                 title="${escapeHtml(e.title)} ${hasReminder ? '(Reminder set)' : ''}">
                                ${escapeHtml(e.title)}
                            </div>
                        `;
                    }).join('')}
                </div>
            </div>
        `;
    }

    // Next month days (fill the grid)
    const totalSlots = 42;
    const currentSlots = firstDay + daysInMonth;
    for (let i = 1; i <= totalSlots - currentSlots; i++) {
        html += `<div class="calendar-day other-month"><span class="calendar-day-number">${i}</span></div>`;
    }

    html += `
            </div>
        </div>
    `;

    container.innerHTML = html;

    // Listeners
    container.querySelector("#btn-prev-month").addEventListener("click", () => {
        calendarDate.setMonth(calendarDate.getMonth() - 1);
        renderCalendar();
    });
    container.querySelector("#btn-next-month").addEventListener("click", () => {
        calendarDate.setMonth(calendarDate.getMonth() + 1);
        renderCalendar();
    });
    container.querySelectorAll(".calendar-event-pill").forEach(pill => {
        pill.addEventListener("click", (e) => {
            e.stopPropagation();
            const event = events.find(ev => ev.id === pill.dataset.id);
            if (event) openEventDetail(event);
        });
    });
}
