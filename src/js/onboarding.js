// First-run onboarding wizard (P-04).
// Shows when the backend emits `onboarding_required` (onboarding_complete setting
// unset). Runs AFTER the vault is unlocked (settings writes require the DB).
// Steps: Welcome → Done.

import { invoke } from "./ipc.js";
import { recordDiagnostic } from "./diagnostics.js";
import { requirementRows, requirementsTableHtml } from "./requirements.js";

const TOTAL_STEPS = 2; // panes 0..1
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
    } else {
      nextBtn.textContent = "Done";
    }
  }

  if (currentStep === 0) refreshRequirements();
  window.renderIcons?.();
  recordDiagnostic("onboarding:step", { step: currentStep });
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

// --- Wiring ---
function wire() {
  el("btn-onboarding-next")?.addEventListener("click", async () => {
    if (currentStep === TOTAL_STEPS - 1) {
      await completeOnboarding(false);
      return;
    }
    goTo(currentStep + 1);
  });
  el("btn-onboarding-prev")?.addEventListener("click", () =>
    goTo(currentStep - 1),
  );
  el("btn-onboarding-skip")?.addEventListener("click", () =>
    completeOnboarding(true),
  );
}

export function initOnboarding() {
  wire();
}
