import { t } from "./02-i18n.js";
import { $, defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { on } from "./01-runtime-events.js";
import { currentProject, activeProcessId, navigate_view, toastError, syncNewChatEnabled } from "./03-shell.js";
import { setConversationRoot, setQqConversationRoots, rememberConversationMode } from "./03-general-scope.js";
import { switch_workspace, sync_workspace_visibility } from "./03-workspaces.js";
import { activate_execution_root, refreshProcesses, switchProcess, refreshPendingInputs } from "./09-sessions.js";
import { cancelProjectNavigation, setBrowsingProject, workbenchNavigationGuard } from "./12-workbench.js";
import { loadModels } from "./08-models.js";
import { loadConversation } from "./15-views-misc.js";
import { layoutPref, setLayoutPref } from "./03-layout.js";

const element = (tag, text, cls) => { const node = document.createElement(tag); if (text != null) node.textContent = text; if (cls) node.className = cls; return node; };
const button = (text, action, cls = "ghost") => { const node = element("button", text, cls); node.type = "button"; node.addEventListener("click", () => Promise.resolve(action()).catch(error => toastError(String(error)))); return node; };
const key = identity => JSON.stringify([identity.account, identity.kind, identity.peer]);
let snapshot = null, selected = null, tab = "connection", reading = 0, openGeneration = 0;
let form = null, groupForm = null, dirty = false, tokenDraft = "";

function field(form, label, name, value, type = "text") {
  const row = element("label", null, "qq-field"); row.append(element("span", t(label)));
  const input = element(type === "textarea" ? "textarea" : "input"); input.name = name;
  if (type !== "textarea") input.type = type;
  if (type === "checkbox") input.checked = !!value; else input.value = value ?? "";
  input.addEventListener("input", () => { dirty = true; });
  row.append(input); form.append(row); return input;
}

export async function refreshQq() {
  const generation = ++reading;
  try {
    const next = await invoke("qq_status");
    if (generation !== reading) return;
    if (dirty && snapshot) next.settings = snapshot.settings;
    snapshot = next;
    $("qq-page-error").textContent = next.error || "";
    setQqConversationRoots(next.peers.map(peer => peer.root));
    const nav = $("qq-nav");
    nav.dataset.connected = String(next.connected);
    nav.title = next.error || (next.connected ? `QQ ${next.nickname || next.account}` : t("QQ 未连接"));
    $("qq-state").textContent = next.connected ? `${t("已连接")} ${next.nickname || next.account}` : next.error || t("未连接");
    if (!selected) {
      const last = layoutPref("qq", "last_peer");
      selected = next.peers.find(peer => key(peer.identity) === last) || next.peers[0] || null;
    } else selected = next.peers.find(peer => key(peer.identity) === key(selected.identity)) || null;
    syncConversation();
    if (document.body.dataset.view === "qq" && !dirty) renderPage();
  } catch (error) {
    $("qq-page-error").textContent = String(error);
    throw error;
  }
}

export async function openQqPeer(peer) {
  const generation = ++openGeneration;
  cancelProjectNavigation(); const valid = workbenchNavigationGuard();
  if (!await switch_workspace("dev-auto", { isCurrent: valid }) || !valid()) return;
  const opened = await invoke("qq_peer_open", { identity: peer.identity });
  if (!valid() || generation !== openGeneration) return;
  selected = opened; setBrowsingProject(null); setConversationRoot(opened.root);
  activate_execution_root(opened.root);
  await refreshProcesses();
  if (!valid() || generation !== openGeneration) return;
  await switchProcess(opened.process_id, true);
  if (!valid() || generation !== openGeneration) return;
  await loadConversation();
  if (!valid() || generation !== openGeneration) return;
  await loadModels();
  if (!valid() || generation !== openGeneration) return;
  await refreshPendingInputs();
  if (!valid() || generation !== openGeneration) return;
  setLayoutPref("qq", "last_peer", key(opened.identity)); rememberConversationMode("qq");
  sync_workspace_visibility(); syncNewChatEnabled(); navigate_view("chat", { prepared: true });
  syncConversation(); $("prompt").focus();
}

function syncConversation() {
  const peer = snapshot?.peers.find(peer => peer.root === currentProject && peer.process_id === activeProcessId);
  const active = document.body.dataset.view === "qq" || (!!peer && document.body.dataset.view === "chat");
  $("qq-nav").classList.toggle("active", active); $("qq-nav").setAttribute("aria-pressed", String(active));
  const header = $("qq-conversation"); header.hidden = !peer || document.body.dataset.view !== "chat";
  document.body.dataset.qqConversation = String(!!peer);
  if (!peer) return;
  selected = peer;
  $("qq-conversation-name").textContent = `${peer.identity.kind === "group" ? t("群聊") : "好友"} ${peer.title}`;
  const select = $("qq-conversation-select");
  select.replaceChildren(...snapshot.peers.map(item => {
    const option = element("option", `${item.identity.kind === "group" ? t("群聊") : "好友"} ${item.title}`);
    option.value = key(item.identity); option.selected = key(item.identity) === key(peer.identity); return option;
  }));
}

function renderPage() {
  const host = $("qq-content"); host.replaceChildren();
  if (!snapshot) return;
  const settings = snapshot.settings;
  $("qq-tabs").querySelectorAll("button").forEach(button => { button.classList.toggle("active", button.dataset.tab === tab); button.setAttribute("aria-selected", String(button.dataset.tab === tab)); });
  form = null; groupForm = null;
  if (tab === "connection" || tab === "persona") {
    form = element("form", null, "qq-form"); form.id = "qq-settings-form";
    if (tab === "connection") {
      field(form, "启用 QQ", "enabled", settings.enabled, "checkbox");
      field(form, "OneBot 地址", "url", settings.url);
      const token = field(form, "连接令牌", "token", tokenDraft, "password"); token.autocomplete = "new-password"; token.placeholder = snapshot.token_configured ? t("已配置，留空保留") : t("OneBot 访问令牌");
      field(form, "机器人 QQ 号", "account", settings.account);
      field(form, "允许好友私聊唤起", "friends_enabled", settings.friends_enabled, "checkbox");
      field(form, "模型", "model", settings.model || "").placeholder = t("使用默认模型");
    } else field(form, "共享人设", "persona", settings.persona, "textarea");
    const save = element("button", t("保存"), "primary"); save.type = "submit"; form.append(save);
    form.addEventListener("submit", event => { event.preventDefault(); void saveSettings().catch(error => toastError(String(error))); });
    host.append(form); return;
  }
  if (tab === "peers") {
    const list = element("div", null, "qq-peer-list");
    for (const kind of ["friend", "group"]) {
      list.append(element("h2", kind === "friend" ? t("好友私聊") : t("群聊")));
      for (const peer of snapshot.peers.filter(peer => peer.identity.kind === kind)) {
        const row = element("div", null, "qq-peer-row");
        row.append(button(peer.title, () => openQqPeer(peer)), element("span", peer.identity.peer, "dim"), button(t("记忆"), () => { selected = peer; changeTab("memory"); }), button(t("记录"), () => { selected = peer; changeTab("log"); })); list.append(row);
      }
    }
    if (!snapshot.peers.length) list.append(element("p", t("收到已授权的好友或群消息后，会话会出现在这里。"), "dim"));
    host.append(list); return;
  }
  if (tab === "groups") {
    const toolbar = element("div", null, "qq-toolbar");
    toolbar.append(button(t("从 QQ 读取群列表"), async () => {
      captureGroups();
      const directory = await invoke("qq_directory");
      for (const group of directory.groups) if (!snapshot.settings.groups.some(item => item.id === String(group.group_id))) snapshot.settings.groups.push(groupDefaults(String(group.group_id), group.group_name, false));
      dirty = true; renderGroups(host);
    })); host.append(toolbar); renderGroups(host); return;
  }
  if (tab === "memory" || tab === "log") {
    const select = element("select"); select.setAttribute("aria-label", t("选择 QQ 会话"));
    for (const peer of snapshot.peers) { const option = element("option", `${peer.identity.kind === "group" ? t("群聊") : "好友"} ${peer.title}`); option.value = key(peer.identity); option.selected = selected && key(peer.identity) === key(selected.identity); select.append(option); }
    select.addEventListener("change", () => { selected = snapshot.peers.find(peer => key(peer.identity) === select.value); void renderRecords(host).catch(error => toastError(String(error))); });
    host.append(select); const content = element("div"); content.id = "qq-records"; host.append(content);
    void renderRecords(host).catch(error => toastError(String(error)));
  }
}

function groupDefaults(id = "", name = "", enabled = true) { return { id, name, enabled, interval_seconds: 120, proactive_minutes: 0, quiet_start: 0, quiet_end: 8, persona: "" }; }
function renderGroups(host) {
  host.querySelector("#qq-groups-form")?.remove();
  groupForm = element("form", null, "qq-group-form"); groupForm.id = "qq-groups-form";
  snapshot.settings.groups.forEach((group, index) => {
    const box = element("fieldset", null, "qq-group"); box.dataset.index = String(index); box.append(element("legend", group.name || group.id || t("新群")));
    field(box, "群号", "id", group.id); field(box, "名称", "name", group.name); field(box, "参与群聊", "enabled", group.enabled, "checkbox");
    field(box, "发言间隔（秒）", "interval_seconds", group.interval_seconds, "number").min = "1";
    field(box, "主动搭话间隔（分钟，0 为关闭）", "proactive_minutes", group.proactive_minutes, "number").min = "0";
    field(box, "安静时段开始", "quiet_start", group.quiet_start, "number").max = "23";
    field(box, "安静时段结束", "quiet_end", group.quiet_end, "number").max = "23";
    field(box, "本群人设", "persona", group.persona, "textarea"); groupForm.append(box);
  });
  groupForm.append(button(t("添加群"), () => { captureGroups(); snapshot.settings.groups.push(groupDefaults()); dirty = true; renderGroups(host); }));
  const save = element("button", t("保存群聊规则"), "primary"); save.type = "submit"; groupForm.append(save);
  groupForm.addEventListener("submit", event => { event.preventDefault(); captureGroups(); void persist(snapshot.settings).catch(error => toastError(String(error))); }); host.append(groupForm);
}
function captureGroups() {
  if (!groupForm) return;
  snapshot.settings.groups = [...groupForm.querySelectorAll("fieldset")].map(box => {
    const get = name => box.querySelector(`[name="${name}"]`);
    return { id: get("id").value.trim(), name: get("name").value.trim(), enabled: get("enabled").checked, interval_seconds: Number(get("interval_seconds").value), proactive_minutes: Number(get("proactive_minutes").value), quiet_start: Number(get("quiet_start").value), quiet_end: Number(get("quiet_end").value), persona: get("persona").value };
  });
}
async function saveSettings() {
  if (!form) return;
  const settings = structuredClone(snapshot.settings); let token = null;
  for (const input of form.querySelectorAll("[name]")) {
    if (input.name === "token") token = input.value || null;
    else settings[input.name] = input.type === "checkbox" ? input.checked : input.name === "model" ? input.value || null : input.value;
  }
  await persist(settings, token);
}
async function persist(settings, token = null) { await invoke("qq_config_save", { settings, token: token || tokenDraft || null }); dirty = false; tokenDraft = ""; await refreshQq(); }
function changeTab(next) {
  if (groupForm) captureGroups();
  if (form) for (const input of form.querySelectorAll("[name]")) {
    if (input.name === "token") tokenDraft = input.value;
    else snapshot.settings[input.name] = input.type === "checkbox" ? input.checked : input.name === "model" ? input.value || null : input.value;
  }
  tab = next; renderPage();
}
async function renderRecords(host) {
  const content = host.querySelector("#qq-records"); content.replaceChildren(); if (!selected) return;
  const identity = selected.identity, generation = key(identity), targetTab = tab;
  const data = await invoke(tab === "memory" ? "qq_memories" : "qq_peer_log", { identity });
  if (!selected || generation !== key(selected.identity) || targetTab !== tab || !content.isConnected) return;
  if (tab === "memory") {
    content.append(button(t("新建记忆"), () => memoryEditor(content, identity)));
    for (const entry of data) {
      const row = element("article", null, "qq-memory-entry");
      row.append(element("strong", entry.title), element("span", `${entry.status} ${entry.scope}`, "dim"), element("p", entry.body), button(t("编辑"), () => memoryEditor(content, identity, entry))); content.append(row);
    }
  } else {
    for (const message of data.messages) { const row = element("article", null, "qq-log-entry"); row.append(element("small", `${message.nickname} (${message.sender}) ${new Date(message.at).toLocaleString()}`), element("p", message.text)); content.append(row); }
    content.append(element("h2", t("发送回执")));
    for (const receipt of data.deliveries) { const row = element("article", null, "qq-log-entry"); row.append(element("strong", receipt.state), element("p", receipt.body)); if (receipt.error) row.append(element("p", receipt.error, "dim")); content.append(row); }
  }
}
function memoryEditor(host, identity, entry = null) {
  host.querySelector("#qq-memory-editor")?.remove();
  const editor = element("form", null, "qq-form"); editor.id = "qq-memory-editor";
  field(editor, "标题", "title", entry?.title || ""); field(editor, "召回线索", "description", entry?.description || ""); field(editor, "正文", "body", entry?.body || "", "textarea");
  const save = element("button", t("保存记忆"), "primary"); save.type = "submit"; editor.append(save);
  editor.addEventListener("submit", event => {
    event.preventDefault();
    const values = Object.fromEntries(new FormData(editor));
    void invoke("qq_memory_save", { identity, id: entry?.id || null, expectedHash: entry?.revision || null, ...values }).then(() => { dirty = false; renderPage(); }).catch(error => toastError(String(error)));
  }); host.prepend(editor);
}

defer(() => {
  $("qq-nav").addEventListener("click", async () => {
    try { await refreshQq(); if (selected) await openQqPeer(selected); else navigate_view("qq"); } catch (error) { navigate_view("qq"); toastError(String(error)); }
  });
  $("qq-manage").addEventListener("click", () => { navigate_view("qq"); void refreshQq().catch(error => toastError(String(error))); });
  $("qq-refresh").addEventListener("click", () => { void refreshQq().catch(error => toastError(String(error))); });
  $("qq-open-chat").addEventListener("click", () => { if (selected) void openQqPeer(selected).catch(error => toastError(String(error))); });
  $("qq-conversation-select").addEventListener("change", event => { const peer = snapshot?.peers.find(peer => key(peer.identity) === event.target.value); if (peer) void openQqPeer(peer).catch(error => toastError(String(error))); });
  for (const [id, label] of [["connection", "连接"], ["peers", "会话"], ["groups", "群聊规则"], ["persona", "人设"], ["memory", "记忆"], ["log", "记录"]]) {
    const item = button(t(label), () => changeTab(id)); item.dataset.tab = id; item.setAttribute("role", "tab"); $("qq-tabs").append(item);
  }
  document.addEventListener("kz:view-changed", event => { syncConversation(); if (event.detail?.view === "qq") void refreshQq().catch(error => toastError(String(error))); });
  document.addEventListener("kz:conversation-selected", syncConversation);
  document.addEventListener("kz:language", () => { syncConversation(); if (document.body.dataset.view === "qq" && !dirty) renderPage(); });
  on("kz:qq", () => { void refreshQq().catch(error => toastError(String(error))); });
  void refreshQq().catch(error => toastError(String(error)));
});
