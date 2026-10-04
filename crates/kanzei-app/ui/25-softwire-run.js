import { $ } from "./01-core.js";
import { t } from "./02-i18n.js";
import { fillTemplate } from "./04-structured-parse.js";
import { openMenu, openPopover, closeSurface } from "./00-surface.js";
import { activeProcessId, sessionState } from "./03-shell.js";
import { lineAgent, currentAutoRounds, autoContinueTimers, awaitingUserSessions } from "./08-auto.js";
import { lineAutoConfig, setLineAutoState, processUpdateQueues } from "./08-compose-runtime.js";
import { node, button, lineLabel } from "./25-softwire-view.js";

// One projection of the existing per-line loop. Preparing a line is explicit user intent.
export function createRunControl({ resolve, changed }) {
  const element = node("div", null, "sw-run-control");
  const mode = button("", () => openMode(), "sw-run-mode");
  const toggle = button("", () => void perform(async line => {
    const config = lineAutoConfig(line.id);
    const pausing = config.enabled && !config.paused;
    await setLineAutoState(line.id, pausing ? { paused: true } : { enabled: true, paused: false, stopAfterRound: false });
  }), "sw-run-toggle");
  toggle.id = "sw-run-toggle";
  const tune = button("⚙", () => void perform(async () => {
    element.append(menu); menu.append(continuePanel);
    openPopover(tune, menu, { type: "menu", placement: "top-end" });
  }), "sw-run-tune");
  tune.setAttribute("aria-label", t("自动推进设置"));
  const off = button("■", () => void perform(line => setLineAutoState(line.id, { enabled: false, paused: false, stopAfterRound: false })), "sw-run-off");
  off.setAttribute("aria-label", t("关闭自动推进")); off.title = t("停止自动推进；当前轮继续完成");
  const status = node("span", null, "sw-run-status"); status.setAttribute("role", "status");
  const controls = node("div", null, "sw-run-buttons"); controls.append(mode, toggle, tune, off);
  element.append(controls, status);
  const menu = $("autorun-menu"), origin = document.createComment("native autorun menu"); menu.before(origin);
  const continuePanel = $("continue-panel"), continueOrigin = document.createComment("native continue editor"); continuePanel.before(continueOrigin);
  let selected = null, busy = false, failure = "";
  function close() {
    closeSurface(menu); origin.after(menu); continueOrigin.after(continuePanel);
  }
  function sync(line) {
    if (line?.project !== selected?.project || line?.id !== selected?.id) { close(); failure = ""; }
    selected = line;
    // 静态标签是创建时写的:每次同步重写,切语言后立刻跟上。
    tune.setAttribute("aria-label", t("自动推进设置"));
    off.setAttribute("aria-label", t("关闭自动推进")); off.title = t("停止自动推进；当前轮继续完成");
    const config = lineAutoConfig(line?.id), research = line?.profile === "research";
    const phase = line?.session_id ? sessionState(line.session_id).phase : "idle";
    const running = line?.running || ["starting", "running", "stopping"].includes(phase);
    mode.textContent = research ? t("研究") : !line?.id ? t("沿用设置") + " ▾" : lineAgent(line).agent === "dev" ? t("自主推进") + " ▾" : t("结伴开发") + " ▾";
    const pausing = config.enabled && !config.paused;
    toggle.textContent = busy ? t("处理中…") : pausing ? "Ⅱ " + t("暂停自动推进") : config.paused ? "▶ " + t("恢复自动推进") : "▶ " + t("启动自动推进");
    toggle.dataset.active = String(pausing);
    toggle.title = pausing ? t("当前轮继续完成，暂停后续轮次") : fillTemplate(t("连续推进 {name} 的工作队列"), { name: lineLabel(line?.label || "主对话") });
    for (const control of [mode, toggle, tune, off]) control.disabled = busy || !line?.project || research;
    off.hidden = !config.enabled;
    const waiting = awaitingUserSessions.has(line?.session_id);
    status.textContent = failure || (research ? t("研究模式不使用自动推进") : config.paused ? running ? t("已暂停 · 本轮继续") : t("已暂停")
      : waiting ? t("等待你的回复") : config.stopAfterRound ? t("本轮后停")
        : autoContinueTimers.has(line?.session_id) ? t("即将开始下一轮") : config.enabled ? fillTemplate(t("{n} 轮 · {state}"), { n: currentAutoRounds(line?.session_id), state: running ? t("推进中") : t("等待运行状态") }) : "");
    status.dataset.error = String(Boolean(failure));
  }
  async function perform(action) {
    if (busy || !selected) return;
    const destination = { ...selected };
    let owner = destination;
    busy = true; failure = ""; sync(selected);
    try {
      const line = await resolve(destination);
      if (line) { owner = { ...line, project: destination.project }; await action(line); }
    } catch (error) {
      if (selected?.project === owner.project && selected?.id === owner.id && selected?.session_id === owner.session_id) failure = String(error);
    }
    finally { busy = false; if (selected) sync(selected); changed(); }
  }
  function openMode() {
    openMenu(mode, [["dev-pair", t("结伴开发")], ["dev-auto", t("自主推进")]].map(([value, label]) => ({
      label, onSelect: () => void perform(async line => {
        if (line.id !== activeProcessId) return;
        $("profile-select").value = value;
        $("profile-select").dispatchEvent(new Event("change"));
        await processUpdateQueues.get(line.id);
      }),
    })), { placement: "top-end", label: t("推进方式") });
  }
  return { element, sync, close };
}
