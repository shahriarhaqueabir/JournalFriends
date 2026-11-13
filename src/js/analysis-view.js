// Pure, escape-first renderer for the analysis card's "Extracted" section
// (spec 2.2 / D-145, D-165, D-167). No DOM access — node harnesses import this
// directly.
import { escapeHtml } from "./sanitize.js";

function group(title, itemsHtml) {
  if (!itemsHtml) return "";
  return `<div class="extracted-group"><h4>${escapeHtml(title)}</h4><ul class="extracted-list">${itemsHtml}</ul></div>`;
}

function taskItem(t) {
  if (t == null) return "";
  if (typeof t !== "object") {
    return `<li><span class="extracted-chip">${escapeHtml(t)}</span></li>`;
  }
  const title = String(t.title ?? "").trim() || "Untitled task";
  const meta = [t.due_date, t.priority]
    .filter(Boolean)
    .map(escapeHtml)
    .join(" · ");
  return `<li><span class="extracted-chip">${escapeHtml(title)}${
    meta ? `<span class="extracted-meta"> · ${meta}</span>` : ""
  }</span></li>`;
}

function countLabel(count, singular, plural) {
  return `${count} ${count === 1 ? singular : plural}`;
}

function joinList(parts) {
  if (parts.length <= 1) return parts[0] || "";
  if (parts.length === 2) return `${parts[0]} and ${parts[1]}`;
  return `${parts.slice(0, -1).join(", ")}, and ${parts[parts.length - 1]}`;
}

function hasText(value) {
  return String(value ?? "").trim().length > 0;
}

function collection(result, key) {
  return Array.isArray(result[key]) ? result[key] : [];
}

/**
 * Notes, facts, and events are internal (D-167): they are saved without an
 * interactive confirmation card, so the analysis card only reports how many
 * were captured. Never include their content here — counts only.
 */
function otherItemsSummary(result) {
  const notes = collection(result, "notes").filter((n) =>
    n && typeof n === "object"
      ? hasText(n.content) || hasText(n.title)
      : hasText(n),
  );
  const facts = collection(result, "facts").filter((f) =>
    f && typeof f === "object"
      ? hasText(f.content) || hasText(f.fact_key)
      : hasText(f),
  );
  const events = collection(result, "events").filter((e) =>
    e && typeof e === "object"
      ? hasText(e.title) || hasText(e.start_at)
      : hasText(e),
  );

  const saved = [];
  if (notes.length) saved.push(countLabel(notes.length, "note", "notes"));
  if (facts.length) saved.push(countLabel(facts.length, "fact", "facts"));

  const sentences = [];
  if (saved.length) {
    sentences.push(
      `I've saved ${joinList(saved)} to memory for future context.`,
    );
  }
  if (events.length) {
    sentences.push(
      `I've added ${countLabel(events.length, "event", "events")} to your calendar.`,
    );
  }
  return sentences.join(" ");
}

export function extractedItemsHtml(result) {
  if (!result || typeof result !== "object") return "";
  const tasks = collection(result, "tasks");
  const summary = otherItemsSummary(result);
  return [
    group("Tasks", tasks.map(taskItem).join("")),
    // Built only from internal text + numeric counts — no user content.
    summary ? `<p class="extracted-summary">${summary}</p>` : "",
  ].join("");
}
