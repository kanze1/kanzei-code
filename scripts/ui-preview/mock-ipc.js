// kanzei UI 预览:模拟 Tauri IPC(经典脚本,由 server.mjs 注入到 index.html <head> 最前面)。
//
// 01-core.js 在模块求值时读 window.__TAURI__.core.invoke / .event.listen,
// 13-memory-chat.js / 23-voice.js 用 window.__TAURI__.core.Channel;这里全部补齐。
//
// 页面内 API(window.__kzPreview):
//   emit(event, payload)        向前端回放一条后端事件,如 emit("kz:text", {sessionId, text})
//   calls                       全部 invoke 调用记录 [{cmd, args, at}]
//   unknown()                   未模拟的命令名列表(每个只 console.info 一次)
//   setCommand(cmd, value|fn)   运行时覆盖某条命令的返回(fn(args, ctx) 可返回 Promise;抛错 = 后端报错)
//   settle(ms)                  等 IPC 空闲(无在途 invoke)
//   ready / readyPromise        场景编排完成标志;<html data-kz-preview-ready="<scene>"> 同步置位
//   scene / theme / params      当前 URL 参数
(() => {
  "use strict";
  const params = new URLSearchParams(location.search);
  const scene = params.get("scene") || "chat";
  const theme = params.get("theme") === "light" ? "light" : "dark";
  const PREFIX = "[kz-preview]";

  // 可复现:默认清空上次交互残留的偏好(工作区视图、面板开合、主题……)。?keep=1 保留。
  try {
    if (params.get("keep") !== "1") {
      for (const key of Object.keys(localStorage)) if (key.startsWith("kz")) localStorage.removeItem(key);
    }
    localStorage.setItem("kz-theme", theme);
  } catch { /* 隐私模式等拿不到 storage:忽略 */ }
  document.documentElement.setAttribute("data-theme", theme);
  document.documentElement.dataset.kzPreview = scene;

  const listeners = new Map(); // event -> Set<handler>
  const overrides = new Map();
  const unknown = new Set();
  const calls = [];
  let inflight = 0;
  let eventSeq = 0;
  let channelSeq = 0;

  const fixturesPromise = import("/__preview/fixtures.mjs").then((module) =>
    module.createFixtures({ scene, theme, params: Object.fromEntries(params) }));

  const clone = (value) => {
    if (value === undefined) return null;
    try { return structuredClone(value); } catch { return JSON.parse(JSON.stringify(value)); }
  };

  function emit(event, payload) {
    const handlers = listeners.get(event);
    if (!handlers || !handlers.size) return 0;
    const envelope = { event, id: ++eventSeq, windowLabel: "main", payload: clone(payload) };
    for (const handler of [...handlers]) {
      try { handler(envelope); } catch (error) { console.warn(PREFIX, `事件 ${event} 的处理函数抛错`, error); }
    }
    return handlers.size;
  }

  const ctx = { emit, scene, theme, params, calls };

  async function invoke(cmd, args) {
    const input = args && typeof args === "object" ? args : {};
    calls.push({ cmd, args: clone(input), at: Date.now() });
    inflight += 1;
    try {
      const fixtures = await fixturesPromise;
      // 真机 IPC 是毫秒级异步:给一个宏任务的间隔,别让所有 await 都在微任务里抢跑。
      await new Promise((resolve) => setTimeout(resolve, fixtures.latencyMs ?? 0));
      const handler = overrides.has(cmd) ? overrides.get(cmd) : fixtures.commands[cmd];
      if (handler === undefined) {
        if (!unknown.has(cmd)) {
          unknown.add(cmd);
          console.info(PREFIX, `未模拟的命令 ${cmd},返回默认空值`);
        }
        return clone(fixtures.defaultFor(cmd, input));
      }
      const value = typeof handler === "function" ? await handler(input, ctx) : handler;
      return clone(value);
    } catch (error) {
      // Tauri 的 invoke 以字符串 reject;保持同形,前端的 `${err}` 拼接才不会多出 "Error: "。
      throw typeof error === "string" ? error : String(error?.message ?? error);
    } finally {
      inflight -= 1;
    }
  }

  function listen(event, handler) {
    if (!listeners.has(event)) listeners.set(event, new Set());
    listeners.get(event).add(handler);
    return Promise.resolve(() => listeners.get(event)?.delete(handler));
  }

  class Channel {
    constructor() {
      this.id = ++channelSeq;
      this.onmessage = () => {};
    }
    toJSON() { return `__CHANNEL__:${this.id}`; }
  }

  window.__TAURI__ = {
    core: { invoke, Channel, transformCallback: (fn) => fn },
    event: {
      listen,
      once: (event, handler) => {
        let off = null;
        const wrapped = (payload) => { off?.(); handler(payload); };
        return listen(event, wrapped).then((unlisten) => (off = unlisten));
      },
      emit: async (event, payload) => { emit(event, payload); },
    },
  };

  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  async function settle(quietMs = 120, timeoutMs = 6000) {
    const started = performance.now();
    let quietSince = performance.now();
    while (performance.now() - started < timeoutMs) {
      await sleep(30);
      if (inflight > 0) quietSince = performance.now();
      else if (performance.now() - quietSince >= quietMs) return true;
    }
    return false;
  }

  let markReady;
  const readyPromise = new Promise((resolve) => { markReady = resolve; });
  const api = {
    scene, theme, params, calls, emit, settle, readyPromise, ready: false,
    unknown: () => [...unknown],
    listeners: () => [...listeners.keys()],
    setCommand: (cmd, value) => overrides.set(cmd, value),
    fixtures: () => fixturesPromise,
  };
  window.__kzPreview = api;

  // 启动编排:18-startup.js 最后一步 navigate_view 会写 body.dataset.view;
  // 再等 IPC 静默,然后跑场景脚本。任何一步失败都只记 warn,不让预览页冒 console.error。
  async function boot() {
    const started = performance.now();
    while (!document.body?.dataset.view && performance.now() - started < 10000) await sleep(40);
    if (!document.body?.dataset.view) console.warn(PREFIX, "启动序列 10s 内未完成,仍继续编排场景");
    await settle();
    try {
      const scenes = await import("/__preview/scenes.mjs");
      const fixtures = await fixturesPromise;
      await scenes.runScene(scene, { ...ctx, fixtures, settle, sleep });
    } catch (error) {
      console.warn(PREFIX, `场景 ${scene} 编排失败`, error);
    }
    await settle();
    api.ready = true;
    document.documentElement.dataset.kzPreviewReady = scene;
    markReady(scene);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", () => void boot());
  else void boot();
})();
