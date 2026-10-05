import { paneFor, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeSessionId, currentProject, noteQuestionReply } from "./03-shell.js";
import { view_allowed } from "./03-workspaces.js";
import { inboxFromProjects, isMultiReply, sameProject, pathKey } from "./25-softwire-model.js";
import { button, node, renderInteraction } from "./25-softwire-view.js";

// A native conversation without a project overview still needs a reply surface.
// Keep its editor separate from the conversation draft and bind every send to
// the captured question identity/revision, including after a session switch.
const cards = new Map(), opened = new Set(), dismissed = new Set();
let pending = [];
const refresh = () => document.dispatchEvent(new CustomEvent("kz:refresh-work-questions"));

const identity = message => JSON.stringify([pathKey(message.project), message.sessionId, message.id]);
function createCard(message, draft = "") {
  const card = { message, element: node("section", null, "chat-question"), busy: false, request: null };
  const body = node("div");
  const input = node("textarea", null, "chat-question-input");
  input.rows = 2;
  input.value = draft;
  card.input = input;
  input.placeholder = t("填写你的选择或补充说明…");
  input.setAttribute("aria-label", t("回复提问"));
  const send = button(t("发送"), () => void submit(), "chat-question-send");
  renderInteraction(body, message, {
    back: () => { dismissed.add(message.key); card.element.remove(); },
    choice: (text, { edit = false } = {}) => {
      if (card.busy) return;
      const lines = input.value ? input.value.split("\n") : [];
      if (isMultiReply(message)) {
        const index = lines.indexOf(text);
        if (index < 0) lines.push(text); else lines.splice(index, 1);
        input.value = lines.join("\n");
      } else {
        const labels = new Set(message.choices.map(option => typeof option === "string" ? option : option.label));
        input.value = [...lines.filter(line => !labels.has(line)), text].join("\n");
      }
      if (edit || isMultiReply(message) || lines.length) input.focus();
      else void submit();
    },
    explain: () => input.focus(),
    refresh,
  });
  const receipt = body.querySelector(".sw-interaction-receipt");
  const retry = body.querySelector(".sw-interaction-refresh");
  card.element.setAttribute("aria-label", t("回复提问"));
  card.element.dataset.questionKey = message.key;
  card.element.append(body, input, send);
  input.addEventListener("input", sync);
  function sync() {
    input.disabled = card.busy;
    send.disabled = card.busy || !input.value.trim();
    body.querySelectorAll("[data-reply-choice], .sw-reply-explain").forEach(choice => {
      choice.disabled = card.busy;
      if (choice.dataset.replyChoice) choice.setAttribute("aria-pressed", String(input.value.split("\n").includes(choice.dataset.replyChoice)));
    });
  }
  async function submit() {
    const reply = input.value;
    if (card.busy || !reply.trim()) return;
    card.busy = true;
    if (card.request?.reply !== reply) card.request = { reply, id: crypto.randomUUID() };
    receipt.textContent = t("发送中"); sync();
    try {
      await invoke("softwire_answer_question", { projectDir: message.project, sessionId: message.sessionId,
        id: message.id, expectedRevision: String(message.revision), requestId: card.request.id, reply });
      noteQuestionReply(message.sessionId, message.id);
      dismissed.add(message.key); cards.delete(message.key); card.element.remove(); refresh();
    } catch (error) {
      receipt.textContent = String(error).replace(/^(?:Error:\s*)?question_expired:\s*/, "");
      retry.hidden = false;
    } finally { card.busy = false; sync(); }
  }
  sync();
  return card;
}

export function syncChatQuestions(rows = pending) {
  pending = rows;
  const eligible = document.body.dataset.view === "chat" && !view_allowed("project");
  const messages = eligible ? inboxFromProjects([], rows).filter(message => sameProject(message.project, currentProject)
    && message.sessionId === activeSessionId && !dismissed.has(message.key)
    && ((!message.source.background && !message.source.agentId) || opened.has(message.key))) : [];
  const keys = new Set(messages.map(message => message.key));
  const drafts = new Map([...cards.values()].map(card => [identity(card.message), card.input.value]));
  for (const [key, card] of cards) {
    if (!keys.has(key)) card.element.remove();
    if (!rows.some(row => row.id === card.message.id && row.sessionId === card.message.sessionId
      && sameProject(row.projectDir, card.message.project) && String(row.revision) === String(card.message.revision))) cards.delete(key);
  }
  for (const message of messages) {
    let card = cards.get(message.key);
    const fresh = !card;
    if (fresh) { card = createCard(message, drafts.get(identity(message))); cards.set(message.key, card); }
    const pane = paneFor(message.sessionId, { forDisplay: true });
    if (card.element.parentElement !== pane) pane.append(card.element);
    if (fresh) requestAnimationFrame(() => {
      if (message.sessionId === activeSessionId && document.body.dataset.view === "chat" && card.element.isConnected)
        card.element.scrollIntoView({ block: "nearest" });
    });
  }
}

export function openChatQuestion(row) {
  const message = inboxFromProjects([], [row])[0];
  opened.add(message.key); dismissed.delete(message.key);
  syncChatQuestions();
  cards.get(message.key)?.element.scrollIntoView({ block: "nearest" });
}
