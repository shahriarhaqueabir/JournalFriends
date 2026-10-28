import { escapeHtml } from "./sanitize.js";

const ACTION_LABELS = {
  create: "Create",
  update: "Update",
  complete: "Complete",
  delete: "Delete",
};

/**
 * Derive the confirm-button label from the stored proposal payload.
 * Chat tools tag their payloads with a `type` marker such as
 * `event_update` or `note_delete`; legacy analysis proposals have no marker
 * and become creates.
 */
export function proposalActionLabel(item) {
  const raw = item?.proposed_data;
  let data = {};
  if (typeof raw === "string" && raw.trim()) {
    try {
      data = JSON.parse(raw);
    } catch {
      data = {};
    }
  } else if (raw && typeof raw === "object") {
    data = raw;
  }
  const marker = typeof data.type === "string" ? data.type : "";
  const suffix = marker.split("_").pop();
  return ACTION_LABELS[suffix] || "Create";
}

export function proposalCardHtml(item) {
  const conf =
    item.confidence != null ? ` · ${Math.round(item.confidence * 100)}%` : "";
  const body = item.source_text ? escapeHtml(item.source_text) : "";
  const label = proposalActionLabel(item);
  return `
    <div class="proposal-card ai-proposal-card" data-id="${escapeHtml(item.id)}">
      <div class="ai-proposal-title">${escapeHtml(item.item_type)}: ${escapeHtml(item.proposed_title)}</div>
      <div class="ai-proposal-body">${body}${conf}</div>
      <div class="ai-proposal-actions">
        <button type="button" class="btn-secondary ai-prop-accept" data-id="${escapeHtml(item.id)}">${label}</button>
        <button type="button" class="btn-ghost ai-prop-dismiss" data-id="${escapeHtml(item.id)}">Dismiss</button>
      </div>
    </div>`;
}
