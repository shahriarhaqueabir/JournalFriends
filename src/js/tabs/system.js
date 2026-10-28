// System tab — runtime grid + log viewer (moved out of Settings, spec 2.4).
// Reads snake_case AppDiagnostics keys (D-148): the old Settings copy read
// camelCase keys and silently rendered 8 dead "—" cells.
// Task 8 appends engines / analysis queue / DB / SQL controls below.
import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";
import { showToast, stringifyError } from "../notifications.js";
import { recordDiagnostic } from "../diagnostics.js";

const RUNTIME_MS = 3000;
const TAIL_MS = 2000;
const ENGINE_MS = 10000;
const LOG_TAIL_LINES = 200;
const LEVEL_ORDER = ["DEBUG", "INFO", "WARN", "ERROR"];

let initialized = false;
let runtimeTimer = null;
let tailTimer = null;
let engineTimer = null;
let healthBusy = false;
let lastDataDir = "";
let logLevel = "INFO";

function systemTabActive() {
  const nav = document.querySelector('.nav-item[data-tab="system"]');
  return nav ? nav.classList.contains("active") : false;
}

function fmtTime(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso || "";
  const pad = (n, w = 2) => String(n).padStart(w, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(d.getMilliseconds(), 3)}`;
}

function fmtBytes(n) {
  if (n == null || n === 0) return "—";
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(2)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

function setCell(id, text) {
  const el = document.getElementById(id);
  if (!el) return;
  el.textContent = text ?? "—";
  if (el.tagName === "CODE") el.title = text ?? "";
}

async function refreshRuntime() {
  try {
    const d = await invoke("app_diagnostics", {}, { silent: true });
    lastDataDir = d.data_dir || lastDataDir;
    setCell("rt-data-dir", d.data_dir);
    setCell("rt-db-path", d.db_path);
    setCell("rt-db-size", fmtBytes(d.db_size_bytes));
    setCell("rt-log-path", d.log_path);
    setCell("rt-journal", d.journal_entries);
    setCell("rt-tasks", d.tasks);
    setCell("rt-facts", d.memory_facts);
    setCell("rt-convos", d.conversations);
    setCell("rt-queue", d.analysis_queue_len);

    const counts = d.analysis_statuses || {};
    setCell(
      "rt-statuses",
      `queued ${counts.queued ?? 0} · processing ${counts.processing ?? 0} · done ${counts.done ?? 0} · failed ${counts.failed ?? 0}`,
    );
    const models = Array.isArray(d.models) ? d.models : [];
    setCell(
      "rt-models",
      models.length
        ? models.map((m) => `${m.kind}: ${m.name} (${fmtBytes(m.size_bytes)})`).join(" · ")
        : "no models imported",
    );

    const st = (s) => `${s.status || "unknown"}${s.pid ? ` (pid ${s.pid})` : ""}`;
    setCell("rt-chat", st(d.engine?.chat || {}));
    setCell("rt-embed", st(d.engine?.embed || {}));

    const chatLat = d.engine?.chat?.latency_ms;
    const embedLat = d.engine?.embed?.latency_ms;
    const checked = d.engine?.last_health_check;
    setCell(
      "rt-health",
      checked
        ? `${chatLat != null ? chatLat + "ms" : "?"} / ${embedLat != null ? embedLat + "ms" : "?"} @ ${fmtTime(checked)}`
        : "never checked",
    );
  } catch (e) {
    console.warn("[SYSTEM] runtime refresh failed", e);
  }
}

function stripAnsi(line) {
  return line.replace(/\x1b\[[0-9;]*m/g, "");
}

function lineLevel(line) {
  const m = stripAnsi(line).match(/\b(DEBUG|INFO|WARN|ERROR)\b/);
  return m ? m[1] : "INFO";
}

async function refreshLogTail() {
  const pre = document.getElementById("log-tail");
  if (!pre) return;
  const filter = logLevel;
  const minIdx = LEVEL_ORDER.indexOf(filter);
  if (minIdx < 0) return;

  try {
    const resp = await invoke("log_tail", { lines: LOG_TAIL_LINES }, { silent: true });
    const lines = resp?.lines || [];
    if (!lines.length) {
      pre.innerHTML = '<span class="log-tail-placeholder">Log file not available yet.</span>';
      return;
    }
    const from = LOG_TAIL_LINES - Math.min(LOG_TAIL_LINES, lines.length);
    const filtered = lines
      .map((line, i) => ({ line, level: lineLevel(line), n: from + i + 1 }))
      .filter((x) => LEVEL_ORDER.indexOf(x.level) >= minIdx);

    pre.innerHTML = filtered
      .map(
        (x) =>
          `<span class="log-line-${x.level.toLowerCase()}">${x.n}. ${escapeHtml(stripAnsi(x.line))}</span>` + "\n",
      )
      .join("");
  } catch {
    pre.innerHTML = '<span class="log-tail-placeholder">Log file not available yet.</span>';
  }
}

function wireLogToolbar() {
  document.getElementById("log-tail-levels")?.addEventListener("click", (ev) => {
    const btn = ev.target.closest("button[data-level]");
    if (!btn) return;
    logLevel = btn.dataset.level || "INFO";
    document.querySelectorAll("#log-tail-levels button[data-level]").forEach((b) => {
      const on = b === btn;
      b.classList.toggle("active", on);
      b.setAttribute("aria-pressed", String(on));
    });
    refreshLogTail();
  });
  document.getElementById("btn-open-log")?.addEventListener("click", async () => {
    try {
      const d = await invoke("app_diagnostics", {}, { silent: true });
      // Wire key is snake_case (D-148) — the old Settings code read d.logPath.
      if (window.__TAURI__?.shell?.open && d.log_path) {
        await window.__TAURI__.shell.open(d.log_path);
      } else {
        showToast("Cannot open log file here.", { variant: "warn" });
      }
    } catch (e) {
      console.warn("[SYSTEM] open log failed", e);
    }
  });
}

function wireCopyButtons() {
  document.querySelector(".system-page")?.addEventListener("click", async (ev) => {
    const btn = ev.target.closest("[data-copy-target]");
    if (!btn) return;
    const target = document.getElementById(btn.getAttribute("data-copy-target"));
    const text = target?.textContent?.trim() || "";
    if (!text || text === "—") return;
    try {
      await navigator.clipboard.writeText(text);
      showToast("Path copied.", { variant: "success" });
    } catch {
      showToast("Could not copy the path.", { variant: "warn" });
    }
  });
}

// ── Engines ─────────────────────────────────────

function setStatus(id, text, cls) {
  const el = document.getElementById(id);
  if (!el) return;
  el.textContent = text || "";
  el.className = cls ? `settings-status ${cls}` : "settings-status";
}

function paintEngine(service, status) {
  const state = document.querySelector(`[data-engine-state="${service}"]`);
  if (state) state.textContent = status || "—";
  const dot = document.querySelector(`[data-engine-dot="${service}"]`);
  if (!dot) return;
  dot.className = "status-dot";
  if (status === "running") dot.classList.add("ok");
  else if (status === "starting") dot.classList.add("warn");
  else if (status && status.startsWith("failed")) dot.classList.add("error");
}

function paintHealth(service, ok, latencyMs) {
  const el = document.querySelector(`[data-engine-latency="${service}"]`);
  if (!el) return;
  el.textContent = ok ? (latencyMs != null ? `${latencyMs} ms` : "ok") : "unreachable";
}

async function refreshEngines() {
  try {
    const s = await invoke("get_engine_status", {}, { silent: true });
    paintEngine("chat", s.chat);
    paintEngine("embed", s.embed);
  } catch (e) {
    console.warn("[SYSTEM] engine status failed", e);
  }
}

async function checkHealthNow() {
  if (healthBusy) return; // never overlap the HTTP probes (10 s timer + button)
  healthBusy = true;
  try {
    const h = await invoke("check_engine_health", {}, { silent: true });
    paintHealth("chat", h.chat, h.chat_latency_ms);
    paintHealth("embed", h.embed, h.embed_latency_ms);
  } catch (e) {
    console.warn("[SYSTEM] engine health failed", e);
  } finally {
    healthBusy = false;
  }
}

function wireEngineControls() {
  document.getElementById("btn-engine-start")?.addEventListener("click", async () => {
    try {
      recordDiagnostic("system:engine_start", {});
      const summary = await invoke("provision_model_cmd");
      setStatus("engine-start-status", summary, "success");
      await refreshEngines();
      await checkHealthNow();
    } catch (e) {
      setStatus("engine-start-status", stringifyError(e), "error");
    }
  });

  document.getElementById("engine-controls")?.addEventListener("click", async (ev) => {
    const btn = ev.target.closest("[data-engine-action]");
    if (!btn) return;
    const action = btn.dataset.engineAction;
    const service = btn.dataset.service;
    if (action === "check") {
      await checkHealthNow();
      return;
    }
    btn.disabled = true;
    try {
      recordDiagnostic("system:engine_action", { action, service });
      if (action === "restart") await invoke("restart_engine", { service });
      else if (action === "stop") await invoke("stop_engine", { service });
      await refreshEngines();
      await checkHealthNow();
    } catch {
      /* ipc.js already toasted the error */
    } finally {
      btn.disabled = false;
    }
  });
}

// ── Analysis queue ──────────────────────────────

function queueBadge(status) {
  const cls =
    status === "failed" ? "badge-fail" : status === "done" ? "badge-pass" : status === "processing" ? "badge-info" : "badge-warn";
  return `<span class="badge ${cls}">${escapeHtml(status)}</span>`;
}

async function refreshQueue() {
  const tbody = document.getElementById("queue-body");
  if (!tbody) return;
  try {
    const rows = await invoke("analysis_queue_list", {}, { silent: true });
    const count = document.getElementById("queue-count");
    if (count) count.textContent = rows.length ? `${rows.length} row(s)` : "";
    if (!rows.length) {
      tbody.innerHTML = '<tr class="table-empty"><td colspan="6">Queue is empty.</td></tr>';
      return;
    }
    tbody.innerHTML = rows
      .map((r) => {
        const retry =
          r.status === "failed"
            ? `<button class="btn-ghost-sm" data-requeue="${escapeHtml(r.entry_id)}">Retry</button>`
            : "";
        return (
          "<tr>" +
          `<td class="mono" title="${escapeHtml(r.entry_id)}">${escapeHtml(String(r.entry_id).slice(0, 8))}</td>` +
          `<td>${queueBadge(r.status)}</td>` +
          `<td class="mono col-time">${escapeHtml(fmtTime(r.queued_at))}</td>` +
          `<td class="mono col-time">${escapeHtml(fmtTime(r.updated_at))}</td>` +
          `<td class="mono col-dur">${Number(r.attempts) || 0}</td>` +
          `<td>${retry}</td>` +
          "</tr>"
        );
      })
      .join("");
  } catch (e) {
    console.warn("[SYSTEM] queue refresh failed", e);
  }
}

function wireQueueControls() {
  document.getElementById("btn-queue-refresh")?.addEventListener("click", refreshQueue);
  document.getElementById("queue-body")?.addEventListener("click", async (ev) => {
    const btn = ev.target.closest("button[data-requeue]");
    if (!btn) return;
    const entryId = btn.getAttribute("data-requeue");
    btn.disabled = true;
    try {
      recordDiagnostic("system:queue_requeue", { entryId });
      await invoke("analysis_requeue", { entryId });
      showToast("Entry re-queued for analysis.", { variant: "success" });
      await refreshQueue();
      refreshRuntime(); // queue count + statuses cells move with it
    } catch {
      /* ipc.js already toasted the error */
    } finally {
      btn.disabled = false;
    }
  });
}

// ── Database ────────────────────────────────────

function wireDbControls() {
  document.getElementById("btn-backup-now")?.addEventListener("click", async () => {
    try {
      recordDiagnostic("system:backup_now", {});
      const path = await invoke("backup_now");
      setStatus("db-action-status", `Backup written: ${path}`, "success");
      showToast("Backup complete.", { variant: "success" });
    } catch (e) {
      setStatus("db-action-status", stringifyError(e), "error");
    }
  });

  document.getElementById("btn-open-data-dir")?.addEventListener("click", async () => {
    try {
      if (!lastDataDir) {
        const d = await invoke("app_diagnostics", {}, { silent: true });
        lastDataDir = d.data_dir || "";
      }
      if (window.__TAURI__?.shell?.open && lastDataDir) {
        await window.__TAURI__.shell.open(lastDataDir);
      } else {
        showToast("Cannot open the data directory here.", { variant: "warn" });
      }
    } catch (e) {
      console.warn("[SYSTEM] open data dir failed", e);
    }
  });
}

// ── SQL runner (read-only, D-147) ───────────────

function cellText(v) {
  if (v === null || v === undefined) return "NULL";
  if (typeof v === "object") {
    try {
      return JSON.stringify(v);
    } catch {
      return String(v);
    }
  }
  return String(v);
}

function renderSqlResult(res) {
  const wrap = document.getElementById("sys-sql-result");
  const meta = document.getElementById("sys-sql-meta");
  const thead = document.getElementById("sys-sql-thead");
  const tbody = document.getElementById("sys-sql-body");
  if (!wrap) return;
  const rows = res.rows || [];
  const truncated = rows.length >= 500 ? " (first 500)" : "";
  if (meta) meta.textContent = `${rows.length} row(s)${truncated} · read-only`;
  const cols = res.columns || [];
  if (thead) thead.innerHTML = `<tr>${cols.map((c) => `<th>${escapeHtml(String(c))}</th>`).join("")}</tr>`;
  if (tbody) {
    tbody.innerHTML = rows
      .map(
        (row) =>
          `<tr>${row.map((cell) => `<td class="mono">${escapeHtml(cellText(cell))}</td>`).join("")}</tr>`,
      )
      .join("");
  }
  wrap.classList.remove("hidden");
}

async function runSql() {
  const input = document.getElementById("sys-sql-input");
  const query = input?.value ?? "";
  const result = document.getElementById("sys-sql-result");
  if (!query.trim()) {
    setStatus("sys-sql-status", "Enter a statement first.", "error");
    return;
  }
  try {
    recordDiagnostic("system:sql_run", { chars: query.length });
    const res = await invoke("sql_run", { query });
    renderSqlResult(res);
    setStatus("sys-sql-status", `${(res.rows || []).length} row(s) · read-only`, "success");
  } catch (e) {
    result?.classList.add("hidden");
    setStatus("sys-sql-status", stringifyError(e), "error");
  }
}

function wireSqlControls() {
  document.getElementById("btn-sql-run")?.addEventListener("click", runSql);
  document.getElementById("sys-sql-input")?.addEventListener("keydown", (ev) => {
    if (ev.key === "Enter" && (ev.ctrlKey || ev.metaKey)) runSql();
  });
}

export function initSystem() {
  if (initialized) return;
  initialized = true;

  wireLogToolbar();
  wireCopyButtons();
  wireEngineControls();
  wireQueueControls();
  wireDbControls();
  wireSqlControls();

  refreshRuntime();
  refreshLogTail();
  refreshEngines();
  checkHealthNow();
  refreshQueue();

  runtimeTimer = setInterval(() => {
    if (systemTabActive()) refreshRuntime();
  }, RUNTIME_MS);
  tailTimer = setInterval(() => {
    if (systemTabActive()) refreshLogTail();
  }, TAIL_MS);
  engineTimer = setInterval(() => {
    if (systemTabActive()) {
      refreshEngines();
      checkHealthNow();
    }
  }, ENGINE_MS);

  window.refreshSystemTab = () => {
    refreshRuntime();
    refreshLogTail();
    refreshEngines();
    checkHealthNow();
    refreshQueue();
  };
}
