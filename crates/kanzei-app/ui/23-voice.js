import { $, defer, invoke, listen } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeSessionId, activeProcessId, currentProject, sessionStates } from "./03-shell.js";
import { sendText } from "./08-compose-runtime.js";
import { ocVoiceSignal } from "./22-neural-flow.js";
import { VoiceConversation, microphoneFor } from "./23-voice-controller.js";

export let voiceConversation = null;

defer(() => {
  const button = $("voice-toggle");
  if (!button) return;
  const panel = $("voice-panel"); const status = $("voice-status"); const caption = $("voice-caption");
  const stage = $("voice-stage");
  const view = $("view-chat");
  const labels = {off:"语音已关闭", connecting:"正在连接语音服务…", listening:"正在听，你可以说话", hearing:"正在聆听…", recognizing:"正在识别…", thinking:"等待回复…", speaking:"正在说话…", error:"语音暂不可用"};
  const friendly = {voice_no_session:"请先选择一个会话", voice_service_unavailable:"语音服务尚未就绪", voice_microphone_ended:"麦克风已断开", voice_stop_pending:"上一轮尚未停止，请稍后重试", voice_queue_full:"播报队列已满，请查看文字回复"};
  let checked = false;
  const controller = new VoiceConversation({
    invoke, Channel:window.__TAURI__.core.Channel,
    getTarget:() => ({sessionId:activeSessionId, processId:activeProcessId, project:currentProject, running:Boolean(sessionStates.get(activeSessionId)?.running)}),
    sendText,
    stopReply:target => invoke("stop_run", {projectDir:target.project, processId:target.processId}),
    createContext:() => new (window.AudioContext || window.webkitAudioContext)(),
    createMicrophone:microphoneFor,
    onState:(state, detail = "") => {
      const active = !["off", "error"].includes(state);
      button.setAttribute("aria-pressed", String(active)); button.dataset.i18nKey = active ? "结束语音" : "语音";
      button.textContent = t(button.dataset.i18nKey);
      panel.classList.toggle("hidden", state === "off"); panel.dataset.voiceState = state;
      view.classList.toggle("voice-mode", active);
      view.dataset.voiceState = state;
      stage?.classList.toggle("hidden", !active);
      if (!active) { view.classList.remove("voice-transcript"); $("voice-history-toggle")?.setAttribute("aria-pressed", "false"); }
      $("voice-interrupt").disabled = !active;
      status.textContent = `${t(labels[state] || labels.error)}${detail ? ` · ${t(friendly[detail] || detail)}` : ""}`;
      if ($("voice-stage-status")) $("voice-stage-status").textContent = t(labels[state] || labels.error);
      if (active && !$("voice-live-caption")?.textContent) $("voice-live-caption").textContent = t("我在，你可以开始说话。");
      document.dispatchEvent(new CustomEvent("kz:voice-layout", { detail: { active } }));
    },
    onCaption:(text, role) => {
      caption.textContent = text; caption.dataset.role = role;
      if ($("voice-live-caption")) $("voice-live-caption").textContent = text || t("我在，你可以开始说话。");
      if ($("voice-speaker")) $("voice-speaker").textContent = role === "user" ? t("你") : "kanzei";
    },
    onLevel:value => {
      const level = Math.min(1, value * 12);
      $("voice-level").value = level;
      view.style.setProperty("--voice-level", String(level));
    },
    onSignal:(sessionId, phase, level) => ocVoiceSignal?.(sessionId, phase, level),
  });
  voiceConversation = controller;
  button.addEventListener("click", async () => {
    if (controller.enabled || controller.starting) { controller.stop(); return; }
    caption.textContent = "";
    if ($("voice-live-caption")) $("voice-live-caption").textContent = "";
    try { await controller.start(); } catch (error) { controller.fail(error); }
    if (!checked) {
      checked = true;
      try {
        const settings = await invoke("voice_settings_get");
        if (settings) { $("voice-port").value = settings.port; $("voice-language").value = settings.language; }
      } catch (error) { status.textContent = String(error); }
    }
  });
  $("voice-interrupt").addEventListener("click", () => controller.interrupt(true));
  $("voice-exit")?.addEventListener("click", () => { controller.stop(); button.focus(); });
  $("voice-history-toggle")?.addEventListener("click", event => {
    const visible = view.classList.toggle("voice-transcript");
    event.currentTarget.setAttribute("aria-pressed", String(visible));
  });
  document.addEventListener("kz:view-changed", event => {
    if (event.detail?.view !== "chat" && (controller.enabled || controller.starting)) controller.stop();
  });
  $("voice-check").addEventListener("click", async () => {
    controller.stop(); panel.classList.remove("hidden");
    try {
      const port = Number($("voice-port").value);
      if (!Number.isInteger(port) || port < 1024 || port > 65535) throw new Error(t("端口须在 1024–65535 之间"));
      await invoke("voice_settings_set", {settings:{port, language:$("voice-language").value}});
      const health = await invoke("voice_status");
      status.textContent = health?.ready ? t("语音服务已就绪，点击语音开始") : `${t("语音服务尚未就绪")} · ${health?.detail || ""}`;
    } catch (error) { status.textContent = String(error); }
  });
  const subscriptions = ["kz:turn", "kz:text", "kz:done", "kz:error", "kz:stopped"].map(name =>
    listen(name, event => controller.handle(name, event.payload || {})));
  Promise.all(subscriptions).catch(error => controller.fail(error));
  // Voice belongs to one visible conversation, including in-flight ASR results.
  const watch = setInterval(() => {
    if ((controller.enabled || controller.starting) && (controller.target?.sessionId !== activeSessionId || controller.target?.project !== currentProject || !$("view-chat").classList.contains("active"))) controller.stop();
  }, 150);
  document.addEventListener("visibilitychange", () => { if (document.hidden) controller.stop(); });
  $("stop").addEventListener("click", () => { if (controller.enabled) controller.interrupt(false); });
  window.addEventListener("beforeunload", () => {
    clearInterval(watch); controller.stop();
    subscriptions.forEach(subscription => { void subscription.then(unlisten => unlisten()).catch(() => {}); });
  });
});
