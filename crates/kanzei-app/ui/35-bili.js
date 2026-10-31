import { $, defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { on } from "./01-runtime-events.js";
import { t } from "./02-i18n.js";
import { navigate_view, toastError, currentProject } from "./03-shell.js";
import { setBiliConversationRoot } from "./03-general-scope.js";
import { openChannelConversation } from "./34-channel-conversations.js";

const node = (tag, text, cls) => { const e = document.createElement(tag); if (text != null) e.textContent = text; if (cls) e.className = cls; return e; };
const button = (label, action, cls = "ghost") => { const b = node("button", t(label), cls); b.type = "button"; b.addEventListener("click", () => { b.disabled = true; Promise.resolve().then(action).catch(e => toastError(String(e))).finally(() => { b.disabled = false; }); }); return b; };
let snapshot = null, tab = "browse", epoch = 0, videoEpoch = 0, loginEpoch = 0, login = null, poll = null;
let feed = [], keyword = "", feedPage = 1, video = null, comments = null, commentEpoch = 0, commentPage = 1, thread = null, reply = null, settingsDirty = false, settingsForm = null;
const drafts = new Map();
const bvidOf = value => String(value).match(/BV[A-Za-z0-9]{10}/)?.[0] || "";
const field = (form, label, name, value, type = "text") => { const row = node("label", t(label), "qq-field"); const input = node(type === "textarea" ? "textarea" : "input"); input.name = name; if (type !== "textarea") input.type = type; if (type === "checkbox") input.checked = !!value; else input.value = value ?? ""; row.append(input); form.append(row); return input; };
export async function refreshBili() {
  const own = ++epoch, next = await invoke("bili_status"); if (own !== epoch) return;
  if (settingsDirty && snapshot) next.settings = snapshot.settings;
  snapshot = next; setBiliConversationRoot(next.conversation_root);
  $("bili-state").textContent = next.account ? `${t("账号")} ${next.name || next.account}` : t("尚未登录");
  $("bili-error").textContent = next.error || ""; $("bili-nav").dataset.connected = String(!!next.account);
  syncConversation(); if (document.body.dataset.view === "bili" && !settingsDirty) render();
}
async function openConversation(target) { await openChannelConversation(() => target ? Promise.resolve(target) : invoke("bili_conversation_open"), "bili"); syncConversation(); }
function syncConversation() {
  const active = !!snapshot?.conversation_root && currentProject === snapshot.conversation_root;
  $("bili-conversation").hidden = !active || document.body.dataset.view !== "chat";
  $("bili-nav").classList.toggle("active", document.body.dataset.view === "bili" || active && document.body.dataset.view === "chat");
}
function switchTab(next) { if (settingsForm && settingsDirty) captureSettings(); tab = next; settingsForm = null; render(); }
function captureSettings() {
  if (!settingsForm) return;
  for (const input of settingsForm.querySelectorAll("[name]")) snapshot.settings[input.name] = input.type === "checkbox" ? input.checked : input.type === "number" ? Number(input.value) : input.name === "model" ? input.value || null : input.value;
}
function render() {
  const host = $("bili-content"); host.replaceChildren(); if (!snapshot) return;
  $("bili-tabs").querySelectorAll("button").forEach(b => { b.classList.toggle("active", b.dataset.tab === tab); b.setAttribute("aria-selected", String(b.dataset.tab === tab)); }); settingsForm = null;
  if (tab === "login") { renderLogin(host); return; }
  if (tab === "settings") { renderSettings(host); return; }
  if (tab === "drafts") { renderDrafts(host); return; }
  if (tab === "records") {
    for (const row of snapshot.records.browsing) { const article = node("article", null, "qq-log-entry"); article.append(node("strong", row.title), node("p", `${row.bvid} ${new Date(row.at).toLocaleString()}`), button("打开视频", () => selectVideo(row.bvid))); host.append(article); } return;
  }
  const search = node("form", null, "bili-toolbar"), input = node("input"); input.id = "bili-search"; input.value = keyword; input.placeholder = t("关键词、BV 编号或视频链接"); input.setAttribute("aria-label", t("搜索 B站"));
  const submit = node("button", t("搜索")); submit.type = "submit"; search.append(input, submit, button("热门视频", () => { keyword = ""; feedPage = 1; return loadFeed(); }), button("浏览一轮", async () => { await openConversation(); await invoke("bili_browse"); }));
  search.addEventListener("submit", e => { e.preventDefault(); const bv = bvidOf(input.value); keyword = input.value.trim(); feedPage = 1; void (bv ? selectVideo(bv) : loadFeed()).catch(error => toastError(String(error))); }); host.append(search);
  if (video) renderVideo(host);
  const list = node("div", null, "bili-feed");
  for (const item of feed) {
    const bv = bvidOf(item.bvid); if (!bv) continue;
    const row = node("article", null, "bili-feed-row"), cover = node("img"); cover.src = String(item.pic || "").replace(/^\/\//, "https://"); cover.alt = ""; cover.loading = "lazy";
    const title = node("button", String(item.title || bv).replace(/<[^>]*>/g, ""), "bili-video-title"); title.type = "button"; title.addEventListener("click", () => { void selectVideo(bv).catch(e => toastError(String(e))); });
    row.append(cover, title, node("span", item.owner?.name || item.author || "", "dim")); list.append(row);
  }
  host.append(list); if (feed.length) host.append(button("上一页", () => { if (feedPage > 1) { feedPage--; return loadFeed(); } }), button("下一页", () => { feedPage++; return loadFeed(); }));
}
async function loadFeed() {
  const own = ++videoEpoch, result = await invoke("bili_read", { action: "feed", keyword, page: feedPage, bvid: null, root: null }); if (own !== videoEpoch) return;
  feed = keyword ? result.result : result.list; if (!Array.isArray(feed)) throw new Error(t("B站视频列表格式错误")); video = null; comments = null; tab = "browse"; render();
}
function renderDrafts(host) {
  const states = { draft: "草稿", sending: "正在发送", sent: "已发送", failed: "发送失败", unknown: "结果未知", discarded: "已丢弃" };
  host.append(node("p", t("助手生成的评论在此审核，点击发布后发送。结果未知的评论不会自动重发。"), "dim"));
  for (const row of snapshot.records.drafts) {
    const article = node("article", null, "qq-log-entry"); article.dataset.draftId = row.id;
    article.append(node("strong", `${row.bvid} · ${t(states[row.state])} · ${t("账号")} ${row.account}`));
    if (row.root) article.append(node("p", `${t("回复")} ${row.root} / ${row.parent || row.root}`));
    if (row.state === "draft") {
      const text = node("textarea"); text.setAttribute("aria-label", t("评论正文")); text.value = drafts.get(row.id)?.body ?? row.body;
      text.addEventListener("input", () => drafts.set(row.id, { body: text.value, revision: drafts.get(row.id)?.revision || row.revision }));
      const publish = button("发布评论", async () => {
        const buffer = drafts.get(row.id), revision = buffer?.revision || row.revision;
        if (text.value.trim() !== row.body) {
          const saved = await invoke("bili_draft_edit", { id: row.id, body: text.value, revision });
          drafts.set(row.id, { body: text.value.trim(), revision: saved.revision });
          await invoke("bili_publish", { id: row.id, revision: saved.revision });
        } else await invoke("bili_publish", { id: row.id, revision });
        drafts.delete(row.id); await refreshBili();
      }, "primary"); publish.disabled = row.account !== snapshot.account;
      article.append(text, button("保存草稿", async () => {
        const saved = await invoke("bili_draft_edit", { id: row.id, body: text.value, revision: drafts.get(row.id)?.revision || row.revision }); drafts.set(row.id, { body: text.value.trim(), revision: saved.revision }); await refreshBili();
      }), publish, button("丢弃草稿", async () => { await invoke("bili_draft_discard", { id: row.id }); drafts.delete(row.id); await refreshBili(); }));
    } else article.append(node("p", row.body));
    if (row.receipt) article.append(node("p", row.receipt, "bili-receipt"));
    if (row.error) article.append(node("p", row.error, "bili-receipt"));
    host.append(article);
  }
}
function renderSettings(host) {
  const form = node("form", null, "qq-form"); settingsForm = form;
  const s = snapshot.settings;
  field(form, "自动浏览", "auto_browse", s.auto_browse, "checkbox");
  field(form, "浏览间隔（分钟）", "interval_minutes", s.interval_minutes, "number").min = "1";
  field(form, "安静时段开始（小时）", "quiet_start", s.quiet_start, "number").max = "23";
  field(form, "安静时段结束（小时）", "quiet_end", s.quiet_end, "number").max = "23";
  field(form, "浏览关键词", "keyword", s.keyword);
  field(form, "模型（留空沿用当前配置）", "model", s.model);
  field(form, "每日评论上限", "daily_comments", s.daily_comments, "number").min = "1";
  field(form, "评论间隔（秒）", "comment_interval_seconds", s.comment_interval_seconds, "number").min = "0";
  form.append(node("p", t("B站与个人助手、QQ 好友共用个人记忆；群聊记忆按群隔离。自动浏览生成日志和草稿，评论由你点击发布。"), "dim"));
  const save = node("button", t("保存设置"), "primary"); save.type = "submit"; form.append(save);
  form.addEventListener("input", () => { settingsDirty = true; });
  form.addEventListener("submit", e => { e.preventDefault(); captureSettings(); save.disabled = true; void invoke("bili_settings_save", { settings: snapshot.settings }).then(async () => { settingsDirty = false; await refreshBili(); }).catch(e => toastError(String(e))).finally(() => { save.disabled = false; }); }); host.append(form);
}
function stopPoll() { if (poll) clearTimeout(poll); poll = null; }
async function pollLogin(own) {
  if (own !== loginEpoch || !login || document.body.dataset.view !== "bili" || tab !== "login") return;
  try {
    const result = await invoke("bili_login_poll", { id: login.id }); if (own !== loginEpoch) return;
    login = { ...login, ...result };
    if (result.status === "success") { stopPoll(); await refreshBili(); }
    else { render(); if (["waiting", "scanned"].includes(login.status)) poll = setTimeout(() => { void pollLogin(own); }, 2000); }
  } catch (e) { if (own === loginEpoch) { stopPoll(); login = { ...login, status: "error", image: null }; render(); toastError(String(e)); } }
}
function renderLogin(host) {
  host.append(node("p", t("用哔哩哔哩客户端扫码登录，凭据保存在本机。"), "dim"));
  host.append(button("生成登录二维码", async () => { stopPoll(); const own = ++loginEpoch; const result = await invoke("bili_login_start"); if (own !== loginEpoch) return; login = result; render(); void pollLogin(own); }));
  if (login) {
    const labels = { waiting: "等待扫码", scanned: "已扫码，请在手机确认", success: "登录成功", expired: "二维码已过期", cancelled: "已取消", error: "登录失败" };
    const row = node("div", null, "social-qr"); row.append(node("p", t(labels[login.status] || login.status)));
    if (login.image && ["waiting", "scanned"].includes(login.status)) { const img = node("img"); img.src = login.image; img.alt = t("B站登录二维码"); row.append(img, button("取消登录", async () => { stopPoll(); ++loginEpoch; await invoke("bili_login_cancel", { id: login.id }); login = null; render(); })); } host.append(row);
  }
  const advanced = node("details"), summary = node("summary", t("手动登录凭据")), form = node("form", null, "qq-form");
  const cookie = field(form, "B站 Cookie", "cookie", "", "password"); cookie.autocomplete = "off"; cookie.required = true;
  const save = node("button", t("验证并保存登录")); save.type = "submit"; form.append(save);
  form.addEventListener("submit", e => { e.preventDefault(); save.disabled = true; void invoke("bili_cookie_save", { cookie: cookie.value }).then(async () => { cookie.value = ""; stopPoll(); ++loginEpoch; login = null; await refreshBili(); }).catch(e => toastError(String(e))).finally(() => { save.disabled = false; }); }); advanced.append(summary, form); host.append(advanced);
  if (snapshot.account) host.append(button("退出 B站登录", async () => { stopPoll(); ++loginEpoch; await invoke("bili_logout"); login = null; await refreshBili(); }));
}
defer(() => {
  for (const [key, label] of [["browse", "浏览视频"], ["drafts", "评论草稿"], ["records", "浏览记录"], ["login", "账号登录"], ["settings", "浏览设置"]]) {
    const b = button(label, () => switchTab(key)); b.dataset.tab = key; b.setAttribute("role", "tab"); $("bili-tabs").append(b);
  }
  $("bili-nav").addEventListener("click", () => { navigate_view("bili"); void refreshBili().catch(e => toastError(String(e))); });
  $("bili-refresh").addEventListener("click", () => { void refreshBili().catch(e => toastError(String(e))); });
  $("bili-open-chat").addEventListener("click", () => { void openConversation().catch(e => toastError(String(e))); });
  $("bili-manage").addEventListener("click", () => navigate_view("bili"));
  on("kz:bili", () => { void refreshBili().catch(e => toastError(String(e))); });
  document.addEventListener("kz:conversation-selected", syncConversation);
  document.addEventListener("kz:language", () => { if (document.body.dataset.view === "bili" && !settingsDirty) render(); });
  new MutationObserver(() => { syncConversation(); stopPoll(); if (document.body.dataset.view === "bili" && tab === "login" && login && ["waiting", "scanned"].includes(login.status)) void pollLogin(loginEpoch); }).observe(document.body, { attributes: true, attributeFilter: ["data-view"] });
  void refreshBili().catch(e => toastError(String(e)));
});
async function selectVideo(bvid) {
  const own = ++videoEpoch, result = await invoke("bili_read", { action: "video", bvid, keyword: null, page: 1, root: null }); if (own !== videoEpoch) return;
  video = result; comments = null; reply = null; thread = null; commentPage = 1; tab = "browse"; render();
}
async function loadComments(root = null) {
  const bv = video?.bvid, selection = videoEpoch, own = ++commentEpoch; if (!bv) return;
  const result = await invoke("bili_read", { action: "comments", bvid: bv, page: root ? 1 : commentPage, root, keyword: null }); if (own !== commentEpoch || selection !== videoEpoch || video?.bvid !== bv) return;
  comments = result.comments; thread = root; render();
}
function renderVideo(host) {
  const section = node("section", null, "bili-video"); section.append(node("h2", video.title));
  const player = node("iframe"); player.id = "bili-player"; player.src = `https://player.bilibili.com/player.html?bvid=${encodeURIComponent(video.bvid)}&autoplay=0`; player.title = video.title; player.allow = "fullscreen; picture-in-picture"; player.referrerPolicy = "strict-origin-when-cross-origin"; section.append(player);
  section.append(node("p", video.desc || "", "bili-description"), button("读取评论", () => loadComments()), button("读取字幕", async () => {
    const bv = video.bvid, own = videoEpoch, value = await invoke("bili_read", { action: "transcript", bvid: bv, keyword: null, page: 1, root: null }); if (own !== videoEpoch || video?.bvid !== bv) return;
    const transcript = node("div", null, "bili-transcript"); transcript.append(node("p", value.modality)); if (Array.isArray(value.transcript)) for (const line of value.transcript) transcript.append(node("p", `${line.from} ${line.content}`)); section.querySelector(".bili-transcript")?.remove(); section.append(transcript);
  }));
  if (comments) {
    const list = node("div", null, "bili-comments"); if (thread) list.append(button("返回视频评论", () => loadComments()));
    const rows = [...(comments.top_replies || []), ...(comments.replies || [])], seen = new Set(); if (thread && comments.root) rows.unshift(comments.root);
    for (const row of rows) {
      const id = Number(row.rpid); if (!id || seen.has(id)) continue; seen.add(id);
      const article = node("article", null, "qq-log-entry"); article.append(node("strong", row.member?.uname || ""), node("p", row.content?.message || ""), button("回复", () => { reply = { root: thread || id, parent: id, name: row.member?.uname || "" }; render(); }));
      if (!thread && row.rcount > 0) article.append(button("展开回复", () => loadComments(id))); list.append(article);
    }
    if (!thread) list.append(button("上一页", () => { if (commentPage > 1) { commentPage--; return loadComments(); } }), button("下一页", () => { commentPage++; return loadComments(); })); section.append(list);
  }
  const form = node("form", null, "qq-form"); form.id = "bili-comment-form";
  if (reply) form.append(node("p", `${t("回复")} ${reply.name}`), button("取消回复", () => { reply = null; render(); }));
  const text = field(form, "评论正文", "body", "", "textarea"); text.required = true; const save = node("button", t("保存评论草稿"), "primary"); save.type = "submit"; form.append(save);
  form.addEventListener("submit", e => { e.preventDefault(); void invoke("bili_draft_create", { bvid: video.bvid, body: text.value, root: reply?.root || null, parent: reply?.parent || null }).then(async () => { await refreshBili(); switchTab("drafts"); }).catch(error => toastError(String(error))); }); section.append(form); host.append(section);
}
