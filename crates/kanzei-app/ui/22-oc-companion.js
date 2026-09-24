import { initOcPerformance, ocPerformanceMarkup } from "./22-oc-performance.js";

export function initOcSpriteMotion(root) {
  return initOcPerformance(root);
}

export function ocCompanionMarkup() {
  return ocPerformanceMarkup();
}

// 只缓存表现细节，不写入运行状态。会话真源的终态始终优先于动画缓存。
export function createOcStateStore({ getSessionId, getRuntime, now = Date.now }) {
  const sessions = new Map();
  const progress = new Set(["reasoning_active", "assistant_streaming", "tool_started", "tool_progressed", "tool_completed", "context_compacted"]);

  function emit(type, detail = {}) {
    const id = detail.session_id;
    if (!id || (type !== "run_started" && !progress.has(type) && !["run_completed", "run_failed", "run_stopped"].includes(type))) return;
    const runtime = getRuntime(id);
    if (progress.has(type) && (runtime?.converged || ["stopping", "stopped", "failed"].includes(runtime?.phase))) return;
    const previous = sessions.get(id);
    const entry = previous || { state: "idle", tools: new Set(), until: 0 };
    if (type === "run_started") {
      entry.state = "thinking";
      entry.tools.clear();
      entry.until = 0;
    } else if (type === "tool_started" || type === "tool_progressed") {
      entry.tools.add(detail.tool_call_id || "tool");
      entry.state = "executing";
    } else if (type === "tool_completed") {
      entry.tools.delete(detail.tool_call_id || "tool");
      entry.state = entry.tools.size ? "executing" : "thinking";
    } else if (type === "reasoning_active" || type === "context_compacted") {
      entry.state = entry.tools.size ? "executing" : "thinking";
    } else if (type === "assistant_streaming") {
      entry.state = entry.tools.size ? "executing" : "replying";
    } else {
      entry.tools.clear();
      entry.state = type === "run_completed" ? "complete" : type === "run_failed" ? "blocked" : "idle";
      entry.until = type === "run_completed" ? now() + 1800 : 0;
    }
    sessions.delete(id);
    sessions.set(id, entry);
    // 表现缓存有界，久未打开的会话仍可从运行真源恢复。
    if (sessions.size > 128) sessions.delete(sessions.keys().next().value);
  }

  function current() {
    const id = getSessionId();
    if (!id) return "idle";
    const runtime = getRuntime(id);
    const entry = sessions.get(id);
    if (["stopping", "stopped"].includes(runtime?.phase)) return "idle";
    if (runtime?.phase === "failed") return "blocked";
    if (entry?.state === "complete" && now() < entry.until) return "complete";
    if (runtime?.converged || ["idle", "auto_pending"].includes(runtime?.phase)) {
      sessions.delete(id);
      return "idle";
    }
    if (runtime?.running) {
      return entry && ["thinking", "executing", "replying", "blocked"].includes(entry.state) ? entry.state : "thinking";
    }
    return entry?.state === "complete" ? "idle" : entry?.state || "idle";
  }

  return { emit, current };
}

export function initOcCompanion(root, options) {
  if (!root) return null;
  const store = createOcStateStore(options);
  // 初始 HTML 与动态空态复用同一份人物/光纹结构。
  root.querySelectorAll(".empty-art, #oc-companion, .voice-art").forEach((host) => {
    if (!host.querySelector(".oc-figure")) host.innerHTML = ocCompanionMarkup();
  });
  const motion = initOcSpriteMotion(root);
  let voice = null;
  let lastSessionId = options.getSessionId();
  function sync() {
    const sessionId = options.getSessionId();
    if (sessionId !== lastSessionId) { motion.reset(); lastSessionId = sessionId; }
    const paused = document.hidden || !root.closest(".view")?.classList.contains("active");
    const currentVoice = voice?.sessionId === sessionId ? voice : null;
    const stopping = ["stopping", "stopped"].includes(options.getRuntime(sessionId)?.phase);
    motion.setState(stopping ? "interrupted" : currentVoice?.phase === "speaking" ? "replying" : currentVoice?.phase || store.current(), paused);
    motion.setSpeaking(currentVoice?.phase === "speaking");
    motion.setMouthLevel(currentVoice?.level || 0);
  }
  sync();
  // 状态投影只做低频只读同步，覆盖切会话、后台终态和轮询恢复。
  const timer = setInterval(sync, 250);
  document.addEventListener("visibilitychange", sync);
  return {
    emit(type, detail) { store.emit(type, detail); sync(); },
    voice(sessionId, phase, level = 0) { voice = phase ? { sessionId, phase, level } : null; sync(); },
    destroy() {
      clearInterval(timer);
      motion.destroy();
      document.removeEventListener("visibilitychange", sync);
    },
  };
}
