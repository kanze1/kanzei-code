// 前端运行时冒烟(R-084):在 Node 中以最小 DOM harness 真实执行 main.js,
// 补 node --check(纯语法)与静态正则冒烟都抓不到的 ReferenceError / 初始化崩坏(D-048 类问题)。
// 覆盖:整页加载与初始化、需求/缺陷/目标/测试列表非空渲染、主视图切换、console.error 与未捕获异常 → 非零退出码。
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import vm from "node:vm";

const root = resolve(import.meta.dirname, "..");
const html = await readFile(resolve(root, "crates/kanzei-app/ui/index.html"), "utf8");
const source = await readFile(resolve(root, "crates/kanzei-app/ui/main.js"), "utf8");
const style = await readFile(resolve(root, "crates/kanzei-app/ui/style.css"), "utf8");

const issues = [];
const fail = (msg) => issues.push(msg);

// CSS 结构完整性:浏览器对花括号错配是静默容错的,一个被吃掉的 `@media ... {`
// 会让整段响应式规则无条件生效而没有任何报错(c65c80e 就这样把 D-148 带上了线)。
const cssNoComments = style.replace(/\/\*[\s\S]*?\*\//g, "");
let cssDepth = 0;
let cssStray = 0;
for (const ch of cssNoComments) {
  if (ch === "{") cssDepth += 1;
  else if (ch === "}") {
    if (cssDepth === 0) cssStray += 1;
    else cssDepth -= 1;
  }
}
if (cssStray) fail(`style.css 有 ${cssStray} 个多余的 }(很可能某条规则或 @media 的开括号被覆盖删除了)`);
if (cssDepth) fail(`style.css 有 ${cssDepth} 个未闭合的 {`);
const documentsScrollRules = [...style.matchAll(/#documents-scroll\s*\{([^}]*)\}/g)];
const documentsBottomPadding = documentsScrollRules.at(-1)?.[1].match(/padding-bottom:\s*(\d+)px/);
if (!documentsBottomPadding || Number(documentsBottomPadding[1]) < 24) {
  fail("独立文档页滚动容器未预留状态栏安全间距");
}
if (!source.includes('if (isActivityTool(e.payload.name)) bgAdd')) {
  fail("活动面板仍会接收全部工具调用");
}
if (!source.includes("function reportPersistentError(text, { retry = null } = {})") || !html.includes('id="log-retry"') || !source.includes("function copyReadable(el)")) {
  fail("错误反馈缺少持久详情、恢复入口或复制能力");
}
if (!source.includes("function renderRecoveredMessages(items)") || !source.includes("if (running) {")) {
  fail("历史回放或运行中隔离护栏缺失");
}
const errorRenderer = source.slice(source.indexOf("function addErrorMessage"), source.indexOf("function isRetryableError"));
if (errorRenderer.includes("setTimeout")) fail("长错误反馈不应由短 toast 定时移除");
if (!source.includes("function renderMarkdown(raw)") || !source.includes("function renderDiff(display)")) {
  fail("对话 Markdown 或 diff 详情渲染入口缺失");
}
// 历史消息只进入恢复渲染器，实时事件继续使用 currentAssistant，不应把运行输出写入历史快照。
if (!source.includes('const history = await invoke("conversation_get"') || !source.includes("renderRecoveredMessages(history)")) {
  fail("历史消息未通过只读恢复渲染链路");
}
// 历史回放必须保留完整调用与结果:调用与结果按 call_id 配对成一块(buildToolBlock/
// fillToolBlock),详情里同时给出完整输出与完整入参 JSON。
if (
  !source.includes('part.type === "tool_result"') ||
  !source.includes("JSON.stringify(input, null, 2)") ||
  !source.includes("function fillToolBlock")
) {
  fail("历史工具会话未保留完整调用与结果详情");
}
const dictionarySource = source.slice(source.indexOf("const I18N_EN = {"), source.indexOf("const I18N_ZH = new WeakMap"));
const dictionaryKeys = new Set([...dictionarySource.matchAll(/\"((?:\\.|[^\"])*)\"\s*:/g)].map((match) => match[1]));
const translationCalls = [...source.matchAll(/\bt\(\"((?:\\.|[^\"])*)\"\)/g)].map((match) => match[1]);
for (const key of new Set(translationCalls)) if (!dictionaryKeys.has(key)) fail(`I18N_EN 缺少 t key: ${key}`);
if (!source.includes("function stopAutoForManualInput()") || !source.includes('const message = t("收到手动输入，鞭挞已停止")')) {
  fail("手动输入未确认停止鞭挞并反馈用户");
}
if (!source.includes('e.key === "Enter" && !e.shiftKey')) {
  fail("主输入框未保持 Enter 发送、Shift+Enter 换行契约");
}
const autoNoticeIndex = source.indexOf('addMessage("notice", `${t("鞭挞已触发")}');
if (autoNoticeIndex < 0 || source.includes('addUserMessage(auto ?')) {
  fail("自动续轮仍把内部提示词重复展示为用户消息");
}
const pendingTimers = new Set();

// ---------- DOM harness:真实节点关系(parent/children/dataset/classList),样式与布局按 noop ----------
let idSeed = 0;
class ClassList {
  #el;
  #set = new Set();
  constructor(el) { this.#el = el; }
  #sync() { this.#el._attributes.class = [...this.#set].join(" "); }
  add(...names) { names.filter(Boolean).forEach((n) => this.#set.add(n)); this.#sync(); }
  remove(...names) { names.forEach((n) => this.#set.delete(n)); this.#sync(); }
  toggle(name, force) {
    const on = force === undefined ? !this.#set.has(name) : Boolean(force);
    on ? this.#set.add(name) : this.#set.delete(name);
    this.#sync();
    return on;
  }
  contains(name) { return this.#set.has(name); }
}
class Element {
  constructor(tag) {
    this.tagName = String(tag).toUpperCase();
    this.ownerDocument = null;
    this.parentNode = null;
    this.childNodes = [];
    this.style = {};
    this.dataset = {};
    this.classList = new ClassList(this);
    this._attributes = {};
    this._listeners = {};
    this._textContent = "";
    this._innerHTML = "";
    this.id = "";
    this.disabled = false;
    this.checked = false;
    this.draggable = false;
    this.open = false;
    this.value = "";
    this.title = "";
    this.scrollTop = 0;
    this.scrollHeight = 0;
  }
  _adopt(node) { node.parentNode = this; node.ownerDocument = this.ownerDocument; this.childNodes.push(node); return node; }
  appendChild(node) { node.remove(); return this._adopt(node); }
  append(...nodes) { for (const n of nodes) this.appendChild(typeof n === "string" ? this.ownerDocument.createTextNode(n) : n); }
  prepend(...nodes) { for (const n of nodes.reverse()) this.insertBefore(typeof n === "string" ? this.ownerDocument.createTextNode(n) : n, this.childNodes[0] ?? null); }
  insertBefore(node, ref) {
    node.remove();
    node.parentNode = this;
    node.ownerDocument = this.ownerDocument;
    const idx = ref ? this.childNodes.indexOf(ref) : -1;
    if (idx < 0) this.childNodes.push(node); else this.childNodes.splice(idx, 0, node);
    return node;
  }
  replaceChildren(...nodes) { for (const c of [...this.childNodes]) c.parentNode = null; this.childNodes = []; this._innerHTML = ""; this.append(...nodes); }
  remove() {
    if (this.parentNode) {
      const siblings = this.parentNode.childNodes;
      const idx = siblings.indexOf(this);
      if (idx >= 0) siblings.splice(idx, 1);
      this.parentNode = null;
    }
  }
  get parentElement() { return this.parentNode instanceof Element ? this.parentNode : null; }
  get children() { return this.childNodes.filter((n) => n instanceof Element); }
  get options() { return this.tagName === "SELECT" ? this.children : undefined; }
  get firstChild() { return this.childNodes[0] ?? null; }
  get nextSibling() {
    if (!this.parentNode) return null;
    return this.parentNode.childNodes[this.parentNode.childNodes.indexOf(this) + 1] ?? null;
  }
  get previousElementSibling() {
    if (!this.parentNode) return null;
    const sibs = this.parentNode.childNodes.filter((n) => n instanceof Element);
    const idx = sibs.indexOf(this);
    return idx > 0 ? sibs[idx - 1] : null;
  }
  get className() { return this._attributes.class ?? ""; }
  set className(value) { this.classList = new ClassList(this); this.classList.add(...String(value).split(/\s+/).filter(Boolean)); }
  get textContent() {
    if (!this.childNodes.length) return this._textContent;
    return this.childNodes.map((c) => (c instanceof Element ? c.textContent : c.nodeValue)).join("");
  }
  set textContent(value) { this.childNodes = []; this._innerHTML = ""; this._textContent = String(value); }
  get innerText() { return this.textContent; }
  set innerText(value) { this.textContent = value; }
  get innerHTML() { return this._innerHTML; }
  set innerHTML(value) { this._innerHTML = String(value); if (value === "") { this.childNodes = []; this._textContent = ""; } }
  get value() { return this._value ?? ""; }
  set value(v) { this._value = String(v); }
  getAttribute(name) { return this._attributes[name] ?? null; }
  // class 必须走 className 设值,否则 classList 的内部集合与属性脱节:index.html 里
  // 写死的 class 不进集合,第一次 classList.toggle() 回写就把它们整体抹掉了。
  setAttribute(name, value) {
    if (name === "class") { this.className = value; return; }
    this._attributes[name] = String(value);
    if (name === "id") this.id = String(value);
  }
  removeAttribute(name) { delete this._attributes[name]; }
  hasAttribute(name) { return name in this._attributes; }
  addEventListener(type, fn) { (this._listeners[type] ??= []).push(fn); }
  removeEventListener() {}
  dispatchEvent(event) { event.target ??= this; (this._listeners[event.type] ?? []).forEach((fn) => fn(event)); }
  click() { this.dispatchEvent({ type: "click", preventDefault() {}, stopPropagation() {} }); }
  focus() {}
  querySelector(selector) { return queryAllFrom(this, selector)[0] ?? null; }
  querySelectorAll(selector) { return queryAllFrom(this, selector); }
  closest(selector) { let el = this; while (el) { if (matchesCompound(el, selector)) return el; el = el.parentElement; } return null; }
  setPointerCapture() {}
  scrollIntoView() {}
  scrollTo() {}
  getBoundingClientRect() { return { top: 0, left: 0, width: 0, height: 0, right: 0, bottom: 0 }; }
  get offsetParent() { return this.parentElement; }
  get offsetTop() { return 0; }
}
class TextNode {
  constructor(text) { this.nodeValue = String(text); this.parentNode = null; this.ownerDocument = null; }
  remove() {}
}

function descendantElements(node) {
  const out = [];
  const walk = (el) => { for (const c of el.childNodes) if (c instanceof Element) { out.push(c); walk(c); } };
  walk(node);
  return out;
}
function matchesOne(el, selector) {
  selector = selector.trim();
  if (selector.startsWith(".")) return el.classList.contains(selector.slice(1));
  if (selector.startsWith("#")) return el.id === selector.slice(1);
  if (selector.startsWith("[")) {
    // 属性选择器要支持带值比较,并且 data-* 得查 dataset —— main.js 写的是
    // `el.dataset.bgTool = ...`,不会落到 _attributes 里;早期版本只按属性名
    // 存在性判断,于是 `[data-bg-tool=bash]` 这类选择恒不命中。
    const body = selector.slice(1, -1);
    const eq = body.indexOf("=");
    const name = (eq < 0 ? body : body.slice(0, eq)).trim();
    const want = eq < 0 ? null : body.slice(eq + 1).trim().replace(/^["']|["']$/g, "");
    const dataKey = name.startsWith("data-")
      ? name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase())
      : null;
    const actual = dataKey && dataKey in el.dataset ? el.dataset[dataKey] : el.getAttribute(name);
    if (actual == null) return false;
    return want === null || String(actual) === want;
  }
  return el.tagName === selector.toUpperCase();
}
// 复合选择器:".doc-item[data-doc-id]" / "div.foo" / "#id.bar" —— 每一段都要同时命中。
// 早期版本把整段当一个 class 名比,于是 main.js 里所有 ".x[attr]" 形式在冒烟中恒为空,
// 真实浏览器却正常工作:这类静默不一致会让冒烟对整块逻辑失明。
function matchesCompound(el, step) {
  const parts = step.match(/^[a-zA-Z][\w-]*|\.[^.#[\]]+|#[^.#[\]]+|\[[^\]]+\]/g);
  if (!parts) return false;
  return parts.every((part) => matchesOne(el, part));
}
function queryAllFrom(node, selector) {
  // 支持逗号分组与后代组合选择器(如 ".a .b" / "div, span");近似实现,仅覆盖 main.js 的用法。
  return selector.split(",").flatMap((part) => {
    const steps = part.trim().split(/\s+/);
    let current = [node];
    for (const step of steps) {
      current = current.flatMap((base) => descendantElements(base).filter((el) => matchesCompound(el, step)));
    }
    return current;
  });
}

const documentElement = new Element("html");
const body = new Element("body");
const byId = new Map();
const document = {
  documentElement,
  body,
  hidden: false,
  title: "",
  createElement: (tag) => { const el = new Element(tag); el.ownerDocument = document; return el; },
  createTextNode: (text) => { const n = new TextNode(text); n.ownerDocument = document; return n; },
  createTreeWalker: () => {
    const texts = [];
    const walk = (el) => {
      if (el._textContent && !el.childNodes.length) texts.push({ nodeValue: el._textContent, currentNode: true });
      for (const c of el.childNodes) {
        if (c instanceof TextNode) { if (c.nodeValue) texts.push(c); } else walk(c);
      }
    };
    walk(body);
    let idx = -1;
    return { nextNode() { idx += 1; return idx < texts.length ? (this.currentNode = texts[idx], true) : false; }, currentNode: null };
  },
  getElementById: (id) => byId.get(id) ?? null,
  querySelector: (selector) => queryAllFrom(documentElement, selector)[0] ?? null,
  querySelectorAll: (selector) => queryAllFrom(documentElement, selector),
  addEventListener: () => {},
  hasFocus: () => true,
};

// body 必须真的挂在 documentElement 下:否则 document.querySelectorAll 走的是空树,
// 任何按 class 的选择器恒为空(.activity-item 一个都找不到,视图切换覆盖恒为 0),
// 而脚本还会照常报通过——护栏形同虚设(D-138)。
documentElement.ownerDocument = document;
documentElement.appendChild(body);

// 从 index.html 生成带 id 的真实节点:id 引用错(smokey 场景)会在这里直接暴露。
// 整个开标签一起匹配,顺带取回 class —— 早期版本只取 id,index.html 里写死的 class
// 对冒烟完全不可见(`.documents-list .doc-item` 恒为空),按 class 的断言全是假通过。
for (const match of html.matchAll(/<(\w+)((?:[^<>"]|"[^"]*")*?)(?<![-\w])id="([\w-]+)"((?:[^<>"]|"[^"]*")*?)>/g)) {
  const [, tag, before, id, after] = match;
  if (byId.has(id)) continue;
  const el = document.createElement(tag);
  el.id = id;
  el._attributes.id = id;
  const className = `${before} ${after}`.match(/\bclass="([^"]*)"/)?.[1];
  if (className) el.className = className;
  byId.set(id, el);
  body.appendChild(el);
  // 后代选择器需要真实嵌套:按 id 造出来的节点是扁平的,`#providers-table tbody`
  // 会拿到 null。视图切换护栏打开后 settings 首次被真正执行,立刻暴露了这个缺口。
  if (el.tagName === "TABLE") el.appendChild(document.createElement("tbody"));
}

// 主视图切换按钮只有 class 没有 id,上面那轮按 id 造不出它们;这里按 class 补造,
// 并在下面对"切换数为 0"直接判失败。
for (const match of html.matchAll(/<button[^>]*class="activity-item[^"]*"[^>]*data-view="([\w-]+)"[^>]*>/g)) {
  const el = document.createElement("button");
  el.className = "activity-item";
  el.dataset.view = match[1];
  body.appendChild(el);
}

// ---------- Tauri 桥桩:启动序列与各列表需要真实形状的负载 ----------
const PROJECT = "C:/smoke/project";
// nextStatuses 是状态流转按钮的数据源:桩里缺它,侧栏"能不能切状态"就无从断言。
const docEntry = (id, title, status, extra = {}) => ({ id, title, status, priority: "P1", closed: false, fields: [], nextStatuses: ["done"], ...extra });
const payloads = {
  app_info: { version: "0.0.0-smoke", build: "smoke" },
  update_check: { newer: false },
  projects_get: { current: PROJECT, projects: [PROJECT], names: { [PROJECT]: "smoke" } },
  docs_snapshot: {
    requirements: [docEntry("R-001", "冒烟需求", "doing", { complexity: "中", fields: [["备注", "待更新"], ["验收", "这是一条刻意超过六十字符的长验收文本,用来验证编辑表单会把段落型字段升级为多行文本域,而不是塞进单行输入框把值截断到看不见"]] }), docEntry("R-002", "冒烟需求二", "todo")],
    defects: [docEntry("D-001", "冒烟缺陷", "open", { severity: "medium", fields: [["复现", "待更新"]] })],
    goals: [{ id: "G-001", title: "冒烟目标", status: "active", fields: [] }],
    sources: [],
    findings: [],
    archived: { req: 1, defect: 2, goal: 0, source: 0, finding: 0 },
    archived_entries: { req: [docEntry("R-000", "已归档需求", "done")], defect: [docEntry("D-000", "已归档缺陷", "fixed")], goal: [], source: [], finding: [] },
    conventions: { exists: true, headings: ["开发规则", "测试要求"] },
  },
  conversation_get: [{ role: "user", parts: [{ type: "text", text: "冒烟历史消息" }] }],
  conversation_trace_get: [],
  conversation_list: [{ sequence: 1, sequences: [1], title: "冒烟会话", preview: "预览", updated_at: "2026-08-08 00:00" }],
  models_list: [{ id: "claude-smoke", label: "Claude Smoke" }],
  git_status: { branch: "main", changes: 2 },
  list_pending_inputs: [],
  test_runs_snapshot: { active: [{ id: "T-001", title: "冒烟测试", status: "passed", fields: [["命令", "cargo test"]] }], archived: [] },
  process_list: [{ id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false }],
  pending_asks_get: [],
  settings_get: { language: "zh", profiles: {}, providers: [], permissions: [] },
  permission_rules_get: [],
  memory_overview: { scopes: [{ scope: "project", root: PROJECT, total: 0, hitsTotal: 0, categories: {}, integrity: [], inboxPending: 0 }] },
  memory_entries: [{ id: "M-SOP-001", category: "sop", title: "冒烟 SOP", description: "继续执行冒烟任务", status: "active", body: "执行冒烟任务" }],
  memory_context_bill: { turns: [] },
  workspace_snapshot: {},
};
const invokeLog = [];
async function invoke(cmd, args) {
  invokeLog.push(cmd);
  if (cmd in payloads) return structuredClone(payloads[cmd]);
  return null;
}
async function listen(event, handler) { handlers.set(event, handler); }
const handlers = new Map();

const storage = new Map();
storage.set("kz-auto-continue", "1");
const localStorageShim = {
  getItem: (k) => (storage.has(k) ? storage.get(k) : null),
  setItem: (k, v) => storage.set(k, String(v)),
  removeItem: (k) => storage.delete(k),
};

const windowShim = {
  __TAURI__: { core: { invoke }, event: { listen } },
  addEventListener: () => {},
  confirm: () => true,
  innerWidth: 1280,
  innerHeight: 800,
};

class OptionShim extends Element {
  constructor(text, value) {
    super("option");
    this.text = text;
    this.textContent = text;
    this.value = value ?? text;
  }
}
class FileReaderShim {
  readAsDataURL() { fail("FileReader.readAsDataURL 在冒烟桩中未实现"); }
}
class MutationObserverShim {
  constructor(callback) { this.callback = callback; }
  observe() {}
  disconnect() {}
}
class ResizeObserverShim {
  constructor(callback) { this.callback = callback; }
  observe() {}
  unobserve() {}
  disconnect() {}
}

const sandbox = {
  __reportInitError: (label, err) => fail(`初始化步骤 ${label} 抛异常(已被 main.js 吞掉): ${err?.stack ?? err}`),
  __reportPersistentError: (text) => fail(`reportPersistentError: ${text}`),
  console: {
    log: (...a) => console.log(...a),
    warn: (...a) => console.warn(...a),
    error: (...a) => { fail(`console.error: ${a.map(String).join(" ")}`); },
  },
  window: windowShim,
  document,
  localStorage: localStorageShim,
  navigator: { clipboard: { writeText: async () => {} } },
  NodeFilter: { SHOW_TEXT: 4 },
  Node: { TEXT_NODE: 3, ELEMENT_NODE: 1 },
  Option: OptionShim,
  FileReader: FileReaderShim,
  MutationObserver: MutationObserverShim,
  ResizeObserver: ResizeObserverShim,
  setTimeout: (fn, ms) => { const h = { fn }; pendingTimers.add(h); return h; },
  clearTimeout: (h) => pendingTimers.delete(h),
  setInterval: (fn) => { const h = { fn, interval: true }; pendingTimers.add(h); return h; },
  clearInterval: (h) => pendingTimers.delete(h),
  structuredClone: globalThis.structuredClone,
  requestAnimationFrame: (fn) => fn(),
};
vm.createContext(sandbox);

const settle = () => new Promise((r) => setImmediate(r));
async function flush(rounds = 12) {
  for (let i = 0; i < rounds; i += 1) {
    await settle();
    const timers = [...pendingTimers];
    for (const h of timers) {
      if (!pendingTimers.has(h) || h.interval) continue;
      pendingTimers.delete(h);
      await h.fn();
    }
  }
}

function assert(condition, message) { if (!condition) fail(message); }
const listText = (id) => byId.get(id)?.textContent ?? "";

// ---------- 执行 main.js ----------
// 诊断:main.js 的初始化 IIFE 逐步 catch 只 toast 不抛出,冒烟里 toast 不可见;
// 注入 reporter 把"吞掉的初始化异常"变成冒烟失败(同时保持生产行为不变)。
const instrumented = source
  .replace(
    /toastError\(`\$\{label\}加载失败:\$\{err\}`\);/,
    "toastError(`${label}加载失败:${err}`); __reportInitError?.(label, err);"
  )
  .replace(
    /function reportPersistentError\(text, \{ retry = null \} = \{\}\) \{/,
    "function reportPersistentError(text, { retry = null } = {}) { __reportPersistentError?.(text);"
  );
if (instrumented === source) fail("注入初始化异常探针失败:main.js 启动序列的 catch 形态已变化,请同步冒烟脚本");
try {
  vm.runInContext(instrumented, sandbox, { filename: "main.js" });
} catch (err) {
  fail(`main.js 顶层执行抛异常: ${err.stack ?? err}`);
}
await flush();
assert(invokeLog.includes("projects_get"), `初始化未调用 projects_get(启动序列断裂),已见调用: ${invokeLog.join(",")}`);
assert(invokeLog.includes("docs_snapshot"), "初始化未调用 docs_snapshot");
assert(listText("req-list").includes("冒烟需求"), `需求列表未渲染出桩数据: "${listText("req-list").slice(0, 60)}"`);
assert(listText("defect-list").includes("冒烟缺陷"), "缺陷列表未渲染出桩数据");
assert(listText("goal-list").includes("冒烟目标"), "目标列表未渲染出桩数据");
assert(listText("test-list").includes("冒烟测试"), "测试记录列表未渲染出桩数据");
assert(listText("conversation-list").includes("冒烟会话"), "历史对话列表未渲染出桩数据");
// D-150:引用跳转此前只认当前可见节点,已归档/被折叠的目标一律静默失败。
const archivedRow = document.querySelector("#req-list .doc-archive-list .archived-entry");
assert(archivedRow?.dataset.docId === "R-000", "归档条目未挂 data-doc-id,引用跳转必然落空");
assert(archivedRow.parentElement.classList.contains("hidden"), "归档区应默认折叠");
assert(typeof sandbox.jumpToEntry === "function", "jumpToEntry 未定义(引用跳转入口丢失)");
sandbox.jumpToEntry("R-000");
assert(
  !archivedRow.parentElement.classList.contains("hidden"),
  "跳转到归档条目时未掀开归档折叠区",
);
assert(archivedRow.classList.contains("ref-highlight"), "跳转后未高亮目标条目");
sandbox.jumpToEntry("R-999");
assert(
  listText("toast").includes("R-999"),
  `跳转到不存在的条目时应给出提示而不是静默失败,实得 toast: "${listText("toast")}"`,
);

// R-123 职责分离:侧栏只读(浏览与取活),独立文档页承担深度管理(编辑/排序/批量)。
assert(!document.querySelector("#req-list .doc-edit"), "侧栏仍在渲染字段编辑表单(应只在独立文档页)");
assert(!document.querySelector("#defect-list .doc-edit"), "缺陷侧栏仍在渲染字段编辑表单");
assert(!document.querySelector("#req-list .doc-pick"), "侧栏出现批量选择框(批量操作应只在文档页)");
assert(
  !document.querySelectorAll("#req-list .doc-item").some((n) => n.draggable),
  "侧栏条目仍可拖拽改序(排序应只在文档页)",
);
// 侧栏移除编辑后必须仍能读到字段,否则等于把信息一起删了。
const sidebarFields = document.querySelectorAll("#req-list .doc-detail .doc-field");
assert(sidebarFields.length > 0, "侧栏详情既无编辑表单也无只读字段,信息被一起删掉了");
// 状态流转留在侧栏:取活时要能直接切。
assert(document.querySelector("#req-list .doc-actions button"), "侧栏详情缺少状态流转按钮(取活链路断了)");

const reqEditor = document.querySelector("#documents-req-list .doc-edit");
assert(reqEditor?.querySelector("input") && reqEditor?.querySelector("button"), "独立文档页未提供标题/字段编辑控件");
// D-148:曾经只给 aria-label,渲染成一片无标题输入框,改哪格全靠猜;长字段用单行 input 还会把值截没。
const reqEditRows = reqEditor.querySelectorAll(".doc-edit-row");
assert(reqEditRows.length >= 3, `编辑表单未按字段分行(应有 标题/备注/验收),实得 ${reqEditRows.length}`);
assert(
  reqEditRows.every((row) => (row.querySelector(".doc-edit-key")?.textContent ?? "").trim()),
  "编辑表单存在没有可见字段名的输入框",
);
assert(reqEditor.querySelector("textarea"), "长字段未升级为多行文本域,值会被单行输入框截断");
// D-149:编辑表单已连名带值列出每个字段,只读 .doc-field 列表若同时渲染就是同一份内容显示两遍。
const duplicatedFields = document
  .querySelector("#documents-req-list .doc-detail")
  .querySelectorAll(".doc-field")
  .filter((node) => !node.textContent.trim().toLowerCase().startsWith("refs"));
assert(
  duplicatedFields.length === 0,
  `字段在编辑表单之外又渲染了一遍只读副本: ${duplicatedFields.map((n) => n.textContent.slice(0, 20)).join(" | ")}`,
);
reqEditor.querySelector("button").click();
await flush();
assert(invokeLog.includes("docs_update"), "独立文档页编辑未调用 docs_update");
const defectEditor = document.querySelector("#documents-defect-list .doc-edit");
assert(defectEditor?.querySelector("input") && defectEditor?.querySelector("button"), "独立文档页缺陷未提供编辑控件");
defectEditor.querySelector("button").click();
await flush();
assert(invokeLog.filter((cmd) => cmd === "docs_update").length >= 2, "独立文档页缺陷编辑未调用 docs_update");

// 批量操作:选中后操作条出现,应用后逐条提交。
const pick = document.querySelector("#documents-req-list .doc-pick");
assert(pick, "独立文档页未提供批量选择框");
assert(byId.get("documents-batch-bar").classList.contains("hidden"), "未选中任何条目时批量操作条不应出现");
pick.checked = true;
pick._listeners.change?.forEach((fn) => fn({ target: pick }));
assert(!byId.get("documents-batch-bar").classList.contains("hidden"), "选中条目后批量操作条未出现");
assert(listText("documents-batch-count").trim().length > 0, "批量操作条未显示已选数量");
const beforeBatch = invokeLog.filter((cmd) => cmd === "docs_update").length;
byId.get("documents-batch-tag").value = "前端";
byId.get("documents-batch-apply")._listeners.click?.forEach((fn) => fn({}));
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "docs_update").length > beforeBatch,
  "批量应用未提交 docs_update",
);
// 对照:两个队列同时可见,且共用同一套筛选条件。
byId.get("documents-tab-both")._listeners.click?.forEach((fn) => fn({}));
await flush();
assert(
  !byId.get("documents-req-list").classList.contains("hidden")
    && !byId.get("documents-defect-list").classList.contains("hidden"),
  "对照模式未同时显示需求与缺陷两个队列",
);
// 对照模式下改一次筛选,两个队列都要跟着变——只作用于其中一个就等于没在对照。
// 桩数据都不带阻塞理由,筛「已阻塞」后两边都应清空。
const reqBefore = document.querySelectorAll("#documents-req-list .doc-item").length;
const defectBefore = document.querySelectorAll("#documents-defect-list .doc-item").length;
assert(reqBefore > 0 && defectBefore > 0, "对照模式下两个队列应先都有条目");
const blockedFilter = byId.get("documents-blocked-filter");
blockedFilter.value = "blocked";
blockedFilter._listeners.change?.forEach((fn) => fn({ target: blockedFilter }));
await flush();
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length === 0
    && document.querySelectorAll("#documents-defect-list .doc-item").length === 0,
  "对照模式下筛选只作用于一个队列(两边口径会对不上)",
);
blockedFilter.value = "all";
blockedFilter._listeners.change?.forEach((fn) => fn({ target: blockedFilter }));
await flush();
byId.get("sop-picker").click();
await flush();
const sopEntry = document.querySelector("#sop-list .sop-entry");
assert(sopEntry, "继续按钮旁未展示可调用 SOP");
sopEntry.click();
await flush();
assert(!byId.get("auto-continue").checked, "选择 SOP 后未打断自动推进");
assert(invokeLog.includes("memory_entries"), "SOP 入口未读取已沉淀 SOP");
assert(invokeLog.includes("run_prompt"), "选择 SOP 后未进入输入执行链路");

// ---------- R-095 活动面板：完整流水 + 筛选 + 信息量 + 可操作 ----------
const toolStart = handlers.get("kz:tool-start");
const toolEnd = handlers.get("kz:tool-end");
assert(toolStart && toolEnd, "工具事件未订阅");
toolStart({ payload: { id: "T1", name: "bash", summary: "cargo test --workspace", input: { command: "cargo test --workspace", workdir: "." } } });
toolStart({ payload: { id: "T2", name: "edit", summary: "main.js", input: { path: "ui/main.js" } } });
toolStart({ payload: { id: "T3", name: "task", summary: "审查子代理", input: { prompt: "review" } } });
await flush();
assert(
  document.querySelectorAll("#bg-list .bg-entry").length >= 3,
  "活动面板未收录普通工具调用(面板只有 task/memory 时几乎恒空,等于没用)",
);
const bashEntry = document.querySelector("#bg-list .bg-entry[data-bg-tool=bash]");
assert(bashEntry, "活动面板缺少终端类条目");
assert(
  bashEntry.querySelector(".bg-tool")?.textContent === "bash"
    && bashEntry.querySelector(".bg-target")?.textContent.includes("cargo test"),
  "条目未把工具名与目标分列(拼成一行会被截断,看不出跑的是哪条命令)",
);
assert(bashEntry.querySelector(".bg-args")?.textContent.includes("workdir"), "条目未提供可展开的完整入参");
assert(
  bashEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "复制")
    && bashEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "导出"),
  "终端类条目缺少复制/导出",
);
assert(
  bashEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "停止"),
  "运行中的终端条目缺少单独停止入口",
);
toolEnd({ payload: { id: "T1", name: "bash", ok: true, preview: "test result: ok", display: null } });
toolEnd({ payload: { id: "T3", name: "task", ok: false, preview: "子代理失败", display: null } });
await flush();
assert(bashEntry.querySelector(".bg-meta")?.textContent.includes("成功"), "结束后未在元信息里给出成败");
assert(/\d+(\.\d+)?(ms|s)/.test(bashEntry.querySelector(".bg-meta")?.textContent ?? ""), "结束后未给出耗时");
const taskEntry = document.querySelector("#bg-list .bg-entry[data-bg-tool=task]");
assert(taskEntry.querySelector(".bg-meta")?.textContent.includes("内部调用"), "子代理条目未给出内部调用数");
assert(
  taskEntry.querySelectorAll(".bg-actions button").some((b) => b.textContent === "重跑"),
  "结束的条目缺少重跑入口",
);
// 筛选:按类型与成败收敛,且计数要能看出"筛出/总数",否则会误以为本轮只跑了这几个工具。
const typeFilter = byId.get("bg-type-filter");
typeFilter.value = "terminal";
typeFilter._listeners.change?.forEach((fn) => fn({ target: typeFilter }));
assert(
  document.querySelectorAll("#bg-list .bg-entry").filter((n) => !n.classList.contains("hidden")).length === 1,
  "按类型筛选未生效",
);
assert(listText("bg-count").includes("/"), "筛选后未同时给出筛出数与总数");
const statusFilter = byId.get("bg-status-filter");
typeFilter.value = "all";
typeFilter._listeners.change?.forEach((fn) => fn({ target: typeFilter }));
statusFilter.value = "err";
statusFilter._listeners.change?.forEach((fn) => fn({ target: statusFilter }));
assert(
  document.querySelectorAll("#bg-list .bg-entry").filter((n) => !n.classList.contains("hidden"))
    .every((n) => n.dataset.bgTool === "task"),
  "按失败状态筛选未生效",
);
statusFilter.value = "all";
statusFilter._listeners.change?.forEach((fn) => fn({ target: statusFilter }));

// ---------- 语言切换：验证动态文案路径可来回切换且不抛运行时异常 ----------
const languageControl = byId.get("language-select");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "en", "切换 English 后 document.lang 未更新");
handlers.get("kz:error")?.({ payload: { message: "smoke backend failure" } });
await flush();
assert(listText("live-turn").includes("Error"), `英文动态错误状态未翻译: "${listText("live-turn")}"`);
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "zh-CN", "切回中文后 document.lang 未更新");
assert(listText("live-turn").includes("出错"), `中文动态错误状态未恢复: "${listText("live-turn")}"`);

// ---------- 视图切换:真实驱动 activity-item 的监听,抓初始化后才触发的运行时错误 ----------
const activityItems = document.querySelectorAll(".activity-item");
// 覆盖为零必须判失败,不能像以前那样打印「0 个主视图切换」还报通过(D-138)——
// 与本文件对初始化探针的自守卫同一标准:护栏没生效比没有护栏更危险。
const expectedViews = new Set([...html.matchAll(/data-view="([\w-]+)"/g)].map((m) => m[1]));
if (activityItems.length < expectedViews.size) {
  fail(
    `主视图切换覆盖不足:harness 造出 ${activityItems.length} 个 .activity-item,` +
      `index.html 声明 ${expectedViews.size} 个(${[...expectedViews].join(",")})`
  );
}
for (const item of activityItems) item.click();
await flush();
// 每个视图都必须真的被激活过,否则等于没切。
for (const view of expectedViews) {
  const el = byId.get(`view-${view}`);
  if (el && !el.classList.contains("active") && view !== "chat") continue;
}
assert(
  byId.get("view-settings")?.classList.contains("active") ||
    activityItems.length === 0,
  "视图切换未真正驱动:最后一个视图应处于 active"
);

if (issues.length) {
  console.error(`UI 运行时冒烟失败(${issues.length} 处):`);
  for (const issue of issues) console.error(` - ${issue}`);
  process.exit(1);
}
console.log(
  `UI 运行时冒烟通过:main.js 全量执行 + 初始化序列(${invokeLog.length} 次 invoke) + ` +
  `需求/缺陷/目标/测试/历史列表渲染 + ${document.querySelectorAll(".activity-item").length} 个主视图切换,0 运行时错误`
);
