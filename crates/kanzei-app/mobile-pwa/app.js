// kanzei 移动端 PWA(R-271):配对 + 通知流只读(批1)。
// 原生 JS,零构建零框架,由 R-270 桥接 serve。
// 协议契约沿用 docs/design/r059_mobile_agent_communication.md 阶段A字段。

const STORAGE_KEY = "kanzei_device";

// ---- 配对:输入配对码换 device_id + token,存 localStorage ----
async function pair(pairCode) {
  const res = await fetch("/v1/pair", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ pair_code: pairCode }),
  });
  if (!res.ok) {
    const data = await res.json().catch(() => ({}));
    throw new Error(data.error || `配对失败(${res.status})`);
  }
  const device = await res.json();
  if (!device.device_id || !device.token) {
    throw new Error("配对响应缺 device_id/token");
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
        onStatus(`连接失败: ${data.error || res.status}`);
        scheduleReconnect();
        return;
      }
      onStatus("已连接");
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
      onStatus("连接断开,重连中…");
      scheduleReconnect();
    } catch (err) {
      if (err.name === "AbortError") return;
      onStatus(`连接错误: ${err.message}`);
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

// ---- 渲染 ----
function renderPairForm() {
  app.innerHTML = `
    <h1>配对</h1>
    <div class="card">
      <p>在电脑的 kanzei 设置页启动移动端桥接,输入显示的配对码:</p>
      <input id="pair-code" placeholder="配对码" autocomplete="off">
      <button id="pair-btn">配对</button>
      <p id="pair-msg"></p>
    </div>`;
  document.getElementById("pair-btn").addEventListener("click", async () => {
    const code = document.getElementById("pair-code").value.trim();
    const msg = document.getElementById("pair-msg");
    msg.textContent = "配对中…";
    try {
      const device = await pair(code);
      msg.textContent = `配对成功: ${device.device_id}`;
      setTimeout(renderApp, 300);
    } catch (err) {
      msg.textContent = String(err.message || err);
    }
  });
}

function renderNotifications(device) {
  app.innerHTML = `
    <h1>kanzei <span id="conn-status" class="status">连接中…</span></h1>
    <div class="card" id="thread-row">
      <label>会话 ID</label>
      <input id="thread-id" placeholder="输入会话 ID(如 session-1)" value="${lastThreadId}">
      <button id="subscribe-btn">订阅</button>
    </div>
    <div class="card"><div id="notice-list"></div></div>
    <button id="unpair-btn" class="danger">解除配对</button>`;

  document.getElementById("subscribe-btn").addEventListener("click", () => {
    const threadId = document.getElementById("thread-id").value.trim();
    if (!threadId) return alert("请输入会话 ID");
    lastThreadId = threadId;
    subscribe(device, threadId);
  });
  document.getElementById("unpair-btn").addEventListener("click", () => {
    if (sseController) sseController.abort();
    localStorage.removeItem(STORAGE_KEY);
    location.reload();
  });
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
