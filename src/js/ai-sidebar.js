import { invoke } from "./ipc.js";
import { recordDiagnostic } from "./diagnostics.js";
import { escapeHtml } from "./sanitize.js";
import { formatAiText } from "./chat-format.js";
import { stringifyError } from "./notifications.js";
import { proposalCardHtml } from "./proposals.js";
import { extractedItemsHtml } from "./analysis-view.js";

let messagesEl = null;
let inputEl = null;
let currentEntryId = null;
let currentConversationId = null;
let currentAiBubble = null;
let inboxEl = null;
// M7: buffer events that arrive before the sidebar is initialized.
const pendingEvents = [];

// ── ANALYSIS STATUS STRIP (spec 2.3) ────────────────────
// Global: updates for ANY entry id (not gated on currentEntryId).
const strip = {
  state: "idle",
  entryId: null,
  startedAt: 0,
  doneIn: null,
  errorText: null,
  timer: null,
};
const stripTitles = new Map(); // entryId -> cached title (foreign entries)

function stripEls() {
  return {
    root: document.getElementById("analysis-strip"),
    state: document.getElementById("analysis-strip-state"),
    detail: document.getElementById("analysis-strip-detail"),
    time: document.getElementById("analysis-strip-time"),
    retry: document.getElementById("btn-analysis-retry"),
  };
}

function stripTitle(entryId) {
  if (!entryId) return "";
  if (currentEntryId && currentEntryId.value === entryId) {
    const openTitle = document.getElementById("journal-title")?.value?.trim();
    return openTitle || "Untitled Entry";
  }
  return stripTitles.get(entryId) || "";
}

function fetchStripTitle(entryId) {
  if (!entryId || stripTitles.has(entryId)) return;
  if (currentEntryId && currentEntryId.value === entryId) return; // open entry reads live from the title input
  stripTitles.set(entryId, ""); // reserve so parallel events don't double-fetch
  invoke("journal_get", { id: entryId }, { silent: true })
    .then((entry) => {
      stripTitles.set(
        entryId,
        entry ? entry.title?.trim() || "Untitled Entry" : "",
      );
      renderStrip();
    })
    .catch(() => {});
}

function stripElapsed() {
  if (!strip.startedAt) return "";
  return `${((Date.now() - strip.startedAt) / 1000).toFixed(1)}s`;
}

function renderStrip() {
  const els = stripEls();
  if (!els.root) return;
  els.root.classList.remove(
    "is-idle",
    "is-queued",
    "is-analyzing",
    "is-done",
    "is-error",
  );
  els.root.classList.add(`is-${strip.state}`);
  if (els.retry) els.retry.classList.toggle("hidden", strip.state !== "error");

  const railDot = document.getElementById("rail-analysis-dot");
  if (railDot) {
    const tone =
      strip.state === "analyzing"
        ? "warn"
        : strip.state === "error"
          ? "error"
          : strip.state === "done"
            ? "ok"
            : "";
    railDot.className = `status-dot${tone ? ` ${tone}` : ""}`;
  }

  const title = stripTitle(strip.entryId);
  const ticking = strip.state === "queued" || strip.state === "analyzing";

  switch (strip.state) {
    case "queued":
      els.state.textContent = "Queued";
      els.detail.textContent = title
        ? `waiting — ${title}`
        : "waiting for analysis…";
      break;
    case "analyzing":
      els.state.textContent = "Analyzing…";
      els.detail.textContent = title || "working on it…";
      break;
    case "done":
      els.state.textContent = "Done";
      els.detail.textContent =
        strip.entryId && currentEntryId?.value === strip.entryId
          ? title
          : "open the entry to view.";
      break;
    case "error":
      els.state.textContent = "Error";
      els.detail.textContent = strip.errorText || "analysis failed";
      break;
    default:
      els.state.textContent = "Idle";
      els.detail.textContent = "Auto-analyzed as you type — no save needed.";
  }

  if (ticking) {
    els.time.textContent = stripElapsed();
    if (!strip.timer) strip.timer = setInterval(renderStrip, 200);
  } else {
    if (strip.timer) {
      clearInterval(strip.timer);
      strip.timer = null;
    }
    els.time.textContent = strip.doneIn != null ? `in ${strip.doneIn}s` : "";
  }
}

function updateAnalysisStrip(state, entryId, error) {
  const prev = strip.state;
  const sameEntry = strip.entryId === entryId;
  if (!sameEntry) {
    strip.entryId = entryId;
    strip.startedAt = Date.now();
    strip.doneIn = null;
  } else if (
    state === "queued" &&
    (prev === "idle" || prev === "done" || prev === "error")
  ) {
    strip.startedAt = Date.now(); // a new analysis cycle for the same entry
    strip.doneIn = null;
  } else if (state === "analyzing" && !strip.startedAt) {
    strip.startedAt = Date.now();
  }
  strip.state = state;
  if (state === "done") {
    strip.doneIn = strip.startedAt
      ? Number(((Date.now() - strip.startedAt) / 1000).toFixed(1))
      : null;
  }
  strip.errorText =
    state === "error" ? String(error ?? "analysis failed").slice(0, 200) : null;
  if (entryId && state !== "error") fetchStripTitle(entryId);
  renderStrip();
}

export function initAiSidebar() {
  messagesEl = document.getElementById("ai-chat-messages");
  inputEl = document.getElementById("ai-chat-input");
  currentEntryId = document.getElementById("current-entry-id");
  inboxEl = document.getElementById("ai-inbox");

  const inboxBtn = document.getElementById("btn-open-inbox");
  if (inboxBtn) {
    inboxBtn.addEventListener("click", async () => {
      if (!inboxEl) return;
      if (inboxEl.classList.contains("hidden")) {
        await loadInbox();
        inboxEl.classList.remove("hidden");
      } else {
        inboxEl.classList.add("hidden");
      }
    });
  }

  if (inputEl) {
    const sendBtn = document.getElementById("btn-send-chat");
    const syncComposer = () => {
      if (sendBtn) sendBtn.disabled = !inputEl.value.trim();
    };
    const autosize = () => {
      inputEl.style.height = "auto";
      inputEl.style.height = `${Math.min(inputEl.scrollHeight, 160)}px`;
    };

    inputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        sendChatMessage();
      }
    });
    inputEl.addEventListener("input", () => {
      autosize();
      syncComposer();
    });
    sendBtn?.addEventListener("click", () => sendChatMessage());
    syncComposer();
  }

  const retryBtn = document.getElementById("btn-analysis-retry");
  retryBtn?.addEventListener("click", async () => {
    if (!strip.entryId) return;
    retryBtn.disabled = true;
    try {
      await invoke("analysis_requeue", { entryId: strip.entryId });
      recordDiagnostic("analysis:requeued", { entry_id: strip.entryId });
      updateAnalysisStrip("queued", strip.entryId);
    } catch (err) {
      // invoke() already toasted the failure (non-silent); just record it.
      recordDiagnostic("analysis:requeue_failed", {
        entry_id: strip.entryId,
        error: stringifyError(err),
      });
    } finally {
      retryBtn.disabled = false;
    }
  });

  // M7: replay any events that arrived before init.
  while (pendingEvents.length) {
    const ev = pendingEvents.shift();
    handleAppEvent(ev);
  }
  loadLatestConversation();
}

async function loadInbox() {
  if (!inboxEl) return;
  inboxEl.innerHTML = `<div class="inbox-loading">Loading proposals…</div>`;
  try {
    const items = await invoke(
      "proposed_list",
      { status: "pending", limit: 50 },
      { silent: true },
    );
    renderInbox(items || []);
  } catch (err) {
    inboxEl.innerHTML = `<div class="inbox-error">Failed to load inbox</div>`;
    console.error("Failed to load inbox:", err);
  }
}

function renderInbox(items) {
  if (!inboxEl) return;
  if (!items.length) {
    inboxEl.innerHTML = `<div class="inbox-empty">No pending proposals.</div>`;
    return;
  }

  const html = items
    .map((item) => {
      const creator = item.created_by || "ai";
      const conf =
        item.confidence != null ? `${Math.round(item.confidence * 100)}%` : "";
      return `
      <div class="inbox-item" data-id="${escapeHtml(item.id)}">
        <div class="inbox-item-header">
          <strong>${escapeHtml(item.proposed_title)}</strong>
          <span class="badge">${escapeHtml(item.item_type)}</span>
          <small class="inbox-meta">by ${escapeHtml(creator)} ${conf ? "• " + conf : ""}</small>
        </div>
        <div class="inbox-item-body">${item.source_text ? escapeHtml(item.source_text) : ""}</div>
        <div class="inbox-item-actions">
          <button class="btn-inbox-accept btn-primary" data-id="${escapeHtml(item.id)}">Accept</button>
          <button class="btn-inbox-dismiss btn-ghost-sm" data-id="${escapeHtml(item.id)}">Dismiss</button>
        </div>
      </div>
    `;
    })
    .join("");

  inboxEl.innerHTML = html;

  inboxEl.querySelectorAll(".btn-inbox-accept").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.id;
      btn.disabled = true;
      btn.textContent = "...";
      try {
        await invoke("proposed_action", { id, action: "accept" });
        btn.textContent = "Accepted";
        recordDiagnostic("inbox:accepted", { id });
        // remove item after a short delay
        setTimeout(() => {
          const el = inboxEl.querySelector(`[data-id="${id}"]`);
          if (el) el.remove();
        }, 600);
      } catch (err) {
        console.error("Accept failed", err);
        btn.textContent = "Error";
        btn.disabled = false;
      }
    });
  });

  inboxEl.querySelectorAll(".btn-inbox-dismiss").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const id = btn.dataset.id;
      btn.disabled = true;
      try {
        await invoke("proposed_action", { id, action: "dismiss" });
        recordDiagnostic("inbox:dismissed", { id });
        const el = inboxEl.querySelector(`[data-id="${id}"]`);
        if (el) el.remove();
      } catch (err) {
        console.error("Dismiss failed", err);
        btn.textContent = "Error";
        btn.disabled = false;
      }
    });
  });
}

async function renderProposalCards(count) {
  if (!messagesEl || !count) return;
  try {
    const items = await invoke(
      "proposed_list",
      { status: "pending", limit: 25 },
      { silent: true },
    );
    if (!items || !items.length) return;
    const cardWrap = document.createElement("div");
    cardWrap.className = "ai-proposal-flow";
    cardWrap.innerHTML = items.map((it) => proposalCardHtml(it)).join("");
    messagesEl.appendChild(cardWrap);
    cardWrap.querySelectorAll(".ai-prop-accept").forEach((btn) => {
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        try {
          await invoke("proposed_action", {
            id: btn.dataset.id,
            action: "accept",
          });
          btn.textContent = "Accepted";
        } catch (err) {
          console.error("proposal accept failed", err);
          btn.disabled = false;
        }
      });
    });
    cardWrap.querySelectorAll(".ai-prop-dismiss").forEach((btn) => {
      btn.addEventListener("click", async () => {
        btn.disabled = true;
        try {
          await invoke("proposed_action", {
            id: btn.dataset.id,
            action: "dismiss",
          });
          btn.closest(".ai-proposal-card").remove();
        } catch (err) {
          console.error("proposal dismiss failed", err);
          btn.disabled = false;
        }
      });
    });
  } catch (err) {
    console.error("proposal cards failed", err);
  }
}

// ── PUBLIC EVENT HANDLER (D-96) ────────────────────────
export function handleAppEvent(payload) {
  if (!messagesEl || !currentEntryId) {
    pendingEvents.push(payload);
    return;
  }

  const { type } = payload;
  recordDiagnostic("ai_sidebar:event", {
    type,
    entry_id: payload.entry_id,
    conversation_id: payload.conversation_id,
  });

  switch (type) {
    case "journal_analysis_queued":
      updateAnalysisStrip("queued", payload.entry_id);
      break;

    case "journal_analysis_processing":
      updateAnalysisStrip("analyzing", payload.entry_id);
      break;

    case "journal_analysis_completed":
      updateAnalysisStrip("done", payload.entry_id);
      if (currentEntryId.value === payload.entry_id) {
        renderAnalysisResult(payload.result);
      }
      break;

    case "journal_analysis_error":
      updateAnalysisStrip("error", payload.entry_id, payload.error);
      break;

    case "ai_reasoning_token":
      if (payload.conversation_id === currentConversationId) {
        appendReasoningToken(payload.token);
      }
      break;

    case "ai_token":
      if (payload.conversation_id === currentConversationId) {
        appendToken(payload.token);
      }
      break;

    case "ai_response_complete":
      if (payload.conversation_id === currentConversationId) {
        finalizeAiBubble();
        currentAiBubble = null;
      }
      break;

    case "ai_model_missing":
      const missingModel = payload.model;
      showErrorStatus(
        `The AI model ${missingModel} is not installed.`,
        "Model Not Found",
      );
      // Add provision button specifically for this case
      const errorBubble = messagesEl.querySelector(".chat-assistant.error");
      if (errorBubble) {
        const div = document.createElement("div");
        div.className = "assistant-error-actions";
        const btn = document.createElement("button");
        btn.className = "btn-primary-sm";
        btn.textContent = `Install ${missingModel}`;
        btn.onclick = async () => {
          btn.disabled = true;
          btn.textContent = "Installing...";
          try {
            await invoke("provision_model_cmd");
            if (window.refreshAiSettings) await window.refreshAiSettings();
            showErrorStatus("Model installed. You can try again.");
          } catch (e) {
            showErrorStatus(`Failed to install: ${e}`);
          }
        };
        div.appendChild(btn);
        errorBubble.appendChild(div);
      }
      break;

    case "ai_proposals_ready":
      // Show badge on inbox button and auto-load if inbox is open
      console.log("[DEBUG] Received ai_proposals_ready:", payload);
      const inboxBtn = document.getElementById("btn-open-inbox");
      if (inboxBtn) {
        inboxBtn.innerHTML = `<hj-icon name="inbox" size="16" aria-hidden="true"></hj-icon><span>Inbox</span><span class="badge-dot"></span>`;
        window.renderIcons?.();
      }
      const railCount = document.getElementById("rail-inbox-count");
      if (railCount) {
        railCount.textContent = String(payload.count ?? "");
        railCount.classList.toggle("hidden", !payload.count);
      }
      // Main tab badge update
      const mainInboxBadge = document.getElementById("inbox-badge");
      if (mainInboxBadge) {
        mainInboxBadge.textContent = payload.count;
        mainInboxBadge.className = "badge-count";
      }

      if (inboxEl && !inboxEl.classList.contains("hidden")) {
        loadInbox();
      }

      // Notify main inbox tab if it exists
      if (window.refreshInbox) {
        window.refreshInbox();
      }

      renderProposalCards(payload.count);
      break;
  }
}

export function renderErrorStatus(container, error, title = "Insight Error") {
  container.replaceChildren();
  const bubble = document.createElement("div");
  bubble.className = "chat-assistant error";

  const heading = document.createElement("strong");
  heading.textContent = title;
  bubble.append(heading, document.createElement("br"));

  const message = document.createElement("span");
  message.textContent = String(error);
  bubble.append(message);

  const actions = document.createElement("div");
  actions.className = "assistant-error-actions";
  const retryButton = document.createElement("button");
  retryButton.className = "btn-ghost-sm btn-retry-engine";
  retryButton.textContent = "Retry Connection";
  actions.append(retryButton);
  bubble.append(actions);
  container.append(bubble);
  return retryButton;
}

function showErrorStatus(error, title = "Insight Error") {
  if (!messagesEl) return;
  const retryButton = renderErrorStatus(messagesEl, error, title);
  retryButton.addEventListener("click", () => {
    if (window.checkEngine) window.checkEngine();
    // Re-trigger analysis if possible, or just refresh models
    if (window.refreshAiSettings) window.refreshAiSettings();
  });
}

function renderAnalysisResult(result) {
  if (!messagesEl) return;
  const { summary, emotions, insights, mood } = result || {};
  recordDiagnostic("analysis:rendered", {
    mood,
    emotion_count: emotions?.length ?? 0,
    task_count: result?.tasks?.length ?? 0,
    insight_count: insights?.length ?? 0,
    fact_count: result?.facts?.length ?? 0,
    event_count: result?.events?.length ?? 0,
    note_count: result?.notes?.length ?? 0,
  });

  let html = `
    <div class="analysis-card">
      <div class="analysis-section summary">
        <div class="mood-badge">${escapeHtml(mood || "Reflective")}</div>
        <div class="analysis-summary-head">
          <h3>Key Summary</h3>
          <button class="btn-ghost-sm btn-view-source-entry analysis-source-btn" data-id="${escapeHtml(result.id)}" title="View original entry" aria-label="View original entry">
            <hj-icon name="eye" size="16" aria-hidden="true"></hj-icon>
          </button>
        </div>
        <p>${escapeHtml(summary)}</p>
      </div>
  `;

  if (emotions && emotions.length > 0) {
    html += `
      <div class="analysis-section emotions">
        <h3>Emotions</h3>
        <div class="tag-cloud">
          ${emotions.map((e) => `<span class="tag emotion">${escapeHtml(e)}</span>`).join("")}
        </div>
      </div>
    `;
  }

  if (insights && insights.length > 0) {
    html += `
      <div class="analysis-section insights">
        <h3>Deeper Insights</h3>
        <ul>${insights.map((i) => `<li>${escapeHtml(i)}</li>`).join("")}</ul>
      </div>
    `;
  }

  // Read-only extractions (D-145) — accept/dismiss happens in the
  // proposalCardHtml cards that AiProposalsReady renders (Task 3).
  const extracted = extractedItemsHtml(result);
  if (extracted) {
    html += `<div class="analysis-section extracted"><h3>Extracted</h3>${extracted}</div>`;
  }

  html += `</div>`;

  // APPEND — never wipe chat history (spec 1c/2.3).
  const wrap = document.createElement("div");
  wrap.className = "ai-result-flow";
  wrap.innerHTML = html;
  messagesEl.appendChild(wrap);
  messagesEl.scrollTop = messagesEl.scrollHeight;

  // Scoped to this card so earlier cards don't accumulate duplicate listeners.
  wrap.querySelectorAll(".btn-view-source-entry").forEach((btn) => {
    btn.addEventListener("click", () => {
      const id = btn.dataset.id;
      const journalTab = document.querySelector('[data-tab="journal"]');
      if (journalTab) journalTab.click();
      if (window.loadJournalEntry) window.loadJournalEntry(id);
    });
  });
}

async function sendChatMessage() {
  const text = inputEl.value.trim();
  if (!text) return;

  console.log("[CHAT] Sending message:", text);
  inputEl.value = "";
  inputEl.style.height = "auto";
  const sendBtn = document.getElementById("btn-send-chat");
  if (sendBtn) sendBtn.disabled = true;
  appendUserMessage(text);

  if (!currentConversationId) {
    currentConversationId =
      typeof globalThis.crypto?.randomUUID === "function"
        ? globalThis.crypto.randomUUID()
        : `conv_${Date.now()}_${Math.random().toString(36).substring(2, 9)}`;
  }

  currentAiBubble = createAiBubble();

  try {
    recordDiagnostic("chat:user_message_sent", {
      conversation_id: currentConversationId,
      length: text.length,
    });
    await invoke("ai_chat", {
      conversationId: currentConversationId,
      message: text,
      sourceEntryId: currentEntryId?.value || null,
      source: "sidebar",
    });
    console.log("[CHAT] Conversation established:", currentConversationId);
    recordDiagnostic("chat:conversation_ready", {
      conversation_id: currentConversationId,
    });
  } catch (err) {
    console.error("AI Chat Error:", err);
    const bubble = currentAiBubble || createAiBubble();
    bubble.classList.add("error");
    bubble.innerHTML = `
      I'm having trouble thinking right now. Is the local AI engine running? (${escapeHtml(stringifyError(err))})
      <div class="assistant-error-actions">
        <button class="btn-ghost-sm btn-retry-chat">Retry Connection</button>
      </div>
    `;
    bubble
      .querySelector(".btn-retry-chat")
      ?.addEventListener("click", async () => {
        if (window.checkEngine) await window.checkEngine();
        if (window.refreshAiSettings) await window.refreshAiSettings();
        bubble.remove();
        currentAiBubble = null;
        sendChatMessage(); // Retry sending
      });
  }
}

async function loadLatestConversation() {
  if (!messagesEl) return;

  try {
    const conversations = await invoke(
      "conversation_list",
      { limit: 1 },
      { silent: true },
    );
    if (!conversations.length) return;

    currentConversationId = conversations[0].id;
    const messages = await invoke(
      "conversation_messages",
      { conversationId: currentConversationId },
      { silent: true },
    );

    if (!messages.length) return;
    messagesEl.innerHTML = "";
    messages.forEach((message) => {
      if (message.role === "user" || message.role === "assistant") {
        // A persisted assistant tool-call turn carries empty content; skip it
        // so it does not render as a blank message bubble.
        if (!String(message.content || "").trim()) return;
        appendPersistedMessage(
          message.role === "assistant" ? "ai" : "user",
          message.content,
        );
      }
    });
    recordDiagnostic("chat:history_loaded", {
      conversation_id: currentConversationId,
      message_count: messages.length,
    });
  } catch (err) {
    recordDiagnostic("chat:history_load_failed", {
      error: stringifyError(err),
    });
  }
}

function appendUserMessage(text) {
  const row = document.createElement("div");
  row.className = "chat-user";
  row.textContent = text;
  messagesEl.appendChild(row);
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function appendPersistedMessage(role, text) {
  if (role === "ai") {
    const bubble = document.createElement("div");
    bubble.className = "chat-assistant";
    const reply = document.createElement("div");
    reply.className = "reply-text";
    reply.innerHTML = formatAiText(text);
    attachLinkHandlers(reply);
    bubble.appendChild(reply);
    messagesEl.appendChild(bubble);
    messagesEl.scrollTop = messagesEl.scrollHeight;
    return bubble;
  }
  const row = document.createElement("div");
  row.className = "chat-user";
  row.textContent = text;
  messagesEl.appendChild(row);
  messagesEl.scrollTop = messagesEl.scrollHeight;
  return row;
}

function createAiBubble() {
  const bubble = document.createElement("div");
  bubble.className = "chat-assistant";
  const reply = document.createElement("div");
  reply.className = "reply-text";
  bubble.appendChild(reply);
  messagesEl.appendChild(bubble);
  messagesEl.scrollTop = messagesEl.scrollHeight;
  return bubble;
}

function appendReasoningToken(token) {
  if (!currentAiBubble) {
    currentAiBubble = createAiBubble();
  }
  const bubble = currentAiBubble;
  let panel = bubble.querySelector(".thinking-panel");
  if (!panel) {
    panel = document.createElement("div");
    panel.className = "thinking-panel";
    panel.innerHTML =
      '<button type="button" class="thinking-toggle"><hj-icon name="message-circle" size="16" aria-hidden="true"></hj-icon><span class="thinking-label">Thinking…</span></button><div class="thinking-content"></div>';
    panel.addEventListener("click", () => panel.classList.toggle("collapsed"));
    bubble.insertBefore(panel, bubble.firstChild);
    bubble.classList.add("has-thinking");
    window.renderIcons?.();
  }
  panel.querySelector(".thinking-content").textContent += token;
  messagesEl.scrollTop = messagesEl.scrollHeight;
}

function finalizeThinking(bubble) {
  const panel = bubble.querySelector(".thinking-panel");
  if (!panel) return;
  if (!panel.classList.contains("collapsed")) {
    panel.classList.add("collapsed");
  }
  const label = panel.querySelector(".thinking-label");
  if (label) label.textContent = "Thought through this";
}

function finalizeAiBubble() {
  if (!currentAiBubble) return;
  finalizeThinking(currentAiBubble);
  const reply = currentAiBubble.querySelector(".reply-text");
  if (reply) {
    reply.innerHTML = formatAiText(reply.textContent);
    attachLinkHandlers(reply);
  }
}

function appendToken(token) {
  if (!currentAiBubble) {
    currentAiBubble = createAiBubble();
  }
  finalizeThinking(currentAiBubble);
  const reply = currentAiBubble.querySelector(".reply-text");
  if (reply) {
    reply.textContent += token;
  }
  messagesEl.scrollTop = messagesEl.scrollHeight;
}
function attachLinkHandlers(container) {
  container.querySelectorAll(".event-link").forEach((link) => {
    link.addEventListener("click", () => {
      const id = link.dataset.id;
      if (window.loadEvent) {
        window.loadEvent(id);
      }
    });
  });
  container.querySelectorAll(".task-link").forEach((link) => {
    link.addEventListener("click", () => {
      const id = link.dataset.id;
      if (window.loadTask) {
        window.loadTask(id);
      }
    });
  });
  container.querySelectorAll(".journal-link").forEach((link) => {
    link.addEventListener("click", () => {
      const id = link.dataset.id;
      const journalTab = document.querySelector('[data-tab="journal"]');
      if (journalTab) journalTab.click();
      if (window.loadJournalEntry) {
        window.loadJournalEntry(id);
      }
    });
  });
}
