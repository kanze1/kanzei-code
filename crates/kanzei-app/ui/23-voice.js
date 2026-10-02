import { $, defer, invoke, listen } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeSessionId, activeProcessId, currentProject, sessionStates } from "./03-shell.js";
import { sendText } from "./08-compose-runtime.js";
import { visualVoiceSignal } from "./22-neural-flow.js";
import { VoiceConversation, microphoneFor } from "./23-voice-controller.js";
import { VOICE_STATUS, voicePresenceLine } from "./23-voice-copy.js";

export let voiceConversation = null;

defer(() => {
  const button = $("voice-toggle");
  if (!button) return;
  const panel = $("voice-panel"); const status = $("voice-status"); const caption = $("voice-caption");
  const stage = $("voice-stage");
  const view = $("view-chat");
  const labels = VOICE_STATUS;
  const friendly = {voice_no_session:"请先选择一个对话", voice_service_unavailable:"语音服务尚未就绪", voice_microphone_ended:"麦克风已断开", voice_stop_pending:"上一轮尚未停止，请稍后重试", voice_queue_full:"播报队列已满，请查看文字回复"};
  const dismiss = $("voice-dismiss");
  let settingsFlight = null;
  let currentState = "off", currentCaption = "", watch = 0;
  function refreshPresence() {
    const line = $("voice-live-caption");
    if (line) line.textContent = currentCaption || t(voicePresenceLine(currentState));
  }
  const controller = new VoiceConversation({
    invoke, Channel:window.__TAURI__.core.Channel,
    getTarget:() => ({sessionId:activeSessionId, processId:activeProcessId, project:currentProject, running:Boolean(sessionStates.get(activeSessionId)?.running)}),
    sendText,
    stopReply:target => invoke("stop_run", {projectDir:target.project, processId:target.processId}),
    createContext:() => new (window.AudioContext || window.webkitAudioContext)(),
    createMicrophone:microphoneFor,
    onState:(state, detail = "") => {
      currentState = state;
      const active = !["off", "error"].includes(state);
      if (active && !watch) watch = setInterval(watchOwnership, 150);
      if (!active && watch) { clearInterval(watch); watch = 0; }
      // UI2-0926 #11:语音键是麦克风图标按钮,文字不能再写进 textContent(会冲掉图标);名字走 title/aria-label,
      // 并写回 data-i18n-*,切语言时由 applyDataI18nKeys 按它重算。
      const voiceKey = active ? "结束语音" : "语音";
      button.setAttribute("aria-pressed", String(active));
      button.dataset.i18nTitle = button.dataset.i18nAriaLabel = voiceKey;
      button.title = t(voiceKey);
      button.setAttribute("aria-label", t(voiceKey));
      panel.classList.toggle("hidden", state === "off"); panel.dataset.voiceState = state;
      view.classList.toggle("voice-mode", active);
      view.dataset.voiceState = state;
      stage?.classList.toggle("hidden", !active);
      if (!active) { view.classList.remove("voice-transcript"); $("voice-history-toggle")?.setAttribute("aria-pressed", "false"); }
      $("voice-interrupt").disabled = !active;
      // UX-116:出错(或「保存并启动」后留着的)面板不再永久占着输入区上方——非进行中态给一个关闭键。
      dismiss?.classList.toggle("hidden", active);
      status.textContent = `${t(labels[state] || labels.error)}${detail ? ` · ${t(friendly[detail] || detail)}` : ""}`;
      if ($("voice-stage-status")) $("voice-stage-status").textContent = t(labels[state] || labels.error);
      refreshPresence();
      document.dispatchEvent(new CustomEvent("kz:voice-layout", { detail: { active } }));
    },
    onCaption:(text, role) => {
      currentCaption = text;
      caption.textContent = text; caption.dataset.role = role;
      refreshPresence();
      if ($("voice-speaker")) $("voice-speaker").textContent = role === "user" ? t("你") : "kanzei";
    },
    onLevel:value => {
      const level = Math.min(1, value * 12);
      $("voice-level").value = level;
      view.style.setProperty("--voice-level", String(level));
    },
    onSignal:(sessionId, phase, level) => visualVoiceSignal?.(sessionId, phase, level),
  });
  voiceConversation = controller;
  function loadSettings() {
    if (!settingsFlight) settingsFlight = invoke("voice_settings_get").then(settings => {
      if (settings) { $("voice-port").value = settings.port; $("voice-language").value = settings.language; }
    }).catch(error => { settingsFlight = null; throw error; });
    return settingsFlight;
  }
  button.addEventListener("click", async () => {
    if (controller.enabled || controller.starting) { controller.stop(); return; }
    caption.textContent = "";
    currentCaption = "";
    if ($("voice-live-caption")) $("voice-live-caption").textContent = "";
    try { await loadSettings(); await controller.start(); } catch (error) { controller.fail(error); }
  });
  $("voice-interrupt").addEventListener("click", () => controller.interrupt(true));
  $("voice-exit")?.addEventListener("click", () => { controller.stop(); button.focus(); });
  dismiss?.addEventListener("click", () => { controller.stop(); button.focus(); });
  $("voice-history-toggle")?.addEventListener("click", event => {
    const visible = view.classList.toggle("voice-transcript");
    event.currentTarget.setAttribute("aria-pressed", String(visible));
  });
  document.addEventListener("kz:view-changed", event => {
    if (event.detail?.view !== "chat" && (controller.enabled || controller.starting)) controller.stop();
  });
  $("voice-check").addEventListener("click", async event => {
    const check = event.currentTarget;
    // UX-116:反馈同时写进设置弹层——状态行就在弹层底下,原来只写那一处,结果被自己的弹层盖住。
    const report = text => { status.textContent = text; $("voice-check-result").textContent = text; };
    check.disabled = true;
    controller.stop(); panel.classList.remove("hidden");
    report(t("正在准备语音服务…"));
    try {
      await loadSettings();
      const port = Number($("voice-port").value);
      if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error(t("端口须在 1024–65535 之间"));
      await invoke("voice_settings_set", {settings:{port, language:$("voice-language").value}});
      const health = await invoke("voice_start");
      report(health?.ready ? t("语音服务已就绪，点击语音开始") : `${t("语音服务尚未就绪")} · ${health?.detail || ""}`);
    } catch (error) { report(String(error)); }
    finally { check.disabled = false; }
  });
  const subscriptions = ["kz:turn", "kz:text", "kz:done", "kz:error", "kz:stopped"].map(name =>
    listen(name, event => controller.handle(name, event.payload || {})));
  Promise.all(subscriptions).catch(error => controller.fail(error));
  // Voice belongs to one visible conversation, including in-flight ASR results.
  function watchOwnership() {
    if ((controller.enabled || controller.starting) && (controller.target?.sessionId !== activeSessionId || controller.target?.project !== currentProject || !$("view-chat").classList.contains("active"))) controller.stop();
  }
  document.addEventListener("visibilitychange", () => { if (document.hidden) controller.stop(); });
  $("stop").addEventListener("click", () => { if (controller.enabled) controller.interrupt(false); });
  window.addEventListener("beforeunload", () => {
    clearInterval(watch); controller.stop();
    subscriptions.forEach(subscription => { void subscription.then(unlisten => unlisten()).catch(() => {}); });
  });
});
