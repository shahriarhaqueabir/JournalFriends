// Settings "Activity Log" card. The Runtime & Logs card moved to the System tab (spec 2.4).
import { invoke } from "../ipc.js";
import { getDiagnosticsSnapshot } from "../diagnostics.js";
import { escapeHtml } from "../sanitize.js";
import { showToast } from "../notifications.js";

const ACTIVITY_CAP = 400;

let initialized = false;
let paused = false;
let activityTimer = null;

function settingsTabActive() {
  const nav = document.querySelector('.nav-item[data-tab="settings"]');
  return nav ? nav.classList.contains("active") : false;
}

function fmtTime(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso || "";
  const pad = (n, w = 2) => String(n).padStart(w, "0");
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}.${pad(
    d.getMilliseconds(),
    3,
  )}`;
}

// ── Activity Log ──────────────────────────────────────

function statusBadge(status) {
  const cls = ["error", "warn", "ok", "info"].includes(status)
    ? status
    : "info";
  return `<span class="badge-status ${cls}">${escapeHtml(status)}</span>`;
}

function httpBadge(status) {
  const cls =
    status >= 500
      ? "error"
      : status >= 400
        ? "warn"
        : status >= 300
          ? "warn"
          : "ok";
  return `<span class="badge-status ${cls} badge-http">HTTP ${status}</span>`;
}

// Map one frontend ring record to a row.
function normalizeFrontRecord(rec) {
  const detail = rec.detail || {};
  const t = rec.type || "";

  if (t === "ipc:ok") {
    return {
      at: rec.at,
      source: "ipc",
      operation: detail.command || "invoke",
      statusText: "ok",
      statusClass: "ok",
      duration: detail.duration_ms,
      detail: formatArgs(detail.args),
      level: "info",
    };
  }
  if (t === "ipc:error") {
    return {
      at: rec.at,
      source: "ipc",
      operation: detail.command || "invoke",
      statusText: "error",
      statusClass: "error",
      duration: detail.duration_ms,
      detail: detail.error || "",
      level: "error",
    };
  }
  if (t === "app_event") {
    const type = detail.type || "";
    // Backend engine LogEvents dupe the engine_http_log poll — drop them here.
    if (type === "log_event" && detail.source === "engine") return null;
    if (type === "ai_token") return null;

    let statusText = "info";
    let statusClass = "info";
    let level = "info";
    let operation = type;
    let extra = "";

    if (type === "log_event") {
      operation = detail.message || "log";
      level = (detail.level || "info").toLowerCase();
      statusText = level;
      statusClass = level;
    } else if (/error$|failed$/.test(type)) {
      statusText = "error";
      statusClass = "error";
      level = "error";
      extra = detail.error || "";
    } else if (/^journal_analysis_completed$/.test(type)) {
      statusText = "ok";
      statusClass = "ok";
    } else if (/^journal_analysis_queued$/.test(type)) {
      statusText = "queued";
      statusClass = "warn";
    }

    const { type: _t, ...rest } = detail;
    const payloadHint = Object.keys(rest).length ? summarizePayload(rest) : "";
    return {
      at: rec.at,
      source: "event",
      operation,
      statusText,
      statusClass,
      duration: null,
      detail: extra || payloadHint || "",
      level,
    };
  }

  // Other app-level diagnostics (tab switches, system checks, etc.).
  return {
    at: rec.at,
    source: "app",
    operation: t,
    statusText: "info",
    statusClass: "info",
    duration: null,
    detail: summarizePayload(detail),
    level: "info",
  };
}

function formatArgs(args) {
  if (!Array.isArray(args) || !args.length) return "";
  return `args: ${args.map((a) => `${a.key}:${a.type}(${a.length})`).join(", ")}`;
}

function summarizePayload(obj) {
  try {
    const s = JSON.stringify(sanitizeForSummary(obj));
    return s.length > 160 ? `${s.slice(0, 160)}…` : s;
  } catch {
    return "";
  }
}

function sanitizeForSummary(obj) {
  // Never render content-bearing values in the table — reuse the shape-only rule.
  if (obj == null) return obj;
  if (typeof obj === "string") return `[text:${obj.length}]`;
  if (Array.isArray(obj)) return obj.map(sanitizeForSummary);
  if (typeof obj === "object") {
    return Object.fromEntries(
      Object.entries(obj).map(([k, v]) =>
        /(content|message|feedback|text|prompt|title|summary|task|fact|insight|result)/i.test(
          k,
        ) && typeof v === "string"
          ? [k, `[text:${v.length}]`]
          : [k, sanitizeForSummary(v)],
      ),
    );
  }
  return obj;
}

async function refreshActivityLog() {
  const tbody = document.getElementById("activity-log-body");
  if (!tbody) return;

  const rows = [];

  // Frontend ring (IPC + events), newest first.
  for (const rec of getDiagnosticsSnapshot().slice(-ACTIVITY_CAP).reverse()) {
    const row = normalizeFrontRecord(rec);
    if (row) rows.push(row);
  }

  // Backend engine-HTTP ring.
  try {
    const engine = await invoke("engine_http_log", {}, { silent: true });
    for (const r of (engine || []).slice(-200).reverse()) {
      rows.push({
        at: r.at,
        source: "engine",
        operation: `${r.method} ${r.path}`,
        statusText: r.error
          ? "error"
          : r.status != null
            ? `HTTP ${r.status}`
            : "err",
        statusClass:
          r.error || (r.status != null && r.status >= 500)
            ? "error"
            : r.status != null && r.status >= 400
              ? "warn"
              : r.status != null
                ? "ok"
                : "error",
        duration: r.durationMs,
        detail: r.error || `service: ${r.service}`,
        level:
          r.error || (r.status != null && r.status >= 500) ? "error" : "info",
      });
    }
  } catch {
    /* engine ring unavailable — keep frontend rows only */
  }

  rows.sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0));

  if (!rows.length) {
    tbody.innerHTML =
      '<tr class="activity-log-empty"><td colspan="6">Activity will appear here as you use the app.</td></tr>';
    return;
  }

  tbody.innerHTML = rows
    .slice(0, ACTIVITY_CAP)
    .map((row) => {
      const isErr = row.level === "error";
      const duration =
        row.duration != null
          ? `<span class="mono">${row.duration}ms</span>`
          : '<span class="mono">—</span>';
      const status =
        row.statusClass === "ok" && /^HTTP /.test(row.statusText)
          ? httpBadge(Number(row.statusText.slice(5)))
          : statusBadge(row.statusText);
      return (
        `<tr class="${isErr ? "log-row-error" : ""}">` +
        `<td class="mono col-time">${escapeHtml(fmtTime(row.at))}</td>` +
        `<td><span class="badge-source ${row.source}">${escapeHtml(row.source)}</span></td>` +
        `<td>${escapeHtml(row.operation)}</td>` +
        `<td>${status}</td>` +
        `<td class="col-dur">${duration}</td>` +
        `<td>${escapeHtml(row.detail) || "—"}</td>` +
        `</tr>`
      );
    })
    .join("");
}

async function clearActivityLog() {
  if (window.__ARNOLD_DIAGNOSTICS__?.clear)
    window.__ARNOLD_DIAGNOSTICS__.clear();
  try {
    await invoke("telemetry_clear", {}, { silent: true });
  } catch {
    /* backend ring may be unavailable */
  }
  await refreshActivityLog();
  showToast("Activity log cleared.", { variant: "success" });
}

async function copyActivityLog() {
  const rows = [];

  for (const rec of getDiagnosticsSnapshot().slice(-ACTIVITY_CAP)) {
    const row = normalizeFrontRecord(rec);
    if (row) rows.push(row);
  }
  try {
    const engine = await invoke("engine_http_log", {}, { silent: true });
    for (const r of engine || []) rows.push(r);
  } catch {
    /* ignore */
  }
  rows.sort((a, b) => (a.at < b.at ? 1 : a.at > b.at ? -1 : 0));

  const text = JSON.stringify(rows, null, 2);
  try {
    await navigator.clipboard.writeText(text);
    showToast("Activity log copied to clipboard.", { variant: "success" });
  } catch {
    showToast("Could not copy — clipboard unavailable.", { variant: "warn" });
  }
}

// ── Init ──────────────────────────────────────────────

export async function initSettingsDiagnostics() {
  if (initialized) return;
  initialized = true;

  const btnPause = document.getElementById("btn-activity-pause");
  const btnClear = document.getElementById("btn-activity-clear");
  const btnCopy = document.getElementById("btn-activity-copy");

  if (btnPause) {
    btnPause.addEventListener("click", () => {
      paused = !paused;
      btnPause.textContent = paused ? "Resume" : "Pause";
      if (!paused) refreshActivityLog();
    });
  }
  btnClear?.addEventListener("click", clearActivityLog);
  btnCopy?.addEventListener("click", copyActivityLog);

  await refreshActivityLog();
  activityTimer = setInterval(() => {
    if (!paused && settingsTabActive()) refreshActivityLog();
  }, 700);

  // Hook so opening the Settings tab forces an immediate refresh.
  window.refreshSettingsDiagnostics = async () => {
    if (!paused) await refreshActivityLog();
  };
}

export function disposeSettingsDiagnostics() {
  if (activityTimer) clearInterval(activityTimer);
  activityTimer = null;
  initialized = false;
}
