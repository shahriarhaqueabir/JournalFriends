// First-run onboarding wizard (P-04).
// Shows when the backend emits `onboarding_required` (onboarding_complete setting
// unset). Runs AFTER the vault is unlocked (settings writes require the DB).
// Steps: Welcome → Engine path → chat GGUF → embed GGUF → System check → Done.

import { invoke } from "./ipc.js";
import { recordDiagnostic } from "./diagnostics.js";
import { stringifyError } from "./notifications.js";
import { requirementRows, requirementsTableHtml } from "./requirements.js";
import {
  renderPersonaForm,
  collectPersonaAnswers,
  savePersonaAnswers,
} from "./persona.js";

const TOTAL_STEPS = 7; // panes 0..6
let currentStep = 0;

function el(id) {
  return document.getElementById(id);
}

function showWizard() {
  const overlay = el("onboarding-overlay");
  if (!overlay) return;
  overlay.classList.remove("hidden");
  window.renderIcons?.();
  recordDiagnostic("onboarding:show", { step: currentStep });
}

function hideWizard() {
  const overlay = el("onboarding-overlay");
  if (overlay) overlay.classList.add("hidden");
}

export function showOnboarding() {
  goTo(0);
  showWizard();
}

export function isOnboardingOpen() {
  return !el("onboarding-overlay")?.classList.contains("hidden");
}

// Same table System shows (single source of truth in requirements.js).
async function refreshRequirements() {
  const container = document.getElementById("onboarding-requirements");
  if (!container) return;
  try {
    const report = await invoke("engine_requirements", {}, { silent: true });
    container.innerHTML = requirementsTableHtml(
      requirementRows({
        engineLabel: report.engine,
        devices: report.devices,
        msvcRuntime: report.msvc_runtime,
        chatModel: report.chat_model,
        embedModel: report.embed_model,
      }),
    );
  } catch (e) {
    console.error("[ONBOARDING] engine_requirements failed", e);
    container.textContent = "Requirements unavailable.";
  }
}

function goTo(step) {
  currentStep = Math.max(0, Math.min(TOTAL_STEPS - 1, step));

  document.querySelectorAll(".onboarding-pane").forEach((pane) => {
    pane.classList.toggle("active", Number(pane.dataset.pane) === currentStep);
  });
  document.querySelectorAll(".onboarding-step").forEach((stepEl) => {
    stepEl.classList.toggle(
      "active",
      Number(stepEl.dataset.step) === currentStep,
    );
  });

  const prevBtn = el("btn-onboarding-prev");
  const nextBtn = el("btn-onboarding-next");
  const skipBtn = el("btn-onboarding-skip");

  if (prevBtn) prevBtn.disabled = currentStep === 0;
  if (skipBtn)
    skipBtn.style.display = currentStep === TOTAL_STEPS - 1 ? "none" : "";
  if (nextBtn) {
    if (currentStep === 0) {
      nextBtn.textContent = "Begin";
      nextBtn.disabled = false;
    } else if (currentStep === TOTAL_STEPS - 1) {
      nextBtn.textContent = "Done";
    } else {
      nextBtn.textContent = "Continue";
    }
  }

  if (currentStep === 0) refreshRequirements();
  if (currentStep === 1) refreshEnginePath();
  if (currentStep === 5) runEngineCheck();
  window.renderIcons?.();
  recordDiagnostic("onboarding:step", { step: currentStep });
}

// --- File picking via tauri-plugin-dialog (Tauri v2 removed file.path) ---
async function pickFile(filters) {
  const dialog = window?.__TAURI__?.dialog;
  if (!dialog || typeof dialog.open !== "function") {
    return null;
  }
  try {
    return await dialog.open({ multiple: false, filters });
  } catch (e) {
    console.error("File dialog failed", e);
    return null;
  }
}

function ggufFilter() {
  return [{ name: "GGUF", extensions: ["gguf"] }];
}

// --- Step 1: engine executable path ---
let exePathLoaded = false;

/** Report the executable the backend actually launches (bundled by default). */
async function refreshEnginePath() {
  const input = el("onboard-exe-path");
  const hint = el("onboard-exe-hint");
  if (!input) return;
  try {
    const info = await invoke("engine_path_info", {}, { silent: true });
    exePathLoaded = true;
    input.value = info.saved || "";
    input.placeholder = info.resolved || "No llama-server.exe found";
    if (!hint) return;
    const label =
      info.source === "app_local"
        ? "Bundled runtime (app data)"
        : info.source === "bundled_resource"
          ? "Bundled runtime"
          : info.source === "saved"
            ? "Saved executable"
            : null;
    if (!label || !info.resolved) {
      hint.textContent =
        "No llama-server.exe found. Restore the bundled engines folder or select your own llama.cpp build.";
    } else {
      let text = `${label} detected: ${info.resolved}`;
      if (info.source === "saved") {
        text += " — clear the field to go back to the bundled runtime.";
      } else if (!info.saved) {
        text += " — no action needed, just continue.";
      } else {
        text +=
          " — your saved path is ignored while the bundled runtime is present.";
      }
      hint.textContent = text;
    }
    hint.title = info.resolved || "";
  } catch (e) {
    if (hint) hint.textContent = "Could not read the engine path.";
    console.error("engine_path_info failed", e);
  }
}

async function saveExePath(path) {
  const input = el("onboard-exe-path");
  const value = (path || "").trim();
  if (input) input.value = value;
  // Never clear a saved override we could not read back (vault still locked).
  if (!value && !exePathLoaded) return;
  await invoke("setting_set", { key: "llama_exe_path", value });
  recordDiagnostic("onboarding:exe-path", { set: Boolean(value) });
  await refreshEnginePath();
}

// --- Step 2/3: model import ---
async function importModel(modelType, resultElId, btnId) {
  const btn = el(btnId);
  const resultEl = el(resultElId);
  if (!btn || !resultEl) return;

  const path = await pickFile(ggufFilter());
  if (!path) return;

  btn.disabled = true;
  resultEl.className = "onboarding-check-result";
  resultEl.innerHTML = '<span class="onboarding-spinner"></span> Importing…';

  try {
    const info = await invoke("import_model", { source: path, modelType });
    const key =
      modelType === "embed" ? "llama_embed_model" : "llama_chat_model";
    await invoke("setting_set", { key, value: info.filename });
    resultEl.className = "onboarding-check-result ok";
    resultEl.textContent = `Imported ${info.name}`;
    recordDiagnostic("onboarding:import", { modelType, ok: true });
  } catch (e) {
    resultEl.className = "onboarding-check-result fail";
    resultEl.textContent = `Import failed: ${stringifyError(e)}`;
    recordDiagnostic("onboarding:import", { modelType, ok: false });
  } finally {
    btn.disabled = false;
  }
}

// --- Step 4: engine check + system check ---
async function runEngineCheck() {
  const listEl = el("onboarding-check-list");
  if (!listEl) return;

  listEl.innerHTML =
    '<div class="onboarding-check-item"><span class="onboarding-spinner"></span> Running system checks…</div>';
  el("btn-rerun-checks")?.classList.add("hidden");

  try {
    // 1. Run preflight immediately so checks render instantly without waiting for engine boot/poll timeouts.
    const initialReport = await invoke("preflight_run", {}, { silent: true });
    renderChecks(listEl, initialReport);
    el("btn-rerun-checks")?.classList.remove("hidden");

    // 2. Trigger provisioning in background (starts chat/embed servers if configured) and refresh checks when ready.
    invoke("provision_model_cmd", {}, { silent: true })
      .then(async () => {
        const updatedReport = await invoke(
          "preflight_run",
          {},
          { silent: true },
        );
        renderChecks(listEl, updatedReport);
      })
      .catch(() => {});
  } catch (e) {
    listEl.innerHTML = `<div class="onboarding-check-item badge-fail">System check failed: ${escapeHtml(stringifyError(e))}</div>`;
    el("btn-rerun-checks")?.classList.remove("hidden");
  }
}

function renderChecks(listEl, report) {
  listEl.innerHTML = "";
  for (const check of report.checks || []) {
    const item = document.createElement("div");
    item.className = "onboarding-check-item";
    const iconName =
      check.status === "pass"
        ? "circle-check"
        : check.status === "warn"
          ? "circle-alert"
          : "circle-x";
    item.innerHTML = `<span><hj-icon name="${iconName}" size="16" aria-hidden="true"></hj-icon> ${escapeHtml(check.label)}</span>
      <span class="badge badge-${check.status}">${check.status}</span>`;
    if (check.message && check.status !== "pass") {
      const msg = document.createElement("div");
      msg.style.cssText =
        "width:100%;font-size:12px;color:var(--color-text-muted);margin-top:4px;";
      msg.textContent = check.message;
      item.appendChild(msg);
    }
    listEl.appendChild(item);
  }
  window.renderIcons?.();

  // Show data dir on the finish step (P-04)
  const dataDirEl = el("onboarding-data-dir");
  if (dataDirEl && report.app_version) {
    dataDirEl.textContent = "data/ (see Settings → About)";
  }
}

// --- Finish / skip ---
async function completeOnboarding(skipped = false) {
  try {
    await invoke("setting_set", {
      key: "onboarding_complete",
      value: new Date().toISOString(),
    });
    recordDiagnostic("onboarding:complete", { skipped });
  } catch (e) {
    console.warn("Failed to persist onboarding_complete", e);
  }
  hideWizard();
}

// --- Persona step (4) ---
async function persistPersona() {
  const answers = collectPersonaAnswers("onba-persona");
  await savePersonaAnswers(answers);
  recordDiagnostic("onboarding:persona", { answered: answers.length });
}

// --- Wiring ---
function wire() {
  renderPersonaForm(el("onboarding-persona-form"), "onba-persona");

  el("btn-onboarding-next")?.addEventListener("click", async () => {
    if (currentStep === 1) {
      // Persist the exe path before leaving the engine step.
      await saveExePath(el("onboard-exe-path")?.value || "");
    }
    if (currentStep === 4) {
      await persistPersona();
    }
    if (currentStep === TOTAL_STEPS - 1) {
      await completeOnboarding(false);
      return;
    }
    goTo(currentStep + 1);
  });
  el("btn-onboarding-skip-persona")?.addEventListener("click", () =>
    goTo(currentStep + 1),
  );
  el("btn-onboarding-prev")?.addEventListener("click", () =>
    goTo(currentStep - 1),
  );
  el("btn-onboarding-skip")?.addEventListener("click", () =>
    completeOnboarding(true),
  );
  el("btn-onboard-browse-exe")?.addEventListener("click", async () => {
    const path = await pickFile([{ name: "Executable", extensions: ["exe"] }]);
    if (path) await saveExePath(path);
  });
  el("btn-onboard-continue-engine")?.addEventListener("click", async () => {
    await saveExePath(el("onboard-exe-path")?.value || "");
    goTo(currentStep + 1);
  });
  el("btn-onboard-import-chat")?.addEventListener("click", () =>
    importModel(
      "chat",
      "onboarding-import-chat-result",
      "btn-onboard-import-chat",
    ),
  );
  el("btn-onboard-import-embed")?.addEventListener("click", () =>
    importModel(
      "embed",
      "onboarding-import-embed-result",
      "btn-onboard-import-embed",
    ),
  );
  el("btn-rerun-checks")?.addEventListener("click", runEngineCheck);
}

function escapeHtml(str) {
  return String(str ?? "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

export function initOnboarding() {
  wire();
}
