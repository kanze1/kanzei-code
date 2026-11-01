import { $, defer } from "./00-dom.js";
import { invoke } from "./00-ipc.js";
import { on } from "./01-runtime-events.js";
import { t } from "./02-i18n.js";
import { currentProject, activeProcessId, toastError } from "./03-shell.js";
import { socialManagement } from "./34-social-state.js";

const drafts = new Map();
let epoch = 0, shown = null;
const key = () => JSON.stringify([currentProject, activeProcessId]);
const node = (tag, text, cls) => { const e = document.createElement(tag); if (text != null) e.textContent = text; if (cls) e.className = cls; return e; };
const button = (label, action) => {
  const e = node("button", t(label), "ghost"); e.type = "button";
  e.addEventListener("click", () => { e.disabled = true; Promise.resolve().then(action).catch(error => toastError(String(error))).finally(() => { e.disabled = false; }); });
  return e;
};
const eligibilityLabels = { ready: "等待评估", disabled: "已关闭", stopped: "已停止", faulted: "需要处理", no_conversation: "先开始聊天", busy: "正在评估或聊天", not_due: "等待下次评估", quiet: "安静时段", daily_limit: "已达每日上限", unanswered_limit: "等待对方回复" };
const reviewLabels = { reviewing: "正在评估", selected: "已选中话题", running: "正在准备消息", sending: "正在发送", silent: "本次保持安静", deferred: "稍后再考虑", sent: "已发送", unknown: "送达不明", cancelled: "已取消", failed: "执行失败", interrupted: "执行已中断" };
const topicLabels = { active: "待聊", discussed: "已聊过", closed: "已关闭", uncertain: "送达不明" };
const kindLabels = { follow_up: "延续聊天", curiosity: "好奇提问", share: "分享内容", discovery: "发现话题" };

export async function refreshInitiative({ reload = false } = {}) {
  if (!socialManagement("initiative") || !currentProject || !activeProcessId) return;
  const own = ++epoch, ownKey = key(), args = { projectDir: currentProject, processId: activeProcessId };
  try {
    const snapshot = await invoke("initiative_status", args);
    if (own !== epoch || ownKey !== key() || !socialManagement("initiative")) return;
    let entry = drafts.get(ownKey);
    if (!entry || reload || !entry.dirty && document.activeElement?.closest("#initiative-settings-form") == null) {
      entry = { args, snapshot, settings: structuredClone(snapshot.target.settings), revision: snapshot.target.revision, dirty: false };
      drafts.set(ownKey, entry);
    } else entry.snapshot = snapshot;
    render(ownKey, entry);
  } catch (error) {
    if (own === epoch && ownKey === key()) $("initiative-error").textContent = String(error);
    throw error;
  }
}
function render(ownKey, entry) {
  const { snapshot } = entry, { target } = snapshot;
  const eligibility = snapshot.eligibility;
  $("initiative-state").textContent = t(target.settings.enabled && eligibility !== "stopped" && eligibility !== "faulted" && snapshot.channelQuiet ? "安静时段" : target.settings.enabled && !snapshot.available ? "连接暂不可用" : eligibilityLabels[eligibility] || eligibility);
  $("initiative-error").textContent = target.error || "";
  $("initiative-target").textContent = `${t("当前会话")}：${target.title}`;
  if (shown !== entry || ownKey !== $("initiative-form").dataset.target) {
    renderForm(entry); shown = entry; $("initiative-form").dataset.target = ownKey;
  }
  const topics = $("initiative-topics"); topics.replaceChildren(node("h2", t("想聊的话题")));
  if (!snapshot.topics.length) topics.append(node("p", t("聊天时会记下想继续聊的事，也会根据兴趣寻找新话题。"), "dim"));
  for (const topic of snapshot.topics) {
    const row = node("article", null, "qq-log-entry"); row.dataset.topicId = topic.id;
    row.append(node("small", `${t(kindLabels[topic.plan.kind])} / ${t(topicLabels[topic.state])}`), node("p", topic.plan.topic));
    if (topic.state === "active") row.append(button("关闭话题", async () => {
      await invoke("initiative_topic_close", { ...entry.args, id: topic.id, revision: topic.revision });
      await refreshInitiative();
    }));
    topics.append(row);
  }
  const reviews = $("initiative-reviews"); reviews.replaceChildren(node("h2", t("近期决策")));
  if (!snapshot.reviews.length) reviews.append(node("p", t("还没有主动评估记录"), "dim"));
  for (const review of snapshot.reviews) {
    const row = node("article", null, "qq-log-entry"); row.dataset.reviewState = review.state;
    row.append(node("strong", t(reviewLabels[review.state])), node("small", new Date(review.created_at).toLocaleString()));
    if (review.decision?.reason) row.append(node("p", review.decision.reason));
    if (review.error) row.append(node("p", review.error, "dim"));
    reviews.append(row);
  }
}
function renderForm(entry) {
  const form = node("form", null, "qq-form"); form.id = "initiative-settings-form";
  const field = (label, name, type = "number", min = 0, max = null) => {
    const row = node("label", null, "qq-field"), input = node("input"); row.append(node("span", t(label)));
    input.type = type; input.name = name;
    if (type === "checkbox") input.checked = entry.settings[name];
    else { input.value = entry.settings[name] ?? ""; if (type === "number") { input.min = min; input.step = "1"; input.required = true; if (max != null) input.max = max; } }
    input.addEventListener("input", () => {
      entry.settings[name] = type === "checkbox" ? input.checked : type === "number" ? Number(input.value) : input.value.trim() || null;
      entry.dirty = true; save.textContent = t("保存设置");
    });
    row.append(input); form.append(row); return input;
  };
  field("允许主动搭话", "enabled", "checkbox");
  field("空闲多久后考虑（分钟）", "idle_minutes");
  field("评估间隔（分钟）", "review_minutes", "number", 1);
  field("每日消息上限", "daily_messages", "number", 1);
  field("对方未回复时最多发送", "unanswered_messages", "number", 1);
  if (entry.snapshot.target.identity.channel === "qq_group") form.append(node("p", t("安静时段沿用当前群聊规则"), "dim"));
  else { field("安静时段开始", "quiet_start", "number", 0, 23); field("安静时段结束", "quiet_end", "number", 0, 23); }
  field("评估模型", "review_model", "text").placeholder = t("跟随当前会话模型");
  const actions = node("div", null, "qq-toolbar"), save = node("button", t(entry.snapshot.target.state === "ready" ? "保存设置" : "保存并恢复"), "primary"); save.type = "submit";
  actions.append(save, button("重新载入设置", () => refreshInitiative({ reload: true }))); form.append(actions);
  form.addEventListener("submit", event => {
    event.preventDefault(); save.disabled = true;
    const settings = structuredClone(entry.settings), revision = entry.revision;
    void invoke("initiative_settings_save", { ...entry.args, revision, settings }).then(snapshot => {
      // A save can complete after navigation. Update only the draft it belongs to.
      entry.snapshot = snapshot; entry.revision = snapshot.target.revision;
      entry.dirty = JSON.stringify(settings) !== JSON.stringify(entry.settings);
      if (key() === $("initiative-form").dataset.target && shown === entry) { save.textContent = t(entry.dirty ? "保存设置" : "已保存"); render(key(), entry); }
    }).catch(error => toastError(String(error))).finally(() => { save.disabled = false; });
  });
  $("initiative-form").replaceChildren(form);
}
defer(() => {
  $("initiative-refresh").addEventListener("click", () => { void refreshInitiative().catch(error => toastError(String(error))); });
  for (const event of ["kz:social-management", "kz:conversation-selected", "kz:language"]) document.addEventListener(event, () => { void refreshInitiative().catch(error => toastError(String(error))); });
  on("kz:initiative", event => { const entry = drafts.get(key()); if (!event.payload.sessionId || event.payload.sessionId === entry?.snapshot.target.identity.session_id) void refreshInitiative().catch(error => toastError(String(error))); });
});
