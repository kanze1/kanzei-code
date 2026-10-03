import { $ } from "./01-core.js";
import { t } from "./02-i18n.js";
import { installDragHandle } from "./00-frame.js";
import { attachments, setAttachments, activeProcessId, currentProject } from "./03-shell.js";
import { renderAttachments } from "./08-compose-runtime.js";
import { button, node } from "./25-softwire-view.js";
import { targetKey, safeRestore, sameProject } from "./25-softwire-model.js";
import { fillTemplate } from "./04-structured-parse.js";
import { acknowledge_composer_draft } from "./03-workspaces.js";

// Move the existing editor; never clone it or recreate it while events arrive.
export function createComposer(actions) {
  const dock = $("composer"), input = $("prompt"), send = $("send");
  const origin = document.createComment("shared composer home");
  dock.before(origin);
  const sendOrigin = document.createComment("shared send home"); send.before(sendOrigin);
  // 模型芯片与「自动放行」开关随输入框一起搬进概览输入卡(原「⚙ 设置」弹出的任务设置菜单已撤,里面只剩自动放行一行)。
  const nativeControls = ["model-picker-group", "auto-allow-wrap"].map(id => {
    const element = $(id), anchor = document.createComment("native " + id);
    element.before(anchor); return { element, anchor };
  });
  let raw = ""; try { raw = localStorage.getItem("kz-softwire-drafts"); } catch {}
  const restored = safeRestore(raw);
  const drafts = new Map(restored.drafts), files = new Map();
  let target = null, active = false, nativeDraft = null, floating = false, drag = null;
  const nativeEdited = new Set();
  const nativeScope = () => JSON.stringify([currentProject, activeProcessId]);
  input.addEventListener("input", () => { if (!active) nativeEdited.add(nativeScope()); });
  let position = restored.position || { x: 0, y: 0 };
  let timer, storageFailed = false, preferredFloating = Boolean(restored.floating);
  const head = node("div", null, "sw-composer-head");
  const handle = button("⠿", () => {}, "sw-drag");
  handle.setAttribute("aria-label", t("拖动对话框；悬浮时可用方向键移动"));
  const recipient = node("strong", "", "sw-recipient");
  const status = node("span", "", "sw-send-state");
  status.setAttribute("role", "status");
  const float = button("↗", () => setFloating(!floating), "sw-float");
  float.setAttribute("aria-label", t("悬浮对话框"));
  const runSlot = node("div", null, "sw-composer-run"); runSlot.id = "sw-run-slot";
  head.append(handle, recipient, status, runSlot, float);
  const controls = node("div", null, "sw-composer-controls");
  const add = button("+", () => $("attach").click(), "sw-add");
  add.setAttribute("aria-label", t("添加附件"));
  controls.append(add);
  const queue = node("span", "", "sw-delivery");
  const stop = button("■", () => actions.stop(), "sw-stop");
  stop.setAttribute("aria-label", t("停止当前对话"));
  controls.append(queue, node("span", "", "sw-spacer"), stop);
  const completed = node("div", null, "sw-reply-complete");
  completed.append(node("span", "✓ " + t("已送达")), button(t("返回工作"), actions.back));
  const unavailable = node("div", "", "sw-compose-unavailable");
  dock.prepend(head);
  dock.append(controls, completed, unavailable);

  function persist() {
    if (target && active) { drafts.set(targetKey(target), input.value); files.set(targetKey(target), [...attachments]); }
    try { localStorage.setItem("kz-softwire-drafts", JSON.stringify({ version: 1, drafts: [...drafts].slice(-100), position, floating: preferredFloating })); }
    catch { storageFailed = true; }
  }
  function change(next) {
    if (targetKey(next) !== (target ? targetKey(target) : "")) {
      rememberNativeDraft();
      persist(); target = { ...next };
      input.value = drafts.get(targetKey(target)) || "";
      setAttachments(files.get(targetKey(target)) || []); renderAttachments();
      input.style.height = "";
    } else target = { ...next };
    sync();
  }
  function sync() {
    if (!active || !target) return;
    recipient.textContent = target.label;
    recipient.title = target.label; // 只给名字:项目路径与会话 id 不外露(UX-127)
    status.textContent = storageFailed ? t("刷新恢复不可用") : t(target.sendStatus || "");
    queue.textContent = target.interactionId ? t("回复原事项") : target.running ? t("下一轮送达") : t("当前工作");
    // 静态标签是创建时写的:每次同步重写一遍,切语言后下一次同步即跟上。
    handle.setAttribute("aria-label", t("拖动对话框；悬浮时可用方向键移动"));
    add.setAttribute("aria-label", t("添加附件"));
    stop.setAttribute("aria-label", t("停止当前对话"));
    completed.lastChild.textContent = t("返回工作");
    stop.hidden = !target.running || Boolean(target.interactionId);
    runSlot.hidden = Boolean(target.interactionId) || Boolean(target.child);
    add.hidden = Boolean(target.interactionId);
    send.disabled = Boolean(target.busy || target.readOnly || target.completed);
    send.setAttribute("aria-label", fillTemplate(t("发给 {name}"), { name: target.label }));
    send.title = fillTemplate(t("发给 {name}"), { name: target.label }) + " · Enter";
    input.disabled = Boolean(target.readOnly || target.completed);
    input.placeholder = target.placeholder || (target.interactionId ? t("回复这条事项…") : t("补充要求，或安排下一步…"));
    dock.dataset.swCompleted = String(Boolean(target.completed));
    dock.dataset.swReadonly = String(Boolean(target.readOnly));
    unavailable.textContent = target.readOnly || "";
    completed.firstChild.textContent = "✓ " + t(target.sendStatus || "已送达");
    for (const { element, anchor } of nativeControls) {
      if (!target.interactionId && sameProject(target.project, currentProject) && target.processId === activeProcessId) {
        controls.insertBefore(element, queue);
      } else anchor.after(element);
    }
  }
  function mount(slot, next) {
    const entering = !active;
    if (!active) {
      nativeDraft = { project: currentProject, processId: activeProcessId, text: input.value, attachments: [...attachments], height: input.style.height };
      active = true;
      document.body.dataset.softwireComposer = "true";
      dock.classList.add("sw-composer");
      slot.append(dock); controls.append(send);
      target = null;
      if (next.module === "main" && !next.interactionId && sameProject(next.project, currentProject) && next.processId === activeProcessId) {
        if (nativeDraft.text || nativeDraft.attachments.length || nativeEdited.has(nativeScope()) || !drafts.has(targetKey(next))) {
          drafts.set(targetKey(next), nativeDraft.text); files.set(targetKey(next), nativeDraft.attachments);
        }
        nativeEdited.delete(nativeScope());
      }
    } else if (dock.parentElement !== slot) slot.append(dock);
    change(next);
    if (entering) setFloating(preferredFloating, false);
  }
  function rememberNativeDraft() {
    // Scope changes may already have changed the global receiver. Return only
    // the draft belonging to the native editor that we borrowed on entry.
    if (target?.module === "main" && !target.interactionId && !target.child && sameProject(target.project, nativeDraft?.project) && target.processId === nativeDraft?.processId) {
      nativeDraft = { ...nativeDraft, text: input.value, attachments: [...attachments], height: input.style.height };
    }
  }
  function leave() {
    if (!active) return;
    rememberNativeDraft();
    persist(); setFloating(false, false);
    active = false; target = null;
    document.body.dataset.softwireComposer = "false";
    dock.classList.remove("sw-composer");
    origin.after(dock); sendOrigin.after(send);
    for (const { element, anchor } of nativeControls) anchor.after(element);
    input.disabled = false;
    input.value = nativeDraft?.text || "";
    input.style.height = nativeDraft?.height || "";
    setAttachments(nativeDraft?.attachments || []); renderAttachments();
    // Native sends clear the editor programmatically. Once this editor has
    // received a draft, an empty value is authoritative too (not a cold reload).
    if (nativeDraft?.project && nativeDraft?.processId) nativeEdited.add(JSON.stringify([nativeDraft.project, nativeDraft.processId]));
    send.disabled = false;
  }
  function clamp() {
    const rect = dock.getBoundingClientRect();
    position.x = Math.max(8, Math.min(position.x, window.innerWidth - rect.width - 8));
    position.y = Math.max(8, Math.min(position.y, window.innerHeight - rect.height - 8));
    dock.style.setProperty("--sw-x", position.x + "px");
    dock.style.setProperty("--sw-y", position.y + "px");
  }
  function setFloating(value, remember = true) {
    if (!active && value) return;
    value = Boolean(value && window.innerWidth >= 760);
    const rect = dock.getBoundingClientRect();
    if (value && !floating && !position.x && !position.y) position = { x: rect.left, y: rect.top };
    floating = value;
    if (remember) preferredFloating = value;
    dock.classList.toggle("sw-floating", floating);
    float.setAttribute("aria-label", floating ? t("停靠对话框") : t("悬浮对话框"));
    float.textContent = floating ? "↙" : "↗";
    dock.parentElement?.classList.toggle("sw-floating-slot", floating);
    if (floating) { dock.parentElement.style.setProperty("--sw-dock-height", rect.height + "px"); clamp(); }
    persist();
  }
  installDragHandle(handle, {
    enabled: () => active && window.innerWidth >= 760,
    start: event => {
      const rect = dock.getBoundingClientRect();
      position = { x: rect.left, y: rect.top }; setFloating(true);
      drag = { x: event.clientX - rect.left, y: event.clientY - rect.top };
    },
    move: event => { position = { x: event.clientX - drag.x, y: event.clientY - drag.y }; clamp(); },
    end: () => { drag = null; persist(); },
  });
  handle.addEventListener("keydown", event => {
    if (!floating || !["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"].includes(event.key)) return;
    event.preventDefault();
    const step = event.shiftKey ? 32 : 8;
    position.x += event.key === "ArrowLeft" ? -step : event.key === "ArrowRight" ? step : 0;
    position.y += event.key === "ArrowUp" ? -step : event.key === "ArrowDown" ? step : 0;
    clamp(); persist();
  });
  window.addEventListener("resize", () => { if (window.innerWidth < 760) setFloating(false); else if (floating) clamp(); });
  input.addEventListener("input", () => { if (active) { clearTimeout(timer); timer = setTimeout(persist, 150); actions.changed?.(); } });
  window.addEventListener("pagehide", persist);
  return {
    mount, leave, sync, active: () => active, target: () => target,
    value: () => input.value,
    capture: () => ({ text: input.value, attachments: [...attachments], target: { ...target } }),
    clear(sent) {
      const key = targetKey(sent.target);
      if (sent.target.module === "main" && !sent.target.interactionId && !sent.target.child) {
        if (sameProject(sent.target.project, nativeDraft?.project) && sent.target.processId === nativeDraft?.processId
          && nativeDraft.text === sent.text && JSON.stringify(nativeDraft.attachments) === JSON.stringify(sent.attachments)) {
          nativeDraft = { ...nativeDraft, text: "", attachments: [], height: "" };
        }
        acknowledge_composer_draft(sent.target.project, sent.target.processId, sent);
      }
      if (active && targetKey(target) === key) {
        if (input.value === sent.text && JSON.stringify(attachments) === JSON.stringify(sent.attachments)) {
          input.value = ""; setAttachments([]); renderAttachments();
        }
      } else if (drafts.get(key) === sent.text && JSON.stringify(files.get(key) || []) === JSON.stringify(sent.attachments)) {
        drafts.set(key, ""); files.delete(key);
      }
      persist();
    },
    insert(text) { input.value += (input.value ? "\n" : "") + text; input.focus(); persist(); },
    replace(text) { input.value = text; persist(); },
    rebind(previous, next) {
      persist();
      const before = JSON.parse(targetKey(previous)), after = JSON.parse(targetKey(next));
      let reboundVisible = false;
      for (const [key, text] of [...drafts]) {
        const identity = JSON.parse(key);
        if (identity[0] !== before[0] || identity[1] !== before[1] || identity[2] !== before[2] || identity[3] === "inbox") continue;
        identity[1] = after[1]; identity[2] = after[2];
        const replacement = JSON.stringify(identity);
        drafts.set(replacement, text); if (files.has(key)) files.set(replacement, files.get(key));
        if (active && target && targetKey(target) === replacement) reboundVisible = true;
        if (replacement !== key) { drafts.delete(key); files.delete(key); }
      }
      if (target && target.module !== "inbox" && target.project === previous.project && target.processId === previous.processId && target.sessionId === previous.sessionId)
        target = { ...target, processId: next.processId, sessionId: next.sessionId };
      // Activating an unresolved project can mount its newly created session
      // before this rebind. Restore the migrated draft before persisting again.
      if (reboundVisible) {
        input.value = drafts.get(targetKey(target)) || "";
        setAttachments(files.get(targetKey(target)) || []); renderAttachments();
      }
      persist();
    },
    setFloating,
  };
}

