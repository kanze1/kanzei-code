// kanzei 桌面端前端逻辑(静态,无构建步骤)。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

// R-126:自加载起累积 console 错误与未捕获异常,供 ui_console 工具取样。
// 必须在最前面装:晚一步就漏掉初始化阶段的错误,而那正是最要命的一段。
const uiConsoleLog = [];
const UI_CONSOLE_MAX = 200;
function recordConsole(level, args) {
  if (uiConsoleLog.length >= UI_CONSOLE_MAX) uiConsoleLog.shift();
  uiConsoleLog.push({
    level,
    at: Date.now(),
    text: args.map((a) => (a instanceof Error ? `${a.message}\n${a.stack ?? ""}` : String(a))).join(" "),
  });
}
for (const level of ["error", "warn"]) {
  const original = console[level].bind(console);
  console[level] = (...args) => {
    recordConsole(level, args);
    original(...args);
  };
}
window.addEventListener("error", (event) => {
  recordConsole("uncaught", [event.message, event.filename ? `${event.filename}:${event.lineno}` : ""]);
});
window.addEventListener("unhandledrejection", (event) => {
  recordConsole("unhandled-rejection", [event.reason]);
});

// 事件订阅统一入口:注册失败必须可见(D-005 教训——ACL 拒绝时曾静默失联)。
const SESSION_PROGRESS_EVENTS = new Set([
  "kz:meta", "kz:status", "kz:text", "kz:reasoning",
  "kz:tool-start", "kz:tool-progress", "kz:task-progress", "kz:step",
]);
// 这些是全局辅助事件,不是某一条运行会话的进度投影:权限询问由自身
// 的队列归属,快速模型安装与 UI 探针也没有运行 session。
// D-381:kz:annotate-progress 是项目级批处理进度(文件标注),同样没有 session 归属。
// 它此前用裸 listen 绕过本函数,于是「没有 sessionId 就丢弃」这条纪律只覆盖了一半的
// 订阅——规则写在代码里,但只写在一条路径上。
// R-267:后台会话也要渲染的事件——它们往消息流里写东西,必须进所属会话的 pane。
// 刻意**不含** kz:status/kz:step/kz:meta/kz:task-progress/kz:tool-progress:
// 那几条改的是状态栏、轮次显示、活动面板与工具进度条,都是**全局 UI**,
// 后台会话触发它们才是串线(用户会看到别的线的状态盖在当前线上)。
const BACKGROUND_RENDER_EVENTS = new Set([
  "kz:text",
  "kz:reasoning",
  "kz:tool-start",
  "kz:tool-end",
  "kz:compacted",
]);
const SESSIONLESS_EVENTS = new Set([
  "kz:ask",
  "kz:fast-setup",
  "kz:ui-probe",
  "kz:annotate-progress",
]);
function on(event, handler) {
  listen(event, (eventPayload) => {
    const sessionId = eventPayload.payload?.sessionId;
    // 除了权限询问(它有专门的缺省会话归属逻辑),所有运行事件都必须带 sessionId。
    // 没有身份就不能安全地投影到当前对话,宁可只留下后端持久事实也不能串线。
    if (!SESSIONLESS_EVENTS.has(event) && !sessionId) {
      log(`丢弃无 session_id 的运行事件:${event}`, "warn");
      return;
    }
    // R-086:kz:turn 是每轮开头必发的信号,拿它把会话状态机拨回运行中并解除
    // converged。后端一次运行可以跨多轮(排队输入 promote 后接着跑),轮末的
    // kz:done 之后会话仍在跑,只有这条能把被前一轮 idle 焊住的状态解开。
    // 必须写在下面那句非活动会话 early-return 之前,否则后台会话永远收不到。
    // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
    if (event === "kz:turn" && sessionId) {
      transitionSession(sessionId, "running", {
        converged: false,
        auto_pending: false,
        local_start_pending: false,
        terminal_status: "",
      });
    }
    if (event === "kz:status" && sessionId) {
      const state = sessionState(sessionId);
      state.stage = eventPayload.payload?.stage || state.stage || "空闲";
      state.detail = eventPayload.payload?.detail || "";
    }
    // 运行事件不保证每条线路都先收到 kz:turn:并行线路可能先收到 meta、status
    // 或工具进度。任何带会话身份的实时进度都说明该线路仍在运行,否则左侧线路按钮
    // 会在实际执行时显示「空闲」,直到下一次轮询或下一轮 turn 才被纠正。
    if (sessionId && SESSION_PROGRESS_EVENTS.has(event)) {
      // stopping 是用户已发出的控制意图；晚到的进度不能把停止按钮重新翻回运行态。
      const state = sessionState(sessionId);
      if (state.phase === "stopping") return;
      // 已收敛的终态同理:kz:idle 之后**迟到**的进度事件不得把会话复活。
      //
      // 复活的代价不是「按钮闪一下」而是鞭挞卡死:processRunning() 在未收敛时读
      // `state.running || item.running`,被迟到事件置回 running 之后就再没有东西
      // 能把它翻回来(只有 kz:idle 会收敛,而它已经发过了)。armAutoContinue 于是
      // 每 2 秒重试一次、满 15 次放弃,报「上一轮尚未结束」。实测现场:
      // 13:58:31 运行完成 → 13:59:03 放弃,正好 32 秒 = 首次 2s + 15×2s。
      //
      // 解除收敛的权力只留给 kz:turn(上面第一段处理,且**不在**本事件集合里)——
      // 注释写明它是「每轮开头必发的信号」,新一轮必定经它。代价是:若某一轮的
      // 进度事件早于 kz:turn 到达,线路按钮会短暂显示空闲,直到 kz:turn 或下一次
      // process_list 轮询纠正。拿这点显示延迟换掉一个会卡死自动续跑的状态陷阱。
      if (state.converged) return;
      // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
      transitionSession(sessionId, "running", {
        converged: false,
        auto_pending: false,
        local_start_pending: false,
        terminal_status: "",
      });
    }
    // 事件流是线路状态的实时投影入口。不能等 kz:done/kz:idle 或下一次
    // process_list 轮询，否则工具执行期间线路按钮和 stop 会按轮次滞后。
    if (sessionId && typeof refreshParallelTaskProjection === "function") {
      refreshParallelTaskProjection(sessionId);
    }
    const controlEvent =
      event === "kz:ask" ||
      event === "kz:status" ||
      event === "kz:done" ||
      event === "kz:error" ||
      event === "kz:stopped" ||
      event === "kz:idle";
    // R-267:后台会话的渲染事件不再丢弃,改为渲染进**它自己**的 pane。
    //
    // 丢弃是「全局唯一容器」时代的必然:渲染进去就串线。有了 per-session pane
    // 之后,正确做法是把渲染上下文切到所属会话——切回来时 DOM 已经是最新的,
    // 不需要快照、不需要「本轮完成后自动补齐」那句 notice。
    //
    // 只有**渲染类**事件走这条路(SESSION_PROGRESS_EVENTS + tool-end/compacted);
    // 其余非控制事件仍按原样忽略——它们改的是全局 UI(状态栏、活动面板、待办),
    // 不属于任何一条会话的消息流,后台会话触发它们才是真的串线。
    if (!controlEvent && !SESSIONLESS_EVENTS.has(event) && sessionId !== activeSessionId) {
      if (BACKGROUND_RENDER_EVENTS.has(event)) {
        withSessionRender(sessionId, () => handler(eventPayload));
      }
      return;
    }
    if (controlEvent) {
      // R-086:控制事件先按 sessionId 更新对应会话状态机,再决定是否投影视图——
      // 后台会话的终态也必须收敛,不能只靠 refreshProcesses 间接拉后端
      // (事件丢失时切回会卡在错误运行态)。
      // 只有**会话级**终态才收敛:kz:done 是一轮的终点,kz:error 也可能只是本轮
      // 失败(后端随后仍会发 kz:idle),拿它们收敛会让排队输入的第二轮起全程显示空闲。
      const terminalError = event === "kz:error" && eventPayload.payload?.terminal !== false;
      if (sessionId && (event === "kz:idle" || event === "kz:stopped" || terminalError)) {
        const targetPhase =
          event === "kz:stopped" ? "stopped" : terminalError ? "failed" : sessionState(sessionId).auto_pending ? "auto_pending" : "idle";
        // R-206:状态写入唯一入口 transitionSession,不再手工复刻 6 布尔标志。
        // terminal_status 由 transitionSession 对 stopped/failed 分支折算,无需重复设置。
        transitionSession(sessionId, targetPhase, {
          stage: "空闲",
          detail: "",
        });
        if (typeof refreshParallelTaskProjection === "function") {
          refreshParallelTaskProjection(sessionId);
        }
      }
      // kz:ask 不走路由分支:它必须始终进 handler,按 sessionId 入队
      // (handler 内只在活动会话时弹窗),否则后台 ask 会被丢弃挂死(D-055 根因)。
      if (event !== "kz:ask" && sessionId !== activeSessionId) {
        // 控制事件的 UI 副作用不能串到活动线路，但所属线路的历史与自主推进必须执行。
        if (event === "kz:done" && typeof handleBackgroundSessionDone === "function") {
          handleBackgroundSessionDone(eventPayload.payload);
        }
        if ((event === "kz:stopped" || terminalError) && typeof cancelAutoContinueTimer === "function") {
          cancelAutoContinueTimer(sessionId);
        }
        if (typeof refreshConversationLists === "function") void refreshConversationLists();
        refreshProcesses();
        log(`后台会话控制事件已路由:${event} ${sessionId}`);
        return;
      }
    }
    handler(eventPayload);
  }).catch((err) => {
    log(`事件订阅失败 ${event}: ${err} — 界面将收不到运行事件,请反馈`, "err");
    $("log-panel").classList.remove("hidden");
  });
}

const $ = (id) => document.getElementById(id);
// localStorage 里的 JSON 可能被手改坏;读不出来就当没有,绝不让偏好读取抛异常
// 把整个初始化带崩。
function readJson(key, fallback) {
  try {
    const parsed = JSON.parse(localStorage.getItem(key) || "null");
    return parsed && typeof parsed === "object" ? parsed : fallback;
  } catch {
    return fallback;
  }
}
function writeJson(key, value) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* 配额满等情况:偏好丢失可以接受,不该打断当前操作 */
  }
}

// R-184 P2:子代理角色名 → 确定性强调色(0-4)。活动面板角色色点与主对话折叠组共用,
// 只做确定性映射(同一角色刷新不变),不承诺语义排序;角色名文本是主标识,颜色是辅助
// (design §2.2 不得只靠颜色区分)。
function agentRoleAccent(role) {
  if (!role) return 0;
  let sum = 0;
  for (let i = 0; i < role.length; i += 1) sum = (sum * 31 + role.charCodeAt(i)) >>> 0;
  return (sum % 4) + 1;
}

// R-267:`messages` 只是**滚动容器**;消息本体挂在它下面的 per-session pane 里。
// 这个分工是整个改造能低风险落地的关键——滚动/跟随/复制那几处照旧读 `messages`,
// 一行不用改;只有「往哪儿追加」换成 activePane。
const messages = $("messages");

// ---------- R-267:每会话渲染面 ----------
//
// 改造前:全局唯一容器 + 切走时存一份 innerHTML 字符串(sessionDomCache,上限 30)。
// 于是非活动会话的 kz:text/kz:tool-*/kz:reasoning 只能整条丢掉(渲染进去就串线),
// 切回来把字符串塞回去,中间那段是缺的,只好挂一句「快照截至上次切走时」等轮末补齐。
// 代价还不止缺口:两条运行中的线来回切,每次都是一次多 MB 字符串的 innerHTML 解析。
//
// 改造后:每个会话一个 pane,事件永远渲染进**自己那个** pane,切线只是换显示。
// 缺口消失、切换零重渲染、sessionDomCache 整个不需要了。
const messagePanes = new Map();
/// pane 保活上限。超出后淘汰最久未访问的(仅**非运行中**会话),下次切回按
/// loadConversation 重建——退化成改造前的行为,而不是退回「有缺口」。
/// 上限取小是因为消息本体没有窗口化(批2 前一个会话可到 993 条/1665 个 part),
/// 多个长会话的 pane 常驻会把内存放大。批2 落地后可以调大。
const MESSAGE_PANE_MAX = 4;
let activePane = messages.querySelector(".msg-pane");

function paneFor(sessionId) {
  const key = sessionId || "";
  let pane = messagePanes.get(key);
  if (!pane) {
    // 启动期那个 data-session-id="" 的占位 pane 直接认领给第一个真实会话,
    // 免得首屏白闪一下再换。
    const placeholder = messages.querySelector('.msg-pane[data-session-id=""]');
    pane = placeholder && !messagePanes.has("") ? placeholder : document.createElement("div");
    pane.className = "msg-pane";
    pane.dataset.sessionId = key;
    if (!pane.parentNode) messages.appendChild(pane);
    messagePanes.set(key, pane);
  }
  pane.dataset.usedAt = String(Date.now());
  return pane;
}

/// 切到某个会话的 pane:隐藏旧的、显示新的。**不重建 DOM**。
/// 返回 true = 该 pane 已有内容(切回来即见),false = 新建的空 pane(调用方需要装历史)。
function showPane(sessionId) {
  const pane = paneFor(sessionId);
  if (activePane && activePane !== pane) {
    activePane.classList.add("hidden");
    delete activePane.dataset.active;
  }
  pane.classList.remove("hidden");
  // 当前显示的 pane 打属性标记:隐藏的 pane 仍在 DOM 里,断言/查询要能只看这一个。
  pane.dataset.active = "1";
  activePane = pane;
  evictStalePanes();
  return pane.dataset.hasContent === "1";
}

/// 往当前 pane 追加消息节点。顺手打上「有内容」标志——切回时靠它判断是否需要装载,
/// 而不是去数子节点或找 .empty-state:空状态本身也是子节点,而冒烟的假 DOM 对
/// 类选择器支持有限,判空一旦不准就会把「已有完整内容」误判成「空 pane」再重建一遍。
function appendToPane(node) {
  activePane.dataset.hasContent = "1";
  activePane.appendChild(node);
}
/// 清空当前 pane 并复位「有内容」标志。
function resetPane() {
  activePane.replaceChildren();
  delete activePane.dataset.hasContent;
}

/// 超出上限时淘汰最久未访问的 pane。**运行中的会话永不淘汰**——它正在往里写,
/// 淘汰了就等于把缺口又造回来。
function evictStalePanes() {
  if (messagePanes.size <= MESSAGE_PANE_MAX) return;
  const candidates = [...messagePanes.entries()]
    .filter(([id, pane]) => pane !== activePane && !(typeof sessionLiveNow === "function" && sessionLiveNow(id)))
    .sort((a, b) => Number(a[1].dataset.usedAt || 0) - Number(b[1].dataset.usedAt || 0));
  for (const [id, pane] of candidates) {
    if (messagePanes.size <= MESSAGE_PANE_MAX) break;
    pane.remove();
    messagePanes.delete(id);
    dropSessionStream(id);
  }
}

// ---------- R-267:每会话的流式装配状态 ----------
//
// currentAssistant / currentReasoning / currentReasoningHead 是「正在拼接的那条
// 消息/思考块」,原本是模块级单例(全局共 38 处读写)。后台会话也要渲染之后,
// 它们必须按会话分开,否则两条线的文本增量会拼进同一个气泡。
//
// **不改那 38 处读写**:改成在渲染前把目标会话的状态**存进**全局变量、渲染后
// **取回**。渲染函数照旧读写全局,语义不变;代价只是一次存取。这比把 sessionId
// 一路穿进 addMessage/appendAssistant/chatToolStart 要小一个数量级,也不会在
// 调用链上留下几十个「这个 id 是哪来的」。
const sessionStreams = new Map();
function streamStateFor(sessionId) {
  const key = sessionId || "";
  let state = sessionStreams.get(key);
  if (!state) {
    state = { assistant: null, reasoning: null, reasoningHead: null };
    sessionStreams.set(key, state);
  }
  return state;
}
function dropSessionStream(sessionId) {
  sessionStreams.delete(sessionId || "");
}

/// 在 `sessionId` 的渲染上下文里执行 `fn`:pane 与流式状态都临时切过去,结束后还原。
/// 活动会话直接跑(省掉一次存取),后台会话走存取路径。
/// 后台渲染进行中。事件 handler 里混着**全局 UI**副作用(状态栏文案、首响应计时),
/// 那些只属于活动会话——后台会话触发它们就会把别人的状态盖在当前线上。
/// 用一个标志让那几处自我屏蔽,比把 sessionId 穿进每个 handler 便宜得多。
let renderingBackground = false;
function withSessionRender(sessionId, fn) {
  if (!sessionId || sessionId === activeSessionId) return fn();
  const state = streamStateFor(sessionId);
  const savedPane = activePane;
  const savedAssistant = currentAssistant;
  const savedReasoning = currentReasoning;
  const savedHead = currentReasoningHead;
  const savedBackground = renderingBackground;
  renderingBackground = true;
  activePane = paneFor(sessionId);
  currentAssistant = state.assistant;
  currentReasoning = state.reasoning;
  currentReasoningHead = state.reasoningHead;
  try {
    return fn();
  } finally {
    state.assistant = currentAssistant;
    state.reasoning = currentReasoning;
    state.reasoningHead = currentReasoningHead;
    renderingBackground = savedBackground;
    activePane = savedPane;
    currentAssistant = savedAssistant;
    currentReasoning = savedReasoning;
    currentReasoningHead = savedHead;
  }
}
const promptBox = $("prompt");// R-260:process_list 定时轮询。01-core 事件处理与 09-sessions 的 renderProcesses
// 校正逻辑都假定「下一次 process_list 轮询」会兜底事件丢失与列表结构变化(外部创建/
// 注销进程、Tauri 事件偶发丢失),但轮询定时器从未实现——侧边栏任务列表只能靠事件
// 投影 + 用户操作刷新,事件一丢或列表结构一变就滞留到下一次手动操作。3s 一轮:
// 工具执行期间能及时看到运行状态变化;process_list 后端是内存 + stat 轻量查询,
// refreshProcesses 内部已按项目单飞去重(processRefreshInFlight),频繁调用安全。
// D-376:节律随运行态自适应。3s 是**运行中**需要的分辨率(工具执行期间要及时看到
// 状态变化);全空闲时它变成纯消耗——每 3 秒一次 IPC + 一次 renderProcesses 全量
// 重建侧栏任务列表,一天两万八千次,而这段时间里列表根本不会变。空闲降到 15s 仍然
// 保住这条兜底的本意(事件丢失、外部创建/注销进程最迟 15 秒被纠正),代价只是
// 「别的进程刚建了一条线」这种低频事件晚几秒出现在列表里。
// 实现上保留**单个** setInterval 跳拍,而不是按需重排的递归 setTimeout:后者每次
// 触发都新建一个定时器,冒烟 harness 的「排空待处理定时器」会因此自我续命(实测三条
// 断言被搅红)。跳拍方案对外只是"少调几次",定时器身份与节拍都不变。
const PROCESS_POLL_MS = 3000;
const PROCESS_POLL_IDLE_EVERY = 5; // 空闲时每 5 拍拉一次 = 15s
let processPollTick = 0;
function anySessionBusy() {
  // 取状态机而不是上一次轮询的 item.running:新一轮由 kz:turn 立刻拨到 running,
  // 不必等下一次轮询才把节律提上来。auto_pending 也算忙——鞭挞正等着下一轮。
  if (typeof sessionStates === "undefined") return false;
  for (const state of sessionStates.values()) {
    if (["starting", "running", "stopping", "auto_pending"].includes(state.phase)) return true;
  }
  return false;
}
setInterval(() => {
  processPollTick += 1;
  if (!anySessionBusy() && processPollTick % PROCESS_POLL_IDLE_EVERY !== 0) return;
  if (typeof refreshProcesses === "function") refreshProcesses();
}, PROCESS_POLL_MS);
