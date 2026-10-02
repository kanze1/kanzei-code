import { invoke, messagePanes } from "./01-core.js";
import { addMarkdownHook } from "./04-markdown.js";
import { renderFileCard } from "./06-activity.js";
import { resourceIcon } from "./04-resource-types.js";

const projects = new Map(), subscribers = new Set();
const key = value => String(value || "").replace(/\\/g, "/").replace(/^\/\/\?\//, "").replace(/\/$/, "").toLowerCase();
export function deliveryState(project) {
  const id = key(project);
  if (!projects.has(id)) projects.set(id, { project, rows: [], revision: 0, loaded: false, error: "", request: null });
  return projects.get(id);
}
export function onDeliveriesChanged(fn) { subscribers.add(fn); }
function changed(state) {
  for (const pane of messagePanes.values()) for (const body of pane.querySelectorAll(".msg.assistant .message-body")) decorateDeliveredReply(body);
  for (const fn of subscribers) fn(state.project);
}
export function loadDeliveredFiles(project, { force = false } = {}) {
  const state = deliveryState(project);
  if (state.request) return state.request;
  if (state.loaded && !force) return Promise.resolve(state.rows);
  const revision = state.revision;
  state.request = invoke("delivered_files", { projectDir: project }).then(rows => {
    const incoming = Array.isArray(rows) ? rows : [];
    const live = state.revision === revision ? [] : state.rows.filter(row => row.liveRevision > revision);
    state.rows = [...live, ...incoming.filter(row => !live.some(newer => key(newer.path) === key(row.path) && newer.session_id === row.session_id))];
    state.loaded = true; state.error = "";
    changed(state); return state.rows;
  }).catch(error => { state.error = String(error); changed(state); return state.rows; })
    .finally(() => { state.request = null; });
  return state.request;
}
export function registerDelivery(display, sessionId) {
  if (!display?.project_dir || !display.path) return;
  const state = deliveryState(display.project_dir);
  const row = { ...display, session_id: display.session_id || sessionId, liveRevision: ++state.revision };
  state.rows = [row, ...state.rows.filter(old => key(old.path) !== key(row.path) || old.session_id !== row.session_id)];
  changed(state);
}
export function matchDeliveredFile(value, rows) {
  const target = key(value);
  const exact = rows.filter(row => [row.path, key(row.path).replace(key(row.worktree_root) + "/", ""), key(row.path).replace(key(row.project_dir) + "/", "")].some(path => key(path) === target));
  const candidates = exact.length ? exact : rows.filter(row => key(row.name) === target);
  const paths = new Set(candidates.map(row => key(row.path)));
  return paths.size === 1 ? candidates[0] : null;
}
export function deliveredFileFor(sessionId, path) {
  return matchDeliveredFile(path, [...projects.values()].flatMap(state => state.rows).filter(row => row.session_id === sessionId));
}
export function decorateDeliveredReply(body) {
  const message = body?.closest?.(".msg.assistant");
  const session = message?.dataset.sessionId;
  if (!session) return;
  const rows = [...projects.values()].flatMap(state => state.rows).filter(row => row.session_id === session);
  if (!rows.length) return;
  const selected = new Map();
  for (const part of body.querySelectorAll("a.md-path, code, .delivery-inline")) {
    if (part.closest("pre") || part.tagName === "CODE" && part.closest("a")) continue;
    const row = matchDeliveredFile(part.dataset.path || part.textContent, rows);
    if (!row) continue;
    selected.set(key(row.path), row);
    if (part.classList.contains("delivery-inline")) continue;
    const link = document.createElement("button");
    link.type = "button"; link.className = "delivery-inline"; link.textContent = part.textContent;
    link.prepend(resourceIcon(row.path));
    link.dataset.path = row.path; link.title = row.path;
    link.addEventListener("click", () => {
      const card = message.querySelectorAll(".file-card");
      const target = [...card].find(card => key(card.dataset.path) === key(row.path));
      target?.scrollIntoView?.({ block: "nearest", behavior: "smooth" });
      target?.querySelector(".file-card-name")?.click();
    });
    part.replaceWith(link);
  }
  // Plain filename mentions also get a card, but ambiguous names never choose a file.
  for (const row of rows) if (row.name && body.textContent.includes(row.name) && matchDeliveredFile(row.name, rows)) selected.set(key(row.path), row);
  let tray = message.querySelector(".message-deliveries");
  if (!selected.size) { tray?.remove(); return; }
  if (!tray) { tray = document.createElement("div"); tray.className = "message-deliveries"; message.append(tray); }
  const signature = JSON.stringify([...selected.values()].map(row => [row.id, row.path, row.status]));
  if (tray.dataset.files === signature) return;
  tray.dataset.files = signature;
  tray.replaceChildren(...[...selected.values()].map(row => renderFileCard(row, { projectDir: row.project_dir })));
  // A tool receipt is visible while work is running. Once the reply carries the
  // same artifact, keep the earlier copy in tool details instead of showing two cards.
  const pane = message.closest(".msg-pane");
  for (const card of pane?.querySelectorAll(".tool-msg > .file-card") || []) {
    if (!selected.has(key(card.dataset.path))) continue;
    const detail = card.closest(".tool-msg")?.querySelector(".tool-msg-detail");
    if (detail) detail.append(card);
  }
}
addMarkdownHook(decorateDeliveredReply);
