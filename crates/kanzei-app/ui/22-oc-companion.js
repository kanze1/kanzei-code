// OC 的身体与光纹共享坐标和呼吸变换；透明图保留原始 alpha。
// 这些路径对齐 oc-pixel-v3.png 的脸颊与颈部纹路，替图时需一起校准。
export function ocCompanionMarkup() {
  const routes = [
    "M80 576H192V416H384V576H584",
    "M944 432H880V576H584",
    "M944 1088H848V976H664V872",
  ];
  return `<div class="oc-figure"><div class="oc-body">`
    + `<svg class="oc-network" viewBox="0 0 1024 1536" fill="none" aria-hidden="true">`
    + routes.map((path) => `<path class="oc-route" d="${path}"/><path class="oc-signal" pathLength="100" d="${path}"/>`).join("")
    + `<g class="oc-nodes"><rect x="74" y="570" width="12" height="12"/><rect x="938" y="426" width="12" height="12"/><rect x="938" y="1082" width="12" height="12"/></g></svg>`
    + `<img class="oc-portrait" src="./assets/oc-pixel-v3.png" alt="" width="1024" height="1536" decoding="async" draggable="false"/>`
    + `<svg class="oc-mark" viewBox="0 0 1024 1536" fill="none" aria-hidden="true">`
    + `<g class="oc-cheek"><path d="M584 558V600M566 580H600"/></g>`
    + `<g class="oc-neck"><path d="M658 860H672V866H678V880H672V886H658V880H652V866H658ZM664 886V912H672V940"/></g></svg>`
    + `</div></div>`;
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
  root.querySelectorAll(".empty-art, #oc-companion").forEach((host) => {
    if (!host.querySelector(".oc-figure")) host.innerHTML = ocCompanionMarkup();
  });
  function sync() {
    const paused = document.hidden || !root.closest(".view")?.classList.contains("active");
    const pauseValue = paused ? "true" : "false";
    if (root.dataset.ocPaused !== pauseValue) root.dataset.ocPaused = pauseValue;
    if (paused) return;
    const state = store.current();
    if (root.dataset.ocState !== state) root.dataset.ocState = state;
  }
  sync();
  // 状态投影只做低频只读同步，覆盖切会话、后台终态和轮询恢复。
  const timer = setInterval(sync, 250);
  document.addEventListener("visibilitychange", sync);
  return {
    emit(type, detail) { store.emit(type, detail); sync(); },
    destroy() {
      clearInterval(timer);
      document.removeEventListener("visibilitychange", sync);
    },
  };
}
