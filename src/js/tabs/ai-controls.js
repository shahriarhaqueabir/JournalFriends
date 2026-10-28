import { invoke } from "../ipc.js";
import {
  personaQuestions,
  renderPersonaForm,
  collectPersonaAnswers,
  savePersonaAnswers,
} from "../persona.js";

const PREFIX = "ai-persona";

export function initAiControls() {
  if (window._aiControlsInitialized) return;
  window._aiControlsInitialized = true;

  renderPersonaForm(document.getElementById("ai-persona-form"), PREFIX);
  loadIdentity().catch(() => {});
  wireSave();
}

async function loadIdentity() {
  const [voice, items] = await Promise.all([
    invoke("setting_get", { key: "ai_voice" }, { silent: true }),
    invoke("memory_list", { kind: null, category: null, search: null }, { silent: true }),
  ]);

  const voiceEl = document.getElementById("ai-voice-input");
  if (voiceEl && !voiceEl.value)
    voiceEl.value = (voice && typeof voice === "string" ? voice : "") || "";

  const byTitle = new Map(
    (items || [])
      .filter((i) => i.created_by === "onboarding")
      .map((i) => [i.title, i.content])
  );
  for (const q of personaQuestions()) {
    const el = document.getElementById(`${PREFIX}-${q.key}`);
    if (el && !el.value && byTitle.has(q.title)) el.value = byTitle.get(q.title) || "";
  }
}

function wireSave() {
  const saveBtn = document.getElementById("btn-save-ai-identity");
  const statusEl = document.getElementById("ai-identity-status");
  const voiceEl = document.getElementById("ai-voice-input");
  if (!saveBtn || !statusEl || !voiceEl) return;

  saveBtn.addEventListener("click", async () => {
    saveBtn.disabled = true;
    statusEl.textContent = "Saving…";
    try {
      const answers = collectPersonaAnswers(PREFIX);
      const res = await savePersonaAnswers(answers);
      await invoke("setting_set", {
        key: "ai_voice",
        value: voiceEl.value.trim(),
      });
      statusEl.textContent = `Saved — ${res.saved} new, ${res.updated} updated persona answers. Voice applies on your next message.`;
    } catch (e) {
      console.error("ai controls save failed", e);
      statusEl.textContent = "Save failed — see console.";
    } finally {
      saveBtn.disabled = false;
    }
  });
}