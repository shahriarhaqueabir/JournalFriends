import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";

export function localDateKey(date) {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

export function makeKey(year, monthIndex, day) {
  return `${year}-${String(monthIndex + 1).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

export const MOOD_COLORS = {
  calm: "#5aa9e6", focused: "#8b7cf6", joyful: "#e6c05a",
  anxious: "#e08b8b", frustrated: "#e86b5a", sad: "#7a8ab8", energetic: "#59d6a1",
};

export function moodColor(mood) {
  return MOOD_COLORS[mood] || "#9aa0a6";
}

export function moodLabel(mood) {
  if (!mood) return "";
  return mood.charAt(0).toUpperCase() + mood.slice(1);
}

const CHIPS = ["calm", "focused", "joyful", "anxious", "frustrated", "sad", "energetic"];

export function renderMoodChips(selected) {
  return CHIPS.map((m, i) => {
    const isSelected = m === selected;
    const tabIndex = selected ? (isSelected ? 0 : -1) : (i === 0 ? 0 : -1);
    return `<button type="button" class="${isSelected ? "mood-chip selected" : "mood-chip"}" data-mood="${m}" role="radio" aria-checked="${isSelected}" tabindex="${tabIndex}">${escapeHtml(moodLabel(m))}</button>`;
  }).join("");
}

export function initMood() {
  const grid = document.getElementById("mood-month-grid");
  if (!grid) return;
  const chipRow = document.getElementById("mood-chips");
  const noteInput = document.getElementById("mood-note");
  const statusEl = document.getElementById("mood-save-status");
  const cursor = new Date();
  cursor.setDate(1);
  cursor.setHours(0, 0, 0, 0);
  let daysByDate = new Map();
  let selectedMood = "";
  let selectedDate = localDateKey(new Date());

  if (chipRow) chipRow.innerHTML = renderMoodChips("");

  function paintChips(selected) {
    if (!chipRow) return;
    chipRow.querySelectorAll(".mood-chip").forEach((b, i) => {
      const on = b.dataset.mood === selected;
      b.classList.toggle("selected", on);
      b.setAttribute("aria-checked", String(on));
      b.tabIndex = on || (!selected && i === 0) ? 0 : -1;
    });
  }

  if (chipRow) chipRow.addEventListener("click", (e) => {
    const btn = e.target.closest(".mood-chip");
    if (!btn) return;
    selectedMood = btn.dataset.mood;
    paintChips(selectedMood);
    if (statusEl) statusEl.textContent = "";
  });

  if (chipRow) chipRow.addEventListener("keydown", (e) => {
    const btns = [...chipRow.querySelectorAll(".mood-chip")];
    const i = btns.indexOf(document.activeElement);
    if (i < 0) return;
    let next = -1;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = (i + 1) % btns.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = (i - 1 + btns.length) % btns.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = btns.length - 1;
    if (next < 0) return;
    e.preventDefault();
    const btn = btns[next];
    btn.focus();
    selectedMood = btn.dataset.mood;
    paintChips(selectedMood);
    if (statusEl) statusEl.textContent = "";
  });

  async function loadMonth() {
    const year = cursor.getFullYear();
    const month = cursor.getMonth();
    const start = makeKey(year, month, 1);
    const end = makeKey(year, month, new Date(year, month + 1, 0).getDate());
    let items = [];
    try {
      items = await invoke("mood_list", { start, end }, { silent: true });
    } catch (err) {
      console.error("mood_list failed", err);
    }
    daysByDate = new Map((items || []).map((d) => [d.date, d]));
    renderGrid();
    renderChart(items || []);
  }

  function renderGrid() {
    const year = cursor.getFullYear();
    const month = cursor.getMonth();
    const firstOffset = new Date(year, month, 1).getDay();
    const daysInMonth = new Date(year, month + 1, 0).getDate();
    const slots = Math.ceil((firstOffset + daysInMonth) / 7) * 7;
    const todayKey = localDateKey(new Date());
    const title = document.getElementById("mood-cal-title");
    if (title) title.textContent = `${new Date(year, month, 1).toLocaleString(undefined, { month: "long" })} ${year}`;

    let html = "";
    for (let slot = 0; slot < slots; slot++) {
      const dayNum = slot - firstOffset + 1;
      const inMonth = dayNum >= 1 && dayNum <= daysInMonth;
      if (!inMonth) {
        html += `<div class="calendar-day other-month" aria-hidden="true"></div>`;
        continue;
      }
      const key = makeKey(year, month, dayNum);
      const day = daysByDate.get(key);
      const isToday = key === todayKey;
      const isSelected = key === selectedDate;
      const chip = day
        ? `<span class="mood-day-label" title="${escapeHtml(moodLabel(day.mood))}${day.note ? " — " + escapeHtml(day.note) : ""}">${escapeHtml(moodLabel(day.mood))}</span>`
        : "";
      const aiDot = day && day.source === "ai"
        ? `<span class="mood-ai-dot" role="img" aria-label="Inferred from your journal"></span>`
        : "";
      const label = `${new Date(year, month, dayNum).toDateString()}${day ? " — " + moodLabel(day.mood) : ""}`;
      html += `
        <button type="button" class="calendar-day ${isToday ? "today" : ""} ${isSelected ? "selected" : ""}" data-day="${key}" data-mood="${day ? escapeHtml(day.mood) : ""}" aria-pressed="${isSelected}" aria-label="${escapeHtml(label)}">
          <span class="calendar-day-number">${dayNum}</span>
          ${chip}${aiDot}
        </button>`;
    }
    grid.innerHTML = html;

    grid.querySelectorAll(".calendar-day[data-day]").forEach((cell) => {
      cell.addEventListener("click", () => {
        if (!cell.dataset.day) return;
        selectedDate = cell.dataset.day;
        const existing = daysByDate.get(selectedDate);
        if (noteInput) noteInput.value = existing && existing.note ? existing.note : "";
        selectedMood = existing ? existing.mood : "";
        paintChips(selectedMood);
        grid.querySelectorAll(".calendar-day[data-day]").forEach((c) => {
          const on = c.dataset.day === selectedDate;
          c.classList.toggle("selected", on);
          c.setAttribute("aria-pressed", String(on));
        });
        if (statusEl) statusEl.textContent = existing ? `Editing ${selectedDate}` : `New check-in for ${selectedDate}`;
      });
    });
  }

  function renderChart(items) {
    const canvas = document.getElementById("mood-chart");
    if (!canvas || typeof Chart === "undefined") return;
    const styles = getComputedStyle(document.documentElement);
    const accent = styles.getPropertyValue("--color-accent").trim();
    const muted = styles.getPropertyValue("--color-text-muted").trim();
    const border = styles.getPropertyValue("--color-border").trim();
    const labels = items.map((d) => d.date.slice(8));
    const MOOD_SCORES = {
      joyful: 9,
      energetic: 8,
      focused: 7,
      calm: 5,
      sad: 3,
      anxious: 2,
      frustrated: 1,
    };
    const data = items.map((d) => {
      return MOOD_SCORES[d.mood] ?? 5;
    });
    if (canvas._chart) canvas._chart.destroy();
    canvas._chart = new Chart(canvas, {
      type: "line",
      data: {
        labels,
        datasets: [{
          data,
          borderColor: accent,
          backgroundColor: "transparent",
          borderWidth: 2,
          pointRadius: 3,
          pointBackgroundColor: accent,
          pointBorderColor: accent,
          tension: 0.25,
          fill: false,
        }],
      },
      options: {
        responsive: true,
        maintainAspectRatio: false,
        scales: {
          x: {
            ticks: { color: muted, font: { size: 12 } },
            grid: { display: false },
            border: { color: border },
          },
          y: {
            min: 0,
            max: 10,
            ticks: {
              stepSize: 1,
              color: muted,
              font: { size: 12 },
              callback: (v) => {
                if (v === 10) return "10 (Peak)";
                if (v === 5) return "5 (Average)";
                if (v === 0) return "0 (Low)";
                if (v % 2 === 0) return String(v);
                return "";
              },
            },
            grid: { color: border },
            border: { display: false },
          },
        },
        plugins: {
          legend: { display: false },
          tooltip: {
            backgroundColor: styles.getPropertyValue("--color-surface-raised").trim(),
            borderColor: border,
            borderWidth: 1,
            titleColor: styles.getPropertyValue("--color-text").trim(),
            bodyColor: styles.getPropertyValue("--color-text-muted").trim(),
            callbacks: {
              label: (ctx) => {
                const item = items[ctx.dataIndex];
                const score = ctx.parsed.y;
                return `${moodLabel(item?.mood)} (${score}/10)`;
              },
            },
          },
        },
      },
    });
  }

  document.getElementById("mood-prev").addEventListener("click", () => {
    cursor.setMonth(cursor.getMonth() - 1);
    loadMonth();
  });
  document.getElementById("mood-next").addEventListener("click", () => {
    cursor.setMonth(cursor.getMonth() + 1);
    loadMonth();
  });
  document.getElementById("mood-today").addEventListener("click", () => {
    const now = new Date();
    cursor.setFullYear(now.getFullYear(), now.getMonth(), 1);
    cursor.setHours(0, 0, 0, 0);
    loadMonth();
  });
  const saveBtn = document.getElementById("btn-mood-save");
  if (saveBtn) saveBtn.addEventListener("click", saveCheckin);
  const removeBtn = document.getElementById("btn-mood-remove");
  if (removeBtn) removeBtn.addEventListener("click", removeCheckin);

  async function saveCheckin() {
    if (!selectedMood) {
      if (statusEl) statusEl.textContent = "Pick a mood first.";
      return;
    }
    saveBtn.disabled = true;
    try {
      await invoke("mood_checkin", { mood: selectedMood, date: selectedDate, note: noteInput.value || null });
      if (statusEl) statusEl.textContent = "Saved.";
      await loadMonth();
    } catch (err) {
      if (statusEl) statusEl.textContent = "Failed to save.";
    } finally {
      saveBtn.disabled = false;
    }
  }

  async function removeCheckin() {
    const hadMood = daysByDate.has(selectedDate);
    if (!hadMood) return;
    removeBtn.disabled = true;
    try {
      await invoke("mood_delete", { date: selectedDate });
      if (statusEl) statusEl.textContent = "Removed.";
      selectedMood = "";
      paintChips("");
      await loadMonth();
    } catch (err) {
      if (statusEl) statusEl.textContent = "Failed to remove.";
    } finally {
      removeBtn.disabled = false;
    }
  }

  // Initial load — gate on vault status (startup runs before unlock, when
  // mood_list fails with VaultLocked) and refetch on vault:unlocked.
  invoke("vault_status", {}, { silent: true })
    .then((s) => {
      if (s.unlocked) loadMonth();
    })
    .catch(() => {});
  document.addEventListener("vault:unlocked", () => loadMonth());
}