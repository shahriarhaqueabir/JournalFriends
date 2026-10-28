// Shared onboarding persona form (wizard step + Memory-tab CTA).
// Saves self-reported answers as knowledge_items (kind=information,
// created_by="onboarding", confidence 1.0, tags ["persona"]).
import { invoke } from "./ipc.js";

const QUESTIONS = [
  {
    key: "name",
    category: "person",
    title: "What should I call you?",
    placeholder: "Name or nickname",
  },
  {
    key: "work",
    category: "person",
    title: "What do you do?",
    placeholder: "Job, studies, role",
  },
  {
    key: "day",
    category: "habit",
    title: "What does a typical day look like?",
    placeholder: "e.g. up at 6am, gym, deep work until lunch",
  },
  {
    key: "goal",
    category: "goal",
    title: "What are you working toward right now?",
    placeholder: "Current focus or project",
  },
  {
    key: "limits",
    category: "constraint",
    title: "Any constraints on your time or energy?",
    placeholder: "e.g. meetings until 3pm, no weekends",
  },
  {
    key: "pref",
    category: "preference",
    title: "What do you like, dislike, or care about?",
    placeholder: "Likes, dislikes, values",
  },
];

export function personaQuestions() {
  return QUESTIONS;
}

export function renderPersonaForm(container, prefix = "persona") {
  if (!container) return;
  container.innerHTML = QUESTIONS.map((q) => `
    <label class="persona-field">
      <span class="persona-field-label">${q.title}</span>
      <input type="text" id="${prefix}-${q.key}" class="text-input-control persona-field-input"
        placeholder="${q.placeholder}" autocomplete="off" />
    </label>
  `).join("");
}

export function collectPersonaAnswers(prefix = "persona") {
  return QUESTIONS
    .map((q) => {
      const el = document.getElementById(`${prefix}-${q.key}`);
      const content = (el ? el.value : "").trim();
      return { key: q.key, category: q.category, title: q.title, content };
    })
    .filter((a) => a.content);
}

export async function savePersonaAnswers(answers) {
  let existing = [];
  try {
    existing = await invoke("memory_list", { kind: null, category: null, search: null });
  } catch {
    existing = [];
  }
  const byTitle = new Map(
    existing.filter((i) => i.created_by === "onboarding").map((i) => [i.title, i])
  );
  let saved = 0;
  let updated = 0;
  for (const a of answers) {
    const prev = byTitle.get(a.title);
    const payload = {
      id: prev ? prev.id : "",
      title: a.title,
      content: a.content,
      kind: "information",
      category: a.category,
      tags: JSON.stringify(["persona"]),
      item_date: null,
      confidence: 1,
      created_by: "onboarding",
    };
    try {
      if (prev) {
        await invoke("memory_update", { input: payload });
        updated++;
      } else {
        await invoke("memory_create", { input: payload });
        saved++;
      }
    } catch (e) {
      console.warn("persona save failed", a.title, e);
    }
  }
  return { saved, updated };
}

export async function hasPersonaAnswers() {
  let existing = [];
  try {
    existing = await invoke("memory_list", { kind: null, category: null, search: null });
  } catch {
    existing = [];
  }
  return existing.some((i) => i.created_by === "onboarding");
}