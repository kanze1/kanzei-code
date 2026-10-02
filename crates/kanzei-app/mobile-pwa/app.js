// kanzei 移动端 PWA(R-271):配对 + 通知流 + 发消息 + 审批。
// 原生 JS,零构建零框架,由 R-270 桥接 serve。
// 协议契约沿用 docs/design/r059_mobile_agent_communication.md 阶段A字段。
// R-292:文案统一走 t() i18n 通道(中文键=文案,英文态查 I18N_EN,与桌面端
// 02-i18n.js 形态一致);原生弹窗 alert/confirm 清零,改为页面内联提示。

const STORAGE_KEY = "kanzei_device";
// UX-017:上次选的会话也记在手机浏览器里(与设备凭据同一处;这里不是桌面端 WebView)。
const THREAD_KEY = "kanzei_thread";

// ---- i18n 最小通道(R-292)----
// 中文态 t(key) 原样返回 key;英文态查 I18N_EN。key 即中文文案,集中在此表。
// t(key, ...args) 支持 {0}{1} 占位,动态值(状态码/错误信息/会话名)不在 key 内。
const I18N_EN = {
  // 配对
  "配对": "Pair",
  "在电脑的 kanzei 设置页启动移动端桥接,输入显示的配对码:": "Start the mobile bridge in kanzei settings on your computer, then enter the pairing code shown:",
  "配对码": "Pairing code",
  "配对中…": "Pairing…",
  "配对成功": "Paired",
  "配对失败({0})": "Pairing failed ({0})",
  "配对响应缺 device_id/token": "Pairing response missing device_id/token",
  "配对已失效:这台设备在电脑上被撤销,或桥接已重置。请重新配对。": "Pairing is no longer valid: this device was revoked on the computer, or the bridge was reset. Please pair again.",
  "已在本机解除配对。电脑没连上,没能通知它撤销——可在电脑设置里手动撤销这台设备。": "Unpaired on this phone. The computer was unreachable, so it could not be told to revoke this device. You can revoke it manually in the desktop settings.",
  // 连接状态
  "连接中…": "Connecting…",
  "已连接": "Connected",
  "连不上电脑,重试中…": "Cannot reach the computer, retrying…",
  "连不上电脑:请确认桌面端桥接还开着,手机和电脑在同一 Wi-Fi": "Cannot reach the computer: check that the desktop bridge is running and both devices are on the same Wi-Fi",
  "实时通知连接中…": "Live notices connecting…",
  "实时通知已连接": "Live notices connected",
  "实时通知断开,重连中…": "Live notices disconnected, reconnecting…",
  "实时通知出错: {0}": "Live notices error: {0}",
  "实时通知连接失败: {0}": "Live notices connection failed: {0}",
  "连接失败({0})": "Connection failed ({0})",
  // 服务端错误码
  "配对码不对,或已经用过。请在电脑设置里重新生成配对码。": "The pairing code is wrong or already used. Generate a new one in the desktop settings.",
  "这台设备已被撤销,或还没配对": "This device was revoked or is not paired",
  "请先选择对话": "Please choose a conversation first",
  "消息内容不能为空": "Message text cannot be empty",
  "这个对话已经不存在了,请刷新对话列表": "That conversation no longer exists, please refresh the conversation list",
  "桥接没有这个接口(桌面端版本可能比手机页面旧)": "The bridge has no such endpoint (the desktop app may be older than this page)",
  // 会话
  "对话": "Conversation",
  "刷新": "Refresh",
  "加载中…": "Loading…",
  "电脑上还没有可选的对话": "No conversations on the computer yet",
  "对话列表加载失败: {0}": "Failed to load conversations: {0}",
  "对话列表加载失败({0})": "Failed to load conversations ({0})",
  "{0} · 运行中": "{0} · running",
  "{0} · 讨论": "{0} · discussion",
  "{0} · 独立任务": "{0} · independent task",
  // 发消息
  "发消息到电脑": "Send a message to desktop",
  "输入消息内容": "Enter message text",
  "发送": "Send",
  "发送中…": "Sending…",
  "发送失败({0})": "Send failed ({0})",
  "已放进「{0}」的对话。不会自动让 AI 回复,在电脑上继续时会带上这条。": "Added to the conversation \"{0}\". The AI will not reply automatically; it will see this message when you continue on the computer.",
  "已放进「{0}」的对话(它正在运行,下一轮才会带上这条)。不会自动让 AI 回复。": "Added to the conversation \"{0}\" (it is running; the next turn will include this message). The AI will not reply automatically.",
  "已放进对话。不会自动让 AI 回复,在电脑上继续时会带上这条。": "Added to the conversation. The AI will not reply automatically; it will see this message when you continue on the computer.",
  // 审批
  "待批准请求": "Pending requests",
  "当前无待批准请求": "No pending requests",
  "待批准请求加载失败: {0}": "Failed to load pending requests: {0}",
  "查询失败({0})": "Query failed ({0})",
  "回答失败({0})": "Answer failed ({0})",
  "批准": "Approve",
  "拒绝": "Reject",
  "允许并记住": "Allow and remember",
  "允许并记住 = 以后遇到完全相同的操作不再询问": "Allow and remember = never ask again for exactly this operation",
  "已批准": "Approved",
  "已拒绝": "Rejected",
  "已允许并记住": "Allowed and remembered",
  "失败: {0}": "Failed: {0}",
  "工作目录": "Working directory",
  "命令过长,只显示了前 {0} 个字符": "The command is long; only the first {0} characters are shown",
  "允许并记住只适用于权限请求": "Allow and remember only applies to permission requests",
  // 问答卡
  "输入你的回答": "Enter your answer",
  "补充说明(可选)": "Optional details",
  "提交答案": "Submit answer",
  "取消问题": "Cancel question",
  "提交中…": "Submitting…",
  "答案已提交": "Answer submitted",
  "问题已取消": "Question cancelled",
  // 通知流与身份
  "通知": "Notices",
  "主对话": "Main conversation",
  "子代理": "Subagent",
  // 解除配对
  "解除配对": "Unpair",
  "解除后这台手机收不到通知,也不能批准请求,需要重新配对。": "After unpairing this phone gets no notices and cannot approve requests; you will need to pair again.",
  "确认解除": "Confirm unpair",
  "取消": "Cancel",
  "解除中…": "Unpairing…",
};

function uiLanguage() {
  return (navigator.language || "zh").toLowerCase().startsWith("zh") ? "zh" : "en";
}

function t(key, ...args) {
  let text = uiLanguage() === "en" ? (I18N_EN[key] || key) : key;
  args.forEach((value, index) => {
    text = text.replaceAll(`{${index}}`, String(value));
  });
  return text;
}

// ---- 本地存储(手机浏览器可能禁用/清空,读写都不能抛)----
function storageGet(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function storageSet(key, value) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // 存不下就当没记住。
  }
}

// ---- 错误:服务端错误码本地化(UX-139)----
// 桥接返回的 error 是机器码(invalid_pair_code 之类),直接显示给用户没法读。
const API_ERRORS = {
  invalid_pair_code: "配对码不对,或已经用过。请在电脑设置里重新生成配对码。",
  device_revoked_or_unauthorized: "这台设备已被撤销,或还没配对",
  thread_id_required: "请先选择对话",
  text_required: "消息内容不能为空",
  unknown_session: "这个对话已经不存在了,请刷新对话列表",
  not_found: "桥接没有这个接口(桌面端版本可能比手机页面旧)",
};

class ApiError extends Error {
  constructor(message, code) {
    super(message);
    this.code = code || "";
  }
}

// 配对失效:已经清了凭据并跳回配对页,调用方静默收尾即可。
class AuthExpiredError extends Error {}

// 把失败响应读成已本地化的 ApiError;`fallback(status)` 给没有错误码时的兜底文案。
async function apiError(res, fallback) {
  const data = await res.json().catch(() => ({}));
  const code = typeof data.error === "string" ? data.error : "";
  const known = API_ERRORS[code];
  return new ApiError(known ? t(known) : code || fallback(res.status), code);
}

// fetch 抛出的 TypeError 是网络层失败(断网/桥接没开),与服务端回了错误是两回事。
function describeError(err) {
  if (err instanceof ApiError) return err.message;
  if (err?.name === "TypeError") return t("连不上电脑:请确认桌面端桥接还开着,手机和电脑在同一 Wi-Fi");
  return String(err?.message || err);
}

// ---- 配对:输入配对码换 device_id + token,存 localStorage ----
async function pair(pairCode) {
  const res = await fetch("/v1/pair", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pair_code: pairCode }),
  });
  if (!res.ok) throw await apiError(res, (status) => t("配对失败({0})", status));
  const device = await res.json();
  if (!device.device_id || !device.token) {
    throw new ApiError(t("配对响应缺 device_id/token"));
  }
  storageSet(STORAGE_KEY, JSON.stringify(device));
  return device;
}

// 已配对的设备;未配对返回 null。
function storedDevice() {
  try {
    return JSON.parse(storageGet(STORAGE_KEY) || "null");
  } catch {
    return null;
  }
}

// 带设备 token 的请求。401 = 凭据已失效(桌面端撤销/桥接重置),重试没有意义:
// 清凭据、停掉所有轮询、跳回配对页(UX-139;此前无限重试并显示机器码)。
async function bridgeFetch(device, path, init = {}) {
  const res = await fetch(path, {
    ...init,
    headers: { ...init.headers, Authorization: `Bearer ${device.token}` },
  });
  if (res.status === 401) {
    expireDevice();
    throw new AuthExpiredError();
  }
  return res;
}

let authExpired = false;
function expireDevice() {
  if (authExpired) return;
  authExpired = true;
  stopRealtime();
  storageSet(STORAGE_KEY, null);
  renderPairForm(t("配对已失效:这台设备在电脑上被撤销,或桥接已重置。请重新配对。"));
}

// ---- 通知流:SSE 订阅(GET /v1/events,带设备 token 认证)----
// EventSource 无法带 Authorization 头,用 fetch 读流(零依赖 SSE 客户端)。
// `cursor` 是这条连接自己的已消费位置:订阅新会话从头开始(通知列表同时清空),断线重连
// 带上已消费的位置补发、不丢终态也不重复(R-270 批2)。此前 cursor 是全局变量,切会话时
// 把上一个会话的序号带进来,吞掉新会话开头的 N 条事件(UX-018)。
function connectNotifications(device, threadId, onEvent, onStatus) {
  let cursor = 0;
  const controller = new AbortController();
  let reconnectTimer = null;

  function status(text) {
    if (!controller.signal.aborted) onStatus(text);
  }

  async function run() {
    const params = new URLSearchParams({
      thread_id: threadId,
      device_id: device.device_id,
      cursor: String(cursor),
    });
    try {
      const res = await fetch(`/v1/events?${params}`, {
        headers: { Authorization: `Bearer ${device.token}` },
        signal: controller.signal,
      });
      if (res.status === 401) {
        expireDevice();
        return;
      }
      if (!res.ok) {
        const err = await apiError(res, (code) => t("连接失败({0})", code));
        status(t("实时通知连接失败: {0}", err.message));
        scheduleReconnect();
        return;
      }
      status(t("实时通知已连接"));
      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        // SSE 帧以空行分隔,逐条解析 data: 行。
        let sep;
        while ((sep = buffer.indexOf("\n\n")) >= 0) {
          const frame = buffer.slice(0, sep);
          buffer = buffer.slice(sep + 2);
          for (const line of frame.split("\n")) {
            if (!line.startsWith("data: ")) continue;
            let event;
            try {
              event = JSON.parse(line.slice(6));
            } catch {
              continue; // 坏帧跳过,不让一条脏数据断掉整条流。
            }
            if (typeof event.sequence === "number") cursor = event.sequence;
            if (!controller.signal.aborted) onEvent(event);
          }
        }
      }
      // 流正常结束(服务端关连接):重连。
      status(t("实时通知断开,重连中…"));
      scheduleReconnect();
    } catch (err) {
      if (err.name === "AbortError") return;
      status(t("实时通知出错: {0}", describeError(err)));
      scheduleReconnect();
    }
  }

  function scheduleReconnect() {
    if (reconnectTimer || controller.signal.aborted) return;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      run();
    }, 2000);
  }

  run();
  return controller;
}

let sseController = null;
let approvalTimer = null;
let pollApprovalsNow = null;
let currentThreadId = "";

// 停掉所有后台通道(解除配对/配对失效时用)。
function stopRealtime() {
  if (sseController) sseController.abort();
  sseController = null;
  if (approvalTimer) clearInterval(approvalTimer);
  approvalTimer = null;
  pollApprovalsNow = null;
}

// 连接状态芯片跟着「待批准请求」轮询走:它是配对后一直在跑的主通道,订不订阅会话都有意义。
// (此前芯片只由通知流写,没订阅会话时永远停在「连接中…」。)
function setConnection(state) {
  const chip = document.getElementById("conn-status");
  if (!chip) return;
  chip.dataset.state = state;
  chip.textContent = state === "connected" ? t("已连接") : state === "error" ? t("连不上电脑,重试中…") : t("连接中…");
}

// ---- approval(R-271 批3):GET pending + POST answer(批准/拒绝/总是允许)----
async function fetchPendingApprovals(device) {
  const res = await bridgeFetch(device, "/v1/approval/pending");
  if (!res.ok) throw await apiError(res, (status) => t("查询失败({0})", status));
  return res.json();
}

// answer 为 { reply } 或 { cancel: true }:取消走显式字段,不与答案文本共用带内 "cancel"。
async function answerApproval(device, id, answer) {
  const res = await bridgeFetch(device, "/v1/approval/answer", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ id, ...answer }),
  });
  if (!res.ok) throw await apiError(res, (status) => t("回答失败({0})", status));
  return res.json();
}

// 已提交/已取消的回执卡片在 ask 离开 pending 后再保留这么久,让用户看清结果。
const APPROVAL_RESULT_LINGER_MS = 5000;

// 轮询 pending approval 并渲染卡片。3s 间隔(轻交互遥控器,不频繁打桥接)。
// D-751 跟进:按 ask id 增量对账,不再整表清空重建——已有卡片原样保留
// (输入值、多选状态、焦点与软键盘都不丢),提交中的卡片不会被重建成可再点的新卡片。
function startApprovalPolling(device) {
  if (approvalTimer) clearInterval(approvalTimer);
  // 桥接慢时响应可能乱序到达:只丢比「最近一次已采纳」更旧的响应,旧响应不回滚卡片列表。
  // 不能按「最后发起的一轮」取舍——响应持续 >3s 时每个响应落地前都已发出更新一轮,
  // 会被全部丢弃,卡片永远不出现、状态行卡在「加载中」。
  let issuedPoll = 0;
  let appliedPoll = 0;
  const render = async () => {
    const container = document.getElementById("approval-list");
    if (!container) return;
    const poll = ++issuedPoll;
    try {
      const data = await fetchPendingApprovals(device);
      if (poll < appliedPoll) return;
      appliedPoll = poll;
      setConnection("connected");
      reconcileApprovalCards(device, container, data.pending || []);
    } catch (err) {
      if (err instanceof AuthExpiredError) return;
      if (poll < appliedPoll) return;
      appliedPoll = poll;
      setConnection("error");
      // 查询失败只更新状态行,保留已有卡片与用户正在填写的内容。
      setApprovalStatus(container, t("待批准请求加载失败: {0}", describeError(err)));
    }
  };
  pollApprovalsNow = render;
  render();
  approvalTimer = setInterval(render, 3000);
}

// 状态行(加载中/空列表/查询失败)是独立节点,改它不触碰任何卡片。
function setApprovalStatus(container, text) {
  let status = container.querySelector(":scope > .approval-status");
  if (!status) {
    status = document.createElement("p");
    status.className = "muted approval-status";
    container.prepend(status);
  }
  status.textContent = text;
  status.hidden = !text;
}

function reconcileApprovalCards(device, container, pending) {
  const existing = new Map();
  for (const card of container.querySelectorAll(":scope > [data-ask-id]")) {
    existing.set(card.dataset.askId, card);
  }
  const live = new Set();
  for (const ask of pending) {
    const key = String(ask.id);
    live.add(key);
    if (existing.has(key)) continue; // 仍在 pending:保留原 DOM,不重建。
    container.appendChild(
      ask.kind === "question" ? renderQuestionCard(device, ask) : renderPermissionCard(device, ask),
    );
  }
  const now = Date.now();
  for (const [key, card] of existing) {
    if (live.has(key)) continue;
    // 提交中的卡片等 POST 返回;刚出结果的回执短暂保留,其余已离开 pending 的卡片移除。
    if (card.dataset.state === "submitting") continue;
    if (card.dataset.state === "done" && now - Number(card.dataset.doneAt || 0) < APPROVAL_RESULT_LINGER_MS) {
      continue;
    }
    card.remove();
  }
  setApprovalStatus(container, pending.length === 0 ? t("当前无待批准请求") : "");
  const badge = document.getElementById("approval-count");
  if (badge) {
    const count = container.querySelectorAll(":scope > [data-ask-id]:not([data-state])").length;
    badge.textContent = String(count);
    badge.hidden = count === 0;
  }
}

// UX-134:卡片来源 =「项目 · 主对话/独立任务名」,不再露出 ses_ 哈希;桥接没给就不显示。
function approvalOrigin(ask) {
  return [ask.project, ask.session_label].filter(Boolean).join(" · ");
}

function originNode(ask) {
  const origin = approvalOrigin(ask);
  if (!origin) return null;
  const node = document.createElement("p");
  node.className = "approval-origin";
  node.textContent = origin;
  return node;
}

// UI-0926 #10:bash 的权限资源是 `{"command","workdir"}` JSON。PWA 与桌面端 ui/ 是不同的
// 服务根(mobile.rs 只 serve mobile-pwa/),不能共享 04-structured-parse.js,这里是同一
// 判据的本地小实现:能解析出 command 就拆成「命令块 + 工作目录」。
function parseBashResource(raw) {
  const trimmed = String(raw ?? "").trim();
  if (!trimmed.startsWith("{")) return null;
  try {
    const value = JSON.parse(trimmed);
    if (value && typeof value.command === "string") {
      return { command: value.command, workdir: typeof value.workdir === "string" ? value.workdir : "" };
    }
  } catch {
    // 不是合法 JSON:按原文显示。
  }
  return null;
}

// UX-019:新桥接直接下发完整 command/workdir 与 resource_full;老桥接只有截断到 80 字符的
// resource(bash 的 JSON 被切坏),这里退回本地解析。永远不显示被切成残片的 JSON。
function permissionDetail(ask) {
  const truncated = ask.truncated === true;
  if (typeof ask.command === "string") {
    return { body: ask.command, workdir: typeof ask.workdir === "string" ? ask.workdir : "", truncated };
  }
  const raw = String(ask.resource_full ?? ask.resource ?? "");
  const parsed = parseBashResource(raw);
  if (parsed) return { body: parsed.command, workdir: parsed.workdir, truncated };
  return { body: raw, workdir: "", truncated };
}

// 桥接给的 resource_full 封顶 4000 字,与 mobile.rs 的 APPROVAL_FULL_LIMIT 一致。
const APPROVAL_FULL_LIMIT = 4000;

function permissionDescHtml(ask) {
  const detail = permissionDetail(ask);
  const workdir = detail.workdir
    ? `<p class="muted approval-workdir">${escapeHtml(t("工作目录"))}: ${escapeHtml(detail.workdir)}</p>`
    : "";
  const cut = detail.truncated
    ? `<p class="muted approval-cut">${escapeHtml(t("命令过长,只显示了前 {0} 个字符", APPROVAL_FULL_LIMIT))}</p>`
    : "";
  return `<div class="approval-desc"><span class="approval-action">${escapeHtml(ask.action)}</span>`
    + `<pre class="approval-cmd">${escapeHtml(detail.body)}</pre></div>${workdir}${cut}`;
}

// 拒绝是次要样式、批准是主按钮,二者隔开(UX-134:此前同色、间距为 0,批准还排在上面,容易误触);
// 「总是允许」(UX-142)是更轻的文字按钮,附一句后果说明。
function renderPermissionCard(device, ask) {
  const card = document.createElement("div");
  card.className = "card approval";
  card.dataset.askId = String(ask.id);
  const origin = originNode(ask);
  if (origin) card.appendChild(origin);
  card.insertAdjacentHTML("beforeend", `
    ${permissionDescHtml(ask)}
    <div class="approval-actions">
      <button type="button" class="reject secondary" data-id="${ask.id}">${t("拒绝")}</button>
      <button type="button" class="approve" data-id="${ask.id}">${t("批准")}</button>
    </div>
    <div class="approval-always">
      <button type="button" class="always link" data-id="${ask.id}">${t("允许并记住")}</button>
      <span class="muted">${t("允许并记住 = 以后遇到完全相同的操作不再询问")}</span>
    </div>
    <p class="muted approval-card-status"></p>`);
  card.querySelector(".approve").addEventListener("click", () => {
    submitAnswer(device, ask.id, "allow", card);
  });
  card.querySelector(".reject").addEventListener("click", () => {
    submitAnswer(device, ask.id, "deny", card);
  });
  card.querySelector(".always").addEventListener("click", () => {
    submitAnswer(device, ask.id, "always", card);
  });
  return card;
}

// 单选/多选一律「先选中、再点提交」:点选项只改选中状态(UX-140;此前单选一点就提交,
// 已经写好的补充文字被丢掉,且 default 同时预填输入框与选项,多选会把它重复提交)。
function renderQuestionCard(device, ask) {
  const card = document.createElement("div");
  card.className = "card approval question";
  card.dataset.askId = String(ask.id);

  const origin = originNode(ask);
  if (origin) card.appendChild(origin);

  const question = document.createElement("p");
  question.className = "approval-desc";
  question.textContent = ask.question || "";
  card.appendChild(question);

  const multiple = ask.multiple === true;
  const labels = [];
  const optionButtons = new Map();
  const selected = new Set();
  const options = document.createElement("div");
  options.className = "question-options";
  for (const raw of Array.isArray(ask.options) ? ask.options : []) {
    const option = typeof raw === "string" ? { label: raw } : raw;
    if (typeof option?.label !== "string" || !option.label) continue;
    labels.push(option.label);
    const button = document.createElement("button");
    button.className = "question-option";
    button.type = "button";
    button.setAttribute("aria-pressed", "false");
    const label = document.createElement("span");
    label.className = "question-option-label";
    label.textContent = option.label;
    button.appendChild(label);
    if (typeof option.note === "string" && option.note.trim()) {
      const note = document.createElement("span");
      note.className = "question-option-note";
      note.textContent = option.note;
      button.appendChild(note);
    }
    optionButtons.set(option.label, button);
    button.addEventListener("click", () => {
      if (selected.has(option.label)) {
        setSelected(option.label, false);
      } else {
        // 单选:选这个就取消别的;多选:叠加。
        if (!multiple) [...selected].forEach((other) => setSelected(other, false));
        setSelected(option.label, true);
      }
      updateSubmitState();
    });
    options.appendChild(button);
  }
  card.appendChild(options);

  function setSelected(label, on) {
    const button = optionButtons.get(label);
    if (!button) return;
    if (on) selected.add(label);
    else selected.delete(label);
    button.setAttribute("aria-pressed", on ? "true" : "false");
    button.classList.toggle("selected", on);
  }

  const answer = document.createElement("input");
  answer.className = "question-answer";
  answer.type = "text";
  answer.placeholder = t(labels.length ? "补充说明(可选)" : "输入你的回答");
  // default 命中选项就预选该选项、输入框留空(不重复);不是选项文本才当作预填文字。
  const defaults = typeof ask.default === "string" && ask.default ? ask.default.split("\n").map((line) => line.trim()).filter(Boolean) : [];
  const preselected = defaults.filter((label) => optionButtons.has(label));
  if (preselected.length) {
    (multiple ? preselected : preselected.slice(0, 1)).forEach((label) => setSelected(label, true));
  } else if (typeof ask.default === "string") {
    answer.value = ask.default;
  }
  card.appendChild(answer);

  const actions = document.createElement("div");
  actions.className = "question-actions";
  const submit = document.createElement("button");
  submit.className = "question-submit";
  submit.type = "button";
  submit.textContent = t("提交答案");
  const cancel = document.createElement("button");
  cancel.className = "question-cancel secondary";
  cancel.type = "button";
  cancel.textContent = t("取消问题");
  actions.append(cancel, submit);
  card.appendChild(actions);

  const status = document.createElement("p");
  status.className = "muted question-status";
  card.appendChild(status);

  function updateSubmitState() {
    submit.disabled = selected.size === 0 && !answer.value.trim();
  }
  function currentAnswer() {
    return [...selected, answer.value.trim()].filter(Boolean).join("\n");
  }

  answer.addEventListener("input", updateSubmitState);
  submit.addEventListener("click", () => {
    const reply = currentAnswer();
    if (reply) submitQuestionAnswer(device, ask, { reply }, card, status, t("答案已提交"));
  });
  cancel.addEventListener("click", () => {
    // 显式取消字段:用户答案恰好是 "cancel" 时仍按答案投递。
    submitQuestionAnswer(device, ask, { cancel: true }, card, status, t("问题已取消"));
  });
  updateSubmitState();
  return card;
}

// 卡片状态:submitting(POST 在途,拒绝再次提交,轮询不移除)/ done(回执,短暂保留)。
function markApprovalDone(card) {
  card.dataset.state = "done";
  card.dataset.doneAt = String(Date.now());
}

async function submitQuestionAnswer(device, ask, answer, card, status, successText) {
  if (card.dataset.state) return; // 在途或已完成:不重复提交。
  card.dataset.state = "submitting";
  const controls = card.querySelectorAll("button, input");
  controls.forEach((control) => { control.disabled = true; });
  status.textContent = t("提交中…");
  try {
    await answerApproval(device, ask.id, answer);
    const result = document.createElement("p");
    result.className = "muted";
    result.textContent = successText;
    card.replaceChildren(result);
    markApprovalDone(card);
  } catch (err) {
    if (err instanceof AuthExpiredError) return;
    delete card.dataset.state;
    status.textContent = t("失败: {0}", describeError(err));
    controls.forEach((control) => { control.disabled = false; });
  }
}

const PERMISSION_RECEIPTS = { allow: "已批准", deny: "已拒绝", always: "已允许并记住" };

async function submitAnswer(device, id, reply, card) {
  if (card.dataset.state) return; // 在途或已完成:不重复提交。
  card.dataset.state = "submitting";
  const controls = card.querySelectorAll("button");
  const status = card.querySelector(".approval-card-status");
  controls.forEach((button) => { button.disabled = true; });
  try {
    await answerApproval(device, id, { reply });
    card.innerHTML = `<p class="muted">${escapeHtml(t(PERMISSION_RECEIPTS[reply] || "已批准"))}</p>`;
    markApprovalDone(card);
  } catch (err) {
    if (err instanceof AuthExpiredError) return;
    // 卡片按 id 保留不再被轮询重建,失败时恢复按钮供重试;ask 已不存在时下一轮轮询会移除卡片。
    delete card.dataset.state;
    if (status) status.textContent = t("失败: {0}", describeError(err));
    controls.forEach((button) => { button.disabled = false; });
  }
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ---- 会话列表(UX-017):桥接给出主对话 + 讨论/独立任务,下拉选择并记住上次的选择 ----
async function fetchSessions(device) {
  const res = await bridgeFetch(device, "/v1/sessions");
  if (!res.ok) throw await apiError(res, (status) => t("对话列表加载失败({0})", status));
  return res.json();
}

// 下拉里的名字:用户命名的会话补上类型,正在运行的标出来。
function sessionOptionText(session) {
  let text = session.label;
  const kindWord = session.kind === "discussion" ? "讨论" : session.kind === "task" ? "独立任务" : "";
  if (kindWord && !text.startsWith(kindWord)) text = t(`{0} · ${kindWord}`, text);
  return session.running ? t("{0} · 运行中", text) : text;
}

let sessionsSignature = "";

// 刷新下拉。返回应当选中的会话 id(没有会话返回 "")。内容没变就不重建 <option>——
// 用户可能正开着选择器。
function applySessions(select, data) {
  const sessions = Array.isArray(data?.sessions) ? data.sessions : [];
  if (sessions.length === 0) {
    sessionsSignature = "";
    select.replaceChildren(new Option(t("电脑上还没有可选的对话"), ""));
    select.disabled = true;
    return "";
  }
  const signature = JSON.stringify(sessions.map((s) => [s.session_id, s.label, s.kind, s.running]));
  if (signature !== sessionsSignature) {
    sessionsSignature = signature;
    select.replaceChildren(...sessions.map((s) => new Option(sessionOptionText(s), s.session_id)));
  }
  select.disabled = false;
  const known = (id) => id && sessions.some((s) => s.session_id === id);
  const wanted = [currentThreadId, storageGet(THREAD_KEY), data.default, sessions[0].session_id].find(known);
  select.value = wanted;
  return wanted;
}

async function loadSessions(device) {
  const select = document.getElementById("thread-id");
  const note = document.getElementById("thread-msg");
  if (!select) return;
  try {
    const wanted = applySessions(select, await fetchSessions(device));
    note.textContent = "";
    if (wanted && wanted !== currentThreadId) selectThread(device, wanted);
  } catch (err) {
    if (err instanceof AuthExpiredError) return;
    note.textContent = t("对话列表加载失败: {0}", describeError(err));
  }
}

// 切到某个会话:记住选择、重订该会话的通知。
function selectThread(device, threadId) {
  currentThreadId = threadId;
  storageSet(THREAD_KEY, threadId);
  subscribe(device, threadId);
}

// ---- 发消息(R-271 批2):POST /v1/messages(thread_id + 消息体)----
async function sendMessage(device, threadId, text) {
  const res = await bridgeFetch(device, "/v1/messages", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ thread_id: threadId, text }),
  });
  if (!res.ok) throw await apiError(res, (status) => t("发送失败({0})", status));
  return res.json();
}

// 回执如实交代桥接做了什么:消息写进了该会话的对话,但不会替电脑发起一轮运行(UX-133)。
function sendReceipt(result) {
  if (!result?.label) return t("已放进对话。不会自动让 AI 回复,在电脑上继续时会带上这条。");
  return t(
    result.running
      ? "已放进「{0}」的对话(它正在运行,下一轮才会带上这条)。不会自动让 AI 回复。"
      : "已放进「{0}」的对话。不会自动让 AI 回复,在电脑上继续时会带上这条。",
    result.label,
  );
}

// ---- 渲染 ----
// 配对码在 URL 片段里时(`/#pair=…`,电脑设置页的「复制配对链接」)预填,免得在手机上敲 30 个字符。
// 片段不会发给服务器;读完就从地址栏抹掉。
function pairCodeFromHash() {
  const match = /(?:^#|&)pair=([^&]+)/.exec(location.hash);
  if (!match) return "";
  history.replaceState(null, "", location.pathname + location.search);
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return "";
  }
}

function renderPairForm(notice = "") {
  const prefill = pairCodeFromHash();
  app.innerHTML = `
    <h1>${t("配对")}</h1>
    <div class="card">
      <p>${t("在电脑的 kanzei 设置页启动移动端桥接,输入显示的配对码:")}</p>
      <input id="pair-code" placeholder="${t("配对码")}" value="${escapeHtml(prefill)}" autocomplete="off" autocapitalize="off" autocorrect="off" spellcheck="false">
      <button id="pair-btn" type="button">${t("配对")}</button>
      <p id="pair-msg" class="muted" role="status">${escapeHtml(notice)}</p>
    </div>`;
  const input = document.getElementById("pair-code");
  const button = document.getElementById("pair-btn");
  const msg = document.getElementById("pair-msg");
  let pairing = false;
  async function submit() {
    if (pairing) return;
    pairing = true;
    button.disabled = true;
    msg.textContent = t("配对中…");
    try {
      await pair(input.value.trim());
      msg.textContent = t("配对成功");
      setTimeout(renderApp, 300);
    } catch (err) {
      msg.textContent = describeError(err);
      pairing = false;
      button.disabled = false;
    }
  }
  button.addEventListener("click", submit);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) submit();
  });
}

function renderNotifications(device) {
  authExpired = false;
  currentThreadId = "";
  sessionsSignature = "";
  // 待批准请求置顶:它是最需要尽快处理的东西(此前排在输入卡之下,要滚一屏才看到)。
  app.innerHTML = `
    <h1>kanzei <span id="conn-status" class="status" data-state="connecting">${t("连接中…")}</span></h1>
    <div class="card" id="approval-card">
      <h2>${t("待批准请求")} <span id="approval-count" class="badge" hidden></span></h2>
      <div id="approval-list"><p class="muted approval-status">${t("加载中…")}</p></div>
    </div>
    <div class="card" id="thread-row">
      <div class="row-head">
        <label for="thread-id">${t("对话")}</label>
        <button id="sessions-refresh" class="link" type="button">${t("刷新")}</button>
      </div>
      <select id="thread-id" disabled><option value="">${t("加载中…")}</option></select>
      <p id="thread-msg" class="muted" role="status"></p>
    </div>
    <div class="card" id="send-row">
      <label for="msg-text">${t("发消息到电脑")}</label>
      <input id="msg-text" placeholder="${t("输入消息内容")}" enterkeyhint="send">
      <button id="send-btn" type="button">${t("发送")}</button>
      <p id="send-msg" class="muted" role="status"></p>
    </div>
    <div class="card">
      <h2>${t("通知")} <span id="notice-status" class="status"></span></h2>
      <div id="notice-list"></div>
    </div>
    <div class="device-footer">
      <button id="unpair-btn" class="link danger-text" type="button">${t("解除配对")}</button>
      <div id="unpair-confirm" class="card" hidden>
        <p>${t("解除后这台手机收不到通知,也不能批准请求,需要重新配对。")}</p>
        <div class="confirm-actions">
          <button id="unpair-cancel" class="secondary" type="button">${t("取消")}</button>
          <button id="unpair-do" class="danger" type="button">${t("确认解除")}</button>
        </div>
        <p id="unpair-msg" class="muted" role="status"></p>
      </div>
    </div>`;

  const select = document.getElementById("thread-id");
  const msgInput = document.getElementById("msg-text");
  const sendBtn = document.getElementById("send-btn");
  const sendMsg = document.getElementById("send-msg");

  select.addEventListener("change", () => {
    if (!select.value) return;
    sendMsg.textContent = "";
    selectThread(device, select.value);
  });
  document.getElementById("sessions-refresh").addEventListener("click", () => loadSessions(device));

  // 发送中禁用按钮(UX-141:弱网双击会发出两条一样的消息)。
  let sending = false;
  async function send() {
    if (sending) return;
    const threadId = select.value;
    const text = msgInput.value.trim();
    if (!threadId) {
      sendMsg.textContent = t("请先选择对话");
      return;
    }
    if (!text) {
      sendMsg.textContent = t("消息内容不能为空");
      return;
    }
    sending = true;
    sendBtn.disabled = true;
    sendMsg.textContent = t("发送中…");
    try {
      const result = await sendMessage(device, threadId, text);
      sendMsg.textContent = sendReceipt(result);
      msgInput.value = ""; // 发送后清空
    } catch (err) {
      if (err instanceof AuthExpiredError) return;
      sendMsg.textContent = describeError(err);
      // 会话在电脑上被关了:刷新下拉,别让用户对着一个不存在的会话反复发。
      if (err instanceof ApiError && err.code === "unknown_session") loadSessions(device);
    } finally {
      sending = false;
      sendBtn.disabled = false;
    }
  }
  sendBtn.addEventListener("click", send);
  msgInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) send();
  });
  msgInput.addEventListener("input", () => {
    sendMsg.textContent = "";
  });

  setupUnpair(device);
  // approval 轮询(已配对即启动,不依赖订阅会话)。
  startApprovalPolling(device);
  loadSessions(device);
}

// 解除配对:先在页面内确认(UX-137;此前一点就清,还紧贴审批区),再通知桥接撤销这台设备,
// 最后才清本机凭据。桥接连不上也允许在本机解除,但如实说明没能通知电脑。
function setupUnpair(device) {
  const trigger = document.getElementById("unpair-btn");
  const panel = document.getElementById("unpair-confirm");
  const doBtn = document.getElementById("unpair-do");
  const cancelBtn = document.getElementById("unpair-cancel");
  const msg = document.getElementById("unpair-msg");
  trigger.addEventListener("click", () => {
    panel.hidden = false;
    trigger.hidden = true;
    cancelBtn.focus();
  });
  cancelBtn.addEventListener("click", () => {
    panel.hidden = true;
    trigger.hidden = false;
    msg.textContent = "";
  });
  doBtn.addEventListener("click", async () => {
    doBtn.disabled = true;
    cancelBtn.disabled = true;
    msg.textContent = t("解除中…");
    let notified = true;
    try {
      const res = await fetch("/v1/unpair", {
        method: "POST",
        headers: { Authorization: `Bearer ${device.token}` },
      });
      // 401 = 桥接早已不认这台设备,等同于已撤销。
      notified = res.ok || res.status === 401;
    } catch {
      notified = false;
    }
    stopRealtime();
    storageSet(STORAGE_KEY, null);
    authExpired = true; // 在途请求若随后回 401,不要再弹「配对已失效」。
    renderPairForm(notified ? "" : t("已在本机解除配对。电脑没连上,没能通知它撤销——可在电脑设置里手动撤销这台设备。"));
  });
}

// UI-0926 #8:通知行「字形 · 身份 · 摘要 · 时间」。旧行是 `[序号] agent_status_changed — 摘要`,
// 序号与恒定的 kind 对用户没有信息量,status 与 agent_id 反而没用上(分不出主/子代理与成败)。
// UX-164:不是今天的通知补上「月-日」,隔天的 09:05 不会被当成刚才的。
// 纯函数:冒烟直接在页面里调它。
const NOTICE_GLYPHS = { running: "●", succeeded: "✓", failed: "✕", stopped: "■", requires_action: "⚠" };
function formatNotice(event) {
  const status = String(event?.status || "");
  const agentId = String(event?.agent_id || "");
  const sub = agentId.startsWith("task:");
  const who = agentId === "primary" ? t("主对话") : sub ? t("子代理") : agentId;
  let time = "";
  const created = event?.created_at;
  if (created !== undefined && created !== null && created !== "") {
    const date = new Date(typeof created === "number" ? created : String(created));
    if (!Number.isNaN(date.getTime())) {
      const pad = (value) => String(value).padStart(2, "0");
      time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
      if (date.toDateString() !== new Date().toDateString()) {
        time = `${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${time}`;
      }
    }
  }
  return { glyph: NOTICE_GLYPHS[status] || "·", status, who, summary: String(event?.summary || ""), time, sub };
}

function buildNoticeItem(event) {
  const view = formatNotice(event);
  const item = document.createElement("div");
  item.className = view.sub ? "notice notice-sub" : "notice";
  const glyph = document.createElement("span");
  glyph.className = "notice-glyph";
  glyph.dataset.status = view.status;
  glyph.setAttribute("aria-hidden", "true");
  glyph.textContent = view.glyph;
  const who = document.createElement("span");
  who.className = "notice-who";
  who.textContent = view.who;
  const summary = document.createElement("span");
  summary.className = "notice-summary";
  summary.textContent = view.summary;
  const time = document.createElement("span");
  time.className = "notice-time";
  time.textContent = view.time;
  item.append(glyph, who, summary, time);
  return item;
}

function subscribe(device, threadId) {
  if (sseController) sseController.abort();
  const statusEl = document.getElementById("notice-status");
  const listEl = document.getElementById("notice-list");
  if (!listEl) return;
  listEl.innerHTML = "";
  statusEl.textContent = t("实时通知连接中…");
  sseController = connectNotifications(
    device,
    threadId,
    (event) => {
      const item = buildNoticeItem(event);
      listEl.prepend(item);
      // 长列表窗口化:最多保留 100 条(轻交互遥控器,不无限堆积)。
      while (listEl.children.length > 100) listEl.removeChild(listEl.lastChild);
    },
    (status) => {
      statusEl.textContent = status;
    },
  );
}

const app = document.getElementById("app");

function renderApp() {
  const device = storedDevice();
  if (!device) {
    renderPairForm();
  } else {
    renderNotifications(device);
  }
}

// UX-143:LAN 纯 HTTP 下浏览器根本不提供 serviceWorker,离线壳形同虚设,sw.js 已删除。
// 早先版本在回环地址(安全上下文)上注册过它的,这里顺手注销并清掉壳缓存,免得旧壳一直挂着。
if (navigator.serviceWorker?.getRegistrations) {
  navigator.serviceWorker
    .getRegistrations()
    .then((registrations) => registrations.forEach((registration) => registration.unregister()))
    .catch(() => {});
  caches.delete("kanzei-shell-v1").catch(() => {});
}

// 手机浏览器切后台会冻住定时器与长连接:回到前台立刻刷新待批准请求和会话列表,
// 不让用户盯着一屏过期的数据等下一个 3 秒。
document.addEventListener("visibilitychange", () => {
  if (document.visibilityState !== "visible" || !pollApprovalsNow) return;
  pollApprovalsNow();
  const device = storedDevice();
  if (device) loadSessions(device);
});

renderApp();
