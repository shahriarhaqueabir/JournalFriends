import { initJournal } from "./tabs/journal.js";
import { initTasks } from "./tabs/tasks.js";
import { initMood } from "./tabs/mood.js";
import { initInsights, refreshInsights } from "./tabs/insights.js";
import { initEvents } from "./tabs/events.js";
import { initSettings, applyBackground } from "./tabs/settings.js";
import { initInbox, refreshInbox } from "./tabs/inbox.js";
import { initReports } from "./tabs/reports.js";
import { initAiSidebar, handleAppEvent } from "./ai-sidebar.js";
import { initOnboarding, showOnboarding } from "./onboarding.js";
import { initVault } from "./vault.js";
import { invoke } from "./ipc.js";
// PureDashboard (vendored) — registers <puredashboard-*> custom elements.
import "/assets/libs/puredashboard/reactive.js";
import "/assets/libs/puredashboard/card.js";
import "/assets/libs/puredashboard/statistic.js";
import "/assets/libs/puredashboard/badge.js";
import "/assets/libs/puredashboard/button.js";
import { recordDiagnostic } from "./diagnostics.js";

// SINGLE typed event listener (D-96)
// Guarded: when the page runs outside the Tauri shell (plain browser, e.g.
// markup verification), __TAURI__ is absent and there is no event channel.
if (globalThis.__TAURI__?.event?.listen) {
  globalThis.__TAURI__.event.listen("app_event", (event) => {
    const payload = event.payload;
    recordDiagnostic("app_event", payload);
    // First-run onboarding (P-04) — dispatched here, not ai-sidebar, so the
    // wizard is never gated behind sidebar element readiness.
    if (payload?.type === "onboarding_required") {
      showOnboarding();
      return;
    }
    // Dispatch to relevant modules
    handleAppEvent(payload);
  });
}

// Assistant sidebar collapse (D-149) — per-tab remembered state.
const ASSISTANT_KEY = (tab) => `ui.assistant.collapsed.${tab}`;
const assistantToggle = document.getElementById("btn-toggle-assistant");
const assistantRailToggle = document.getElementById("btn-toggle-assistant-rail");

function activeTab() {
  return document.querySelector(".nav-item.active")?.dataset.tab || "journal";
}

function setAssistantCollapsed(collapsed, persist = true) {
  document.body.classList.toggle("assistant-collapsed", collapsed);
  assistantToggle?.setAttribute("aria-expanded", String(!collapsed));
  if (persist) {
    try {
      localStorage.setItem(ASSISTANT_KEY(activeTab()), collapsed ? "1" : "0");
    } catch (e) {
      console.warn("assistant state persist failed", e);
    }
  }
}

function restoreAssistantState(tab) {
  let saved = null;
  try {
    saved = localStorage.getItem(ASSISTANT_KEY(tab));
  } catch (e) {
    console.warn("assistant state restore failed", e);
  }
  setAssistantCollapsed(saved === "1", false);
}

assistantToggle?.addEventListener("click", () => setAssistantCollapsed(true));
assistantRailToggle?.addEventListener("click", () => setAssistantCollapsed(false));

// Left sidebar collapse — mirror of assistant collapse (icon-only rail).
const LEFT_KEY = "ui.left.collapsed";
const leftToggle = document.getElementById("btn-toggle-left");
function setLeftCollapsed(collapsed, persist = true) {
  document.body.classList.toggle("left-collapsed", collapsed);
  leftToggle?.setAttribute("aria-expanded", String(!collapsed));
  leftToggle?.setAttribute("aria-label", collapsed ? "Expand sidebar" : "Collapse sidebar");
  leftToggle?.setAttribute("title", collapsed ? "Expand sidebar" : "Collapse sidebar");
  const icon = leftToggle?.querySelector("hj-icon");
  if (icon) icon.setAttribute("name", collapsed ? "chevron-right" : "chevron-left");
  if (persist) {
    try { localStorage.setItem(LEFT_KEY, collapsed ? "1" : "0"); } catch (e) { console.warn("left state persist failed", e); }
  }
}
let leftSaved = null;
try { leftSaved = localStorage.getItem(LEFT_KEY); } catch (e) {}
setLeftCollapsed(leftSaved === "1", false);
leftToggle?.addEventListener("click", () => {
  const now = document.body.classList.contains("left-collapsed");
  setLeftCollapsed(!now);
});

// Tab switching
document.querySelectorAll(".nav-item").forEach((item) => {
  item.addEventListener("click", () => {
    const tab = item.dataset.tab;
    recordDiagnostic("tab:switch", { tab });
    const nextView = document.getElementById(`tab-${tab}`);
    if (!nextView) {
      console.warn(`Tab view not found: tab-${tab}`);
      return;
    }

    document
      .querySelectorAll(".nav-item")
      .forEach((n) => {
        n.classList.remove("active");
        n.setAttribute("aria-current", "false");
      });
    item.classList.add("active");
    item.setAttribute("aria-current", "page");
    restoreAssistantState(tab);
    document
      .querySelectorAll(".tab-view")
      .forEach((v) => v.classList.remove("active"));
    nextView.classList.add("active");

    if (tab === "settings") {
      if (window.refreshAiSettings) window.refreshAiSettings();
      if (window.refreshSettingsDiagnostics) window.refreshSettingsDiagnostics();
      if (!window._settingsInitialized) {
        import("./tabs/settings.js").then(m => m.initSettings());
        window._settingsInitialized = true;
      }
    }

    if (tab === "system") {
      // The view is lazy (empty placeholder until first click) — unlike the
      // inline Settings markup, initSystem must wait for loadView to land DOM.
      loadView("system", "views/system.html").then(() => {
        if (!document.getElementById("tab-system")?.dataset.viewLoaded) return; // fetch failed — retry next click
        if (!window._systemInitialized) {
          window._systemInitialized = true;
          import("./tabs/system.js").then((m) => m.initSystem());
        } else if (window.refreshSystemTab) {
          window.refreshSystemTab();
        }
      });
    }

    if (tab === "insights") {
      try { refreshInsights(); } catch (e) { console.warn("refreshInsights failed", e); }
    }
  });
});

// App version — runtime-derived from Cargo.toml via app_version command
// (single source of truth, P-07). Static fallback text stays in index.html.
async function setAppVersion() {
  const versionEl = document.getElementById("app-version");
  if (!versionEl) return;
  try {
    const v = await invoke("app_version", {}, { silent: true });
    versionEl.textContent = `v${v}`;
  } catch (e) {
    // keep the static fallback text
  }
}

// LLM engine health check — updates the status dot + value in the sidebar footer.
async function checkEngine() {
  const dotEl = document.getElementById("engine-dot");
  const valueEl = document.getElementById("engine-status-value");
  const setStatus = (dotClass, text) => {
    if (dotEl) dotEl.className = `status-dot ${dotClass}`;
    if (valueEl) valueEl.textContent = text;
  };
  try {
    const { chat, embed } = await invoke("check_engine_health", {}, { silent: true });
    if (chat && embed) {
      setStatus("ok", "AI Engine online");
    } else {
      setStatus("error", "AI Engine offline");
    }
  } catch (e) {
    setStatus("error", "AI Engine offline");
  }
}
window.checkEngine = checkEngine;

// Render lucide icons from the vendored build (CSP-safe, local asset).
// v1.14.0 requires an explicit icons object.
function renderIcons() {
  if (window.lucide && typeof lucide.createIcons === "function") {
    try {
      lucide.createIcons({ icons: lucide.icons });
    } catch (e) {
      console.warn("lucide icon render failed", e);
    }
  }
}

window.renderIcons = renderIcons;

// Partial Loading Helper
async function loadView(tabId, filePath) {
  const container = document.getElementById(`tab-${tabId}`);
  if (container.dataset.viewLoaded) return false;
  try {
    const response = await fetch(filePath);
    const html = await response.text();
    container.innerHTML = html;
    container.dataset.viewLoaded = "1";
    renderIcons(); // re-render lucide icons for the injected view
    return true;
  } catch (e) {
    console.error(`Failed to load view: ${filePath}`, e);
  }
  return false;
}

async function checkVaultStatus() {
  try {
    const status = await invoke("vault_status", {}, { silent: true });
    if (!status.unlocked) {
      if (window.showVault) window.showVault();
    } else {
      if (window.hideVault) window.hideVault();
      // If unlocked, check if onboarding is still needed (T-07)
      await invoke("check_onboarding", {}, { silent: true });
    }
  } catch (err) {
    console.error("Failed to check vault status", err);
  }
}

// Initializations
initJournal();
initTasks();
initMood();
initInsights();
initEvents();
initInbox();
initReports();
initAiSidebar();
initSettings();
applyBackground();
initOnboarding();
initVault();
restoreAssistantState("journal");
renderIcons();
checkVaultStatus();
checkEngine();
setAppVersion();
setInterval(checkEngine, 30000);

// Load and Init Memory tab when clicked
document.querySelector('[data-tab="memory"]')?.addEventListener('click', async () => {
  const loaded = await loadView('memory', 'views/memory.html');
  if (loaded && !window._memoryInitialized) {
    window._memoryInitialized = true;
    const m = await import('./tabs/memory.js');
    m.initMemory();
  }
});

// Load Inbox view when clicked
document.querySelector('[data-tab="inbox"]')?.addEventListener('click', async () => {
  await loadView('inbox', 'views/inbox.html');
  initInbox();
});

// Load Reports view when clicked
document.querySelector('[data-tab="reports"]')?.addEventListener('click', async () => {
  await loadView('reports', 'views/reports.html');
  initReports();
});

// Load About view when clicked
document.querySelector('[data-tab="about"]')?.addEventListener('click', async () => {
  await loadView('about', 'views/about.html');
});

// Refresh events on tab switch
document.querySelector('[data-tab="events"]')?.addEventListener('click', () => {
  initEvents();
});

// Load and init AI Control Center when clicked
document.querySelector('[data-tab="ai"]')?.addEventListener('click', async () => {
  if (!window._aiControlsInitialized) {
    const m = await import('./tabs/ai-controls.js');
    m.initAiControls();
  }
});

