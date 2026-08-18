// kanzei 移动端 PWA(R-271):配对 + 通知流只读(批1)。
// 原生 JS,零构建零框架,由 R-270 桥接 serve。
// 协议契约沿用 docs/design/r059_mobile_agent_communication.md 阶段A字段。
// R-292:文案统一走 t() i18n 通道(中文键=文案,英文态查 I18N_EN,与桌面端
// 02-i18n.js 形态一致);原生弹窗 alert 清零,改为页面内联提示。

const STORAGE_KEY = "kanzei_device";

// ---- i18n 最小通道(R-292)----
// 中文态 t(key) 原样返回 key;英文态查 I18N_EN。key 即中文文案,集中在此表。
// t(key, ...args) 支持 {0}{1} 占位,动态值(状态码/错误信息/设备 id)不在 key 内。
const I18N_EN = {
  "配对失败({0})": "Pairing failed ({0})",
  "配对响应缺 device_id/token": "Pairing response missing device_id/token",
  "连接失败: {0}": "Connection failed: {0}",
  "已连接": "Connected",
  "连接断开,重连中…": "Disconnected, reconnecting…",
  "连接错误: {0}": "Connection error: {0}",
  "查询失败({0})": "Query failed ({0})",
  "回答失败({0})": "Answer failed ({0})",
  "当前无待批准请求": "No pending requests",
  "批准": "Approve",
  "拒绝": "Reject",
  "approval 查询失败: {0}": "approval query failed: {0}",
  "已{0} #{1}": "{0} #1", // 占位符 {1} 由 id 填充,文案顺序交给英文表
  "失败: {0}": "Failed: {0}",
  "发送失败({0})": "Send failed ({0})",
  "配对": "Pair",
  "在电脑的 kanzei 设置页启动移动端桥接,输入显示的配对码:": "Start the mobile bridge on kanzei desktop settings and enter the pairing code shown:",
  "配对码": "Pairing code",
  "配对中…": "Pairing…",
  "配对成功: {0}": "Paired: {0}",
  "连接中…": "Connecting…",
  "会话 ID": "Session ID",
  "输入会话 ID(如 session-1)": "Enter session ID (e.g. session-1)",
  "订阅": "Subscribe",
  "发消息到电脑": "Send a message to desktop",
  "输入消息内容": "Enter message text",
  "发送": "Send",
  "待批准请求": "Pending requests",
  "加载中…": "Loading…",
  "解除配对": "Unpair",
  "请输入会话 ID": "Please enter a session ID",
  "请先输入会话 ID": "Please enter a session ID first",
  "消息内容不能为空": "Message text cannot be empty",
  "发送中…": "Sending…",
  "已发送": "Sent",
  "请求": "request",
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

// ---- 配对:输入配对码换 device_id + token,存 localStorage ----
async function pair(pairCode) {
  const res = await fetch("/v1/pair", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pair_code: pairCode }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || t("配对失败({0})", res.status));
  }
  const device = await res.json();
  if (!device.device_id || !device.token) {
    throw new Error(t("配对响应缺 device_id/token"));
  }
  localStorage.setItem(STORAGE_KEY, JSON.stringify(device));
  return device;
}

// 已配对的设备;未配对返回 null。
function storedDevice() {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
  } catch {
    return null;
  }
}

// ---- 通知流:SSE 订阅(GET /v1/events,带设备 token 认证)----
// EventSource 无法带 Authorization 头,用 fetch 读流(零依赖 SSE 客户端)。
function connectNotifications(device, threadId, onEvent, onStatus) {
  const params = new URLSearchParams({
    thread_id: threadId,
    device_id: device.device_id,
    // 断线重连:带上已消费的 cursor,服务端补发不丢终态(R-270 批2)。
    cursor: String(lastCursor || 0),
  });
  const controller = new AbortController();
  let reader = null;

  async function run() {
    try {
      const res = await fetch(`/v1/events?${params}`, {
        headers: { Authorization: `Bearer ${device.token}` },
        signal: controller.signal,
      });
      if (!res.ok) {
        const data = await res.json().catch(() => ({}));
        onStatus(t("连接失败: {0}", data.error || res.status));
        scheduleReconnect();
        return;
      }
      onStatus(t("已连接"));
      reader = res.body.getReader();
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
            if (line.startsWith("data: ")) {
              const event = JSON.parse(line.slice(6));
              if (typeof event.sequence === "number") {
                lastCursor = event.sequence;
              }
              onEvent(event);
            }
          }
        }
      }
      // 流正常结束(服务端关连接):重连。
      onStatus(t("连接断开,重连中…"));
      scheduleReconnect();
    } catch (err) {
      if (err.name === "AbortError") return;
      onStatus(t("连接错误: {0}", err.message));
      scheduleReconnect();
    }
  }

  let reconnectTimer = null;
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

let lastCursor = 0;
let sseController = null;
let approvalTimer = null;

// ---- approval(R-271 批3):GET pending(脱敏摘要)+ POST answer(批准/拒绝)----
async function fetchPendingApprovals(device) {
  const res = await fetch("/v1/approval/pending", {
    headers: { Authorization: `Bearer ${device.token}` },
  });
  if (!res.ok) throw new Error(t("查询失败({0})", res.status));
  return res.json();
}

async function answerApproval(device, id, reply) {
  const res = await fetch("/v1/approval/answer", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${device.token}`,
    },
    body: JSON.stringify({ id, reply }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || t("回答失败({0})", res.status));
  }
  return res.json();
}

// 轮询 pending approval 并渲染卡片。3s 间隔(轻交互遥控器,不频繁打桥接)。
function startApprovalPolling(device) {
  if (approvalTimer) clearInterval(approvalTimer);
  const render = async () => {
    const container = document.getElementById("approval-list");
    if (!container) return;
    try {
      const data = await fetchPendingApprovals(device);
      container.innerHTML = "";
      const pending = data.pending || [];
      if (pending.length === 0) {
        container.innerHTML = `<p class="muted">${t("当前无待批准请求")}</p>`;
        return;
      }
      for (const ask of pending) {
        const card = document.createElement("div");
        card.className = "card approval";
        const desc =
          ask.kind === "question"
            ? `${ask.resource}`
            : `${ask.action}: ${ask.resource}`;
        card.innerHTML = `
          <p class="approval-desc">${escapeHtml(desc)}</p>
          <p class="muted">${escapeHtml(ask.session_id || "")} · ${t("请求")} #${ask.id}</p>
          <div class="approval-actions">
            <button class="approve" data-id="${ask.id}">${t("批准")}</button>
            <button class="reject" data-id="${ask.id}">${t("拒绝")}</button>
          </div>`;
        card.querySelector(".approve").addEventListener("click", async () => {
          await submitAnswer(device, ask.id, "allow", card);
        });
        card.querySelector(".reject").addEventListener("click", async () => {
          await submitAnswer(device, ask.id, "deny", card);
        });
        container.appendChild(card);
      }
    } catch (err) {
      container.innerHTML = `<p class="muted">${escapeHtml(t("approval 查询失败: {0}", err.message || err))}</p>`;
    }
  };
  render();
  approvalTimer = setInterval(render, 3000);
}

async function submitAnswer(device, id, reply, card) {
  try {
    await answerApproval(device, id, reply);
    card.innerHTML = `<p class="muted">${escapeHtml(t("已{0} #{1}", reply === "allow" ? t("批准") : t("拒绝"), id))}</p>`;
  } catch (err) {
    card.innerHTML = `<p class="muted">${escapeHtml(t("失败: {0}", err.message || err))}</p>`;
  }
}

function escapeHtml(text) {
  return String(text)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ---- 发消息(R-271 批2):POST /v1/messages(thread_id + 消息体)----
async function sendMessage(device, threadId, text) {
  const res = await fetch("/v1/messages", {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${device.token}`,
    },
    body: JSON.stringify({ thread_id: threadId, text }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || t("发送失败({0})", res.status));
  }
  return res.json();
}

// ---- 渲染 ----
function renderPairForm() {
  app.innerHTML = `
    <h1>${t("配对")}</h1>
    <div class="card">
      <p>${t("在电脑的 kanzei 设置页启动移动端桥接,输入显示的配对码:")}</p>
      <input id="pair-code" placeholder="${t("配对码")}" autocomplete="off">
      <button id="pair-btn">${t("配对")}</button>
      <p id="pair-msg"></p>
    </div>`;
  document.getElementById("pair-btn").addEventListener("click", async () => {
    const code = document.getElementById("pair-code").value.trim();
    const msg = document.getElementById("pair-msg");
    msg.textContent = t("配对中…");
    try {
      const device = await pair(code);
      msg.textContent = t("配对成功: {0}", device.device_id);
      setTimeout(renderApp, 300);
    } catch (err) {
      msg.textContent = String(err.message || err);
    }
  });
}

function renderNotifications(device) {
  app.innerHTML = `
    <h1>kanzei <span id="conn-status" class="status">${t("连接中…")}</span></h1>
    <div class="card" id="thread-row">
      <label>${t("会话 ID")}</label>
      <input id="thread-id" placeholder="${t("输入会话 ID(如 session-1)")}" value="${escapeHtml(lastThreadId)}">
      <button id="subscribe-btn">${t("订阅")}</button>
      <p id="thread-msg" class="muted"></p>
    </div>
    <div class="card" id="send-row">
      <label>${t("发消息到电脑")}</label>
      <input id="msg-text" placeholder="${t("输入消息内容")}">
      <button id="send-btn">${t("发送")}</button>
      <p id="send-msg" class="muted"></p>
    </div>
    <div class="card">
      <h2>${t("待批准请求")}</h2>
      <div id="approval-list"><p class="muted">${t("加载中…")}</p></div>
    </div>
    <div class="card"><div id="notice-list"></div></div>
    <button id="unpair-btn" class="danger">${t("解除配对")}</button>`;

  document.getElementById("subscribe-btn").addEventListener("click", () => {
    const threadId = document.getElementById("thread-id").value.trim();
    const threadMsg = document.getElementById("thread-msg");
    if (!threadId) {
      threadMsg.textContent = t("请输入会话 ID");
      return;
    }
    threadMsg.textContent = "";
    lastThreadId = threadId;
    subscribe(device, threadId);
  });
  document.getElementById("send-btn").addEventListener("click", async () => {
    const threadId = document.getElementById("thread-id").value.trim();
    const text = document.getElementById("msg-text").value.trim();
    const sendMsg = document.getElementById("send-msg");
    if (!threadId) {
      sendMsg.textContent = t("请先输入会话 ID");
      return;
    }
    if (!text) {
      sendMsg.textContent = t("消息内容不能为空");
      return;
    }
    sendMsg.textContent = t("发送中…");
    try {
      await sendMessage(device, threadId, text);
      sendMsg.textContent = t("已发送");
      document.getElementById("msg-text").value = ""; // 发送后清空
    } catch (err) {
      sendMsg.textContent = String(err.message || err);
    }
  });
  document.getElementById("unpair-btn").addEventListener("click", () => {
    if (sseController) sseController.abort();
    if (approvalTimer) clearInterval(approvalTimer);
    localStorage.removeItem(STORAGE_KEY);
    location.reload();
  });
  // approval 轮询(已配对即启动,不依赖订阅会话)。
  startApprovalPolling(device);
  // 默认自动订阅上次会话。
  if (lastThreadId) subscribe(device, lastThreadId);
}

function subscribe(device, threadId) {
  if (sseController) sseController.abort();
  const statusEl = document.getElementById("conn-status");
  const listEl = document.getElementById("notice-list");
  listEl.innerHTML = "";
  sseController = connectNotifications(
    device,
    threadId,
    (event) => {
      const item = document.createElement("div");
      item.className = "notice";
      item.textContent = `[${event.sequence}] ${event.kind || "event"} — ${event.summary || ""}`;
      listEl.prepend(item);
      // 长列表窗口化:最多保留 100 条(轻交互遥控器,不无限堆积)。
      while (listEl.children.length > 100) listEl.removeChild(listEl.lastChild);
    },
    (status) => {
      statusEl.textContent = status;
    },
  );
}

let lastThreadId = "";
const app = document.getElementById("app");

function renderApp() {
  const device = storedDevice();
  if (!device) {
    renderPairForm();
  } else {
    renderNotifications(device);
  }
}

renderApp();
