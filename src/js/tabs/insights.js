import { invoke } from "../ipc.js";
import { escapeHtml } from "../sanitize.js";

function calculateStreak(dates) {
  if (!dates || !dates.length) return 0;
  const uniqueDays = Array.from(new Set(dates.map((d) => d.slice(0, 10))))
    .sort()
    .reverse();
  const now = new Date();
  const today = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  const yesterday = new Date(now);
  yesterday.setDate(yesterday.getDate() - 1);
  const yest = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, "0")}-${String(yesterday.getDate()).padStart(2, "0")}`;

  if (uniqueDays[0] !== today && uniqueDays[0] !== yest) return 0;

  let streak = 0;
  let curr = new Date(uniqueDays[0]);
  for (const dStr of uniqueDays) {
    const d = new Date(dStr);
    const diff = Math.round((curr - d) / (1000 * 60 * 60 * 24));
    if (diff <= 1) {
      streak++;
      curr = d;
    } else {
      break;
    }
  }
  return streak;
}

const MOOD_SCORES = {
  joyful: 9,
  energetic: 8,
  focused: 7,
  calm: 5,
  sad: 3,
  anxious: 2,
  frustrated: 1,
};

// Update an <hj-metric-card> value (falls back to a plain span if the element
// has not been upgraded to the custom element yet).
function setMetric(id, value) {
  const el = document.getElementById(id);
  if (!el) return;
  if (typeof el.setValue === "function") el.setValue(value);
  else {
    const span = el.querySelector(".metric-value");
    if (span) span.textContent = value;
  }
}

export async function refreshInsights() {
  const now = new Date();
  const year = now.getFullYear();
  const month = now.getMonth();
  const start = `${year}-${String(month + 1).padStart(2, "0")}-01`;
  const end = `${year}-${String(month + 1).padStart(2, "0")}-${String(new Date(year, month + 1, 0).getDate()).padStart(2, "0")}`;

  const [entries, moodItems, nameSetting, bioSetting, goalsSetting, reflectionSetting] =
    await Promise.all([
      invoke("journal_list", { limit: 1000 }, { silent: true }).catch(() => []),
      invoke("mood_list", { start, end }, { silent: true }).catch(() => []),
      invoke("setting_get", { key: "user_profile_name" }, { silent: true }).catch(() => ""),
      invoke("setting_get", { key: "user_profile_bio" }, { silent: true }).catch(() => ""),
      invoke("setting_get", { key: "user_profile_goals" }, { silent: true }).catch(() => ""),
      invoke("setting_get", { key: "ai_reflection_cache" }, { silent: true }).catch(() => ""),
    ]);

  // Personalized greeting & date header
  const greetEl = document.getElementById("insights-user-greeting");
  if (greetEl) {
    const hours = now.getHours();
    const timeOfDay = hours < 12 ? "Good morning" : hours < 18 ? "Good afternoon" : "Good evening";
    const userName = nameSetting ? nameSetting.trim() : "";
    greetEl.textContent = userName ? `${timeOfDay}, ${userName}` : `${timeOfDay}`;
  }
  const dateEl = document.getElementById("insights-current-date");
  if (dateEl) {
    dateEl.textContent = now.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric", year: "numeric" });
  }

  // Calculate Average Mood (0-10)
  const validMoods = (moodItems || []).map((m) => MOOD_SCORES[m.mood]).filter((s) => typeof s === "number");
  const avgMood = validMoods.length ? (validMoods.reduce((a, b) => a + b, 0) / validMoods.length).toFixed(1) : "5.0";
  setMetric("insight-avg-mood", avgMood);

  const totalEntries = entries?.length || 0;
  const totalWords = (entries || []).reduce((acc, e) => acc + (e.word_count || 0), 0);
  const entryDates = (entries || []).map((e) => e.date || e.created_at || "").filter(Boolean);
  const streak = calculateStreak(entryDates);

  setMetric("insight-total-entries", totalEntries.toLocaleString());
  setMetric("insight-total-words", totalWords.toLocaleString());
  setMetric("insight-streak-days", streak.toString());

  const nameInput = document.getElementById("insight-profile-name");
  const bioInput = document.getElementById("insight-profile-bio");
  const goalsInput = document.getElementById("insight-profile-goals");
  if (nameInput && !nameInput.value) nameInput.value = nameSetting || "";
  if (bioInput && !bioInput.value) bioInput.value = bioSetting || "";
  if (goalsInput && !goalsInput.value) goalsInput.value = goalsSetting || "";

  const reflectionBox = document.getElementById("insight-ai-reflection-box");
  if (reflectionBox && reflectionSetting) {
    reflectionBox.innerHTML = escapeHtml(reflectionSetting)
      .replace(/\n\n/g, "</p><p>")
      .replace(/\n/g, "<br>");
    reflectionBox.innerHTML = `<p>${reflectionBox.innerHTML}</p>`;
  }
}

export function initInsights() {
  const saveBtn = document.getElementById("btn-save-profile");
  const statusEl = document.getElementById("insight-profile-status");
  const nameInput = document.getElementById("insight-profile-name");
  const bioInput = document.getElementById("insight-profile-bio");
  const goalsInput = document.getElementById("insight-profile-goals");
  const reflectBtn = document.getElementById("btn-generate-ai-reflection");
  const reflectionBox = document.getElementById("insight-ai-reflection-box");

  if (saveBtn) {
    saveBtn.addEventListener("click", async () => {
      saveBtn.disabled = true;
      if (statusEl) statusEl.textContent = "Saving profile…";
      try {
        await Promise.all([
          invoke("setting_set", { key: "user_profile_name", value: nameInput?.value.trim() || "" }),
          invoke("setting_set", { key: "user_profile_bio", value: bioInput?.value.trim() || "" }),
          invoke("setting_set", { key: "user_profile_goals", value: goalsInput?.value.trim() || "" }),
        ]);
        const greetEl = document.getElementById("insights-user-greeting");
        if (greetEl && nameInput) {
          const hours = new Date().getHours();
          const timeOfDay = hours < 12 ? "Good morning" : hours < 18 ? "Good afternoon" : "Good evening";
          const userName = nameInput.value.trim();
          greetEl.textContent = userName ? `${timeOfDay}, ${userName}` : `${timeOfDay}`;
        }
        if (statusEl) statusEl.textContent = "Profile saved.";
        setTimeout(() => {
          if (statusEl) statusEl.textContent = "";
        }, 3000);
      } catch (err) {
        console.error("Save profile failed:", err);
        if (statusEl) statusEl.textContent = "Failed to save.";
      } finally {
        saveBtn.disabled = false;
      }
    });
  }

  if (reflectBtn && reflectionBox) {
    reflectBtn.addEventListener("click", async () => {
      reflectBtn.disabled = true;
      reflectionBox.innerHTML = `<p class="reflection-empty">Reflecting on your journals and profile…</p>`;
      let accumulated = "";
      let unlisten = null;
      try {
        const [entries, name, goals] = await Promise.all([
          invoke("journal_list", { limit: 10 }, { silent: true }).catch(() => []),
          invoke("setting_get", { key: "user_profile_name" }, { silent: true }).catch(() => ""),
          invoke("setting_get", { key: "user_profile_goals" }, { silent: true }).catch(() => ""),
        ]);

        const titles = (entries || []).map((e) => `• ${e.title || "Untitled"}`).join("\\n");
        const prompt = `Based on my recent journal activity (${entries.length} recent entries):\\n${titles}\\nMy name: ${name || "User"}\\nMy long-term goals: ${goals || "Unspecified"}\\n\\nPlease give me a 2-3 sentence personalized reflection on my mindset and 2 concise long-term actionable suggestions to stay aligned with my goals.`;

        const convId = (typeof crypto !== "undefined" && crypto.randomUUID)
          ? crypto.randomUUID()
          : `insights-${Date.now()}`;

        const listen = globalThis.__TAURI__?.event?.listen;
        if (listen) {
          unlisten = await listen("app_event", (event) => {
            const p = event?.payload;
            if (!p || p.conversation_id !== convId) return;
            if (p.type === "ai_token" && p.token) {
              accumulated += p.token;
              reflectionBox.innerHTML = `<p>${escapeHtml(accumulated).replace(/\\n\\n/g, "</p><p>").replace(/\\n/g, "<br>")}</p>`;
            }
          });
        }

        await invoke("ai_chat", { message: prompt, conversationId: convId, source: "analysis" });

        if (unlisten) { try { unlisten(); } catch (e) {} unlisten = null; }
        const reply = accumulated.trim() || "(No response generated.)";
        await invoke("setting_set", { key: "ai_reflection_cache", value: reply }).catch(() => {});
        reflectionBox.innerHTML = `<p>${escapeHtml(reply).replace(/\\n\\n/g, "</p><p>").replace(/\\n/g, "<br>")}</p>`;
      } catch (err) {
        console.error("AI reflection failed:", err);
        if (unlisten) { try { unlisten(); } catch (e) {} unlisten = null; }
        reflectionBox.innerHTML = `<p class="reflection-empty">Reflection generation unavailable. Ensure local AI engine is running in Settings > LLM Engine.</p>`;
      } finally {
        reflectBtn.disabled = false;
      }
    });
  }

  refreshInsights();
}