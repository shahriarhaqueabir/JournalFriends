// Pure, escape-first renderer for the analysis card's "Extracted" section
// (spec 2.2 / D-145). No DOM access — node harnesses import this directly.
import { escapeHtml } from "./sanitize.js";

const SNIPPET_LEN = 120;

function truncate(text, max = SNIPPET_LEN) {
  const s = String(text ?? "");
  return s.length > max ? `${s.slice(0, max - 1)}…` : s;
}

function localStamp(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return String(iso ?? "");
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

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

function eventItem(e) {
  if (!e || typeof e !== "object") return "";
  const title = String(e.title ?? "").trim() || "Untitled event";
  const when = localStamp(e.start_at);
  const where = e.location ? ` · ${escapeHtml(e.location)}` : "";
  return `<li><span class="extracted-chip">${escapeHtml(title)} — ${escapeHtml(when)}${where}</span></li>`;
}

function noteItem(n) {
  if (!n || typeof n !== "object") return "";
  const title = String(n.title ?? "").trim() || "Untitled note";
  const snippet = truncate(n.content);
  return `<li><span class="extracted-chip"><strong>${escapeHtml(title)}</strong>${
    snippet ? ` — ${escapeHtml(snippet)}` : ""
  }</span></li>`;
}

function factItem(f) {
  if (!f || typeof f !== "object") return "";
  const cat = f.category
    ? `<span class="extracted-cat">${escapeHtml(f.category)}</span> `
    : "";
  return `<li><span class="extracted-chip">${cat}${escapeHtml(f.content ?? "")}</span></li>`;
}

export function extractedItemsHtml(result) {
  if (!result || typeof result !== "object") return "";
  const tasks = Array.isArray(result.tasks) ? result.tasks : [];
  const events = Array.isArray(result.events) ? result.events : [];
  const notes = Array.isArray(result.notes) ? result.notes : [];
  const facts = Array.isArray(result.facts) ? result.facts : [];
  return [
    group("Tasks", tasks.map(taskItem).join("")),
    group("Events", events.map(eventItem).join("")),
    group("Notes", notes.map(noteItem).join("")),
    group("Facts", facts.map(factItem).join("")),
  ].join("");
}
