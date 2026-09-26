import { $, defer, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject } from "./03-shell.js";
import { renderMarkdownInto } from "./04-markdown.js";
import { toolResultSummary } from "./05-tool-summary.js";

// Project ownership is captured at send time. A late stream never writes into another project.
const conversations = new Map();
function conversation(project) {
  if (!conversations.has(project)) conversations.set(project, { turns: [], target: null, draft: "", request: null, loaded: false, loading: null, status: "" });
  return conversations.get(project);
}

defer(() => {
  const form = $("memory-chat-form");
  if (!form) return;
  const input = $("memory-chat-input");
  const list = $("memory-chat-messages");
  let displayedProject = null;
  let displayedTurns = null;
  let paintFrame = 0;
  const rows = new WeakMap();

  function render() {
    const project = currentProject;
    const chat = conversation(project);
    if (displayedProject !== project) { displayedProject = project; input.value = chat.draft; }
    const following = list.scrollHeight - list.scrollTop - list.clientHeight < 70;
    if (displayedTurns !== chat.turns) { list.replaceChildren(); displayedTurns = chat.turns; }
    if (!chat.turns.length) {
      list.replaceChildren();
      const intro = document.createElement("div");
      intro.className = "memory-reader-empty";
      intro.innerHTML = `<strong>${t("直接说明你想怎么改")}</strong><p>${t("可以讨论内容、精简条目，或合并重复记忆。")}</p>`;
      list.appendChild(intro);
    }
    for (const turn of chat.turns) {
      let cached = rows.get(turn);
      if (!cached) {
        const row = document.createElement("article");
        row.className = "memory-chat-message markdown";
        row.dataset.role = turn.role;
        const body = document.createElement("div");
        row.appendChild(body);
        cached = { row, body }; rows.set(turn, cached);
      }
      const { row, body } = cached;
      if (cached.text !== turn.text) {
        if (turn.role === "user") body.textContent = turn.text;
        else renderMarkdownInto(body, turn.text || t("正在处理…"));
        cached.text = turn.text;
      }
      if (turn.changes?.length && cached.changes !== turn.changes) {
        row.querySelector(".memory-chat-changes")?.remove();
        const changes = document.createElement("details");
        changes.className = "memory-chat-changes";
        const summary = document.createElement("summary");
        summary.textContent = `${t("修改记录")} · ${turn.changes.length}`;
        changes.appendChild(summary);
        for (const change of turn.changes) {
          const item = document.createElement("p");
          item.className = change.ok ? "memory-change-ok" : "memory-change-failed";
          // 后端只给 preview(首行 + 行数):与主对话同一个摘要器,记忆工具的回执说成人话。
          const said = toolResultSummary(change.tool, { ok: change.ok, preview: change.summary }).text;
          item.textContent = `${change.ok ? "✓" : "!"} ${change.tool} · ${said}`;
          changes.appendChild(item);
        }
        row.appendChild(changes);
        cached.changes = turn.changes;
      }
      if (row.parentElement !== list) {
        list.querySelector(".memory-reader-empty")?.remove();
        list.appendChild(row);
      }
    }
    const context = $("memory-chat-context");
    context.replaceChildren();
    if (chat.target) {
      const label = document.createElement("span");
      label.textContent = `${t(chat.target.scope === "global" ? "全局记忆" : "项目记忆")} ${chat.target.id} · ${chat.target.title || ""}`;
      const clear = document.createElement("button");
      clear.type = "button"; clear.className = "ghost mini"; clear.textContent = t("取消选择");
      clear.addEventListener("click", () => { chat.target = null; render(); });
      context.append(label, clear);
    }
    $("memory-chat-status").textContent = chat.status;
    $("memory-chat-send").disabled = !project || !chat.loaded || Boolean(chat.request);
    $("memory-chat-stop").classList.toggle("hidden", !chat.request);
    if (following) list.scrollTop = list.scrollHeight;
  }

  function scheduleRender(project) {
    if (project !== currentProject || paintFrame) return;
    paintFrame = requestAnimationFrame(() => { paintFrame = 0; render(); });
  }

  async function load(project, recovery = false) {
    if (!project) { render(); return; }
    const chat = conversation(project);
    if (chat.loading || (chat.loaded && !chat.externalRequest)) { render(); return chat.loading; }
    clearTimeout(chat.recoveryTimer);
    if (recovery && (project !== currentProject || $("memory-chat").classList.contains("hidden") || !$("view-memory").classList.contains("active"))) return;
    if (!chat.loaded) chat.status = t("正在加载对话…");
    render();
    chat.loading = (async () => {
      try {
        const result = await invoke("memory_chat_history", { projectDir: project });
        chat.turns = result.messages || []; chat.loaded = true; chat.request = result.requestId || null;
        chat.externalRequest = Boolean(chat.request);
        chat.status = chat.request ? t("正在处理…") : "";
        if (chat.externalRequest) chat.recoveryTimer = setTimeout(() => { void load(project, true); }, 1500);
        else if (recovery) document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
      } catch (error) { chat.status = String(error); }
      finally { chat.loading = null; if (currentProject === project) render(); }
    })();
    return chat.loading;
  }

  document.addEventListener("kz:memory-chat-open", () => { void load(currentProject); });
  document.addEventListener("kz:view-changed", event => {
    if (event.detail.view === "memory" && !$("memory-chat").classList.contains("hidden")) void load(currentProject);
  });
  document.addEventListener("kz:memory-project", () => {
    render();
    if (!$("memory-chat").classList.contains("hidden")) void load(currentProject);
  });
  document.addEventListener("kz:memory-selected", event => {
    const { project, ...target } = event.detail;
    conversation(project).target = target;
    if (project === currentProject) render();
  });
  document.addEventListener("kz:memory-selection-cleared", event => {
    conversation(event.detail.project).target = null;
    if (event.detail.project === currentProject) render();
  });
  input.addEventListener("input", () => { conversation(currentProject).draft = input.value; });
  input.addEventListener("keydown", event => {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !event.isComposing) { event.preventDefault(); form.requestSubmit(); }
  });
  form.addEventListener("submit", async event => {
    event.preventDefault();
    const project = currentProject;
    const chat = conversation(project);
    const message = input.value.trim();
    if (!project || !message || chat.request || !chat.loaded) return;
    const request = crypto.randomUUID();
    chat.request = request; chat.status = t("正在处理…"); chat.draft = ""; input.value = "";
    chat.turns.push({ role: "user", text: message });
    const reply = { role: "assistant", text: "", changes: [] };
    chat.turns.push(reply);
    const channel = new window.__TAURI__.core.Channel();
    channel.onmessage = payload => {
      if (chat.request !== request) return;
      if (payload.type === "text") reply.text += payload.text;
      if (payload.type === "tool") chat.status = `${t("正在处理…")} ${payload.name}`;
      scheduleRender(project);
    };
    render(); list.scrollTop = list.scrollHeight;
    try {
      const result = await invoke("memory_chat_send", { projectDir: project, requestId: request, message,
        target: chat.target ? { scope: chat.target.scope, id: chat.target.id } : null, onEvent: channel });
      reply.text = result.text; reply.changes = result.changes || [];
      chat.status = result.error ? result.error : result.stopped ? t("已停止") : t("处理完成");
    } catch (error) { reply.text = `${t("处理失败")}: ${error}`; chat.status = t("处理失败"); }
    finally {
      chat.request = null;
      if (project === currentProject) render();
      document.dispatchEvent(new CustomEvent("kz:memory-changed", { detail: { project } }));
    }
  });
  $("memory-chat-stop").addEventListener("click", async () => {
    const project = currentProject;
    const chat = conversation(project);
    if (!chat.request) return;
    try {
      await invoke("memory_chat_stop", { projectDir: project, requestId: chat.request });
      chat.status = t("正在停止…");
    } catch (error) { chat.status = String(error); }
    if (project === currentProject) render();
  });
});
