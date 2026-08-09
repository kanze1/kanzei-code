// 前端运行时冒烟(R-084):在 Node 中以最小 DOM harness 真实执行 main.js,
// 补 node --check(纯语法)与静态正则冒烟都抓不到的 ReferenceError / 初始化崩坏(D-048 类问题)。
// 覆盖:整页加载与初始化、需求/缺陷/目标/测试列表非空渲染、主视图切换、console.error 与未捕获异常 → 非零退出码。
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import vm from "node:vm";
import { loadUiSources } from "./ui-sources.mjs";

const root = resolve(import.meta.dirname, "..");
// B0:按 index.html 的 <script src> 清单按序读入全部脚本(单文件 = 清单的退化情形)。
// 后续 R-154 批次把 main.js 拆成 18 个文件时,冒烟对文件形态透明,无需再改。
const { html, scriptSrcs, sources, joined: source } = loadUiSources();
const style = await readFile(resolve(root, "crates/kanzei-app/ui/style.css"), "utf8");

const issues = [];
const fail = (msg) => issues.push(msg);

// CSS 结构完整性:浏览器对花括号错配是静默容错的,一个被吃掉的 `@media ... {`
// 会让整段响应式规则无条件生效而没有任何报错(c65c80e 就这样把 D-164 带上了线)。
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
// 图标一致性:活动栏与 .icon-btn 是一整套单色描边字形/SVG(⌂ ☷ ❖ ＋ ↻ ↗ ✎ …),
// 混进一个彩色 emoji 会突兀得像贴上去的。靠眼睛发现太晚——机械挡住。
// 只管图标位,正文里的 💬/⚠ 之类语义标记不在此列。
const ICON_MARKUP = [
  ...html.matchAll(/<button[^>]*class="[^"]*\b(?:activity-item|icon-btn)\b[^"]*"[^>]*>([\s\S]*?)<\/button>/g),
];
const COLOR_EMOJI = /[\u{1F000}-\u{1FAFF}]/u;
for (const [, inner] of ICON_MARKUP) {
  const glyphs = inner.replace(/<[^>]*>/g, "").trim();
  if (COLOR_EMOJI.test(glyphs)) {
    fail(`图标位出现彩色 emoji「${glyphs}」:活动栏与 icon-btn 必须是单色字形或描边 SVG`);
  }
}
if (ICON_MARKUP.length < 10) fail("图标一致性检查没扫到足够的图标按钮,正则可能已与标记脱节");

// 深色主题下原生控件必须跟着深色渲染,否则勾选框会是一块白底(D-154)。
// 这条只能静态查:计算样式里看不出"浏览器用了哪套控件配色"。
const rootRule = style.match(/:root\s*\{([\s\S]*?)\}/)?.[1] ?? "";
if (!/color-scheme:\s*dark/.test(rootRule)) {
  fail(":root 未声明 color-scheme: dark,原生勾选框/下拉在深色界面里会是白底");
}
if (!/input\[type="checkbox"\][^{]*\{[^}]*accent-color/.test(style)) {
  fail("勾选框未统一 accent-color,选中态会用系统蓝而不是界面强调色");
}

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
  if (!source.includes("逐条对照验收原文") || !source.includes("每项给出精确代码位置证据")) {
    fail("自动续跑提示缺少逐条验收与精确代码位置证据约束");
  }
  if (
    !source.includes("真实调用方或消费者") ||
    !source.includes("显式标注为既有能力而非本次交付") ||
    !source.includes("不得缩小验收里的平台或范围限定词")
  ) {
    fail("自动续跑提示缺少真实调用方、既有能力标注或平台/范围保持约束");
  }

const pendingTimers = new Set();
const rafQueue = [];
// D-202:统计"从 document.body 起的全文档文本节点重扫"次数。流式渲染期间它必须为 0,
// 否则就是有人把 i18n observer 改回了全量重扫(卡顿主因)。
let fullDocumentWalks = 0;
const mutationObservers = new Set();
let mutationQueued = false;
// observer 回调里的 DOM 写入若再次唤醒 observer 自己,真机上是微任务死循环:主线程
// 饿死、永不绘制,表现为启动黑屏(D-172)。冒烟里这种循环会让进程挂死而非报错,
// 所以数连续自触发轮数,超限就断开 observer 并判失败,把挂死变成可读的失败。
let observerCascade = 0;
// D-202:必须投递真实的 MutationRecord。回调只处理"本次变动带进来的节点"是修复的
// 关键路径,若 harness 一直递空数组,这条路径在冒烟里恒为空转——既测不到本地化是否
// 生效,也测不出有人把它改回全文档重扫。
const mutationRecords = [];
function notifyMutation(record) {
  if (record) mutationRecords.push(record);
  if (!mutationObservers.size) {
    mutationRecords.length = 0;
    return;
  }
  if (mutationQueued) return;
  mutationQueued = true;
  Promise.resolve().then(() => {
    mutationQueued = false;
    const records = mutationRecords.splice(0);
    for (const observer of mutationObservers) observer.callback(records);
    if (mutationQueued) {
      observerCascade += 1;
      if (observerCascade > 25) {
        mutationObservers.clear();
        fail("MutationObserver 连续自触发超过 25 轮:回调内的 DOM 写入又唤醒了 observer(真机=微任务死循环→主线程饿死→黑屏,D-172)");
      }
    } else {
      observerCascade = 0;
    }
  });
}

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
    // style 不能只是空对象:main.js 用 setProperty 写 CSS 变量(批次格的列数就靠它),
    // 缺这个 API 会在渲染中途抛异常,整张列表消失——而这种崩法在冒烟里表现为
    // "元素找不到",看不出真因。
    this.style = {
      _props: {},
      setProperty(name, value) { this._props[name] = String(value); },
      getPropertyValue(name) { return this._props[name] ?? ""; },
      removeProperty(name) { delete this._props[name]; },
    };
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
  _adopt(node) { node.parentNode = this; node.ownerDocument = this.ownerDocument; this.childNodes.push(node); notifyMutation({ type: "childList", target: this, addedNodes: [node] }); return node; }
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
    // innerHTML 写入的文本与后续 appendChild 的子节点会共存(先设 innerHTML 再追加是
    // main.js 的常见写法);只读子节点会把前者整段丢掉,断言就看不见它。
    const own = this._textContent;
    if (!this.childNodes.length) return own;
    return own + this.childNodes.map((c) => (c instanceof Element ? c.textContent : c.nodeValue)).join("");
  }
  // harness 近似:文本节点是 createTreeWalker 里惰性造的,拿不到"新增的那个文本节点",
  // 就把元素自身当作新进子树递出去——localizeRoot 走子树,语义是真机的超集,不会漏。
  set textContent(value) { this.childNodes = []; this._innerHTML = ""; this._textContent = String(value); notifyMutation({ type: "childList", target: this, addedNodes: [this] }); }
  get innerText() { return this.textContent; }
  set innerText(value) { this.textContent = value; }
  get innerHTML() { return this._innerHTML; }
  // innerHTML 写入要同步出可读文本:main.js 里大量行是 innerHTML 拼的,若 textContent
  // 读不到它们,所有基于文本的断言对这些内容都是瞎的(和 D-151 同一类盲区)。
  // 只做去标签的近似,不实现真正的解析——冒烟要的是"文字在不在",不是 DOM 树。
  set innerHTML(value) {
    this._innerHTML = String(value);
    this.childNodes = [];
    this._textContent = String(value)
      .replace(/<[^>]*>/g, "")
      .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&").replace(/&quot;/g, '"');
    notifyMutation({ type: "childList", target: this, addedNodes: [this] });
  }
  get value() { return this._value ?? ""; }
  set value(v) { this._value = String(v); }
  getAttribute(name) { return this._attributes[name] ?? null; }
  // class 必须走 className 设值,否则 classList 的内部集合与属性脱节:index.html 里
  // 写死的 class 不进集合,第一次 classList.toggle() 回写就把它们整体抹掉了。
  setAttribute(name, value) {
    if (name === "class") { this.className = value; return; }
    const next = String(value);
    // 同值也必须通知 observer:DOM 规范里 setAttribute 无条件入 mutation 队列,
    // 早退吞通知会让"observer 回调里无条件写属性"的死循环在冒烟里隐形(D-172)。
    if (this._attributes[name] === next) { notifyMutation({ type: "attributes", target: this, attributeName: name, addedNodes: [] }); return; }
    this._attributes[name] = next;
    if (name === "id") this.id = next;
    notifyMutation({ type: "attributes", target: this, attributeName: name, addedNodes: [] });
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
  createTreeWalker: (root = body) => {
    if (root === body) fullDocumentWalks += 1;
    const texts = [];
    const walk = (el) => {
      if (el._textContent && !el.childNodes.length) {
        if (!el._textNodeProxy) {
          const text = { parentNode: el, parentElement: el };
          Object.defineProperty(text, "nodeValue", {
            get: () => el._textContent,
            set: (value) => {
              const next = String(value);
              if (el._textContent === next) return;
              el._textContent = next;
              notifyMutation({ type: "characterData", target: text, addedNodes: [] });
            },
          });
          el._textNodeProxy = text;
        }
        texts.push(el._textNodeProxy);
      }
      for (const c of el.childNodes) {
        if (c instanceof TextNode) { if (c.nodeValue) texts.push(c); } else walk(c);
      }
    };
    // 文本节点当 root:characterData 变动会把它直接递进来。
    if (root && typeof root.querySelectorAll !== "function") {
      if (root.nodeValue) texts.push(root);
    } else walk(root ?? body);
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
  const attributes = `${before} ${after}`;
  const className = attributes.match(/\bclass="([^"]*)"/)?.[1];
  if (className) el.className = className;
  for (const attribute of ["title", "placeholder", "aria-label"]) {
    const value = attributes.match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  if (/\bdata-i18n-raw\b/.test(attributes)) el.setAttribute("data-i18n-raw", "");
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
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
  for (const attribute of ["title", "aria-label"]) {
    const value = match[0].match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
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
  // 所选目录没有 .kanzei,实际根落在上级 —— 这正是需求串项目的形态。
  project_root_info: { selected: PROJECT, resolved: "C:/smoke/parent", shared: true },
  project_detach: null,
  // R-136:Ollama 装了但服务没起 —— 最常见的"子代理静默失效"形态。
  fast_model_status: { managed: true, model: "qwen3.5:4b", installed: true, serviceUp: false, modelPresent: false, ready: false },
  fast_model_setup: "fast 子代理已就绪:qwen3.5:4b",
  files_snapshot: {
    files: [
      { path: "src/lib.rs", size: 2048, lines: 120, oversized: false, note: "冒烟样例:库入口" },
      { path: "docs/note.md", size: 512, chars: 300, oversized: false },
    ],
    dirs: {
      "": { files: 2, size: 2560, lines: 120 },
      "src": { files: 1, size: 2048, lines: 120 },
      "docs": { files: 1, size: 512, lines: 0 },
    },
    dirNotes: { "src": "源码目录" },
    unannotated: 1,
  },
  docs_snapshot: {
    requirements: [docEntry("R-001", "冒烟需求", "doing", { complexity: "中", batches: { done: 3, total: 11 }, fields: [["备注", "待更新"], ["验收", "这是一条刻意超过六十字符的长验收文本,用来验证编辑表单会把段落型字段升级为多行文本域,而不是塞进单行输入框把值截断到看不见"]] }), docEntry("R-002", "冒烟需求二", "todo", { batches: { done: 0, total: 1 } })],
    defects: [docEntry("D-001", "冒烟缺陷", "open", { severity: "medium", fields: [["复现", "待澄清: 用户视角的易用性还是模型可消费性?"]] })],
    goals: [{ id: "G-001", title: "冒烟目标", status: "active", fields: [] }],
    sources: [],
    findings: [],
    archived: { req: 1, defect: 2, goal: 0, source: 0, finding: 0 },
    archived_entries: { req: [docEntry("R-000", "已归档需求", "done")], defect: [docEntry("D-000", "已归档缺陷", "fixed")], goal: [], source: [], finding: [] },
    conventions: { exists: true, headings: ["开发规则", "测试要求"] },
  },
  defect_review: {
    empty: false,
    defectCount: 1,
    report: "# 缺陷自动审查报告\n\n- D-001: `src/main.rs:10` 有可复核证据",
  },
  // R-125:召回明细。一轮里既有被拉取的(算采纳)也有没拉的,才能验证两种状态都渲染得出来。
  memory_recalls: {
    rounds: [{
      recall_id: "1-M",
      at: 1_760_000_000_000,
      prompt_head: "这轮要发版",
      injected_bytes: 512,
      hits: [
        { id: "M-002", title: "发版 SOP", scope: "project", category: "sop", score: 3.25, snippet: "package.ps1 -Publish", fetched: true },
        { id: "M-001", title: "CRLF 未命中", scope: "project", category: "fact", score: 1.1, snippet: "edit 换行", fetched: false },
      ],
    }],
    rounds_total: 1,
    rounds_with_fetch: 1,
  },
  // R-124:SOP 候选(带指纹,用于丢弃定位)。
  memory_note_candidates: [{
    scope: "global",
    hint: "sop",
    summary: "候选 SOP:完成 R-123(done)的流程[sop:R-123]",
    detail: "- 实际工具顺序: read → edit → bash → req",
    fingerprint: "[sop:R-123]",
  }],
  memory_note_discard: true,
  // R-099/R-127:一轮有画像、一轮早于度量落地,验证两者区分得开。
  run_metrics: {
    rounds: [
      {
        at: 1_760_000_000_000, prompt: "收口 R-123", outcome: "completed", steps: 12,
        inputTokens: 48_000, outputTokens: 3_200,
        tools: { edit: 6, bash: 4, req: 2 },
        context: [["agent/system", 4000], ["memory", 800]],
        metrics: { terminal_calls: 4, git_calls: 2, git_groups: 1, edit_calls: 6, edit_misses: 1, subagent_calls: 0, total_calls: 12, failed_calls: 1 },
        measured: true,
      },
      {
        at: 1_759_000_000_000, prompt: "更早的一轮", outcome: "completed", steps: 5,
        inputTokens: 10_000, outputTokens: 900, tools: {}, context: [], metrics: {}, measured: false,
      },
    ],
  },
  conversation_get: [{ role: "user", parts: [{ type: "text", text: "冒烟历史消息" }] }],
  conversation_trace_get: [],
  conversation_list: [{ sequence: 1, sequences: [1], title: "冒烟会话", preview: "预览", updated_at: "2026-08-08 00:00" }],
  // 角色项 + 一个真实模型:角色不该出现在设置页的角色下拉里(会绕成自指)。
  models_list: [
    { id: "primary", label: "primary → anthropic:claude-sonnet-5" },
    { id: "anthropic:claude-sonnet-5", label: "anthropic:claude-sonnet-5" },
    { id: "ollama:qwen3", label: "ollama:qwen3" },
  ],
  git_status: { branch: "main", changes: 2 },
  list_pending_inputs: [],
  test_runs_snapshot: { active: [{ id: "T-001", title: "冒烟测试", status: "passed", fields: [["命令", "cargo test"]] }], archived: [] },
  process_list: [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false },
    // R-086 多会话并发:后台会话初始为运行中,桩里的旧 running=true 正是
    // "事件已收敛但轮询采样仍在事件之前"的竞态值,converged 必须挡住它。
    { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true },
  ],
  pending_asks_get: [],
  // primary 是探测不到的已存值(端点没实现 /models),必须原样保留;
  // effective 与全局不同 = 项目级覆盖,界面要明说。
  settings_get: {
    language: "zh",
    path: "C:/smoke/.kanzei/kanzei.toml",
    primary: "deepseek:deepseek-chat",
    fast: "ollama:qwen3",
    proxy: "env",
    profileDefault: "dev",
    reasoning: "off",
    codexFastMode: false,
    limits: { maxTokens: 4096, subagentTimeoutSecs: null },
    limitDefaults: {
      maxTokens: 8192, subagentMaxTokens: 4096, subagentTimeoutSecs: 900,
      contextBudgetRatio: 0.7, recentVerbatimRatio: 0.35, maxTasksPerTurn: 8,
      maxParallelTools: 8, transportRetries: 2, rateLimitRetries: 2, streamRestarts: 2,
    },
    // R-157:生效节奏(项目配置覆盖)+ 内置默认。继续文案应渲染 "全量测试每 3 批跑一次"。
    cadence: { full_test: "every_n_batches", full_test_batches: 3, targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
    cadenceDefaults: { full_test: "entry_close", full_test_batches: null, targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
    profiles: {},
    providers: [],
    permissions: [],
    effective: { primary: "anthropic:claude-sonnet-5", fast: "ollama:qwen3", reasoning: null },
    projectConfig: "C:/smoke/project/.kanzei/kanzei.toml",
  },
  permission_rules_get: [],
  memory_overview: { scopes: [{ scope: "project", root: PROJECT, total: 0, hitsTotal: 0, categories: {}, integrity: [], inboxPending: 0 }] },
  // 两条:一条有命中,一条陈旧且零命中(验证「长期零命中」标记与清理入口)。
  memory_entries: [
    { id: "M-SOP-001", category: "sop", title: "冒烟 SOP", description: "继续执行冒烟任务", status: "active", body: "执行冒烟任务", hits: 4, lastHitAt: 1_760_000_000_000, updated: "2026-08-01" },
    { id: "M-DEAD-001", category: "fact", title: "从没被用到的记忆", description: "冒烟用:零命中条目", status: "active", body: "陈旧结论", hits: 0, lastHitAt: 0, updated: "2026-01-01" },
  ],
  memory_context_bill: { turns: [] },
  workspace_snapshot: {},
};
const invokeLog = [];
const savedPayloads = new Map();
// 探针回传要看具体参数(id 配对、取样内容),所以单独留一份带参日志。
const probeResults = [];
async function invoke(cmd, args) {
  invokeLog.push(cmd);
  if (cmd === "settings_save") savedPayloads.set(cmd, args);
  if (cmd === "ui_probe_result") probeResults.push(args);
  if (cmd in payloads) return structuredClone(payloads[cmd]);
  return null;
}
async function listen(event, handler) { handlers.set(event, handler); }
const handlers = new Map();

const storage = new Map();
storage.set("kz-auto-continue", "1");
// R-157 批2:预置旧版默认继续文案(镜像 08-compose.js LEGACY_CONTINUE_PROMPTS[0]),
// 启动块应把它静默升级为新默认并写入 localStorage。夹具若与 LEGACY 列表脱节,
// 升级不再命中,下面断言会失败——那是提醒同步夹具,不是误报。
storage.set(
  "kz-continue-prompt",
  "继续推进。取活顺序按本轮末尾给出的「开发重心」执行(它来自记忆里的用户定调,是唯一权威);" +
    "两个队列内部都按文档顺序自上而下拿第一个可做的,列表已按阶段排好,不要自行挑看起来容易的。\n" +
    "1. 本轮必须产生落地动作:改代码或跑测试。先做再说明,不要只做判断。\n" +
    "2. 粒度 = 一轮一个完整条目:以做完当前这一条缺陷/需求为本轮目标;" +
    "同构批量改动(i18n、重命名、迁移这类)一轮吃掉完整类别,不要按两三处微切片。" +
    "确实超出单轮容量才按验收子项分轮,并在进展里写明批次边界。" +
    "「工作量大」「要改多个文件」都是正常工作,不是停下的理由。\n" +
    "3. 卡住就换一条:某条一时推不动,在「进展」里记一句原因,直接跳到下一条继续,不要停下来等。\n" +
    "「阻塞」字段只写解除权不在你手里的事(已问过用户在等回复/缺凭据/依赖外部服务/用户直营)," +
    "且要写出具名解除人;「涉及多文件」「跨层改动」「需先确认方案(但没真问过)」都不是阻塞,写进展。" +
    "顺手复核碰到的条目:阻塞条件已满足的当场清空「阻塞」字段。看到 [调度死锁] 横幅时按横幅执行。\n" +
    "4. 关闭条目前逐条对照验收原文,每项给出精确代码位置证据;声称完成的能力必须有真实调用方或消费者," +
    "没有消费者的命令、死代码或只展示不接数据源的壳不算完成;沿用既有实现要显式标注为既有能力而非本次交付;" +
    "不得缩小验收里的平台或范围限定词。任一项证据不足就保留活动态写清缺口,不要打勾。\n" +
    "5. doing 最多 2 个;已满就继续推进这两项。标着「阶段 5 后」的功能需求暂不启动。\n" +
    "6. 已通过测试的未提交改动,先按规范 §6 用 git 提交(不带署名)再继续。" +
    "验证选择与改动面匹配:纯 ui/ 改动跑 node 检查与冒烟脚本,动了 crates/ 才跑 cargo test。\n" +
    "一直做下去,不要用纯文本收尾。"
);
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
  observe() { mutationObservers.add(this); }
  disconnect() { mutationObservers.delete(this); }
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
  requestAnimationFrame: (fn) => { rafQueue.push(fn); return rafQueue.length; },
  cancelAnimationFrame: () => {},
};
vm.createContext(sandbox);

const settle = () => new Promise((r) => setImmediate(r));
async function flush(rounds = 12) {
  for (let i = 0; i < rounds; i += 1) {
    await settle();
    const frames = rafQueue.splice(0);
    for (const fn of frames) await fn();
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

// ---------- 执行 ui/*.js ----------
// 诊断:main.js 的初始化 IIFE 逐步 catch 只 toast 不抛出,冒烟里 toast 不可见;
// 注入 reporter 把"吞掉的初始化异常"变成冒烟失败(同时保持生产行为不变)。
// 逐文件 vm.runInContext(与浏览器多 <script> 语义一致,含 TDZ):拼接后一次执行会把
// 函数声明提升到整串顶部,浏览器多脚本下会炸的 ReferenceError 在 vm 里反而跑通。
const PROBE_INIT = /toastError\(`\$\{label\}加载失败:\$\{err\}`\);/;
const PROBE_PERSIST = /function reportPersistentError\(text, \{ retry = null \} = \{\}\) \{/;
let probeHits = 0;
try {
  for (let i = 0; i < sources.length; i += 1) {
    let instrumented = sources[i].replace(
      PROBE_INIT,
      "toastError(`${label}加载失败:${err}`); __reportInitError?.(label, err);"
    );
    if (instrumented !== sources[i]) probeHits += 1;
    const beforePersist = instrumented;
    instrumented = instrumented.replace(
      PROBE_PERSIST,
      "function reportPersistentError(text, { retry = null } = {}) { __reportPersistentError?.(text);"
    );
    if (instrumented !== beforePersist) probeHits += 1;
    vm.runInContext(instrumented, sandbox, { filename: scriptSrcs[i] });
  }
} catch (err) {
  fail(`ui/*.js 顶层执行抛异常: ${err.stack ?? err}`);
}
if (probeHits < 2) fail(`注入初始化异常探针失败:累计命中 ${probeHits}/2,启动序列的 catch 或错误上报形态已变化,请同步冒烟脚本`);
// R-076:追加鞭挞状态测试钩子(访问模块级 let 状态,冒烟外部拿不到)。hook 单独最后执行,
// 不拼进任何脚本文件——拆分后它属于冒烟注入层,不属于生产代码。
try {
  vm.runInContext(
    "globalThis.__kzTest = { rounds: () => autoRounds, noAction: () => noActionRounds, stopReason: () => autoStopReason, setRounds: (v) => { autoRounds = v; }, setStopAfterRound: (v) => { autoStopAfterRound = v; }, setPaused: (v) => { autoPaused = v; }, reset: () => { autoRounds = 0; noActionRounds = 0; autoStopAfterRound = false; autoPaused = false; } };",
    sandbox,
    { filename: "__kzTest-hook.js" }
  );
} catch (err) {
  fail(`__kzTest hook 执行抛异常: ${err.stack ?? err}`);
}
await flush();
assert(invokeLog.includes("projects_get"), `初始化未调用 projects_get(启动序列断裂),已见调用: ${invokeLog.join(",")}`);
assert(invokeLog.includes("docs_snapshot"), "初始化未调用 docs_snapshot");
assert(listText("req-list").includes("冒烟需求"), `需求列表未渲染出桩数据: "${listText("req-list").slice(0, 60)}"`);
assert(listText("defect-list").includes("冒烟缺陷"), "缺陷列表未渲染出桩数据");
// R-157 批2:预置的 LEGACY 默认文案必须被静默升级,且 18-startup「节奏配置」步骤把
// mock 的生效节奏(every_n_batches/3)渲染进继续文案——证明参数化真的到达注入提示词。
{
  const storedPrompt = storage.get("kz-continue-prompt") ?? "";
  const textareaPrompt = (byId.get("continue-prompt")?.value ?? "").trim();
  assert(
    !storedPrompt.includes("粒度 = 一轮一个完整条目"),
    "LEGACY 旧默认文案未被升级:仍留在 localStorage"
  );
  assert(
    storedPrompt.includes("全量测试每 3 批跑一次") && storedPrompt.includes("继续推进"),
    `继续文案未按生效节奏渲染(应含「全量测试每 3 批跑一次」): ${storedPrompt.slice(0, 120)}`
  );
  assert(
    textareaPrompt === storedPrompt,
    "textarea 与 localStorage 的默认文案不一致(升级/节奏渲染不同步)"
  );
}
// 批次进度格(R-160):格数与已填格必须来自后端算好的 entry.batches,前端不得另存
// 一份复杂度→格数的映射;总数为 1 的条目不画格(一轮做完的东西不需要进度条)。
{
  const meter = document.querySelector('#req-list .doc-item[data-doc-id="R-001"] .batch-meter');
  assert(meter, "批次进度格没渲染出来");
  const cells = meter.querySelectorAll(".complexity-cell");
  assert(cells.length === 11, `11 批应画 11 格,实际 ${cells.length}`);
  assert(
    cells.filter((c) => c.className.includes("filled")).length === 3,
    "已完成 3 批就该填 3 格",
  );
  assert(
    meter.style.getPropertyValue("--cells") === "11",
    `轨道要按批次数等分,--cells 实际为 ${meter.style.getPropertyValue("--cells")}`,
  );
  assert(
    (meter.getAttribute("aria-label") ?? "").includes("3/11"),
    `读屏标签要带准确批次数:${meter.getAttribute("aria-label")}`,
  );
  assert(
    !document.querySelector('#req-list .doc-item[data-doc-id="R-002"] .batch-meter'),
    "总数为 1 的条目不该画进度格(一轮做完的东西没有进度可言)",
  );
}
assert(listText("goal-list").includes("冒烟目标"), "目标列表未渲染出桩数据");
assert(listText("test-list").includes("冒烟测试"), "测试记录列表未渲染出桩数据");
assert(listText("conversation-list").includes("冒烟会话"), "历史对话列表未渲染出桩数据");
// D-207:取活焦点标记——在做的(doing/fixing)高亮,取活序下一个(defect-first 下
// 第一个无阻塞的 open 缺陷)次亮。基于数据计算,与视图排序/分组无关。
{
  const active = document.querySelector('#req-list .doc-item[data-doc-id="R-001"]');
  assert(active?.classList.contains("agent-active"), "doing 条目 R-001 未标记 agent-active(在做高亮丢失)");
  const next = document.querySelector('#defect-list .doc-item[data-doc-id="D-001"]');
  assert(next?.classList.contains("agent-next"), "defect-first 下首个可开工缺陷 D-001 未标记 agent-next(取活预览丢失)");
  assert(!next.classList.contains("agent-active"), "open 条目不该被标成在做");
  const notNext = document.querySelector('#req-list .doc-item[data-doc-id="R-002"]');
  assert(!notNext?.classList.contains("agent-next"), "缺陷队列有可开工项时,需求 R-002 不该被标为下一个");
}
// D-207 补:blocked doing 不计入运行焦点。R-157 类阻塞 doing 曾被标成
// 「agent 正在做这一条」,而 §1.1 阻塞项不进 WIP、取活会跳过它——渲染必须与
// 取活一致:保留 blocked 标记但不标 agent-active,且 next 不被它挡住。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [docEntry("R-001", "阻塞的 doing", "doing", { blocked: true })],
    defects: [docEntry("D-001", "可开工缺陷", "open", {})],
  };
  await sandbox.refreshDocs();
  const blockedDoing = document.querySelector('#req-list .doc-item[data-doc-id="R-001"]');
  assert(blockedDoing?.classList.contains("blocked"), "阻塞 doing 应保留 blocked 标记(阻塞展示不受影响)");
  assert(!blockedDoing.classList.contains("agent-active"), "阻塞 doing 不该标 agent-active(运行焦点只标可执行条目)");
  const next = document.querySelector('#defect-list .doc-item[data-doc-id="D-001"]');
  assert(next?.classList.contains("agent-next"), "blocked doing 不应挡住 next:可开工缺陷 D-001 仍应为下一个");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-166:引用跳转此前只认当前可见节点,已归档/被折叠的目标一律静默失败。
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

// R-123 职责分离(侧栏只读浏览、文档页深度管理)经 D-211 修订:拖拽改序两侧一致——
// 侧栏照常渲染"解锁"按钮,解锁后就必须能拖,否则是承诺与能力脱节(D-211)。
assert(!document.querySelector("#req-list .doc-edit"), "侧栏仍在渲染字段编辑表单(应只在独立文档页)");
assert(!document.querySelector("#defect-list .doc-edit"), "缺陷侧栏仍在渲染字段编辑表单");
assert(!document.querySelector("#req-list .doc-pick"), "侧栏出现批量选择框(批量操作应只在文档页)");
assert(
  !document.querySelectorAll("#req-list .doc-item").some((n) => n.draggable),
  "分组锁状态下侧栏条目不应可拖(解锁后才设置 draggable)",
);
// D-211 修复链路:侧栏解锁 → 锁提示消失 → draggable=true → 拖拽 → reorder 落库。
{
  const sidebarReq = document.querySelector("#req-list");
  const hint = sidebarReq.querySelector(".drag-hint");
  assert(hint, "侧栏默认分组视图未渲染锁提示");
  const unlockBtn = [...hint.querySelectorAll("button")].find((b) => b.textContent.includes("解锁"));
  assert(unlockBtn, "锁提示缺少一键解锁按钮(D-210 能力丢失)");
  unlockBtn.click();
  await flush();
  assert(!sidebarReq.querySelector(".drag-hint"), "解锁后锁提示未消失");
  const items = [...sidebarReq.querySelectorAll(".doc-item[data-doc-id]")];
  assert(items.length >= 2, `解锁后侧栏需求条目不足(无法验证拖拽落库): ${items.length}`);
  assert(items.every((n) => n.draggable), "侧栏解锁后条目未设置 draggable(D-211:解锁了却拖不动)");
  const before = invokeLog.filter((c) => c === "docs_update").length;
  const [a, b] = items;
  a.dispatchEvent({ type: "dragstart", dataTransfer: { effectAllowed: "", setData() {} } });
  b.dispatchEvent({ type: "dragover", clientY: 0, preventDefault() {} });
  a.dispatchEvent({ type: "dragend" });
  await flush();
  const after = invokeLog.filter((c) => c === "docs_update").length;
  assert(after > before, `侧栏拖拽未提交 docs_update(reorder 落库缺失),增量=${after - before}`);
}
// D-207 验收③:优先级语义 UI 明示——priority 只是背景信息,不参与取活(用户定调),
// 避免满屏 P0~P3 徽章让人按优先级猜取活序。
{
  const priFilter = document.querySelector("#req-priority-filter");
  assert(priFilter?.getAttribute("title").includes("仅参考"), `侧栏优先级筛选未明示"仅参考,不影响取活": "${priFilter?.getAttribute("title")}"`);
  const badge = document.querySelector("#req-list .pri-badge");
  assert(badge?.title.includes("仅参考"), `优先级徽章未明示"仅参考,不影响取活": "${badge?.title}"`);
}
// D-205 验收③:带「待澄清」复现的缺陷在侧栏可辨识——用户能一眼看到哪些条目等他补话,
// 不会把"待澄清"当真实复现拿去开工。
{
  const clarifyBadge = document.querySelector("#defect-list .clarify-badge");
  assert(clarifyBadge, "带「待澄清」复现的缺陷未渲染待澄清徽标(D-205)");
  assert(clarifyBadge.title.includes("待澄清"), `待澄清徽标未带具体问题提示: "${clarifyBadge.title}"`);
  assert(!document.querySelector("#req-list .clarify-badge"), "需求列表误渲染待澄清徽标(仅缺陷快记有此形态)");
}
// 侧栏移除编辑后必须仍能读到字段,否则等于把信息一起删了。
const sidebarFields = document.querySelectorAll("#req-list .doc-detail .doc-field");
assert(sidebarFields.length > 0, "侧栏详情既无编辑表单也无只读字段,信息被一起删掉了");
// 状态流转留在侧栏:取活时要能直接切。
assert(document.querySelector("#req-list .doc-actions button"), "侧栏详情缺少状态流转按钮(取活链路断了)");

const reqEditor = document.querySelector("#documents-req-list .doc-edit");
assert(reqEditor?.querySelector("input") && reqEditor?.querySelector("button"), "独立文档页未提供标题/字段编辑控件");
// D-164:曾经只给 aria-label,渲染成一片无标题输入框,改哪格全靠猜;长字段用单行 input 还会把值截没。
const reqEditRows = reqEditor.querySelectorAll(".doc-edit-row");
assert(reqEditRows.length >= 3, `编辑表单未按字段分行(应有 标题/备注/验收),实得 ${reqEditRows.length}`);
assert(
  reqEditRows.every((row) => (row.querySelector(".doc-edit-key")?.textContent ?? "").trim()),
  "编辑表单存在没有可见字段名的输入框",
);
assert(reqEditor.querySelector("textarea"), "长字段未升级为多行文本域,值会被单行输入框截断");
// D-165:编辑表单已连名带值列出每个字段,只读 .doc-field 列表若同时渲染就是同一份内容显示两遍。
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

// R-092:缺陷自动审查必须是一个真实按钮调用，不是静态报告链接或展示壳。
const reviewButton = byId.get("defect-review");
reviewButton.click();
assert(listText("defect-review-status").includes("正在审查缺陷"), "点击审查按钮后未立即反馈处理中状态");
await flush();
assert(invokeLog.includes("defect_review"), "缺陷自动审查按钮未调用后端 defect_review");
assert(listText("defect-review-status").includes("审查完成"), "缺陷自动审查成功后未反馈完成状态");
assert(!byId.get("viewer-overlay").classList.contains("hidden"), "缺陷自动审查报告未在应用内打开");
assert(listText("viewer-body").includes("D-001") && listText("viewer-body").includes("可复核证据"), "审查报告查看器未渲染后端结果");
assert(byId.get("viewer-external").classList.contains("hidden"), "运行时审查报告不应显示无效的外部文件按钮");
assert(!reviewButton.disabled, "缺陷审查完成后按钮仍处于禁用状态");
byId.get("viewer-close").click();

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

// ---------- R-125 记忆召回可视化：召回了什么、为什么召回、是否被采纳 ----------
// 走真实路径:点活动栏的「记忆」进入该视图,由它触发 refreshMemory。
const memoryTab = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "memory");
assert(memoryTab, "活动栏缺少记忆入口");
memoryTab.click();
await flush();
assert(invokeLog.includes("memory_recalls"), "记忆页未拉取召回明细(没有召回明细就没有评估手段)");

// R-124:SOP 候选必须停在用户面前,不能自己入库。
assert(invokeLog.includes("memory_note_candidates"), "记忆页未拉取待确认候选");
const candidate = document.querySelector("#memory-candidates .memory-candidate");
assert(candidate, "SOP 候选未渲染");
assert(candidate.classList.contains("sop"), "SOP 候选未按分类标记");
assert(listText("memory-candidates").includes("read → edit → bash → req"), "候选未展示提炼原料(工具顺序)");
const candidateButtons = document.querySelectorAll("#memory-candidates .memory-candidate-actions button");
assert(
  candidateButtons.some((b) => b.textContent === "采纳") && candidateButtons.some((b) => b.textContent === "丢弃"),
  "候选缺少采纳/丢弃入口(候选一旦自动入库就违背「用户的模板由用户定」)",
);
candidateButtons.find((b) => b.textContent === "丢弃").click();
await flush();
assert(invokeLog.includes("memory_note_discard"), "丢弃候选未调用后端");
const recallHits = document.querySelectorAll("#memory-recalls .memory-recall-hit");
assert(recallHits.length === 2, `召回明细未渲染命中条目,实得 ${recallHits.length}`);
assert(
  listText("memory-recalls").includes("M-002") && listText("memory-recalls").includes("3.25"),
  "召回明细未给出条目 id 与检索得分(看不出为什么召回这几条)",
);
assert(listText("memory-recalls").includes("package.ps1"), "召回明细未给出命中片段");
assert(listText("memory-recalls").includes("512B"), "召回明细未给出注入字节数(上下文账单无从算起)");
assert(
  recallHits.filter((n) => n.classList.contains("adopted")).length === 1,
  "采纳标记未按 fetched 区分:召回了但没拉正文不能算起了作用",
);
assert(listText("memory-recall-rate").includes("1/1"), "标题未给出采纳率");
// 效果画像:零命中要在列表里看得出来,且能直接删。
// 列表只在选中某个 scope/category 后渲染,冒烟里直接驱动该入口。
await sandbox.loadMemoryList("project", null);
await flush();
const dormantRow = document.querySelector("#memory-list .memory-row.dormant");
assert(dormantRow, "长期零命中的记忆未被标记(无从判断哪些记忆该清理)");
assert(listText("memory-list").includes("从未命中"), "记忆列表未给出最近命中时间");
dormantRow.click();
await flush();
const detailButtons = document.querySelectorAll("#memory-detail .memory-detail-actions button");
assert(
  detailButtons.some((b) => b.textContent === "删除"),
  "记忆详情缺少删除入口(stale 只是降权,仍占索引)",
);
assert(listText("memory-detail").includes("累计命中"), "记忆详情未给出效果画像");

// ---------- R-095 活动面板：完整流水 + 筛选 + 信息量 + 可操作 ----------
const toolStart = handlers.get("kz:tool-start");
const toolEnd = handlers.get("kz:tool-end");
const taskProgress = handlers.get("kz:task-progress");
assert(toolStart && toolEnd, "工具事件未订阅");
assert(taskProgress, "子代理进度事件未订阅");
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
// D-234:批次格由 Git 提交标题推导。直接 agent 与子 agent 提交都必须立即刷新快照，
// 不能等整轮结束才看到进度变化。
const docsBeforeBatchCommit = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
toolEnd({ payload: { id: "T4", name: "git", ok: true, preview: "committed verified staged set (abc123)", display: null } });
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "docs_snapshot").length > docsBeforeBatchCommit,
  "agent 提交后未即时刷新 Git 推导的批次进度",
);
const docsBeforeChildBatchCommit = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
taskProgress({ payload: { id: "T3", text: "子代理已提交", trace: { name: "git", phase: "end", ok: true, preview: "committed verified staged set (def456)" } } });
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "docs_snapshot").length > docsBeforeChildBatchCommit,
  "子代理提交后未即时刷新 Git 推导的批次进度",
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

// ---------- D-237 活动面板:diff 汇总着色 + bash 完整输出可展开 ----------
const d237ToolStart = handlers.get("kz:tool-start");
const editEnd = handlers.get("kz:tool-end");
d237ToolStart({ payload: { id: "T5", name: "edit", summary: "ui/main.js", input: { path: "ui/main.js" } } });
d237ToolStart({ payload: { id: "T6", name: "bash", summary: "cargo test -p kanzei-app", input: { command: "cargo test -p kanzei-app" } } });
editEnd({ payload: { id: "T5", name: "edit", ok: true, preview: "replaced 1 occurrence", display: { kind: "diff", path: "ui/main.js", additions: 3, deletions: 1, language: "js", lines: [] } } });
editEnd({ payload: { id: "T6", name: "bash", ok: true, preview: "exit code: 0", display: { kind: "terminal", command: "cargo test -p kanzei-app", output: "短输出(截断版)", full: "长输出…".repeat(100) } } });
await flush();
const diffRow = document.querySelector("#diff-summary");
assert(diffRow && diffRow.textContent.includes("+3") && diffRow.textContent.includes("−1"), "diff 汇总未收录文件的增删计数");
assert(
  diffRow.textContent.includes("ui/main.js"),
  "diff 汇总未显示文件路径",
);
// 冒烟的 innerHTML 是去标签近似(不建真实子节点),着色 span 的选择器断言不可用;
// 着色结构由 renderDiffSummary 的模板字符串保证(实际浏览器里 .diff-add/.diff-del 生效)。
const bashFullEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "T6");
assert(bashFullEntry, "bash 完整输出条目未出现");
assert(
  bashFullEntry.querySelector(".bg-detail")?.textContent.includes("长输出"),
  "bash 展开区未使用完整输出(full),仍停留在 4000 截断版",
);

// ---------- D-170 项目隔离失效必须报出来 ----------
assert(invokeLog.includes("project_root_info"), "切项目时未检查项目根是否与所选目录一致");
const sharedWarn = byId.get("project-shared-warn");
assert(sharedWarn, "缺少项目隔离告警位");
assert(!sharedWarn.classList.contains("hidden"), "所选目录与实际根不一致却没有告警(需求会在项目间串)");
assert(listText("project-shared-warn").includes("C:/smoke/parent"), "告警未给出实际生效的根");
const detachBtn = sharedWarn.querySelector("button");
assert(detachBtn, "缺少一键建立独立空间");
detachBtn.click();
await flush();
assert(invokeLog.includes("project_detach"), "点了建立独立空间却没调后端");

// ---------- D-169 列表被筛空必须说破，不能留一片空白 ----------
// 持久化的标签在当前项目可能不存在:下拉回落成"全部"而状态没跟着回落,
// 列表就被一个看不见的条件筛空——用户看到的是"需求凭空掉了"。
// 走真实持久化路径:把一个当前项目里不存在的标签写进偏好,再触发恢复与重绘。
const filtersKey = [...storage.keys()].find((k) => k.startsWith("kz-filters")) ?? "kz-filters:C:/smoke/project";
storage.set(filtersKey, JSON.stringify({ req: { tag: "这个标签不存在", status: "all", priority: "all", complexity: "all", blocked: "all", sort: "manual" } }));
sandbox.restoreDocFilters();
await sandbox.refreshDocs();
await flush();
// 不变量:列表不得"无声变空"。要么标签回落后条目照常显示,要么明说被筛掉了多少。
assert(
  document.querySelectorAll("#req-list .doc-item").length > 0
    || document.querySelector("#req-list .doc-filtered-empty"),
  "不存在的标签把列表筛空了,且界面没有任何说明——看起来就是需求凭空掉了",
);
assert(
  document.querySelectorAll("#req-list .doc-item").length > 0,
  "当前项目没有这个标签,筛选状态应回落成「全部」而不是筛空",
);

// 真实存在但无匹配的筛选:验证"被筛空"的提示与一键清除。
const statusFilterEl = byId.get("req-status-filter");
statusFilterEl.value = "dropped";
statusFilterEl._listeners.change?.forEach((fn) => fn({ target: statusFilterEl }));
await flush();
const filteredEmpty = document.querySelector("#req-list .doc-filtered-empty");
assert(filteredEmpty, "列表被筛空却没有任何说明(一片空白最容易被当成数据丢失)");
assert(/\d/.test(filteredEmpty.textContent), "未给出被隐藏的条数");
const clearFiltersBtn = filteredEmpty.querySelector("button");
assert(clearFiltersBtn, "被筛空时缺少一键清除筛选");
clearFiltersBtn.click();
await flush();
assert(
  document.querySelectorAll("#req-list .doc-item").length > 0,
  "点了清除筛选,条目没有回来",
);

// ---------- D-168 设置页模型角色：可选、不丢已存值、被覆盖时明示 ----------
const settingsTab = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "settings");
settingsTab?.click();
await flush();
const primarySelect = byId.get("set-primary");
assert(primarySelect.tagName === "SELECT", "模型角色仍是自由文本框(手打 provider:model 太容易拼错)");
const primaryValues = [...primarySelect.options].map((o) => o.value);
assert(primaryValues.includes(""), "缺少「未设」选项(不该强迫用户必须指定一个模型)");
assert(primaryValues.includes("__manual__"), "缺少手填兜底");
assert(
  primaryValues.some((v) => v.includes(":")),
  `未把探测到的模型灌进下拉,实得: ${primaryValues.join(",")}`,
);
// 已保存的值若探测不到,必须原样保留 —— 否则一进设置页就被悄悄改掉,保存一次配置就坏了。
assert(
  primarySelect.value === "deepseek:deepseek-chat",
  `已保存的模型未被保留,实得 "${primarySelect.value}"`,
);
assert(
  primaryValues.includes("deepseek:deepseek-chat"),
  "探测不到的已存值未补进选项列表",
);
// 项目级覆盖必须明说。
assert(
  !byId.get("settings-effective").classList.contains("hidden"),
  "项目级覆盖了 primary,但设置页没有任何提示",
);
assert(listText("settings-effective").includes("实际生效"), "覆盖提示未说明实际生效值");
// 表单脏状态:改一下就该出现「未保存」。
assert(byId.get("settings-dirty").classList.contains("hidden"), "刚载入时不该显示未保存");
const fastSelect = byId.get("set-fast");
fastSelect.value = "";
fastSelect.dispatchEvent({ type: "change" });
assert(
  !byId.get("settings-dirty").classList.contains("hidden"),
  "改了表单却没有「未保存」提示(界面显示 A、运行用 B 就是这么来的)",
);

assert(html.includes('id="set-codex-fast-mode"'), "设置页缺少 Codex Fast mode 开关标记");
// 运行上限([limits]):读、存、脏状态三条线缺一条就是"界面显示 A、运行用 B"(D-157)。
{
  const ids = ["set-max-tokens", "set-subagent-max-tokens", "set-subagent-timeout", "set-max-tasks",
    "set-context-ratio", "set-verbatim-ratio", "set-max-parallel", "set-stream-restarts",
    "set-transport-retries", "set-rate-retries"];
  for (const id of ids) {
    assert(html.includes(`id="${id}"`), `设置页缺少运行上限输入框 ${id}`);
    assert(source.includes(id), `main.js 没有接线运行上限字段 ${id}`);
  }
  assert(byId.get("set-max-tokens")?.value === "4096", `已配置的上限没回填到表单:${byId.get("set-max-tokens")?.value}`);
  assert(
    byId.get("set-subagent-timeout")?.value === "",
    "未配置的上限必须留空(空=用默认),不能填成默认值——否则一保存就把默认固化进配置",
  );
  assert(
    (byId.get("set-subagent-timeout")?.placeholder ?? "").includes("900"),
    `留空项要用占位符显示内置默认:${byId.get("set-subagent-timeout")?.placeholder}`,
  );
  assert(
    source.includes("limits: collectLimits()"),
    "保存设置未透传运行上限",
  );
  assert(
    SETTINGS_FORM_IDS_IN_SOURCE(source, "set-max-tokens"),
    "运行上限没登记进脏状态列表:改了数字不会提示未保存(D-157 复现路径)",
  );
}
function SETTINGS_FORM_IDS_IN_SOURCE(src, id) {
  const block = src.slice(src.indexOf("const SETTINGS_FORM_IDS"), src.indexOf("let settingsSnapshot"));
  return block.includes(id);
}
assert(source.includes('$("set-codex-fast-mode").checked = s.codexFastMode === true'), "设置页未恢复 Codex Fast mode 状态");
assert(source.includes("codexFastMode: $(\"set-codex-fast-mode\").checked"), "保存设置未透传 Codex Fast mode");

// 节奏([cadence],R-157):读、存、脏状态三条线,与运行上限同一套防线。
{
  const cadenceIds = ["set-cadence-full-test", "set-cadence-full-test-batches",
    "set-cadence-targeted-test", "set-cadence-commit", "set-cadence-push"];
  for (const id of cadenceIds) {
    assert(html.includes(`id="${id}"`), `设置页缺少节奏表单 ${id}`);
    assert(SETTINGS_FORM_IDS_IN_SOURCE(source, id), `main.js 没有登记节奏字段 ${id}(改了没未保存提示)`);
  }
  assert(
    byId.get("set-cadence-full-test")?.value === "every_n_batches",
    `节奏下拉未回填生效值,实得: ${byId.get("set-cadence-full-test")?.value}`,
  );
  assert(
    byId.get("set-cadence-full-test-batches")?.value === "3",
    "每 N 批间隔未回填已存值",
  );
  assert(
    byId.get("set-cadence-targeted-test")?.value === "every_commit",
    "定向测试下拉未回填",
  );
  // 存一次:载荷必须带上 cadence(camelCase 外壳里嵌套 snake_case 键)。
  byId.get("set-cadence-full-test").value = "release_only";
  byId.get("set-cadence-full-test-batches").value = "";
  byId.get("set-cadence-full-test").dispatchEvent({ type: "change" });
  byId.get("settings-save")?.click();
  await flush();
  const saveArgs = invokeLog.includes("settings_save")
    ? savedPayloads.get("settings_save")
    : null;
  assert(saveArgs, "点保存未调 settings_save");
  assert(
    saveArgs?.payload?.cadence?.full_test === "release_only" && saveArgs?.payload?.cadence?.full_test_batches === null,
    `保存载荷未透传 cadence: ${JSON.stringify(saveArgs?.payload?.cadence)}`,
  );
}

// ---------- R-136 子代理模型一键就绪 ----------
assert(invokeLog.includes("fast_model_status"), "设置页未检测子代理模型就绪状态");
assert(
  listText("fast-status").includes("服务未运行"),
  `子代理不可用却没说清缺哪一环,实得: "${listText("fast-status")}"`,
);
assert(
  listText("fast-status").includes("暂不可用"),
  "未说明后果(记忆整理/快速记录这类杂活会静默失效)",
);
const fastSetupBtn = byId.get("fast-setup");
assert(!fastSetupBtn.classList.contains("hidden"), "未就绪时应显示一键安装按钮");
fastSetupBtn.click();
await flush();
assert(invokeLog.includes("fast_model_setup"), "点了一键就绪却没调后端");
// 安装进度事件要能刷到状态行。
handlers.get("kz:fast-setup")?.({ payload: { text: "pulling 50%(1500/3000 MB)" } });
assert(listText("fast-status").includes("50%"), "安装进度未反映到界面");

// ---------- D-167 手填模型：探测不到不等于用不了 ----------
const modelSelect = byId.get("model-select");
const manualOption = [...modelSelect.options].find((o) => o.value === "__manual__");
assert(manualOption, "模型下拉缺少手填入口(端点不实现 /models 时就彻底没法选)");
sandbox.window.prompt = () => "deepseek:deepseek-chat";
modelSelect.value = "__manual__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
const manualKey = [...storage.keys()].find((k) => k.startsWith("kz-manual-models"));
assert(manualKey, "手填模型未落盘(下次重开又要再填一遍)");
assert(JSON.parse(storage.get(manualKey)).includes("deepseek:deepseek-chat"), "手填模型落盘值不对");
assert(
  [...byId.get("model-select").options].some((o) => o.value === "deepseek:deepseek-chat"),
  "手填后模型未回到下拉列表里",
);
// 格式不对要挡住:provider 名对不上配置键时后端 resolve_model 会直接失败。
sandbox.window.prompt = () => "随便写的";
modelSelect.value = "__manual__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
assert(
  !JSON.parse(storage.get(manualKey)).includes("随便写的"),
  "非 provider:model 格式不应被接受",
);

// ---------- R-115 偏好持久化：写了必须能读回 ----------
// 「写了却从不读回」是这块最容易出的问题:kz-reasoning 曾经全仓零处 getItem,
// 看起来存了,重启后照样回默认档。这里逐项验"改一次 → 落盘 → 能回填"。
const reasoningSelect = byId.get("reasoning-select");
reasoningSelect.value = "high";
reasoningSelect._listeners.change?.forEach((fn) => fn({ target: reasoningSelect }));
const reasoningKey = [...storage.keys()].find((k) => k.startsWith("kz-reasoning"));
assert(reasoningKey, "思考强度未落盘");
assert(storage.get(reasoningKey) === "high", `思考强度落盘值不对: ${storage.get(reasoningKey)}`);
assert(reasoningKey.includes(":"), "思考强度应按项目分键,不同项目常配不同模型");

const deliverySelect = byId.get("delivery-select");
deliverySelect.value = "steer";
deliverySelect._listeners.change?.forEach((fn) => fn({ target: deliverySelect }));
assert(storage.get("kz-delivery") === "steer", "交付方式未落盘");

const reqStatusFilter = byId.get("req-status-filter");
reqStatusFilter.value = "doing";
reqStatusFilter._listeners.change?.forEach((fn) => fn({ target: reqStatusFilter }));
await flush();
const filterKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
assert(filterKey, "需求筛选未落盘(重启后会回到「全部」)");
assert(JSON.parse(storage.get(filterKey)).req.status === "doing", "筛选落盘值不对");

// 模式回退链:本进程记忆 → 全局上次选择 → dev-pair。中间那档缺了就会静默降级。
assert(typeof sandbox.applyProfileValue === "function", "applyProfileValue 未定义");
storage.set("kz-profile", "dev-auto");
sandbox.applyProfileValue("dev");
assert(
  byId.get("profile-select").value === "dev-auto",
  `无进程记忆时应回退到全局上次选择,实得 ${byId.get("profile-select").value}(重启后自主推进会被降级成结伴开发)`,
);
storage.set("kz-profile", "research");
sandbox.applyProfileValue("dev");
assert(
  byId.get("profile-select").value === "dev-pair",
  "全局值与后端 profile 冲突时应回落 dev-pair,不能把 research 塞进 dev 进程",
);

// ---------- R-099/R-127 运行画像面板 ----------
const metricsTab = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "metrics");
assert(metricsTab, "活动栏缺少运行画像入口");
metricsTab.click();
await flush();
assert(invokeLog.includes("run_metrics"), "运行画像页未拉取度量");
const metricRounds = document.querySelectorAll("#metrics-rounds .metrics-round");
assert(metricRounds.length === 2, `逐轮画像未渲染,实得 ${metricRounds.length}`);
const metricsText = listText("metrics-rounds");
assert(metricsText.includes("edit 1/6"), `未给出 edit 未命中比,实得: ${metricsText.slice(0, 100)}`);
assert(metricsText.includes("git 2"), "未给出 git 查询次数与组数");
assert(metricsText.includes("edit×6"), "未给出工具分布");
assert(metricsText.includes("4800"), "未汇总上下文占用");
// 未度量的轮次要明说,不能显示成"全零"——那会让人误判冗余在下降。
assert(metricsText.includes("该轮早于度量落地"), "未区分「没度量」与「度量为零」");
const trendText = listText("metrics-trend");
assert(trendText.includes("1 ") && trendText.includes("轮均值"), "趋势未按已度量轮次统计");
assert(trendText.includes("17%"), `均值应只算已度量轮次(1/6≈17%),实得: ${trendText}`);

// ---------- R-126 UI 自查探针：在真实窗口里取样并回传 ----------
const probe = handlers.get("kz:ui-probe");
assert(probe, "未订阅 UI 探针事件(agent 无法自查界面)");
probe({ payload: { id: 1, kind: "dom", arg: "#req-list" } });
await flush();
assert(probeResults.length === 1, "DOM 探针未回传结果");
assert(probeResults[0].id === 1, "探针回传未带上请求 id(后端无法配对)");
assert(probeResults[0].result.includes("doc-item"), `DOM 探针未给出真实渲染结构: ${probeResults[0].result.slice(0, 80)}`);
probe({ payload: { id: 2, kind: "dom", arg: "#nonexistent-xyz" } });
await flush();
assert(
  probeResults[1].result.includes("没有匹配"),
  "选择器无匹配时应明确说明,而不是回空串让人以为渲染了空内容",
);
// 用 warn 验证捕获链路:sandbox 的 console.error 本身就是冒烟的失败护栏,
// 拿它当样本会把这条测试变成自失败。捕获逻辑对 error/warn 是同一条。
sandbox.console.warn("smoke probe marker");
probe({ payload: { id: 3, kind: "console", arg: "" } });
await flush();
assert(
  probeResults[2].result.includes("smoke probe marker"),
  `console 探针未捕获(ReferenceError 一类问题就是这样漏过去的),实得: ${probeResults[2]?.result?.slice(0, 60)}`,
);
probe({ payload: { id: 4, kind: "unknown-kind", arg: "" } });
await flush();
assert(probeResults[3].result.includes("未知探针类型"), "未知探针类型应回传说明而不是静默");

// ---------- 语言切换：静态文本/属性与动态错误必须 zh→en→zh→en 可逆 ----------
const languageControl = byId.get("language-select");
const projectInit = byId.get("project-init");
const chatActivity = document.querySelectorAll(".activity-item")[0];
assert(projectInit.getAttribute("title") === "初始化新项目目录", "HTML title 未进入真实冒烟 DOM");
assert(chatActivity.getAttribute("aria-label") === "切换到对话", "HTML aria-label 未进入真实冒烟 DOM");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "en", "切换 English 后 document.lang 未更新");
assert(storage.get("kz-language") === "en", "English 选择未持久化");
assert(projectInit.getAttribute("title") === "Initialize a new project directory", "静态 title 未翻译");
assert(chatActivity.getAttribute("aria-label") === "Switch to chat", "静态 aria-label 未翻译");
handlers.get("kz:error")?.({ payload: { message: "smoke backend failure" } });
await flush();
assert(listText("live-turn").includes("Error"), `英文动态错误状态未翻译: "${listText("live-turn")}"`);
assert(document.querySelector(".error-level")?.textContent === "Fatal error", "英文错误等级未翻译");
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "zh-CN", "切回中文后 document.lang 未更新");
assert(storage.get("kz-language") === "zh", "中文选择未持久化");
assert(listText("live-turn").includes("出错"), `中文动态错误状态未恢复: "${listText("live-turn")}"`);
assert(
  document.querySelector(".error-level")?.textContent === "致命错误",
  `动态错误等级切回中文失败:${document.querySelector(".error-level")?.textContent}`,
);
assert(projectInit.getAttribute("title") === "初始化新项目目录", "静态 title 切回中文失败");
assert(chatActivity.getAttribute("aria-label") === "切换到对话", "静态 aria-label 切回中文失败");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(projectInit.getAttribute("title") === "Initialize a new project directory", "静态 title 二次切英文失败");
assert(chatActivity.getAttribute("aria-label") === "Switch to chat", "静态 aria-label 二次切英文失败");
assert(document.querySelector(".error-level")?.textContent === "Fatal error", "动态错误等级二次切英文失败");
const askHandler = handlers.get("kz:ask");
askHandler?.({ payload: { id: 91, sessionId: "sess-smoke", kind: "permission", action: "执行用户动作 Ω", resource: "用户/路径甲", remember: "用户/路径甲" } });
askHandler?.({ payload: { id: 92, sessionId: "sess-smoke", kind: "permission", action: "写入用户数据 Ω", resource: "用户/路径乙", remember: "用户/路径乙" } });
await flush();
assert(listText("ask-title") === "Permission request", `英文权限标题未翻译:${listText("ask-title")}`);
assert(listText("ask-queue-status").includes("1 pending"), `英文权限队列说明未翻译:${listText("ask-queue-status")}`);
assert(listText("ask-action") === "执行用户动作 Ω", "权限 action 用户数据被翻译或改写");
assert(byId.get("ask-deny").textContent === "Deny", "英文权限拒绝按钮未翻译");
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(listText("ask-title") === "权限请求", "权限标题切回中文失败");
assert(listText("ask-queue-status").includes("还有 1 条待处理"), "权限队列说明切回中文失败");
assert(byId.get("ask-deny").textContent === "拒绝", "权限拒绝按钮切回中文失败");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(listText("ask-title") === "Permission request", "权限标题二次切英文失败");

// ---------- R-086 多会话并发:控制事件按 sessionId 收敛,切回可见可答复、不丢不串 ----------
// 前置:清空上面语言切换测试留下的主会话 ask(91/92 仍在队列,askActive=91)。
if (byId.get("ask-allow")) byId.get("ask-allow").click();
await flush();
if (!byId.get("ask-overlay").classList.contains("hidden") && byId.get("ask-allow")) byId.get("ask-allow").click();
await flush();
assert(byId.get("ask-overlay").classList.contains("hidden"), "R-086 前置:主会话 ask 未清空");
// 场景:主会话(sess-smoke)活动;后台会话(sess-bg)初始 running=true(桩里故意给旧值,
// 模拟"事件已收敛但轮询采样发生在事件之前"的竞态)。
const activeTab = document.querySelector(".process-tab.active");
assert(activeTab?.textContent.includes("主会话"), `冒烟前置:主会话应为活动进程(实际:${activeTab?.textContent})`);
// 后台会话的权限询问到达:不弹当前窗口,但进入该会话自己的待答队列。
askHandler?.({
  payload: { id: 501, sessionId: "sess-bg", kind: "permission", action: "后台进程要写文件", resource: "后台/路径", remember: "后台/路径" },
});
await flush();
assert(byId.get("ask-overlay").classList.contains("hidden"), "后台会话的 ask 不应在活动会话弹窗");
// 后台会话第一轮结束(kz:done)。kz:done 只是**一轮**的终点:后端 run loop 会 promote
// 排队输入接着跑,runtime.running 仍是 true。拿它收敛会让多轮运行从第二轮起全程显示
// 空闲且再也纠不回来(converged 屏蔽了轮询校正),所以这里必须仍然是运行中。
handlers.get("kz:done")?.({ payload: { steps: 1, halted: false, history: 3, input: 0, output: 0, cacheRead: 0, cacheWrite: 0, sessionId: "sess-bg" } });
await flush();
const bgState = sandbox.sessionState("sess-bg");
assert(bgState.converged === false, "kz:done 是轮末事件,不得收敛会话终态(排队输入还要继续跑)");
assert(bgState.running === true, `kz:done 后会话仍在跑,运行态被误清: ${bgState.running}`);
const bgTabAfterDone = [...document.querySelectorAll(".process-tab")].find((tab) => tab.textContent.includes("后台会话"));
assert(bgTabAfterDone?.textContent.includes("●"), `多轮运行第一轮结束后标签页熄灯(实际:${bgTabAfterDone?.textContent})`);
assert(byId.get("stop").classList.contains("hidden"), "后台会话结束不应改变主会话视图的运行态");
// 第二轮开跑:kz:turn 是每轮开头必发的自愈信号,把状态机拨回运行中并解除 converged。
handlers.get("kz:turn")?.({ payload: { step: 1, maxSteps: 30, sessionId: "sess-bg" } });
await flush();
assert(sandbox.sessionState("sess-bg").running === true, "后台会话第二轮 kz:turn 未把状态机拨回运行中");
assert(sandbox.sessionState("sess-bg").converged === false, "kz:turn 未解除 converged,状态机会被上一轮终态焊死");
// 会话真正转空闲(后端 run loop 退出)才收敛终态。
handlers.get("kz:idle")?.({ payload: { reason: "completed", sessionId: "sess-bg" } });
await flush();
assert(sandbox.sessionState("sess-bg").running === false, "kz:idle 未收敛运行态");
assert(sandbox.sessionState("sess-bg").converged === true, "kz:idle 未标记 converged");
const bgTabAfterIdle = [...document.querySelectorAll(".process-tab")].find((tab) => tab.textContent.includes("后台会话"));
assert(!bgTabAfterIdle?.textContent.includes("●"), "会话已空闲但标签页仍亮着运行标记");
// 切回后台会话:权限询问可见可答复,运行态显示空闲(converged 挡住桩里的旧 running=true)。
await sandbox.switchProcess("p|bg");
await flush();
const bgTab = document.querySelector(".process-tab.active");
assert(bgTab?.textContent.includes("后台会话"), "切换到后台会话后活动进程 tab 未更新");
assert(!byId.get("ask-overlay").classList.contains("hidden"), "切回后台会话后权限询问不可见");
assert(listText("ask-action") === "后台进程要写文件", "切回后弹出的不是该会话自己的 ask(串会话)");
assert(byId.get("stop").classList.contains("hidden"), "后台会话已收敛终态但切回后仍显示运行中(converged 未生效)");
// 可答复:点允许后 invoke answer_ask,弹窗关闭,队列清空。
byId.get("ask-allow").click();
await flush();
assert(invokeLog.includes("answer_ask"), "切回后台会话后权限询问无法答复(answer_ask 未调用)");
assert(byId.get("ask-overlay").classList.contains("hidden"), "答复后权限弹窗未关闭");
// 再切回主会话:不串台,无残留弹窗。
await sandbox.switchProcess("d|smoke");
await flush();
const backTab = document.querySelector(".process-tab.active");
assert(backTab?.textContent.includes("主会话"), "切回主会话后活动进程 tab 未更新");
assert(byId.get("ask-overlay").classList.contains("hidden"), "切回主会话后残留后台 ask 弹窗");

// 重建路径:后端 asks 表活得比 webview 久,界面重载后首次拿到进程列表必须补拉回来,
// 否则重载前挂起的权限询问再也不出现,而后端还在 await 它的答复(验收:后端提供
// pending asks 查询以支持重建)。这里用一个从未见过的会话模拟"重载后的第一次渲染"。
payloads.pending_asks_get = [
  { id: 601, sessionId: "sess-reload", kind: "permission", action: "重载前挂起的询问", resource: "重载/路径", remember: "重载/路径" },
];
const asksPullsBefore = invokeLog.filter((cmd) => cmd === "pending_asks_get").length;
sandbox.renderProcesses([{ id: "r|reload", label: "重载会话", session_id: "sess-reload", running: false }]);
await flush();
assert(
  invokeLog.filter((cmd) => cmd === "pending_asks_get").length > asksPullsBefore,
  "首次拿到进程列表未向后端补拉待答队列(重载后挂起的 ask 会永久失联)"
);
assert(!byId.get("ask-overlay").classList.contains("hidden"), "重载后未从后端重建待答权限询问");
assert(listText("ask-action") === "重载前挂起的询问", `重建出的不是后端返回的那条 ask:${listText("ask-action")}`);
byId.get("ask-allow").click();
await flush();
// 收尾:恢复原进程列表(活动进程回到主会话),后续用例不受影响。
payloads.pending_asks_get = [];
sandbox.renderProcesses([
  { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false },
  { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true },
]);
await flush();
assert(document.querySelector(".process-tab.active")?.textContent.includes("主会话"), "重建用例收尾后活动进程未回到主会话");

// ---------- R-076 鞭挞状态机:防空转硬化与外部阻塞刹车 ----------
// 前置:切回中文(前面 i18n 段把界面留在英文,刹车原因文案断言按中文写),
// 切到 dev-auto(鞭挞仅此档位可跑),把计数拨到已知状态。
assert(sandbox.__kzTest, "未注入鞭挞状态测试钩子");
const savedProfileForWhip = byId.get("profile-select").value;
const savedAutoCheck = byId.get("auto-continue").checked;
const savedLangForWhip = languageControl.value;
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
byId.get("profile-select").value = "dev-auto";
byId.get("auto-continue").checked = true;
sandbox.__kzTest.reset();
// ① 实质进展轮(edit 等非只读工具):计入推进轮次,不刹车。
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { read: 2, edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 1, `实质进展轮应计入推进轮次,实得 ${sandbox.__kzTest.rounds()}`);
assert(byId.get("auto-continue").checked, "实质进展轮不应关掉自动推进");
// ② 只有 memory_note 的轮次(写日记):第一次只追加推进指令,第二次刹车。
handlers.get("kz:done")?.({ payload: { steps: 2, halted: false, tools: { memory_note: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.noAction() === 1, "写日记轮次第一次应记为无动作并追加推进指令");
assert(sandbox.__kzTest.rounds() === 2, "追加推进指令也应占推进轮次");
assert(byId.get("auto-continue").checked, "写日记轮次第一次不应立即刹车");
handlers.get("kz:done")?.({ payload: { steps: 2, halted: false, tools: { memory_note: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 0, "连续两轮无实质动作后推进计数应清零");
assert(sandbox.__kzTest.stopReason().includes("连续两轮无动作"), `刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
assert(byId.get("auto-status").textContent.includes("连续两轮无动作"), `#auto-status 未显示刹车原因: ${byId.get("auto-status")?.textContent}`);
// ③ 真实改动轮(bash/edit)不触发无动作,也不被记成空转。
sandbox.__kzTest.reset();
handlers.get("kz:done")?.({ payload: { steps: 4, halted: false, tools: { bash: 1, edit: 2 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 1, "真实改动轮应继续推进");
assert(sandbox.__kzTest.noAction() === 0, "真实改动轮不得计为无动作");
// ④ 外部阻塞刹车:需求/缺陷全部带 blocked 标记 → 无可推进项,停并给出阻塞原因。
const savedDocsSnapshot = structuredClone(payloads.docs_snapshot);
payloads.docs_snapshot = {
  requirements: [docEntry("R-001", "被阻塞需求", "doing", { blocked: true })],
  defects: [docEntry("D-001", "被阻塞缺陷", "open", { blocked: true })],
};
sandbox.__kzTest.setRounds(3); // 模拟鞭挞进行中
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-continue").checked, "需求/缺陷全部被阻塞时自动推进应停止");
assert(sandbox.__kzTest.rounds() === 0, "阻塞刹车后推进计数应清零");
assert(sandbox.__kzTest.stopReason().includes("全部被阻塞"), `阻塞刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
payloads.docs_snapshot = savedDocsSnapshot;
// ⑤ 恢复桩数据后,存在可推进条目时不得误刹车。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(byId.get("auto-continue").checked, "存在可推进条目时不得因阻塞刹车");
// ⑥ backlog 清空(无任何活动条目):停止,原因与阻塞区分。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(2);
payloads.docs_snapshot = { requirements: [], defects: [] };
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-continue").checked, "需求/缺陷清空时自动推进应停止");
assert(sandbox.__kzTest.stopReason().includes("已清空"), `清空刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
payloads.docs_snapshot = savedDocsSnapshot;
// ⑦ 本轮后停:本轮完成后停,开关自动取消勾选。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(1);
sandbox.__kzTest.setStopAfterRound(true);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-stop-round").checked, "本轮后停后开关应自动取消勾选");
assert(sandbox.__kzTest.stopReason().includes("本轮后停"), `本轮后停原因不对: ${sandbox.__kzTest.stopReason()}`);
// ⑧ 达到上限:推进轮数等于上限即停,原因明确。
byId.get("auto-continue").checked = true;
const autoMaxWhip = Number.parseInt(byId.get("auto-max").value, 10) || 10;
sandbox.__kzTest.setRounds(autoMaxWhip);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 0, "达到上限后推进计数应清零");
assert(sandbox.__kzTest.stopReason().includes("已达连上限"), `上限刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
// ⑨ 暂停:暂停中完成本轮 → 停;恢复后再推进轮次照常增长。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setPaused(true);
sandbox.__kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.stopReason().includes("已暂停"), `暂停刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
sandbox.__kzTest.setPaused(false);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 2, `恢复后推进轮次应继续增长,实得 ${sandbox.__kzTest.rounds()}`);
// ⑩ 用户拒绝(halted):整段鞭挞分支不进入,推进计数原地不动(不续也不清零)。
sandbox.__kzTest.setRounds(4);
handlers.get("kz:done")?.({ payload: { steps: 2, halted: true, tools: { edit: 1 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 4, "用户拒绝后推进计数应保持原样(不再续跑)");
// 收尾:恢复冒烟前置环境(语言/档位/开关/计数)。
byId.get("profile-select").value = savedProfileForWhip;
byId.get("auto-continue").checked = savedAutoCheck;
languageControl.value = savedLangForWhip;
languageControl.dispatchEvent({ type: "change" });
await flush();
sandbox.__kzTest.reset();

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

// ---------- 文件导览(R-148):树渲染出目录聚合、文件度量与标注 ----------
{
  const tree = byId.get("files-tree");
  let treeText = tree?.textContent ?? "";
  assert(treeText.includes("src/"), `文件树缺目录行: "${treeText.slice(0, 80)}"`);
  assert(treeText.includes("源码目录"), "目录用途标注未渲染");
  assert((byId.get("files-summary")?.textContent ?? "").includes("2"), "汇总行缺文件计数");
  // 目录默认折叠(VSCode 同款):点开后文件行才出现,顺带验证展开交互本身。
  for (const row of [...tree.querySelectorAll(".files-dir")]) row.click();
  await flush();
  treeText = tree.textContent;
  assert(treeText.includes("lib.rs") && treeText.includes("120"), "展开后文件行缺行数度量");
  assert(treeText.includes("note.md") && treeText.includes("300"), "展开后 md 文件缺字数度量");
  assert(treeText.includes("冒烟样例:库入口"), "文件用途标注未渲染");
}

// ---------- D-202 流式渲染性能回归 ----------
// 卡顿的两个放大器都在"每个 delta 做一次"上:①i18n observer 全文档重扫;
// ②整条消息重新 renderMarkdown。这里按行为断言,不认代码形态——谁把它们改回
// 每 delta 一次,这两条就红。
{
  const walksBefore = fullDocumentWalks;
  let renders = 0;
  const realRenderMarkdown = sandbox.renderMarkdown;
  sandbox.renderMarkdown = (raw) => { renders += 1; return realRenderMarkdown(raw); };
  const DELTAS = 200;
  for (let i = 0; i < DELTAS; i += 1) sandbox.appendAssistant(`stream-chunk-${i} with some filler text
`);
  await flush();
  sandbox.renderMarkdown = realRenderMarkdown;

  assert(renders > 0, "renderMarkdown 包装未生效,本组断言全是假通过");
  assert(
    renders <= DELTAS / 10,
    `流式渲染没有合帧:${DELTAS} 个 delta 触发了 ${renders} 次 renderMarkdown(每次都整条重渲染 = 单条消息内 O(n²),D-202)`
  );
  assert(
    fullDocumentWalks === walksBefore,
    `流式 delta 触发了 ${fullDocumentWalks - walksBefore} 次全文档 i18n 重扫(单次成本 ∝ 对话长度,轮次越多越卡,D-202)`
  );
  const rendered = sandbox.document.querySelectorAll(".msg.assistant .message-body").at(-1);
  assert(
    rendered?.textContent.includes(`stream-chunk-${DELTAS - 1}`),
    "合帧后最后一段流式文本没渲染出来(延迟渲染丢尾巴比卡顿更糟)"
  );
}

// 增量本地化必须真的翻译新进节点:observer 不再全量重扫后,漏翻就是新的回归面。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  const probe = document.createElement("div");
  probe.textContent = "移动端桥接";
  document.body.appendChild(probe);
  await flush();
  assert(
    probe.textContent === "Mobile bridge",
    `新进节点未被增量本地化(实际 "${probe.textContent}"):observer 只处理变动节点后,漏翻即回归`
  );
  probe.remove();
  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

if (issues.length) {
  console.error(`UI 运行时冒烟失败(${issues.length} 处):`);
  for (const issue of issues) console.error(` - ${issue}`);
  process.exit(1);
}
console.log(
  `UI 运行时冒烟通过:${sources.length} 个 ui/*.js 按序执行 + 初始化序列(${invokeLog.length} 次 invoke) + ` +
  `需求/缺陷/目标/测试/历史列表渲染 + ${document.querySelectorAll(".activity-item").length} 个主视图切换,0 运行时错误`
);
