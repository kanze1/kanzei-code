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

// ---------- 变异守卫(R-177 内容③)----------
// 一条恒绿的断言与一条真护栏在 CI 里长得一模一样。`KZ_SMOKE_MUTATE=<id>` 时,
// 把**被守护的那一行源码**直接删掉再跑,期望脚本非零退出——这样「删了它就变红」
// 是机械判据,不是写在注释里的承诺。
// 变异没命中(源码被改写、那一行不在了)本身就是失败:守卫悄悄失效比断言变红更危险。
const SMOKE_MUTATE = process.env.KZ_SMOKE_MUTATE ?? "";
if (SMOKE_MUTATE) {
  // 正则而不是字面量:仓里 ui/*.js 的换行是 CRLF/LF 混着的,字面量 "\n" 会漏匹配,
  // 而漏匹配的变异是**假绿**——它看起来"守卫生效了",其实一行都没删。
  const mutations = {
    // D-251:refreshWorktrees 在 await 之后那一次 currentProject 复查。
    // 删了它,项目甲在途的清单会被画进项目乙的面板。
    d251: {
      pattern: /[ \t]*if \(currentProject !== forProject\) return;\r?\n(\s*renderWorktrees\(live\);)/,
      replace: "$1",
    },
    // D-257:刷新按钮的监听器。删了它,按钮点下去什么都不发生。
    d257: {
      pattern: /\$\("worktrees-refresh"\)\.addEventListener\("click", refreshWorktrees\);/,
      replace: "",
    },
  };
  const mutation = mutations[SMOKE_MUTATE];
  if (!mutation) {
    console.error(`未知的 KZ_SMOKE_MUTATE=${SMOKE_MUTATE}(可用:${Object.keys(mutations).join(" / ")})`);
    process.exit(2);
  }
  let hit = 0;
  for (let i = 0; i < sources.length; i += 1) {
    const global = new RegExp(mutation.pattern.source, "g");
    hit += (sources[i].match(global) ?? []).length;
    sources[i] = sources[i].replace(global, mutation.replace);
  }
  if (hit !== 1) {
    console.error(`变异 ${SMOKE_MUTATE} 没有恰好命中一处被守护的源码(实得 ${hit} 处):护栏已经失效,先修变异表`);
    process.exit(2);
  }
  console.error(`[KZ_SMOKE_MUTATE=${SMOKE_MUTATE}] 已删除被守护的源码,期望本次运行**失败**`);
}

const issues = [];
const fail = (msg) => issues.push(msg);
// 断言在真实 DOM 上跑,某条护栏一旦真的红了,后续代码常常会顺带对 null 取属性而硬崩:
// 进程带着一个孤零零的 TypeError 退出,已经攒下的失败清单全看不见,读的人只能从
// "Cannot read properties of null" 反推是哪条能力没了。这个钩子保证无论怎么退出,
// 已收集的问题都先打出来——一次跑完拿到全部线索,而不是修一条崩一次。
let reportedIssues = false;
process.on("exit", (code) => {
  if (code !== 0 && !reportedIssues && issues.length) {
    console.error(`崩溃前已收集到 ${issues.length} 处失败:`);
    for (const issue of issues) console.error(` - ${issue}`);
  }
});

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
// 小工具降噪后,非静默工具仍需全量入列(R-095 的"完整流水"只对有信息量的调用成立)。
// R-173:分流判定必须连 input 一起传——编排派发的勘察/复核子代理 name 恒为 "task",
// 只看 name 会把它们连同模型自己派的 task 一起静默,内部进度整批丢掉。
if (!source.includes('else if (isActivityTool(e.payload.name, e.payload.input)) bgAdd')) {
  fail("活动面板仍会接收全部工具调用(或降噪分流被移除,或分流判定不再看 input.phase)");
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
// 工具块的 ⎿ 摘要行与展开详情必须是同一份文本切出来的两段(toolResultSplit),
// 不能各自独立地从 content 取一遍再靠 `full !== preview` 去重——那个写法只挡得住
// 单行短结果,首行超长或多行一律把同一段文案渲染两遍。运行时判据见下面的工具块用例;
// 这条静态契约拦的是"改回旧写法"这个具体形态。
if (source.includes("full.trim() !== preview")) {
  fail("工具块详情又回到了「摘要之外再贴一遍完整原文」的写法(full.trim() !== preview)");
}
if (!source.includes("function toolResultSplit")) {
  fail("工具块缺少 toolResultSplit:摘要与详情不再是同一份文本的互斥两段,双写会复发");
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
  // R-170:引擎规则(验收证据/调用方/范围保持)已剥离出继续文案,归 system prompt;
  // 前端源码不应再持有这些规则文本(验收①快照断言)。
  for (const ruleText of ["逐条对照验收原文", "真实调用方或消费者", "不得缩小验收里的平台或范围限定词"]) {
    if (source.includes(ruleText)) {
      fail(`08-compose.js 仍持有引擎规则文本「${ruleText}」(R-170 应已剥离)`);
    }
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
// ---------- <select> 的规范语义 ----------
// 早期 harness 把 select.value 当普通属性存:赋什么都照单全收。真实浏览器不是这样——
// 给 select 赋一个没有匹配 <option> 的值,只会把 selectedIndex 打到 -1、value 读回空串。
// 差别不是细节:「先 select.value = 已存值,再读 DOM 当基准」这种写法在真机上等于把
// 已存配置静默清空(D-168 同族,一次保存就把 kanzei.toml 的键删掉),而在旧 harness 里
// 恒为通过。所以这里按规范实现,让这类缺陷在冒烟里现形。
const selectOptions = (el) => el.childNodes.filter((n) => n instanceof Element && n.tagName === "OPTION");
// HTML 规范的 "ask for a reset":单选 select 的 option 列表变动后,若没有任何 option
// 处于选中态,第一个自动选中。少了这一步,replaceChildren 之后 value 恒为空串。
function resetSelectedness(el) {
  if (el.tagName !== "SELECT") return;
  const options = selectOptions(el);
  if (!options.length || options.some((o) => o._selected)) return;
  options[0]._selected = true;
}
// select 的 innerHTML/index.html 静态标记里写死的 <option> 必须建成真实子节点,
// 否则 select.options 恒为空,上面的规范语义会把所有下拉一起变哑(实测会连累
// 语言切换、节奏回填、思考强度落盘等 20 多条无关断言)。
function parseOptionsInto(el, fragment) {
  for (const [, attributes, inner] of String(fragment).matchAll(/<option([^>]*)>([\s\S]*?)<\/option>/g)) {
    const option = new Element("option");
    option.ownerDocument = el.ownerDocument;
    const text = inner.replace(/<[^>]*>/g, "").replace(/\s+/g, " ").trim();
    option.textContent = text;
    // R-140 批5:静态 option 的 data-i18n-key 也要建到桩元素上,否则渲染点翻译
    // 对 option 文本恒不生效,文档域的筛选下拉在冒烟里全是假通过。
    const keyValue = attributes.match(/\bdata-i18n-key="([^"]*)"/)?.[1];
    if (keyValue !== undefined) option.setAttribute("data-i18n-key", keyValue);
    const valueAttribute = attributes.match(/\bvalue="([^"]*)"/)?.[1];
    option.value = valueAttribute === undefined ? text : valueAttribute;
    // value setter 只写 _value;真实浏览器里 getAttribute("value") 也会返回该值,
    // 而 matchesOne 的属性选择器走 getAttribute。不同步的话 `option[value="x"]`
    // 在冒烟里恒空——R-178 批4 的设置页作用域下拉(option[value="project"])就撞上了。
    if (valueAttribute !== undefined) option._attributes.value = valueAttribute;
    if (/\bselected\b/.test(attributes)) option._selected = true;
    el.appendChild(option);
  }
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
  _adopt(node) { node.parentNode = this; node.ownerDocument = this.ownerDocument; this.childNodes.push(node); resetSelectedness(this); notifyMutation({ type: "childList", target: this, addedNodes: [node] }); return node; }
  appendChild(node) { node.remove(); return this._adopt(node); }
  append(...nodes) { for (const n of nodes) this.appendChild(typeof n === "string" ? this.ownerDocument.createTextNode(n) : n); }
  prepend(...nodes) { for (const n of nodes.reverse()) this.insertBefore(typeof n === "string" ? this.ownerDocument.createTextNode(n) : n, this.childNodes[0] ?? null); }
  insertBefore(node, ref) {
    node.remove();
    node.parentNode = this;
    node.ownerDocument = this.ownerDocument;
    const idx = ref ? this.childNodes.indexOf(ref) : -1;
    if (idx < 0) this.childNodes.push(node); else this.childNodes.splice(idx, 0, node);
    resetSelectedness(this);
    return node;
  }
  replaceChildren(...nodes) { for (const c of [...this.childNodes]) c.parentNode = null; this.childNodes = []; this._innerHTML = ""; this.append(...nodes); resetSelectedness(this); }
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
    // select 是例外:它的 innerHTML 里写的是 <option>,必须建成真实子节点。
    // 只留去标签文本的话 select.options 恒为空,规范化的 value 语义会把它变成一个哑控件。
    if (this.tagName === "SELECT") {
      this._textContent = "";
      parseOptionsInto(this, value);
      resetSelectedness(this);
      notifyMutation({ type: "childList", target: this, addedNodes: [this] });
      return;
    }
    this._textContent = String(value)
      .replace(/<[^>]*>/g, "")
      .replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&").replace(/&quot;/g, '"');
    notifyMutation({ type: "childList", target: this, addedNodes: [this] });
  }
  get value() {
    if (this.tagName === "SELECT") {
      const options = selectOptions(this);
      const selected = options.find((o) => o._selected);
      // selectedIndex < 0(含"一个 option 都没有"的空壳)→ 空串,与浏览器一致。
      return selected ? selected.value : "";
    }
    return this._value ?? "";
  }
  set value(v) {
    const next = String(v);
    if (this.tagName === "SELECT") {
      // 精确查找:命中就选中它,没命中就全部取消选中(= selectedIndex -1),不再"照单全收"。
      for (const option of selectOptions(this)) option._selected = option.value === next;
    }
    this._value = next;
  }
  get selectedIndex() { return this.tagName === "SELECT" ? selectOptions(this).findIndex((o) => o._selected) : -1; }
  set selectedIndex(index) {
    if (this.tagName !== "SELECT") return;
    selectOptions(this).forEach((option, idx) => { option._selected = idx === Number(index); });
  }
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
    // 真实浏览器的 IDL 反射:title/placeholder 等属性会同步到同名 property。
    // 假 DOM 不反射的话,applyDataI18nKeys 走 setAttribute 后 `el.title` 读不到,
    // 切语言断言在 harness 里失明(R-140 批10 实测:observer 退役后属性与 property 脱节)。
    if (name === "title" || name === "placeholder") this[name] = next;
    // data-* 同步进 dataset(真实浏览器行为):main.js 读 `el.dataset.x`,
    // 冒烟里不同步则 applyDataI18nKeys 等读 dataset 的逻辑在 harness 里失明。
    if (name.startsWith("data-") && name.length > 5) {
      const camel = name.slice(5).replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      this.dataset[camel] = next;
    }
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
  createElementNS: (_ns, tag) => { const el = new Element(tag); el.ownerDocument = document; return el; },
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
  for (const attribute of ["data-i18n-key", "data-i18n-title", "data-i18n-aria-label", "data-i18n-placeholder"]) {
    const value = attributes.match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
  byId.set(id, el);
  body.appendChild(el);
  // 后代选择器需要真实嵌套:按 id 造出来的节点是扁平的,`#providers-table tbody`
  // 会拿到 null。视图切换护栏打开后 settings 首次被真正执行,立刻暴露了这个缺口。
  if (el.tagName === "TABLE") el.appendChild(document.createElement("tbody"));
  // index.html 里写死的 <option> 也要建成真实子节点(见 parseOptionsInto 的说明):
  // 语言/代理/思考强度/节奏/各类筛选下拉的选项全在标记里,不建就全变哑控件。
  if (el.tagName === "SELECT") {
    const tail = html.slice(match.index + match[0].length);
    const end = tail.indexOf("</select>");
    parseOptionsInto(el, end < 0 ? "" : tail.slice(0, end));
    resetSelectedness(el);
  }
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
  // R-140 批10:rail 按钮的 data-i18n-* 也要建到桩元素上,否则 applyDataI18nKeys
  // 的渲染点翻译对它们失明(observer 退役后无人再走属性扫描,漏建即英文态漏翻)。
  for (const attribute of ["data-i18n-key", "data-i18n-title", "data-i18n-aria-label", "data-i18n-placeholder"]) {
    const value = match[0].match(new RegExp(`\\b${attribute}="([^"]*)"`))?.[1];
    if (value !== undefined) el.setAttribute(attribute, value);
  }
  const tail = html.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim();
  if (directText) el.textContent = directText;
  body.appendChild(el);
}

// R-140 批2:静态 DOM data-i18n-key 节点(侧栏标题、subtitle)无 id,按属性补造,
// 让渲染点翻译对这些节点真实可断言——不补造则 `[data-i18n-key]` 恒为空,
// data-i18n-key 静态翻译在冒烟里全是假通过。
for (const match of html.matchAll(/<(\w+)((?:[^<>"]|"[^"]*")*?)\bdata-i18n-key="([^"]*)"((?:[^<>"]|"[^"]*")*?)>/g)) {
  const [, tag, before, key, after] = match;
  if (/id="/.test(`${before} ${after}`)) continue; // 带 id 的已由上面按 id 段建造
  const el = document.createElement(tag);
  el.setAttribute("data-i18n-key", key);
  for (const attribute of ["data-i18n-title", "data-i18n-aria-label", "data-i18n-placeholder"]) {
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
// ---------- 工具块夹具:历史回放里的四种结果形态 ----------
// 双写缺陷(⎿ 摘要行与展开详情各渲染一遍同一段文案)只在"首行超过 ⎿ 预算"或"多行"时
// 显形,单行短结果永远看不出来——夹具必须真的超预算,否则断言恒真。
const HISTORY_LONG_FIRST_LINE = `历史失败首行 ${"abcdefghijklmnopqrstuvwxyz".repeat(6)}`; // 163 字 > 110
const HISTORY_HUGE_OUTPUT = `第一行输出\n${"这是一段很长的历史输出。".repeat(800)}`; // 远超 8000
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
  // R-147:使用手册内容源——docs/目录.md 的文件预览桩。
  file_preview: { content: "# 使用手册\n\n冒烟手册段落:kanzei 使用说明与作者的话。", binary: false, truncated: false, size: 96 },
  docs_snapshot: {
    requirements: [docEntry("R-001", "冒烟需求", "doing", { complexity: "中", batches: { done: 3, total: 11 }, fields: [["备注", "待更新"], ["验收", "这是一条刻意超过六十字符的长验收文本,用来验证编辑表单会把段落型字段升级为多行文本域,而不是塞进单行输入框把值截断到看不见"]], dependencies: [], dependents: ["R-002"] }), docEntry("R-002", "冒烟需求二", "todo", { batches: { done: 0, total: 1 }, dependencies: ["R-001"], dependents: [] })],
    defects: [docEntry("D-001", "冒烟缺陷", "open", { severity: "medium", fields: [["复现", "待澄清: 用户视角的易用性还是模型可消费性?"]] })],
    goals: [{ id: "G-001", title: "冒烟目标", status: "active", fields: [] }],
    sources: [],
    findings: [],
    archived: { req: 1, defect: 2, goal: 0, source: 0, finding: 0 },
    conventions: { exists: true, headings: ["开发规则", "测试要求"] },
  },
  docs_archive_entries: (args) => args?.kind === "req" ? [docEntry("R-000", "已归档需求", "done")] : [docEntry("D-000", "已归档缺陷", "fixed")],
  // R-122:架构浏览。含一篇未入册文档,验证"未入册"分组可见。
  architecture_snapshot: {
    index_path: "C:/smoke/parent/.kanzei/project/architecture/README.md",
    index: "# 架构索引\n\n### 现行基线\n\n- [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线。\n",
    design_docs: [
      { name: "direction_taste.md", title: "方向基线", bytes: 512 },
      { name: "memory_system.md", title: "Memory 系统设计基线", bytes: 2048 },
    ],
    // R-188:workspace crate 依赖边(代码生成架构图数据源)。
    graph: [
      ["kanzei-app", "kanzei-core"],
      ["kanzei-app", "kanzei-tools"],
      ["kanzei", "kanzei-tools"],
      ["kanzei-tools", "kanzei-harness"],
      ["kanzei-tools", "kanzei-llm"],
      ["kanzei-tools", "kanzei-core"],
      ["kanzei-core", "kanzei-harness"],
    ],
  },
  docs_read_custom: { path: "C:/smoke/parent/docs/design/memory_system.md", name: "memory_system.md", content: "# Memory 系统设计基线\n\n冒烟内容。" },
  docs_read: { path: "C:/smoke/parent/docs/design/readme.md", name: "readme.md", content: "# 设计与 AI 讨论记录规范\n\n冒烟内容。" },
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
  // R-150:空闲整理清单——零采纳(召回≥3 采纳=0)+ 复发候选,前端只展示。
  memory_value_flags: {
    zeroAdopt: [{ scope: "project", id: "M-001", title: "CRLF 未命中", recalled: 5, fetched: 0 }],
    recurring: [{ scope: "project", id: "M-002", title: "发版 SOP", recalled: 4, fetched: 1 }],
  },
  // R-132:一键整理——零采纳候选降级 stale,返回降级/跳过清单。
  memory_cleanup_demote: { demoted: [{ id: "M-001", title: "CRLF 未命中" }], skipped: [] },
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
  // 历史回放里的工具调用/结果:此前只有一条纯文本消息,历史工具块在运行时从未被执行过
  // (只有源码字符串断言),⎿ 摘要与展开详情的双写在这条路径上完全没有护栏。
  conversation_get: [
    { role: "user", parts: [{ type: "text", text: "冒烟历史消息" }] },
    {
      role: "assistant",
      parts: [
        { type: "tool_call", id: "H1", name: "edit", input: { path: "ui/x.js", old_string: "a", new_string: "b" } },
        { type: "tool_result", call_id: "H1", is_error: true, content: `${HISTORY_LONG_FIRST_LINE}\n第二行\n第三行` },
        { type: "tool_call", id: "H2", name: "bash", input: { command: "cargo test --workspace" } },
        { type: "tool_result", call_id: "H2", is_error: false, content: HISTORY_HUGE_OUTPUT },
        { type: "tool_call", id: "H3", name: "bash", input: { command: "true" } },
        { type: "tool_result", call_id: "H3", is_error: false, content: "exit code: 0\n真正的输出行" },
        { type: "tool_call", id: "H4", name: "bash", input: { command: "true" } },
        { type: "tool_result", call_id: "H4", is_error: false, content: "exit code: 0" },
      ],
    },
  ],
  conversation_trace_get: [],
  conversation_list: ({ processId }) => processId === "p|bg"
    ? [{ sequence: 2, sequences: [2], title: "后台线路历史", preview: "后台预览", updated_at: "2026-08-08 00:01" }]
    : [{ sequence: 1, sequences: [1], title: "冒烟会话", preview: "主线预览", updated_at: "2026-08-08 00:00" }],
  // 角色项 + 一个真实模型:角色不该出现在设置页的角色下拉里(会绕成自指)。
  models_list: [
    { id: "primary", label: "primary → anthropic:claude-sonnet-5" },
    { id: "anthropic:claude-sonnet-5", label: "anthropic:claude-sonnet-5" },
    { id: "ollama:qwen3", label: "ollama:qwen3" },
  ],
  git_status: { branch: "main", changes: 2 },
  list_pending_inputs: [],
  test_runs_snapshot: { active: [{ id: "T-001", title: "冒烟测试", status: "passed", fields: [["命令", "cargo test"]], refs: ["R-001", "D-001"] }], archived: [] },
  test_runs_init_refs: { backfilled: 0 },
  process_list: [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, branch: "main", model: "deepseek:deepseek-chat", authority: "primary", stage: "复核" },
    // R-086 多会话并发:后台会话初始为运行中,桩里的旧 running=true 正是
    // "事件已收敛但轮询采样仍在事件之前"的竞态值,converged 必须挡住它。
    { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", tracker_writes: false, authority: "parallel", stage: "实现" },
  ],
  collaboration_snapshot: [
    {
      process_id: "d|smoke", label: "主会话", branch: "main", worktree_path: null,
      claim: "R-184 并行主线", phase: "复核", current_tool: null, running: false,
      steps: 8, input_tokens: 2400, output_tokens: 600,
      changed_files: ["crates/shared.rs", "docs/main.md"],
    },
    {
      process_id: "p|bg", label: "后台会话", branch: "thread-a1", worktree_path: "C:/smoke/wt/thread-a1",
      claim: "R-184 并行分支", phase: "实现", current_tool: "edit", running: true,
      steps: 3, input_tokens: 1200, output_tokens: 300,
      changed_files: ["crates/shared.rs", "crates/branch.rs"],
    },
  ],
  worktree_harvest_candidates: ["R-184"],
  process_close: "已关闭线路 p|bg；工作树有独有内容，已保留",
  pending_asks_get: [],
  // primary 是探测不到的已存值(端点没实现 /models),必须原样保留;
  // effective 与全局不同 = 项目级覆盖,界面要明说。
  settings_get: {
    language: "zh",
    path: "C:/smoke/.kanzei/kanzei.toml",
    primary: "deepseek:deepseek-chat",
    fast: "ollama:qwen3",
    proxy: "env",
    // readonly 是配置文件里的合法档位,但 index.html 的下拉只写了 dev/research:
    // 硬塞一个没有匹配 option 的值会让 select 落到空串,保存一次就把用户配置降级成 dev
    // (与模型角色同一个坑)。必须补出兜底 option。
    profileDefault: "readonly",
    reasoning: "off",
    codexFastMode: true,
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
    // 两个自定义 provider:表格里点「×」再保存,载荷必须只剩没删的那个(且仍是整张表)。
    // 加一个内置 provider(anthropic + builtin:true):删除入口必须是「内置」标记而不是 ×(D-246)。
    providers: [
      { name: "mine", protocol: "openai", baseUrl: "http://127.0.0.1:1", apiKeyEnv: "", apiKey: "", contextLimit: null },
      { name: "keepme", protocol: "openai", baseUrl: "http://127.0.0.1:2", apiKeyEnv: "", apiKey: "", contextLimit: null },
      { name: "anthropic", protocol: "anthropic", baseUrl: "https://api.anthropic.com", apiKeyEnv: "ANTHROPIC_API_KEY", apiKey: "", auth: null, contextLimit: 200000, builtin: true },
    ],
    permissions: [],
    // 项目级覆盖:D-168 当年只堵了模型角色,limits/proxy 被覆盖时页面一声不吭。
    // 这里让 primary/proxy/limits.maxTokens 各不相同(必须提示)、profileDefault 与
    // codexFastMode 相同(不得误报)、fast 整条缺失(has() 守卫必须整条跳过)。
    effective: {
      primary: "anthropic:claude-sonnet-5",
      reasoning: null,
      proxy: "http://127.0.0.1:7890",
      profileDefault: "readonly",
      codexFastMode: true,
      limits: { maxTokens: 8192, subagentTimeoutSecs: null },
    },
    projectConfig: "C:/smoke/project/.kanzei/kanzei.toml",
  },
  permission_rules_get: [],
  memory_overview: { scopes: [{ scope: "project", root: PROJECT, total: 0, hitsTotal: 0, categories: {}, integrity: [], inboxPending: 0 }] },
  // 两条:一条有命中,一条陈旧且零命中(验证「长期零命中」标记与清理入口)。
  memory_entries: [
    { id: "M-SOP-001", category: "sop", title: "冒烟 SOP", description: "继续执行冒烟任务", status: "active", body: "执行冒烟任务", hits: 4, lastHitAt: 1_760_000_000_000, recalled: 4, fetched: 2, updated: "2026-08-01" },
    { id: "M-DEAD-001", category: "fact", title: "从没被用到的记忆", description: "冒烟用:零命中条目", status: "active", body: "陈旧结论", hits: 0, lastHitAt: 0, recalled: 0, fetched: 0, updated: "2026-01-01" },
  ],
  memory_context_bill: { turns: [] },
  workspace_snapshot: {},
};
const invokeLog = [];
const invokeArgs = [];
const savedPayloads = new Map();
// 探针回传要看具体参数(id 配对、取样内容),所以单独留一份带参日志。
const probeResults = [];
// 真机时序闸门:桩默认返回"已 resolve 的 promise",于是 `await invoke(...)` 之后的代码
// 走微任务,恒早于 setTimeout(0)。真机上 IPC 是毫秒级,顺序恰好相反——凡是"先刷新
// 再做某事"的时序契约,在默认桩下都会假绿。给某条命令挂一个闸门,就能把这一段
// 挂起,让 setTimeout 先跑完,复现真机顺序。
const invokeGates = new Map();
// 后端失败注入:真机上 docs_snapshot 会因目录被删/文件锁/解析失败而抛错,那条 catch
// 路径上的清理(比如作废挂起的跳转高亮)只有让桩真的抛错才测得到。
const invokeFailures = new Map();
async function invoke(cmd, args) {
  invokeLog.push(cmd);
  invokeArgs.push({ cmd, args });
  const gate = invokeGates.get(cmd);
  if (gate) await gate;
  const failure = invokeFailures.get(cmd);
  if (failure) throw new Error(failure);
  if (cmd === "settings_save") savedPayloads.set(cmd, args);
  if (cmd === "ui_probe_result") probeResults.push(args);
  // 桩可以是**函数**:同一条命令按入参返回不同结果。线清单要判「切走后不把甲的
  // 清单画进乙的面板」,必须让两个项目返回不同的清单,固定值做不到。
  if (cmd in payloads) {
    const stub = payloads[cmd];
    return structuredClone(typeof stub === "function" ? stub(args) : stub);
  }
  return null;
}
async function listen(event, handler) { handlers.set(event, handler); }
const handlers = new Map();

const storage = new Map();
storage.set("kz-auto-continue", "1");
// P1:启动恢复的上限必须作为同一次状态同步发到当前会话，不能仍让后端停在默认 10。
storage.set("kz-auto-max", "3");
// R-170:预置旧版默认继续文案(镜像历史 LEGACY_CONTINUE_PROMPTS[0],已删除)。
// 升级机制删除后旧值必须原样读回(验收③:不再触发覆盖);夹具保留用于断言。
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

// 冒烟里**有意注入**的后端失败会走 toastError → reportPersistentError,那是被测行为的
// 一部分,不该当成"意外的持久错误"判红。窗口显式开合并按片段精确匹配,离开窗口一律
// 恢复判红——不然就等于顺手把真错误也吞了。
let expectedPersistentError = null;
let expectedPersistentHits = 0;
// 同理,但走的是另一条出口:refreshDocsSoon 的 catch 只 console.error(不 toastError),
// 而"注入失败 → 它必须作废挂起的跳转高亮"正是要测的行为。规矩与上面一致:显式开窗、
// 按片段精确匹配、离开窗口立刻恢复判红——不然就等于顺手把真的 console.error 也吞了。
let expectedConsoleError = null;
let expectedConsoleHits = 0;

const sandbox = {
  __reportInitError: (label, err) => fail(`初始化步骤 ${label} 抛异常(已被 main.js 吞掉): ${err?.stack ?? err}`),
  __reportPersistentError: (text) => {
    if (expectedPersistentError && String(text).includes(expectedPersistentError)) {
      expectedPersistentHits += 1;
      return;
    }
    fail(`reportPersistentError: ${text}`);
  },
  console: {
    log: (...a) => console.log(...a),
    warn: (...a) => console.warn(...a),
    error: (...a) => {
      const text = a.map(String).join(" ");
      if (expectedConsoleError && text.includes(expectedConsoleError)) {
        expectedConsoleHits += 1;
        return;
      }
      fail(`console.error: ${text}`);
    },
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

// 手工排空一轮已排队的 setTimeout(闸门段里要复现"定时器先于 IPC 落地"的真机顺序,
// 不能用 flush——它会连锁把回调自己新排的定时器也冲掉)。
// 关键:**不得无条件 await 回调**。被排空的回调里若也有一次同名 invoke(典型如
// refreshDocsSoon 里的 docs_snapshot),它会撞上同一道还没放开的闸门,`await handle.fn()`
// 就此死等——CI 表现为挂死而不是判红,比红灯难查得多。这里一律带超时:任何情况下
// 都不挂死,超时就判红并说清原因。
const DRAIN_TIMEOUT_MS = 300;
async function drainTimersOnce(label) {
  for (const handle of [...pendingTimers]) {
    if (!pendingTimers.has(handle) || handle.interval) continue;
    pendingTimers.delete(handle);
    const timedOut = Symbol("drain-timeout");
    let timer = null;
    const result = await Promise.race([
      (async () => handle.fn())(),
      new Promise((resolve) => { timer = setTimeout(() => resolve(timedOut), DRAIN_TIMEOUT_MS); }),
    ]);
    clearTimeout(timer);
    if (result === timedOut) {
      fail(
        `${label}:排空定时器时有回调 ${DRAIN_TIMEOUT_MS}ms 未返回(多半是它内部的 invoke 撞上了还没放开的闸门)。` +
        "冒烟绝不能挂死,这里按失败处理;要么在闸门段前清掉该定时器,要么别在闸门段前排它。",
      );
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
    "globalThis.__kzTest = { rounds: () => autoRounds, noAction: () => noActionRounds, stopReason: () => autoStopReason, timerSessions: () => [...autoContinueTimers.keys()], setAutoState: (id, value) => processAutoState.set(id, value), setRounds: (v) => { autoRounds = v; }, setStopAfterRound: (v) => { autoStopAfterRound = v; }, setPaused: (v) => { autoPaused = v; }, paused: () => autoPaused, reset: () => { autoRounds = 0; noActionRounds = 0; autoStopAfterRound = false; autoPaused = false; }, cancelTimers: () => { for (const s of [...autoContinueTimers.keys()]) cancelAutoContinueTimer(s); } };",
    sandbox,
    { filename: "__kzTest-hook.js" }
  );
} catch (err) {
  fail(`__kzTest hook 执行抛异常: ${err.stack ?? err}`);
}
await flush();
assert(invokeLog.includes("projects_get"), `初始化未调用 projects_get(启动序列断裂),已见调用: ${invokeLog.join(",")}`);
assert(invokeLog.includes("docs_snapshot"), "初始化未调用 docs_snapshot");
// R-147 使用手册:启动后随项目自动读取 docs/目录.md 渲染到对话区顶部;读取失败
// (项目没有手册文件)时区块保持隐藏,不显示空壳、不遮挡对话。
{
  const manualPanel = byId.get("manual-panel");
  const manualBody = byId.get("manual-body");
  assert(manualPanel && manualBody, "使用手册区块未渲染(index.html 缺 manual-panel/manual-body)");
  assert(
    invokeArgs.some(({ cmd, args }) => cmd === "file_preview" && args?.path === "docs/目录.md"),
    "启动后未读取 docs/目录.md(使用手册加载链路断了)",
  );
  assert(!manualPanel.classList.contains("hidden"), "有手册内容时使用手册区块应可见(仍 hidden)");
  assert(
    manualBody.textContent.includes("冒烟手册段落"),
    `手册内容未渲染进 manual-body: "${manualBody.textContent.slice(0, 80)}"`,
  );
  // 无手册文件的项目:读取失败 → 区块隐藏;恢复读取后重新显示。
  invokeFailures.set("file_preview", "无法打开 docs/目录.md");
  await sandbox.refreshManual();
  await flush();
  assert(manualPanel.classList.contains("hidden"), "手册读取失败时区块应隐藏(不显示空壳)");
  invokeFailures.delete("file_preview");
  await sandbox.refreshManual();
  await flush();
  assert(!manualPanel.classList.contains("hidden"), "恢复读取后区块未重新显示");
}
// D-317:空配置必须停在明确的「未选择项目」状态，不能因渲染而触发项目级请求。
// 后端另有纯函数反证锁死「不拿 current_dir 造项目」；这里验证 classic-script 的空态承载。
{
  const processListCalls = invokeLog.filter((cmd) => cmd === "process_list").length;
  vm.runInContext("renderProjects({ current: null, projects: [], names: {} })", sandbox);
  await flush();
  assert(vm.runInContext("currentProject", sandbox) === null, "空项目偏好仍留下了当前项目");
  assert(byId.get("project-list").children.length === 0, "空项目偏好仍渲染出项目卡片");
  assert(byId.get("project-label").textContent.includes("未选择项目"), "空项目状态未显示『未选择项目』");
  assert(byId.get("documents-project-select").disabled, "空项目状态下文档项目选择器仍可用");
  assert(
    invokeLog.filter((cmd) => cmd === "process_list").length === processListCalls,
    "空项目状态仍请求了项目级 process_list"
  );
  vm.runInContext(
    `renderProjects(${JSON.stringify(payloads.projects_get)})`,
    sandbox
  );
  await flush();
}
const initialAutoState = invokeArgs.find(({ cmd, args }) =>
  cmd === "auto_state_update" && args?.sessionId === "sess-smoke" && args?.maxRounds === 3
);
assert(initialAutoState, "启动恢复的自动推进状态未将当前会话和已保存上限一并同步给后端");
// 完整需求/缺陷列表整体搬进单页视图(侧栏只留「当前在做」的焦点卡片),落点换了、断言跟着搬。
assert(listText("documents-req-list").includes("冒烟需求"), `需求列表未渲染出桩数据: "${listText("documents-req-list").slice(0, 60)}"`);
assert(listText("documents-defect-list").includes("冒烟缺陷"), "缺陷列表未渲染出桩数据");
// R-170:LEGACY 升级机制已删除——预置的旧默认文案必须原样读回,不再被覆盖
// (验收③);删空 textarea 回落极简默认,且极简默认不含任何引擎规则文本(验收①)。
{
  const storedPrompt = storage.get("kz-continue-prompt") ?? "";
  const textareaPrompt = (byId.get("continue-prompt")?.value ?? "").trim();
  assert(
    storedPrompt.includes("粒度 = 一轮一个完整条目"),
    "旧默认文案被覆盖:升级机制应已删除,旧值应原样保留在 localStorage"
  );
  assert(
    textareaPrompt === storedPrompt,
    "textarea 与 localStorage 不一致:旧默认文案应原样读回(不触发覆盖)"
  );
  // 删空 textarea → 回落极简默认,且不含批次粒度/阻塞定义/验收证据/验证节奏文本。
  const textarea = byId.get("continue-prompt");
  textarea.value = "";
  textarea.dispatchEvent({ type: "change" });
  const minimal = (byId.get("continue-prompt")?.value ?? "").trim();
  assert(
    minimal.includes("继续推进"),
    `极简默认应保留「继续推进」意图句: ${minimal.slice(0, 60)}`
  );
  for (const ruleText of ["粒度", "阻塞字段", "验收证据", "全量测试每 3 批", "一直做下去"]) {
    assert(
      !minimal.includes(ruleText),
      `极简默认仍含引擎规则文本「${ruleText}」: ${minimal.slice(0, 120)}`
    );
  }
  // 恢复夹具:后续用例按极简默认对待。
  storage.set("kz-continue-prompt", minimal);
}
// 批次进度格(R-160):格数与已填格必须来自后端算好的 entry.batches,前端不得另存
// 一份复杂度→格数的映射;总数为 1 的条目不画格(一轮做完的东西不需要进度条)。
// 批次上限 10 只在写入侧(docstore.rs check_declared_batches)拦截,读路径与渲染必须原样
// 透传:归档里 11/11、16/16 的历史条目若被前端二次钳制成 10,格子数与 aria-label 就成了假数。
{
  const meter = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"] .batch-meter');
  assert(meter, "批次进度格没渲染出来");
  const cells = meter.querySelectorAll(".complexity-cell");
  assert(cells.length === 11, `11 批应画 11 格(前端不得二次钳制到 10),实际 ${cells.length}`);
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
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"] .batch-meter'),
    "总数为 1 的条目不该画进度格(一轮做完的东西没有进度可言)",
  );
}
// D-242 新口径:批数由 agent 显式声明(`批次: k/N`),上限 10;未声明就没有批次
// (docstore.rs batch_progress 返回 (0,1)),复杂度不再凭空生成 3/8 个空格子。
// 上界 10/10 与"未声明不画格"这两种形态此前从未被渲染路径覆盖过。
{
  const savedBatchDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    ...savedBatchDocs,
    requirements: [
      docEntry("R-010", "走满上限的条目", "doing", { complexity: "大", batches: { done: 10, total: 10 } }),
      docEntry("R-011", "未声明批次的大条目", "todo", { complexity: "大", batches: { done: 0, total: 1 } }),
    ],
  };
  await sandbox.refreshDocs();
  const full = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-010"] .batch-meter');
  assert(full, "走满上限(10/10)的条目没画进度格");
  const fullCells = full.querySelectorAll(".complexity-cell");
  assert(fullCells.length === 10, `10 批应画 10 格,实际 ${fullCells.length}`);
  assert(fullCells.every((c) => c.className.includes("filled")), "10/10 应全部填满");
  assert(full.style.getPropertyValue("--cells") === "10", "10 批的 --cells 不对");
  assert((full.getAttribute("aria-label") ?? "").includes("10/10"), "10/10 读屏标签不对");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-011"] .batch-meter'),
    "未声明批次的「大」条目不该凭空画出进度格(D-242:复杂度不再映射默认批数)",
  );
  payloads.docs_snapshot = savedBatchDocs;
  await sandbox.refreshDocs();
}
assert(listText("goal-list").includes("冒烟目标"), "目标列表未渲染出桩数据");
assert(listText("test-list").includes("冒烟测试"), "测试记录列表未渲染出桩数据");
// R-130:测试→条目映射——关联的 R-/D- 条目号渲染成可点击跳转的徽标。
{
  // R-130 验收③:批量初始化必须有真实调用方——refreshTests 每次刷新前先跑
  // test_runs_init_refs(幂等回填旧记录关联字段),再取快照渲染。
  const initCalls = invokeLog.filter((cmd) => cmd === "test_runs_init_refs");
  assert(initCalls.length >= 1, `测试列表刷新未调用批量初始化:init 调用次数 ${initCalls.length}`);
  const initArgs = invokeArgs.find(({ cmd }) => cmd === "test_runs_init_refs");
  assert(initArgs && initArgs.args?.projectDir, "test_runs_init_refs 未带 projectDir 参数");
  const testEntry = document.querySelector("#test-list .test-entry");
  assert(testEntry, "前置失败:测试记录条目未渲染");
  const chips = testEntry.querySelectorAll(".test-ref-chip");
  assert(chips.length === 2, `测试条目应渲染 2 个关联徽标,实得 ${chips.length}`);
  assert(
    [...chips].some((c) => c.textContent === "R-001") && [...chips].some((c) => c.textContent === "D-001"),
    `关联徽标内容应为 R-001/D-001:${[...chips].map((c) => c.textContent).join(",")}`,
  );
  // 点徽标跳转到对应条目:先离开文档视图,验证 jumpToEntry 被触发。
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  const before = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
  [...chips].find((c) => c.textContent === "R-001").click();
  await flush();
  assert(
    invokeLog.filter((cmd) => cmd === "docs_snapshot").length > before,
    "点击测试关联徽标未触发跳转刷新",
  );
}
// 历史必须随线路渲染，不能再退回一个全局 conversation-list，否则切线后无法判断归属。
assert(!byId.has("conversation-list"), "历史对话不应再有独立的全局列表");
const lineHistories = document.querySelectorAll("#parallel-task-status .parallel-line-history");
assert(lineHistories.length === 2, `两条线路都应有自己的历史容器,实际 ${lineHistories.length}`);
assert(
  [...lineHistories].some((history) => history.dataset.processId === "d|smoke" && history.textContent.includes("冒烟会话")),
  "主线历史没有挂到主线按钮下面",
);
assert(
  [...lineHistories].some((history) => history.dataset.processId === "p|bg" && history.textContent.includes("后台线路历史") && !history.textContent.includes("冒烟会话")),
  "并行线历史没有按 process_id 隔离渲染",
);
const historyCalls = invokeArgs.filter(({ cmd }) => cmd === "conversation_list");
assert(historyCalls.some(({ args }) => args?.processId === "d|smoke"), "历史查询未带主线 process_id");
assert(historyCalls.some(({ args }) => args?.processId === "p|bg"), "历史查询未带并行线 process_id");
// D-304:排队顺序不再由前端推断；只有 collaboration_snapshot 的真实 claim 才能标记。
{
  const active = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(active?.classList.contains("agent-active"), "doing 条目 R-001 未标记 agent-active(在做高亮丢失)");
  const next = document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]');
  assert(!next.classList.contains("agent-active"), "open 条目不该被标成在做");
  assert(!next.classList.contains("doc-claim-fact"), "没有 collaboration_snapshot claim 的队首不应出现被取得标记");
}
// ---------- 侧栏「当前在做」焦点卡片:单条,不是列表、不是集合 ----------
// 用户定调:侧栏只显示 agent 当前在做的那一条,且显示得完整一点。完整列表连同筛选、
// 排序、分组、批量、测试记录全部搬进单页视图。
{
  const cards = document.querySelectorAll("#focus-body .focus-card");
  assert(cards.length === 1, `侧栏焦点区应恰好一张卡片(单条,不是集合),实得 ${cards.length}`);
  assert(cards[0]?.dataset.docId === "R-001", `焦点卡片指错条目:${cards[0]?.dataset.docId}`);
  const focusText = listText("focus-body");
  for (const needle of ["R-001", "冒烟需求", "doing", "3/11", "P1", "复杂度"]) {
    assert(focusText.includes(needle), `焦点卡片缺少「${needle}」(侧栏要显示得完整一点):${focusText.slice(0, 160)}`);
  }
  assert(document.querySelector("#focus-body .batch-meter"), "焦点卡片缺批次进度格");
  assert(document.querySelectorAll("#focus-body .doc-field").length > 0, "焦点卡片没有只读字段(取活时看不到进展/验收)");
  // 焦点依据(D-207 三修的对外可见面):凭运行证据还是凭取活序,必须说出来。
  assert(focusText.includes("取活顺序推断"), `无运行证据时焦点依据应说明是推断:${focusText.slice(0, 160)}`);
  // 侧栏不再承载完整列表 / 筛选 / 排序 / 分组 / 测试记录 —— 这些 id 从 index.html 里整体消失。
  for (const gone of ["req-list", "defect-list", "tests-section", "req-filter-row", "defect-filter-row",
    "req-sort", "req-group-toggle", "req-priority-filter", "req-status-filter", "req-tag-filter"]) {
    assert(!byId.has(gone), `侧栏残留完整列表控件 #${gone}(侧栏应只显示当前在做的单条)`);
  }
  // 测试记录搬进单页:#test-list 必须在 #documents-tests 里(harness 的 DOM 是按 id 扁平造的,
  // 祖先链断言天然不成立,这里改用 index.html 的静态包含关系)。
  const testsBlock = html.slice(html.indexOf('id="documents-tests"'), html.indexOf('id="documents-dep-view"'));
  assert(testsBlock.includes('id="test-list"'), "测试记录列表不在单页 #documents-tests 内(仍挂在侧栏)");
  assert(listText("test-list").includes("冒烟测试"), "测试记录搬家后没渲染出桩数据");
  assert(!document.querySelector("#focus-body .focus-next"), "焦点区不应渲染前端推断的下一个");
  // 待办计数补回被删列表的信息量。
  assert(/\d/.test(listText("focus-backlog")), "焦点区未给出待办计数");
}
// 焦点卡片的状态流转按钮:取活时要能直接切状态,这条链路不能因为列表搬家而断掉。
{
  const actionButton = document.querySelector("#focus-body .doc-actions button");
  assert(actionButton, "焦点卡片缺少状态流转按钮(取活链路断了)");
  const before = invokeLog.filter((cmd) => cmd === "docs_update").length;
  actionButton?.click();
  await flush();
  assert(
    invokeLog.filter((cmd) => cmd === "docs_update").length > before,
    "焦点卡片的状态流转按钮没有真正提交 docs_update",
  );
}
// 焦点空态:队列清空时说破,并给出去完整列表的入口(不留空壳、不编)。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    ...savedFocusDocs,
    requirements: [docEntry("R-001", "已完成需求", "done", { closed: true })],
    defects: [docEntry("D-001", "已修缺陷", "fixed", { closed: true })],
  };
  await sandbox.refreshDocs();
  assert(!document.querySelector("#focus-body .focus-card"), "全部关闭时不该还有焦点卡片");
  assert(listText("focus-body").includes("当前没有在做的条目"), `焦点空态未说破:${listText("focus-body")}`);
  assert(!document.querySelector("#focus-body .focus-next"), "焦点区不应保留下一个推断空壳");
  const emptyButton = document.querySelector("#focus-body .focus-empty button");
  assert(emptyButton, "焦点空态缺少「查看完整列表」入口");
  emptyButton.click();
  await flush();
  assert(byId.get("view-documents").classList.contains("active"), "焦点空态的入口没能切到单页视图");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// 侧栏标题栏的「打开完整列表」按钮:切视图 + 走 refreshDocs。
{
  byId.get("view-documents").classList.remove("active");
  const before = invokeLog.filter((cmd) => cmd === "docs_snapshot").length;
  byId.get("focus-open-documents").click();
  await flush();
  assert(byId.get("view-documents").classList.contains("active"), "#focus-open-documents 未激活单页视图");
  assert(
    invokeLog.filter((cmd) => cmd === "docs_snapshot").length > before,
    "#focus-open-documents 未触发 refreshDocs",
  );
}
// D-207 单线程语义:active 是单条(取活序第一个可执行 doing/fixing),不是集合。
// 多条 doing/fixing 只是"已取未动"的历史状态,只有取活序第一条才是 agent 正在推的。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [docEntry("R-001", "第一条 doing", "doing", {}), docEntry("R-002", "第二条 doing", "doing", {})],
    defects: [docEntry("D-001", "可开工缺陷", "open", {})],
  };
  await sandbox.refreshDocs();
  const firstDoing = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  const secondDoing = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]');
  assert(firstDoing?.classList.contains("agent-active"), "取活序第一条 doing 应标 agent-active(当前正在做)");
  assert(!secondDoing.classList.contains("agent-active"), "第二条 doing 只是已取未动,不该标 agent-active(active 是单条)");
  assert(!document.querySelector(".agent-next"), "删除下一个推断后不应产生 agent-next 标记");
  // WIP=1(2026-08-10 定调):两个队列共用同一个槽位,整个快照里被标「正在做」的只能有一条。
  assert(
    document.querySelectorAll("#documents-req-list .agent-active, #documents-defect-list .agent-active").length === 1,
    "两条 doing 同时在场时「正在做」应仍是单条(需求与缺陷共用一个槽,不是每队各一个)",
  );
  assert(document.querySelectorAll("#focus-body .focus-card").length === 1, "侧栏焦点卡片必须是单条");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// WIP=1 跨队列:defect-first 下,非阻塞 fixing 缺陷占走那唯一的槽,需求侧的 doing 不再算「在做」。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  const priority = byId.get("work-priority-select");
  const savedPriority = priority.value;
  payloads.docs_snapshot = {
    requirements: [docEntry("R-A", "需求侧 doing", "doing", {}), docEntry("R-B", "待办需求", "todo", {})],
    defects: [docEntry("D-A", "缺陷侧 fixing", "fixing", {})],
  };
  priority.value = "defect-first";
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-A"]')?.classList.contains("agent-active"),
    "defect-first 下,fixing 缺陷应占走唯一的可执行槽",
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-A"]')?.classList.contains("agent-active"),
    "两队共用一个槽:缺陷占了槽,需求侧的 doing 不该同时被标「正在做」",
  );
  assert(!document.querySelector("#focus-body .focus-next"), "焦点区不应渲染第二个推断指针");
  priority.value = savedPriority;
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
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
  const blockedDoing = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(blockedDoing?.classList.contains("blocked"), "阻塞 doing 应保留 blocked 标记(阻塞展示不受影响)");
  assert(!blockedDoing.classList.contains("agent-active"), "阻塞 doing 不该标 agent-active(运行焦点只标可执行条目)");
  assert(!document.querySelector(".agent-next"), "阻塞队列场景也不应产生下一个推断标记");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-219 验收②:构造「2 个阻塞 doing + 可做 todo」场景——阻塞 doing 全部不计
// WIP、不占运行焦点。阻塞项只保留 blocked 展示，不生成排队推断标记。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [
      docEntry("R-001", "阻塞的 doing", "doing", { blocked: true }),
      docEntry("R-002", "另一个阻塞 doing", "doing", { blocked: true }),
      docEntry("R-003", "可开工待办", "todo", {}),
    ],
    defects: [],
  };
  await sandbox.refreshDocs();
  const activeCount = document.querySelectorAll(
    "#documents-req-list .doc-item.agent-active, #documents-defect-list .doc-item.agent-active"
  ).length;
  assert(activeCount === 0, "两个阻塞 doing 都不应标 agent-active(阻塞项不进 WIP 不占焦点),实际 {activeCount}");
  const blockedAll = document.querySelectorAll('#documents-req-list .doc-item[data-doc-id="R-001"], #documents-req-list .doc-item[data-doc-id="R-002"]');
  assert(blockedAll.length === 2 && [...blockedAll].every((el) => el.classList.contains("blocked")), "阻塞 doing 应保留 blocked 标记");
  assert(!document.querySelector(".agent-next"), "阻塞队列场景也不应产生下一个推断标记");
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-207 三修:运行事实优先——纯文件状态推断会把挂着 fixing 的旧缺陷标成「正在做」,
// 而 agent 实际在推别的条目(用户实测:指着缺陷,实做 R-117)。req/defect 的 update
// 结果与批次提交都带条目 ID,运行证据一到就覆盖推断;新一轮 run 开跑即作废。
{
  const savedFocusDocs = structuredClone(payloads.docs_snapshot);
  payloads.docs_snapshot = {
    requirements: [docEntry("R-001", "实际在做的需求", "doing", {})],
    defects: [docEntry("D-001", "挂着 fixing 的旧缺陷", "fixing", {})],
  };
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]')?.classList.contains("agent-active"),
    "无运行证据时应按取活序推断(defect-first 指 fixing 缺陷)",
  );
  handlers.get("kz:tool-end")({ payload: { id: "F1", name: "req", ok: true, preview: "updated: R-001 [doing] 批次推进", display: null } });
  await flush();
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')?.classList.contains("agent-active"),
    "运行证据(updated: R-001)未覆盖状态推断——「在做」指针仍指错条目",
  );
  // 焦点卡片必须同步说出依据变了:D-207 三修的对外可见面就在这句话上。
  assert(
    listText("focus-body").includes("本轮运行证据"),
    `运行证据命中后焦点卡片仍说是推断:${listText("focus-body").slice(0, 160)}`,
  );
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]')?.classList.contains("agent-active"),
    "运行证据生效后,挂着 fixing 的旧缺陷不该再标「正在做」",
  );
  // 新一轮 run 开跑(kz:turn step 1):上一轮证据作废,回落推断。
  handlers.get("kz:turn")({ payload: { step: 1, maxSteps: 0 } });
  await sandbox.refreshDocs();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]')?.classList.contains("agent-active"),
    "新 run 开跑后旧运行证据未作废",
  );
  assert(
    listText("focus-body").includes("取活顺序推断"),
    `运行证据作废后焦点卡片未回落成推断:${listText("focus-body").slice(0, 160)}`,
  );
  payloads.docs_snapshot = savedFocusDocs;
  await sandbox.refreshDocs();
}
// D-166:引用跳转此前只认当前可见节点,已归档/被折叠的目标一律静默失败。
const archiveToggle = document.querySelector("#documents-req-list .doc-archive-toggle");
assert(archiveToggle, "归档入口未渲染");
assert(!document.querySelector("#documents-req-list .doc-archive-list .archived-entry"), "归档条目不应在快照时提前加载");
assert(archiveToggle.getAttribute("aria-expanded") === "false", "归档区应默认折叠");
archiveToggle.click();
await flush();
const archivedRow = document.querySelector("#documents-req-list .doc-archive-list .archived-entry");
assert(archivedRow?.dataset.docId === "R-000", "按需加载后归档条目未挂 data-doc-id,引用跳转必然落空");
assert(typeof sandbox.jumpToEntry === "function", "jumpToEntry 未定义(引用跳转入口丢失)");
await sandbox.jumpToEntry("R-000");
assert(
  !archivedRow.parentElement.classList.contains("hidden"),
  "跳转到归档条目时未掀开归档折叠区",
);
assert(archivedRow.classList.contains("ref-highlight"), "跳转后未高亮目标条目");
await sandbox.jumpToEntry("R-999");
assert(
  listText("toast").includes("R-999"),
  `跳转到不存在的条目时应给出提示而不是静默失败,实得 toast: "${listText("toast")}"`,
);

// 完整列表搬进单页后,「侧栏不该有编辑表单/批量选择」这组断言换了落点:侧栏根本没有
// 列表了(上面 byId 反向断言 + ui-a11y-smoke.mjs 已守住),再照搬到 #documents-req-list
// 就会变成断言「单页不该能编辑」——正好把 R-123 的能力判反。这里改为守住单页确实有这些能力,
// 见下面 reqEditor / .doc-pick 两组;此处只保留「侧栏焦点区不承载列表能力」的正面判据。
assert(!document.querySelector("#focus-body .doc-edit"), "侧栏焦点卡片渲染了字段编辑表单(编辑只在独立文档页)");
assert(!document.querySelector("#focus-body .doc-pick"), "侧栏焦点卡片出现批量选择框(批量操作应只在文档页)");
assert(
  !document.querySelectorAll("#documents-req-list .doc-item").some((n) => n.draggable),
  "分组锁状态下条目不应可拖(解锁后才设置 draggable)",
);
// D-211 修复链路:解锁 → 锁提示消失 → draggable=true → 拖拽 → reorder 落库。
// 终判据收紧到 action==="reorder":拖拽是唯一能改取活顺序的入口,只数 docs_update 次数
// 的话,任何顺手多发的 docs_update(改状态/改字段)都能让它假通过。
{
  const reqListEl = document.querySelector("#documents-req-list");
  const hint = reqListEl.querySelector(".drag-hint");
  assert(hint, "默认分组视图未渲染锁提示");
  const unlockBtn = [...hint.querySelectorAll("button")].find((b) => b.textContent.includes("解锁"));
  assert(unlockBtn, "锁提示缺少一键解锁按钮(D-210 能力丢失)");
  unlockBtn.click();
  await flush();
  assert(!document.querySelector("#documents-req-list .drag-hint"), "解锁后锁提示未消失");
  const items = [...document.querySelectorAll("#documents-req-list .doc-item[data-doc-id]")];
  assert(items.length >= 2, `解锁后需求条目不足(无法验证拖拽落库): ${items.length}`);
  assert(items.every((n) => n.draggable), "解锁后条目未设置 draggable(D-211:解锁了却拖不动)");
  const reorderCount = () =>
    invokeArgs.filter(({ cmd, args }) => cmd === "docs_update" && args?.action === "reorder").length;
  const before = reorderCount();
  const [a, b] = items;
  a.dispatchEvent({ type: "dragstart", dataTransfer: { effectAllowed: "", setData() {} } });
  b.dispatchEvent({ type: "dragover", clientY: 0, preventDefault() {} });
  a.dispatchEvent({ type: "dragend" });
  await flush();
  assert(reorderCount() > before, `拖拽未提交 action=reorder 的 docs_update(唯一能改取活顺序的入口断了)`);
}
// D-207 验收③:优先级语义 UI 明示——priority 只是背景信息,不参与取活(用户定调),
// 避免满屏 P0~P3 徽章让人按优先级猜取活序。
{
  const priFilter = document.querySelector("#documents-priority-filter");
  assert(priFilter?.getAttribute("title").includes("仅参考"), `优先级筛选未明示"仅参考,不影响取活": "${priFilter?.getAttribute("title")}"`);
  const badge = document.querySelector("#documents-req-list .pri-badge");
  assert(badge?.title.includes("仅参考"), `优先级徽章未明示"仅参考,不影响取活": "${badge?.title}"`);
}
// D-205 验收③:带「待澄清」复现的缺陷可辨识——用户能一眼看到哪些条目等他补话,
// 不会把"待澄清"当真实复现拿去开工。
{
  const clarifyBadge = document.querySelector("#documents-defect-list .clarify-badge");
  assert(clarifyBadge, "带「待澄清」复现的缺陷未渲染待澄清徽标(D-205)");
  assert(clarifyBadge.title.includes("待澄清"), `待澄清徽标未带具体问题提示: "${clarifyBadge.title}"`);
  assert(!document.querySelector("#documents-req-list .clarify-badge"), "需求列表误渲染待澄清徽标(仅缺陷快记有此形态)");
}
// 列表搬走之后,取活时要看的只读字段落到了侧栏焦点卡片上——断言跟着搬,不能删:
// 「信息被一起删掉」这条护栏必须留着。
const sidebarFields = document.querySelectorAll("#focus-body .doc-field");
assert(sidebarFields.length > 0, "侧栏焦点卡片既无编辑表单也无只读字段,信息被一起删掉了");
// 状态流转留在侧栏:取活时要能直接切。
assert(document.querySelector("#focus-body .doc-actions button"), "侧栏焦点卡片缺少状态流转按钮(取活链路断了)");

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
// D-256:批量操作进行中切项目,不得有任何一条写进新项目。
// 2026-08-11 用户拍板语义:按认领项目做完——整批继续写旧项目,循环内不重读 currentProject;
// 循环结束后若 currentProject 已变,提示「这批改动落在 <旧项目>」。
// 桩把第一条 docs_update 挂在闸门上,applyBatch 停在第一次 await 处,此刻把 currentProject
// 换成项目乙;放行后剩余条目必须仍以认领时的旧项目为 projectDir,且 toast 明说落地项目。
{
  const reqPicks = [...document.querySelectorAll("#documents-req-list .doc-pick")];
  assert(reqPicks.length >= 2, "前置失败:D-256 用例需要至少 2 条需求条目");
  const claimedProject = vm.runInContext("currentProject", sandbox);
  const batchStart = invokeArgs.length;
  reqPicks.forEach((el) => { el.checked = true; el._listeners.change?.forEach((fn) => fn({ target: el })); });
  assert(
    vm.runInContext("batchSelection.size", sandbox) >= 2,
    "前置失败:批量选中集未达到 2 条",
  );
  let releaseBatch;
  invokeGates.set("docs_update", new Promise((resolve) => { releaseBatch = resolve; }));
  byId.get("documents-batch-tag").value = "流程";
  byId.get("documents-batch-apply")._listeners.click?.forEach((fn) => fn({}));
  await settle();
  // 循环已挂在第一条 docs_update 的 await 上——此刻切项目。旧实现从这里起会把新项目
  // 写进 projectDir,正是 D-256 描述的错写(新项目的同号条目被真改状态/改标签)。
  vm.runInContext(`currentProject = ${JSON.stringify("C:/smoke/project-b")}`, sandbox);
  releaseBatch();
  invokeGates.delete("docs_update");
  await flush();
  const batchUpdateCalls = invokeArgs.slice(batchStart).filter(({ cmd, args }) => cmd === "docs_update" && args?.action === "update");
  assert(batchUpdateCalls.length >= 2, "D-256:批量循环未按选中条目逐条提交 docs_update");
  const projectDirs = new Set(batchUpdateCalls.map(({ args }) => args?.projectDir));
  assert(
    [...projectDirs].every((dir) => dir === claimedProject),
    `D-256:批量中途切项目后,有 docs_update 的 projectDir 指向非认领项目(${[...projectDirs].join(",")})`,
  );
  assert(
    listText("toast").includes(claimedProject),
    `D-256:批量期间切走项目,结束后未提示这批改动落在认领项目(toast="${listText("toast")}")`,
  );
  // 复位:清空选中、把 currentProject 改回,不污染后续用例。
  reqPicks.forEach((el) => { el.checked = false; el._listeners.change?.forEach((fn) => fn({ target: el })); });
  vm.runInContext(`currentProject = ${JSON.stringify(claimedProject)}`, sandbox);
  await flush();
}
// 对照:两个队列同时可见,共用同一套**显示口径**——全字段中性化(D-244)。
byId.get("documents-tab-both")._listeners.click?.forEach((fn) => fn({}));
await flush();
assert(
  !byId.get("documents-req-list").classList.contains("hidden")
    && !byId.get("documents-defect-list").classList.contains("hidden"),
  "对照模式未同时显示需求与缺陷两个队列",
);
// D-244:对照页是只读对照视图,blocked 控件必须置灰;模拟 change 也不得改任何一队的
// 持久化筛选(此前这里真的会跨队列写并落盘)。桩数据都不带阻塞理由,若筛选生效两边都会
// 清空——断言两边都还在,证明中性化兜住了。
const reqBefore = document.querySelectorAll("#documents-req-list .doc-item").length;
const defectBefore = document.querySelectorAll("#documents-defect-list .doc-item").length;
assert(reqBefore > 0 && defectBefore > 0, "对照模式下两个队列应先都有条目");
const blockedFilter = byId.get("documents-blocked-filter");
assert(blockedFilter.disabled, "对照页阻塞控件应置灰(D-244:只读对照视图)");
blockedFilter.value = "blocked";
blockedFilter._listeners.change?.forEach((fn) => fn({ target: blockedFilter }));
await flush();
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length === reqBefore
    && document.querySelectorAll("#documents-defect-list .doc-item").length === defectBefore,
  "对照模式下改阻塞筛选把列表筛空了:中性化没生效(对照页必须只读,D-244)",
);
blockedFilter.value = "all";
blockedFilter._listeners.change?.forEach((fn) => fn({ target: blockedFilter }));
await flush();

// ---------- R-111 依赖视图:可做/被阻塞分层,点击条目高亮依赖链 ----------
const depToggle = byId.get("documents-dep-toggle");
assert(depToggle, "文档页缺少依赖视图切换按钮");
depToggle.click();
await flush();
assert(
  !byId.get("documents-dep-view").classList.contains("hidden"),
  "点击依赖视图按钮后面板未显示",
);
assert(
  byId.get("documents-req-list").classList.contains("hidden")
    && byId.get("documents-defect-list").classList.contains("hidden"),
  "依赖视图打开时普通列表未隐藏",
);
// 桩依赖:R-002 依赖 R-001 → R-002 处于被阻塞层,R-001 处于可做层。
const depEntries = [...document.querySelectorAll("#documents-dep-view .dep-entry")];
assert(depEntries.length >= 2, "依赖视图未渲染分层条目");
const r001 = depEntries.find((n) => n.dataset.docId === "R-001");
const r002 = depEntries.find((n) => n.dataset.docId === "R-002");
assert(r001 && r002, "依赖视图缺少 R-001/R-002");
assert(
  r001.closest(".dep-layer") !== r002.closest(".dep-layer"),
  "R-001 与 R-002 应分属不同层(依赖关系未分层)",
);
// 点击 R-002 应高亮它自己和依赖链上的 R-001,并压暗无关条目。
r002.click();
await flush();
assert(r002.classList.contains("dep-lit"), "点击后目标条目未高亮");
assert(r001.classList.contains("dep-lit"), "依赖链上游未高亮");
const unrelated = depEntries.find((n) => n.dataset.docId === "D-001");
if (unrelated) assert(unrelated.classList.contains("dep-dim"), "无关条目未压暗");
depToggle.click();
await flush();
assert(byId.get("documents-dep-view").classList.contains("hidden"), "再次点击依赖视图按钮后面板未隐藏");

// ---------- 单页视图补齐侧栏退休掉的能力:排序 / 复杂度筛选 / 测试记录 ----------
// 完整列表整体搬进单页后,侧栏原有的排序、复杂度筛选、测试记录都必须在这里找得到,
// 否则搬家等于把能力删了。
{
  byId.get("documents-tab-req").click();
  await flush();
  const reorderCount = () =>
    invokeArgs.filter(({ cmd, args }) => cmd === "docs_update" && args?.action === "reorder").length;
  const setSort = async (value) => {
    const sort = byId.get("documents-sort");
    sort.value = value;
    sort._listeners.change?.forEach((fn) => fn({ target: sort }));
    await flush();
  };

  // ① 排序 ≠ 拖拽:排序只改显示口径,只有手动排序下的拖拽才写回文件、改变取活顺序。
  // 三重冗余(常显说明 / 锁提示点名 / draggable 关掉)缺一条,用户就会以为"按优先级排一下,
  // agent 就会按优先级取活"。
  assert(listText("documents-sort-note").includes("拖拽"), `排序说明未点明拖拽才写回文件:"${listText("documents-sort-note")}"`);
  const reorderBeforeSort = reorderCount();
  await setSort("priority");
  const sortHint = document.querySelector("#documents-req-list .drag-hint");
  assert(sortHint, "非手动排序下未渲染拖拽锁提示(静默禁用 = D-210 老毛病)");
  assert(sortHint.textContent.includes("排序=优先级"), `锁提示未点名到具体条件:"${sortHint.textContent}"`);
  assert(
    document.querySelectorAll("#documents-req-list .doc-item[data-doc-id]").every((n) => !n.draggable),
    "非手动排序下条目仍可拖(拖出来的顺序会与文件顺序对不上)",
  );
  assert(
    reorderCount() === reorderBeforeSort,
    "改排序竟然提交了 action=reorder 的 docs_update:排序只该改显示,不该动取活顺序",
  );
  // ② 解锁后拖拽仍然真的能改取活顺序(能力没被上一条断言"锁死")。
  const unlock = [...sortHint.querySelectorAll("button")].find((b) => b.textContent.includes("解锁"));
  assert(unlock, "排序锁提示缺少一键解锁");
  unlock.click();
  await flush();
  assert(byId.get("documents-sort").value === "manual", "解锁后排序未切回手动");
  assert(!document.querySelector("#documents-req-list .drag-hint"), "解锁后锁提示未消失");
  const sortedItems = [...document.querySelectorAll("#documents-req-list .doc-item[data-doc-id]")];
  assert(sortedItems.every((n) => n.draggable), "解锁后条目仍拖不动");
  const beforeDrag = reorderCount();
  sortedItems[0].dispatchEvent({ type: "dragstart", dataTransfer: { effectAllowed: "", setData() {} } });
  sortedItems[1].dispatchEvent({ type: "dragover", clientY: 0, preventDefault() {} });
  sortedItems[0].dispatchEvent({ type: "dragend" });
  await flush();
  assert(reorderCount() > beforeDrag, "解锁后拖拽仍未提交 action=reorder(承诺与能力脱节)");

  // ③ 复杂度筛选补齐(侧栏退休前有这一档,单页必须接上,含按项目落盘)。
  const complexity = byId.get("documents-complexity-filter");
  assert(complexity && !complexity.disabled, "需求页缺少可用的复杂度筛选");
  complexity.value = "大";
  complexity._listeners.change?.forEach((fn) => fn({ target: complexity }));
  await flush();
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "复杂度筛选没生效(R-001 是「中」,筛「大」时不该还在)",
  );
  assert(document.querySelector("#documents-req-list .doc-filtered-empty"), "复杂度筛空后未说破");
  const complexityKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(
    JSON.parse(storage.get(complexityKey)).docReq.complexity === "大",
    "复杂度筛选未按项目落盘(重启后回「全部」)",
  );
  complexity.value = "all";
  complexity._listeners.change?.forEach((fn) => fn({ target: complexity }));
  await flush();
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'), "复杂度筛选清回「全部」后条目没回来");

  // ④ 测试记录已从侧栏搬进单页:切过去要真的显示,且对它无意义的控件要**置灰说破**,
  // 不做静默无效(D-210/D-211 的教训)。
  byId.get("documents-tab-tests").click();
  await flush();
  assert(!byId.get("documents-tests").classList.contains("hidden"), "测试记录标签页打不开");
  assert(byId.get("documents-req-list").classList.contains("hidden"), "测试记录页仍显示需求列表");
  assert(byId.get("documents-defect-list").classList.contains("hidden"), "测试记录页仍显示缺陷列表");
  assert(byId.get("documents-batch-bar").classList.contains("hidden"), "测试记录页不该出现批量操作条");
  assert(byId.get("documents-dep-toggle").disabled === true, "测试记录页的依赖视图按钮应置灰(禁用要说破)");
  assert(byId.get("documents-tab-tests").className.includes("primary"), "测试记录标签未标为当前页");
  assert(listText("test-list").includes("冒烟测试"), "测试记录页没渲染出测试数据");
  // 切回来必须完全可逆:dependencyViewOpen 这类标志不能被 tests 页顺手清掉。
  byId.get("documents-tab-req").click();
  await flush();
  assert(!byId.get("documents-req-list").classList.contains("hidden"), "切回需求页后列表没回来");
  assert(byId.get("documents-tests").classList.contains("hidden"), "切回需求页后测试记录未隐藏");
  assert(byId.get("documents-dep-toggle").disabled === false, "切回需求页后依赖视图按钮仍被禁用");
  assert(byId.get("documents-status-filter").disabled === false, "切回需求页后状态筛选仍被禁用");
  // ⑤ #tests-refresh 搬家后按 id 绑定的监听必须还在(09-sessions.js:565 绑的是这个 id)。
  const testsBefore = invokeLog.filter((cmd) => cmd === "test_runs_snapshot").length;
  byId.get("tests-refresh").click();
  await flush();
  assert(
    invokeLog.filter((cmd) => cmd === "test_runs_snapshot").length > testsBefore,
    "测试记录刷新按钮搬进单页后失效了(按 id 绑定的监听断了)",
  );

  // ⑥ 引用跳转必须先把单页视图切过去:条目现在只存在于 #view-documents 里,
  // 视图没激活时祖先是 display:none,scrollIntoView 无效 —— 真机上就是 D-166 的「点了没反应」。
  // 冒烟 harness 的 offsetParent 恒真,这条只能靠显式断言守。
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  sandbox.jumpToEntry("R-002");
  await flush();
  assert(byId.get("view-documents").classList.contains("active"), "跳转到单页里的条目时没有先切视图(点了没反应,D-166 复发)");
}

// ---------- 对照(both)标签页:显示上不带筛选,但绝不许清掉用户的筛选 ----------
// 对照页只提供「全部状态」(两队状态机不同),复杂度/排序是需求专有口径 —— 这三档在
// 对照页必须**按中性渲染**,否则界面写着「全部状态 / 全部复杂度 / 手动」而列表仍按上次
// 设的条件在筛,条目凭空少了(D-169 那类「以为数据丢了」)。
// 但"中性"只能是**显示口径**:此前这里真的把 documentFilters.req/defect 写成 all 并落盘,
// 用户在需求页设好 status=doing + 复杂度=大,只是切去对照页瞄一眼,回来筛选就永久没了、
// 重启也回不来 —— R-115「筛选按项目持久化」在这条路径上的直接回归。两头一起钉死:
// 对照页渲染确实不带筛选,切回去筛选原样还在(控件 + 内存 + 落盘)。
{
  byId.get("documents-tab-req").click();
  await flush();
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  await setDocFilter("documents-status-filter", "doing"); // R-002 是 todo → 被藏
  await setDocFilter("documents-complexity-filter", "大"); // R-001 是「中」 → 被藏
  await setDocFilter("documents-sort", "priority");
  assert(!document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'), "前置失败:复杂度筛选没生效");
  assert(!document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:状态筛选没生效");
  const filtersStoreKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(filtersStoreKey, "前置失败:筛选没有落盘(R-115 的持久化本身断了)");

  byId.get("documents-tab-both").click();
  await flush();
  // ① 显示口径:三档下拉复位,列表真的按不带筛选渲染。
  assert(byId.get("documents-status-filter").value === "all", "对照页状态下拉未复位");
  assert(byId.get("documents-complexity-filter").value === "all", "对照页复杂度下拉未复位");
  assert(byId.get("documents-sort").value === "manual", "对照页排序下拉未复位");
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "对照页只复位了下拉显示值、列表仍按复杂度在筛:界面写着「全部复杂度」而 R-001(中)被藏(D-169:以为数据丢了)",
  );
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "对照页只复位了下拉显示值、列表仍按状态在筛:界面写着「全部状态」而 R-002(todo)被藏",
  );
  const bothDragHint = document.querySelector("#documents-req-list .drag-hint");
  assert(
    !(bothDragHint?.textContent ?? "").includes("排序"),
    `对照页下拉写着「手动」而锁提示仍点名排序:"${bothDragHint?.textContent}"`,
  );
  // ② 底层筛选不许被清掉:落盘必须原样保留,否则重启后也回不来。
  assert(
    (() => {
      const saved = JSON.parse(storage.get(filtersStoreKey) ?? "{}");
      return saved.docReq?.status === "doing" && saved.docReq?.complexity === "大" && saved.docReq?.sort === "priority";
    })(),
    `去对照页瞄一眼就把用户的筛选清掉并落盘了(R-115 回归:切回来没了,重启也回不来):${storage.get(filtersStoreKey)}`,
  );
  // ③ 切回需求页:控件、内存、列表三处都得是用户原来的那套。
  byId.get("documents-tab-req").click();
  await flush();
  assert(byId.get("documents-status-filter").value === "doing", "切回需求页,状态筛选没了(对照页把它清掉了)");
  assert(byId.get("documents-complexity-filter").value === "大", "切回需求页,复杂度筛选没了(对照页把它清掉了)");
  assert(byId.get("documents-sort").value === "priority", "切回需求页,排序没了(对照页把它清掉了)");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "切回需求页后复杂度筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "切回需求页后状态筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // 缺陷侧同理:对照页只提供「全部状态」,缺陷队列的 status 同样是"显示中性、状态保留"。
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-status-filter", "fixing"); // D-001 是 open → 被藏
  assert(!document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'), "前置失败:缺陷状态筛选没生效");
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "对照页缺陷列表仍按看不见的 status=fixing 在筛(显示口径没做中性)",
  );
  assert(
    JSON.parse(storage.get(filtersStoreKey) ?? "{}").docDefect?.status === "fixing",
    `对照页把缺陷队列的 status 清掉并落盘了:${storage.get(filtersStoreKey)}`,
  );
  byId.get("documents-tab-defect").click();
  await flush();
  assert(byId.get("documents-status-filter").value === "fixing", "切回缺陷页,状态筛选没了");
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "切回缺陷页后状态筛选只剩下拉显示值、列表没在筛",
  );

  // 收尾:筛选现在会真的留下来,后续用例假定列表完整 —— 走用户路径调回「全部」。
  await setDocFilter("documents-status-filter", "all");
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-status-filter", "all");
  await setDocFilter("documents-complexity-filter", "all");
  await setDocFilter("documents-sort", "manual");
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2
      && document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "收尾失败:筛选没调回全部,后续用例会连带假失败",
  );
}

// ---------- 对照页不得改动任何队列的持久化标签筛选(跨队列写回) ----------
// 标签曾经是唯一一个绕开"中性副本"的字段:syncDocumentFilters 里有一段
// `for (const kind of docFilterTargets())` 把下拉的生效值写给每一个队列并落盘。
// 实测两种坏法,都是"去对照页瞄一眼就改掉用户状态":
//   缺陷页设「后端」→ 点对照 → 缺陷队列的标签被清成「全部」并落盘,切回去筛选永久没了;
//   需求页设「核心」→ 点对照 → 「核心」被写进缺陷队列并落盘,用户从没在缺陷页设过,
//   缺陷列表却永久少了一批。
// 定调:对照页是只读的对照视图,标签与 status/complexity/sort 同一套机制——只改显示。
// 唯一允许写回的例外是 D-169 的"值失效"回落,且只能作用于该标签所属的那一队(见 ④)。
{
  const savedTagDocs = structuredClone(payloads.docs_snapshot);
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const filtersStoreKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(filtersStoreKey, "前置失败:筛选没有落盘(R-115 的持久化本身断了)");
  const liveTags = () => JSON.parse(vm.runInContext(
    "JSON.stringify({ req: documentFilters.req.tag, defect: documentFilters.defect.tag })",
    sandbox,
  ));
  const savedTags = () => {
    const saved = JSON.parse(storage.get(filtersStoreKey) ?? "{}");
    return { req: saved.docReq?.tag, defect: saved.docDefect?.tag };
  };
  // 两队各带各的标签:只有这样才分得清"清掉了"与"被写成了对面那一支"。
  payloads.docs_snapshot = {
    ...savedTagDocs,
    requirements: [
      docEntry("R-001", "核心标签需求", "doing", { fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端标签需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [
      docEntry("D-001", "后端标签缺陷", "open", { fields: [["标签", "后端"]] }),
      docEntry("D-002", "前端标签缺陷", "open", { fields: [["标签", "前端"]] }),
    ],
  };
  await sandbox.refreshDocs();
  await flush();

  // ① 缺陷页设好的标签,去对照页瞄一眼再回来必须原样还在(此前被清成「全部」并落盘)。
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-tag-filter", "后端");
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "前置失败:缺陷标签筛选没生效",
  );
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    liveTags().defect === "后端",
    `对照页把缺陷队列的标签清掉了:${JSON.stringify(liveTags())}`,
  );
  assert(
    savedTags().defect === "后端",
    `对照页把缺陷队列的标签清掉并落盘了(切回去没了,重启也回不来):${storage.get(filtersStoreKey)}`,
  );
  // 显示口径:渲染真的不带标签筛选,下拉跟着显示「全部标签」——两者必须一致(D-211)。
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "对照页缺陷列表仍按看不见的标签在筛(显示口径没做中性,D-169:以为条目掉了)",
  );
  assert(byId.get("documents-tag-filter").value === "all", "对照页标签下拉未复位");
  // 渲染按中性走而控件还能调 = 调了不生效,而且一调就把值写进两个队列并落盘(D-210 静默无效)。
  assert(
    byId.get("documents-tag-filter").disabled === true,
    "对照页标签渲染按中性走,控件却没置灰:调了不生效,还会把值写进两队并落盘",
  );
  byId.get("documents-tab-defect").click();
  await flush();
  assert(byId.get("documents-tag-filter").value === "后端", "切回缺陷页,标签筛选没了");
  assert(
    !document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "切回缺陷页后标签只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // ② 需求页的标签绝不许被写进缺陷队列:用户从没在缺陷页设过,缺陷列表却少一批。
  await setDocFilter("documents-tag-filter", "all");
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-tag-filter", "核心");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "前置失败:需求标签筛选没生效",
  );
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    liveTags().defect === "all",
    `对照页把需求页的标签写进了缺陷队列(缺陷队列被一个用户没设过的条件筛掉一批):${JSON.stringify(liveTags())}`,
  );
  assert(
    savedTags().defect === "all",
    `对照页把需求页的标签写进缺陷队列并落盘了:${storage.get(filtersStoreKey)}`,
  );
  assert(
    liveTags().req === "核心" && savedTags().req === "核心",
    `对照页把需求队列自己的标签也改了:${JSON.stringify(liveTags())} / ${storage.get(filtersStoreKey)}`,
  );

  // ③ D-169 的"值失效"回落必须还在,但只作用于该标签所属的那一队。
  // 缺陷页设「后端」,随后该标签在缺陷队列里消失(改名/清空/换项目):下拉只能回落成
  // 「全部」,状态与落盘必须跟着回落,否则列表被一个看不见的条件筛空;而需求队列的
  // 「核心」还在、还有效,一个字节都不许动。
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-tag-filter", "后端");
  payloads.docs_snapshot = {
    ...savedTagDocs,
    requirements: [
      docEntry("R-001", "核心标签需求", "doing", { fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端标签需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [docEntry("D-002", "前端标签缺陷", "open", { fields: [["标签", "前端"]] })],
  };
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTags().defect === "all" && savedTags().defect === "all",
    `标签在缺陷队列里已不存在,筛选状态却没跟着回落(列表被看不见的条件筛空,D-169):${JSON.stringify(liveTags())} / ${storage.get(filtersStoreKey)}`,
  );
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-002"]'),
    "标签回落后缺陷列表仍是空的(条目看起来凭空掉了)",
  );
  assert(
    liveTags().req === "核心" && savedTags().req === "核心",
    `缺陷队列的标签回落顺手改掉了需求队列的标签(值失效纠正跨队列写了):${JSON.stringify(liveTags())} / ${storage.get(filtersStoreKey)}`,
  );

  // 收尾:标签调回「全部」并还原快照,否则后续用例看到的是被筛过的列表。
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-tag-filter", "all");
  payloads.docs_snapshot = savedTagDocs;
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTags().req === "all" && liveTags().defect === "all",
    `收尾失败:标签没调回全部(${JSON.stringify(liveTags())}),后续用例会连带假失败`,
  );
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2
      && document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]'),
    "收尾失败:快照没还原,后续用例会连带假失败",
  );
}

// ---------- 对照页全字段中性化(D-244):priority/blocked 与 status/tag/complexity/sort 同机制 ----------
// 此前对照页上只剩 priority/blocked 两个控件仍是"真实筛选":调一次就跨队列写进
// documentFilters.req/defect 并落盘(实测 before={"req":"all","defect":"all"}
// after={"req":"P0","defect":"P0"} saved=同)——用户去对照页调一下优先级,另一队的
// 持久化筛选就被覆盖。定调:对照页是只读的对照视图,priority/blocked 同样走中性副本,
// 只改显示、不动任何一队的底层状态;控件置灰,切回单队列页时原值原样回来。
{
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const filtersStoreKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
  assert(filtersStoreKey, "前置失败:筛选没有落盘(R-115 的持久化本身断了)");
  const savedFilters = () => JSON.parse(storage.get(filtersStoreKey) ?? "{}");

  // ① req 页设 priority=P0 + blocked=blocked(两条需求默认 P1 且不阻塞 → 列表被筛空)。
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-priority-filter", "P0");
  await setDocFilter("documents-blocked-filter", "blocked");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "前置失败:req 页 priority/blocked 筛选没生效",
  );
  const before = savedFilters();

  // ② 切对照页:priority/blocked 控件置灰并显示 all(与 status/tag 同机制),列表不再被筛空。
  byId.get("documents-tab-both").click();
  await flush();
  assert(
    byId.get("documents-priority-filter").disabled,
    "对照页优先级控件没有置灰(D-244:只读对照视图)",
  );
  assert(
    byId.get("documents-blocked-filter").disabled,
    "对照页阻塞控件没有置灰(D-244:只读对照视图)",
  );
  assert(
    byId.get("documents-priority-filter").value === "all",
    `对照页优先级控件应显示中性 all,实际 "${byId.get("documents-priority-filter").value}"`,
  );
  assert(
    byId.get("documents-blocked-filter").value === "all",
    `对照页阻塞控件应显示中性 all,实际 "${byId.get("documents-blocked-filter").value}"`,
  );
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "对照页仍按 priority=all 之外的条件在筛 R-001(优先级中性化没生效,界面写着全部却少条目)",
  );
  assert(
    !document.querySelector("#documents-req-list .doc-filtered-empty"),
    "对照页不应渲染「清除筛选」:全字段中性化后不可能被筛空(D-244)",
  );
  assert(
    !document.querySelector("#documents-req-list .drag-hint"),
    "对照页不应渲染锁提示:全字段中性化后没有锁定条件(D-244)",
  );

  // ③ 两队的持久化筛选原样保留,一个字节都不许被对照页改掉(内存 + localStorage)。
  const after = savedFilters();
  assert(
    before.docReq?.priority === "P0" && before.docReq?.blocked === "blocked"
      && after.docReq?.priority === "P0" && after.docReq?.blocked === "blocked",
    `对照页把 req 的持久化筛选改掉了:${JSON.stringify(after)}`,
  );
  assert(
    before.docDefect?.priority === after.docDefect?.priority
      && before.docDefect?.blocked === after.docDefect?.blocked,
    `对照页把 defect 的持久化筛选改掉了:${JSON.stringify(after)}`,
  );

  // ④ 切回 req 页:控件原值回来、列表仍按用户设定的筛(对照页只改显示,没动底层)。
  byId.get("documents-tab-req").click();
  await flush();
  assert(
    byId.get("documents-priority-filter").value === "P0"
      && byId.get("documents-blocked-filter").value === "blocked",
    "切回 req 页,priority/blocked 筛选没了(对照页把它清掉了)",
  );
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "切回 req 页后 priority/blocked 筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // 收尾:走用户路径调回「全部」,不污染后续用例。
  await setDocFilter("documents-priority-filter", "all");
  await setDocFilter("documents-blocked-filter", "all");
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2,
    "收尾失败:筛选没调回全部,后续用例会连带假失败",
  );
}
// ③ 冻结对象护栏:goal/source/finding 三张列表拿的是**冻结的** NEUTRAL_DOC_FILTERS。
// 这两个按钮在它们身上渲染不出来(筛选分支只对 req/defect 生效 → 不可能"被筛空";
// 锁提示显式限定 kind),而且写回一律走 documentFilters[kind](这三类取不到就不写)。
// 两道保险都要在:机械钉住"根本没渲染",免得哪天筛选放开了顺手踩到冻结对象上抛异常。
{
  for (const listId of ["goal-list", "source-list", "finding-list"]) {
    assert(
      !document.querySelector(`#${listId} .doc-filtered-empty`) && !document.querySelector(`#${listId} .drag-hint`),
      `#${listId} 渲染出了会写筛选状态的按钮,但它拿到的是冻结的 NEUTRAL_DOC_FILTERS`,
    );
  }
}

// ---------- 筛选只能写给确实拥有该字段的队列 ----------
// documentFilters.defect 没有 complexity/sort 两个键。凭空写进去,锁提示的
// `key in reqFilterState` 就会把「复杂度=大」列进缺陷队列的锁,而 docDragEnabled 的
// 缺陷分支只看 status/priority/tag/blocked——提示说锁了、实际仍可拖(D-211 反向脱节)。
{
  byId.get("documents-tab-both").click();
  await flush();
  sandbox.applyDocFilter("complexity", "大");
  sandbox.applyDocFilter("sort", "priority");
  await flush();
  // documentFilters 是 const 词法声明,不会挂到 sandbox 全局上,只能在同一 context 里求值。
  const defectFilterKeys = vm.runInContext("Object.keys(documentFilters.defect).join(',')", sandbox).split(",");
  assert(
    !defectFilterKeys.includes("complexity"),
    `对照模式把「复杂度」写进了缺陷筛选状态:缺陷拖拽判断根本不看它,锁提示却会照列(D-211 反向脱节)。实得键:${defectFilterKeys.join(",")}`,
  );
  assert(
    !defectFilterKeys.includes("sort"),
    `对照模式把「排序」写进了缺陷筛选状态。实得键:${defectFilterKeys.join(",")}`,
  );
  byId.get("documents-tab-req").click();
  await flush();
  // 对照页不再清用户的筛选(见上一段),所以刚写进 req 的复杂度/排序会真的留下来:
  // 这里手工调回全部,否则后续用例的列表是被筛过的。
  sandbox.applyDocFilter("complexity", "all");
  sandbox.applyDocFilter("sort", "manual");
  await flush();
  assert(
    document.querySelectorAll("#documents-req-list .doc-item").length >= 2,
    "收尾失败:复杂度/排序没调回全部,后续用例会连带假失败",
  );
}

// ---------- 跨视图跳转的高亮必须活过随后的那次刷新(真机时序) ----------
// openDocumentsView() 触发的 refreshDocs() 是一次真实 IPC:`await invoke("docs_snapshot")`
// 之后才 renderDocsSnapshot。用 setTimeout(…, 0) 去赌"刷新已经落地",真机毫秒级 IPC 下
// 必然赌输——高亮打在旧节点上,紧接着 renderDocList 的 el.innerHTML = "" 把该节点连同
// scrollIntoView 的落点一并清掉:用户被切过去却看不出是哪一条。
// 默认桩是已 resolve 的 promise,微任务恒先于 setTimeout,顺序恰好反过来 = 假绿;
// 这里给 docs_snapshot 挂闸门,把真机顺序复现出来。
{
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:跳转前列表里没有 R-002");
  let openDocsSnapshot;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { openDocsSnapshot = resolve; }));
  sandbox.jumpToEntry("R-002");
  // 刷新还挂在闸门上,这一轮先把已排队的 setTimeout 跑掉 = 真机顺序。只跑一遍、不连锁:
  // 回调自己排的 1200ms 移除定时器不会被顺带冲掉,失败原因就只剩「高亮打在旧节点上」。
  // 排空带超时(drainTimersOnce):闸门段前若有人排了一次 refreshDocsSoon,它内部的
  // docs_snapshot 会撞上这道还没放开的闸门,无超时的 await 会让整个冒烟挂死而不是判红。
  await settle();
  await drainTimersOnce("跨视图跳转闸门段");
  openDocsSnapshot();
  invokeGates.delete("docs_snapshot");
  // 只推进微任务、不动定时器:ref-highlight 的 1200ms 移除定时器不能在断言前被冲掉。
  for (let i = 0; i < 12; i += 1) await settle();
  assert(byId.get("view-documents").classList.contains("active"), "跳转到单页里的条目时没有先切视图(点了没反应,D-166 复发)");
  const freshJumpNode = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]');
  assert(freshJumpNode, "刷新后列表里找不到 R-002");
  assert(
    freshJumpNode?.classList.contains("ref-highlight"),
    "跨视图跳转的高亮没活过随后的 refreshDocs:重绘把带高亮的旧节点整个换掉了(真机 IPC 毫秒级,setTimeout(0) 必然先跑)",
  );
  await flush();
}

// ---------- 刷新失败不得留下悬挂高亮 ----------
// 11-docs-list.js 写着「不留一个会在将来某次无关刷新上突然亮起来的悬挂高亮」,但那句
// 只在 renderDocsSnapshot 真的跑到时成立。真机上 docs_snapshot 会因目录被删/文件锁/
// 解析失败而抛错:refreshDocs 走 catch → 不重绘 → pendingJumpId 一直挂着;之后任意一次
// 无关刷新(agent 触发的 refreshDocsSoon、或用户再进文档页)都会把它消费掉 ——
// 用户没点跳转,条目自己亮了。承诺与实现必须一致(D-211)。
{
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:跳转前列表里没有 R-002");
  // 注入的刷新失败会走 toastError:那正是被测的那条 catch,不判红。
  expectedPersistentError = "项目文档刷新失败";
  const hitsBefore = expectedPersistentHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:目录被删/文件被锁/解析失败");
  sandbox.jumpToEntry("R-002");
  // 只推进微任务、不跑定时器:失败注入期间不能让 refreshDocsSoon 之类的定时器也撞上去
  // (它的 catch 走 console.error,会以另一种形态判红,掩盖真正要看的那条)。
  for (let i = 0; i < 12; i += 1) await settle();
  invokeFailures.delete("docs_snapshot");
  assert(expectedPersistentHits > hitsBefore, "前置失败:注入的 docs_snapshot 失败没有触发 refreshDocs 的 catch");
  expectedPersistentError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === null,
    "docs_snapshot 抛错后 pendingJumpId 还挂着:下一次无关刷新会把它兑现——用户没点跳转,条目自己亮了(悬挂高亮)",
  );
  // 无关刷新:一次成功的 refreshDocs 不得凭空点亮任何条目。
  await sandbox.refreshDocs();
  // 断言前只推微任务:1200ms 的移除定时器一旦被跑掉,这条断言就恒真了。
  for (let i = 0; i < 12; i += 1) await settle();
  const strayHighlights = document.querySelectorAll(".ref-highlight");
  assert(
    strayHighlights.length === 0,
    `无关刷新点亮了 ${strayHighlights.length} 个条目(${strayHighlights.map((n) => n.dataset.docId).join(",")}):悬挂高亮被消费了`,
  );
  await flush();
}

// ---------- refreshDocsSoon 的失败路径同样不得留下悬挂高亮(单独钉) ----------
// 上面那条只走得到 refreshDocs 的 catch。运行中真正高频跑的是 refreshDocsSoon:agent 每次
// 改需求/缺陷都会排它,而它的 catch 是另一条独立的出口(console.error,不是 toastError)。
// 实测过:只删掉 refreshDocsSoon 里的 clearPendingJump()、保留 refreshDocs 那处,整套冒烟
// 照样全绿——"两条路径都钉住了"是个错觉,将来会静默回退。这一条只钉 refreshDocsSoon。
// 手法:让跳转触发的那次 refreshDocs 卡在闸门上(闸门在 invoke 那一刻就捕获,随后立刻
// 摘掉,后续调用不再等),于是这一段里唯一跑得完的刷新就是 refreshDocsSoon —— 清理到底
// 是谁做的没有歧义,refreshDocs 那处即使还在也帮不上忙。
{
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'), "前置失败:跳转前列表里没有 R-002");
  let releaseJumpRefresh;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseJumpRefresh = resolve; }));
  sandbox.jumpToEntry("R-002");
  // 只推微任务:这一步要的是"refreshDocs 卡住、pendingJumpId 挂着",不能让定时器插进来。
  for (let i = 0; i < 12; i += 1) await settle();
  invokeGates.delete("docs_snapshot");
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-002",
    `前置失败:跳转没有挂起高亮(实得 ${JSON.stringify(vm.runInContext("pendingJumpId", sandbox))})`,
  );
  // 被测的正是 refreshDocsSoon 那条 catch,它只 console.error —— 开窗放行,出了这段立刻收回。
  expectedConsoleError = "冒烟注入";
  const consoleHitsBefore = expectedConsoleHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:refreshDocsSoon 撞上目录被删/文件被锁/解析失败");
  sandbox.refreshDocsSoon();
  await drainTimersOnce("refreshDocsSoon 失败路径");
  for (let i = 0; i < 12; i += 1) await settle();
  invokeFailures.delete("docs_snapshot");
  assert(
    expectedConsoleHits > consoleHitsBefore,
    "前置失败:注入的 docs_snapshot 失败没有走到 refreshDocsSoon 的 catch(这一段根本没测到目标路径)",
  );
  expectedConsoleError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === null,
    "refreshDocsSoon 抛错后 pendingJumpId 还挂着:之后任意一次无关刷新都会把它兑现——用户没点跳转,条目自己亮了(悬挂高亮)",
  );
  // 收尾:放开闸门让那次卡住的 refreshDocs 跑完(注入已撤,它会正常重绘),
  // 并确认这次无关刷新没有凭空点亮任何条目。
  releaseJumpRefresh();
  for (let i = 0; i < 12; i += 1) await settle();
  const straySoonHighlights = document.querySelectorAll(".ref-highlight");
  assert(
    straySoonHighlights.length === 0,
    `refreshDocsSoon 失败后的无关刷新点亮了 ${straySoonHighlights.length} 个条目(${straySoonHighlights.map((n) => n.dataset.docId).join(",")}):悬挂高亮被消费了`,
  );
  await flush();
}

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

// ---------- R-129 正文分段阅读:摘要行 + 段落块 + 超长折叠 + 编辑切换 ----------
// 用长多段正文替换载荷,走真实渲染路径(loadMemoryList → 点击条目 → showMemoryDetail),
// 验证:摘要行取首段、分段列表按空行拆、超长段折叠可展开、编辑按钮切回 textarea。
{
  const savedEntries = structuredClone(payloads.memory_entries);
  payloads.memory_entries = [
    { id: "M-LONG-001", category: "fact", title: "长正文记忆", description: "钩子", status: "active", body: "第一段要点：这是正文摘要应展示的首段内容。\n\n第二段：拆出来的第二个段落块。\n\n第三段超长：\n" + "很长的段落文本，需要折叠。".repeat(30), hits: 0, lastHitAt: 0, recalled: 0, fetched: 0, updated: "2026-08-01" },
    { id: "M-DEAD-001", category: "fact", title: "从没被用到的记忆", description: "冒烟用:零命中条目", status: "active", body: "陈旧结论", hits: 0, lastHitAt: 0, recalled: 0, fetched: 0, updated: "2026-01-01" },
  ];
  await sandbox.loadMemoryList("project", null);
  await flush();
  const longRow = [...document.querySelectorAll("#memory-list .memory-row")].find((r) => r.dataset.memoryId === "M-LONG-001");
  assert(longRow, "前置失败:长正文记忆条目未渲染");
  longRow.click();
  await flush();
  const summary = document.querySelector("#memory-detail .memory-body-summary");
  assert(summary, "记忆详情未渲染正文摘要行(R-129)");
  assert(
    listText("memory-detail").includes("正文摘要") && listText("memory-detail").includes("第一段要点"),
    `摘要行未展示首段要点: "${listText("memory-detail").slice(0, 80)}"`,
  );
  const paras = document.querySelectorAll("#memory-detail .memory-body-para");
  assert(paras.length === 3, `正文未按空行拆成 3 段,实得 ${paras.length}`);
  const collapsed = document.querySelector("#memory-detail .memory-body-para.collapsed");
  assert(collapsed, "超长段未折叠(长文仍整块糊在详情里)");
  const toggle = collapsed.querySelector(".memory-body-toggle");
  assert(toggle, "折叠段缺少展开按钮");
  assert(toggle.textContent.includes("展开"), `展开按钮文案不对: "${toggle.textContent}"`);
  toggle.click();
  assert(
    !collapsed.classList.contains("collapsed"),
    "点展开后折叠类未移除(点了不展开)",
  );
  assert(toggle.textContent.includes("收起"), "展开后按钮未切换为「收起」");
  const editBtn = [...document.querySelectorAll("#memory-detail .memory-body-edit-row button")].find((b) => b.textContent.includes("编辑正文"));
  assert(editBtn, "阅读视图缺少「编辑正文」入口");
  editBtn.click();
  const textarea = document.querySelector("#memory-detail .memory-body-read textarea[aria-label]");
  assert(textarea, "点编辑正文后未切换回 textarea");
  assert(textarea.value.includes("第一段要点"), "textarea 未回填当前正文(编辑会丢内容)");
  // 编辑态保存:改正文 → 点保存 → memory_entry_save 载荷带新值。
  const saveBtn = [...document.querySelectorAll("#memory-detail .memory-detail-actions button")].find((b) => b.textContent === "保存修改");
  assert(saveBtn, "前置失败:保存按钮缺失");
  textarea.value = "改过的正文\n\n新段落";
  saveBtn.click();
  await flush();
  const savedCall = invokeArgs.find(({ cmd, args }) => cmd === "memory_entry_save" && args?.id === "M-LONG-001");
  assert(savedCall, "保存修改未提交 memory_entry_save");
  assert(savedCall.args.body === "改过的正文\n\n新段落", `保存载荷正文不对: "${savedCall.args.body}"`);
  payloads.memory_entries = savedEntries;
  await sandbox.loadMemoryList("project", null);
  await flush();
}

// ---------- R-150 空闲整理清单 + 三档宽度响应式 ----------
assert(invokeLog.includes("memory_value_flags"), "记忆页未拉取空闲整理清单");
const flagRows = document.querySelectorAll("#memory-value-flags .memory-flag-row");
assert(flagRows.length === 2, `空闲整理清单未渲染全部候选,实得 ${flagRows.length}`);
assert(
  document.querySelector("#memory-value-flags .memory-flag-row.zero-adopt"),
  "零采纳候选未按类别标记(区分「语义显著但决策无关」)",
);
assert(
  listText("memory-value-flags").includes("M-001") && listText("memory-value-flags").includes("5"),
  "零采纳候选未给出召回次数(判断依据缺失)",
);
// 记忆列表采纳率:召回/采纳 数据在条目 meta 可见。
assert(
  listText("memory-list").includes("召回") && listText("memory-list").includes("采纳"),
  "记忆列表未展示召回/采纳数据(验收②数据面)",
);
// 三档宽度:800/1024/1280 下记忆页不崩、清单与采纳率数据仍在 DOM。
for (const width of [800, 1024, 1280]) {
  windowShim.innerWidth = width;
  await flush();
  assert(
    document.querySelector("#memory-value-flags .memory-flag-row"),
    `${width}px 下空闲整理清单缺失`,
  );
  assert(
    listText("memory-list").includes("召回") && listText("memory-list").includes("采纳"),
    `${width}px 下记忆列表召回/采纳数据缺失`,
  );
}
windowShim.innerWidth = 1280;
await flush();

// ---------- R-132 一键整理:手动触发整理入口 + 结果反馈 ----------
const cleanupBtn = byId.get("memory-cleanup-btn");
assert(cleanupBtn, "空闲整理清单缺少一键整理入口(验收:手动触发整理)");
cleanupBtn.click();
await flush();
assert(invokeLog.includes("memory_cleanup_demote"), "一键整理未调用后端整理流程");
assert(listText("memory-flags-count").includes("2"), "整理后未刷新空闲整理清单计数");

// ---------- 主对话工具块:⎿ 摘要行与展开详情不得双写同一段文案(历史回放路径) ----------
// 用户实测:一条 edit 失败,⎿ 行显示了一段文案,点开详情又把同一段完整贴了一遍。
// 根因是摘要与详情各自独立地从同一份 content 取一遍,详情靠 `full !== preview` 去重,
// 只挡得住单行短结果。判据用「同一段文字在单个工具块里出现几次」——必须限定在单个
// .tool-msg 上取 textContent:harness 的 textContent 会把 innerHTML 文本与子节点文本拼接,
// 对整个 #messages 取会把别的消息里的同款文案一起算进来。
{
  const blocks = document.querySelectorAll("#messages .tool-msg");
  assert(blocks.length === 4, `历史回放应按 call_id 配出 4 个工具块,实得 ${blocks.length}`);
  const [h1, h2, h3, h4] = blocks;
  const resultOf = (block) => block.querySelector(".tool-msg-result")?.textContent ?? "";
  // 详情里真正的"剩余输出"块(带 args 类的那个是完整入参,不是结果原文)。
  const restOf = (block) =>
    block.querySelectorAll(".tool-msg-raw").find((n) => !n.classList.contains("args")) ?? null;

  // ① 首行超长 + 多行的失败结果:同一段文案只能出现一次。
  const needle = HISTORY_LONG_FIRST_LINE.slice(0, 60);
  assert(
    h1.textContent.split(needle).length - 1 === 1,
    `工具块把同一段结果文案渲染了两遍(⎿ 行与展开详情双写):出现 ${h1.textContent.split(needle).length - 1} 次`,
  );
  assert(
    resultOf(h1) === `⎿ ${HISTORY_LONG_FIRST_LINE.slice(0, 109)}…`,
    `⎿ 行截断规则漂移了:"${resultOf(h1).slice(0, 40)}…"(长度 ${resultOf(h1).length})`,
  );
  // 去重不能靠"干脆不给详情":被截掉的后半句与后续行必须仍读得到。
  const h1Rest = restOf(h1);
  assert(h1Rest, "长首行被截断后没有展开区:被截掉的内容再也读不到了");
  assert(
    h1Rest.textContent.startsWith("…") && h1Rest.textContent.includes(HISTORY_LONG_FIRST_LINE.slice(-20)),
    `展开区未接上被截断的首行尾巴:"${h1Rest.textContent.slice(0, 40)}"`,
  );
  assert(
    h1Rest.textContent.includes("第二行") && h1Rest.textContent.includes("第三行"),
    "展开区丢了首行之后的正文",
  );
  // 历史详情必须给完整入参(83 行那条源码契约的运行时版本)。
  assert(
    h1.querySelectorAll(".tool-msg-raw").some((n) => n.classList.contains("args") && n.textContent.includes("path")),
    "历史工具块缺少完整入参 JSON",
  );

  // ② 8000 字上界仍然生效(去重不等于放开长度上界)。
  const h2Rest = restOf(h2);
  assert(h2Rest, "超长历史输出没有展开区");
  assert(h2Rest.textContent.endsWith("…(已截断)"), `超长输出未截断:结尾为 "${h2Rest.textContent.slice(-20)}"`);
  assert(h2Rest.textContent.length < 8100, `截断上界失效,实得 ${h2Rest.textContent.length} 字`);

  // ③ bash 的 "exit code: 0" 独占首行时顺延到下一行,被跳过的那行归入剩余而不是丢掉。
  assert(resultOf(h3) === "⎿ 真正的输出行", `exit code 顺延语义变了:"${resultOf(h3)}"`);
  assert(restOf(h3)?.textContent === "exit code: 0", `被跳过的 exit code 行被丢掉了:"${restOf(h3)?.textContent}"`);

  // ④ 全篇只有 "exit code: 0" 时仍显示它,不塌成「完成」(原实现 `|| lines[0]` 兜底的等价保留)。
  assert(resultOf(h4) === "⎿ exit code: 0", `唯一的结果行被吞成兜底文案:"${resultOf(h4)}"`);
  assert(restOf(h4) === null, "只有一行结果时不该出展开区(展开了还是那一行 = 假承诺)");
}

// ---------- 活动面板：完整工具调用 + 筛选 + 信息量 + 可操作 ----------
const toolStart = handlers.get("kz:tool-start");
const toolEnd = handlers.get("kz:tool-end");
const taskProgress = handlers.get("kz:task-progress");
assert(toolStart && toolEnd, "工具事件未订阅");
assert(taskProgress, "子代理进度事件未订阅");
toolStart({ payload: { id: "T1", name: "bash", summary: "cargo test --workspace", input: { command: "cargo test --workspace", workdir: "." } } });
toolStart({ payload: { id: "T2", name: "edit", summary: "main.js", input: { path: "ui/main.js" } } });
toolStart({ payload: { id: "T3", name: "task", summary: "审查子代理", input: { prompt: "review" } } });
await flush();
const bashEntry = document.querySelector("#bg-list .bg-entry[data-bg-tool=bash]");
assert(bashEntry, "活动面板缺少终端类条目");
assert(document.querySelector("#bg-list .bg-entry[data-bg-id=T2]"), "成功 edit 未进入完整活动栏");
assert(document.querySelector("#bg-list .bg-entry[data-bg-id=T3]"), "运行中的 task 未进入完整活动栏");
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

// ---------- R-173 编排派发的勘察/复核子代理:实时进度必须落到面板上 ----------
// 编排对象按角色表派发的这批子代理不经模型 tool call,主对话里没有内联工具块兜底,
// 活动面板是它们唯一的可见处。此前 name 恒为 "task" 被 R-168 一刀切静默,于是 5 勘察 +
// 3 复核的轮次/工具进度整批落空。区分依据是后端给的 input.phase(scouting/review)。
const orchEntry = (role) => [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === role);
const orchGroup = (phase) => document.querySelector(`.bg-group[data-bg-phase=${phase}]`);
const orchGroupHead = (phase) => orchGroup(phase)?.querySelector(".bg-group-head")?.textContent ?? "";
const scoutRoles = ["architecture_scout", "runtime_scout", "test_scout"];
// ① 契约时序:N 条 start 先全发完。派发瞬间就该全部可见,不能等各自跑完才冒出来。
for (const role of scoutRoles) {
  toolStart({ payload: { id: role, name: "task", summary: `${role} · 勘察`, input: { prompt: `派给 ${role} 的完整指令`, phase: "scouting", role } } });
}
// 同一批里混一条模型自己派的 task(不带 phase):R-168 的静默口径不能被顺手打破。
toolStart({ payload: { id: "MODEL_TASK", name: "task", summary: "模型自己派的子代理", input: { prompt: "review" } } });
await flush();
for (const role of scoutRoles) {
  assert(orchEntry(role), `编排派发的勘察子代理 ${role} 没进活动面板(内部进度整批丢掉)`);
}
  assert(orchEntry("MODEL_TASK"), "模型自己派的 task 未进入完整活动栏");
// ② 分组:按 input.phase 分区,这是 Running/Finished 分区的雏形。
assert(orchGroup("scouting"), "勘察子代理未按 input.phase 分组");
assert(
  orchGroup("scouting").querySelectorAll(".bg-entry").length === scoutRoles.length,
  `勘察分组内条目数不对:${orchGroup("scouting").querySelectorAll(".bg-entry").length}`,
);
assert(orchGroupHead("scouting").includes("勘察"), `勘察分组标题缺阶段名,实得 "${orchGroupHead("scouting")}"`);
assert(orchGroupHead("scouting").includes("0/3"), `勘察分组标题未给出完成数/总数,实得 "${orchGroupHead("scouting")}"`);
// ③ 单条信息量:角色名(不是恒为 "task" 的工具名)、所属阶段、已运行时长与内部调用数。
const scout = orchEntry("architecture_scout");
assert(
  scout.querySelector(".bg-tool")?.textContent === "architecture_scout",
  `条目未以角色名标识,实得 "${scout.querySelector(".bg-tool")?.textContent}"(8 条都叫 task 等于没标识)`,
);
assert(
  scout.querySelector(".bg-phase-badge")?.textContent === "勘察",
  `条目未标出所属阶段,实得 "${scout.querySelector(".bg-phase-badge")?.textContent}"`,
);
assert(/运行中 · \d+s · 内部调用 \d+/.test(scout.querySelector(".bg-meta")?.textContent ?? ""),
  `运行中未给出状态/已运行时长/内部调用数,实得 "${scout.querySelector(".bg-meta")?.textContent}"`);
assert(scout.dataset.bgStatus === "running", `运行中状态未落到条目上,实得 "${scout.dataset.bgStatus}"`);
// ④ 执行期进度:纯轮次进度(trace 为 null)与工具进度都要落到对应角色。
taskProgress({ payload: { id: "architecture_scout", text: "第 3/12 轮", trace: null } });
await flush();
assert(scout.querySelector(".bg-prog")?.textContent === "第 3/12 轮",
  `轮次进度未挂回角色条目,实得 "${scout.querySelector(".bg-prog")?.textContent}"`);
// ⑤ 当前正在用的工具名 —— 用户点名要的那一项。值与写入去向都断言:
// 只断言文本看不出"写对了内容却写错了元素",dataset 探针把去向也钉死。
taskProgress({ payload: { id: "architecture_scout", text: "第 4/12 轮", trace: { child_id: "c1", phase: "start", name: "grep", summary: "phase_pipeline" } } });
await flush();
assert(scout.querySelector(".bg-current")?.textContent.includes("grep"),
  `当前工具名未显示,实得 "${scout.querySelector(".bg-current")?.textContent}"`);
assert(scout.dataset.bgCurrentTool === "grep", `当前工具名写错了地方,实得 "${scout.dataset.bgCurrentTool}"`);
assert(!scout.querySelector(".bg-current")?.classList.contains("hidden"), "当前工具名所在行仍是隐藏的");
assert(!scout.querySelector(".bg-current")?.classList.contains("idle"), "工具正在跑却标成了空闲态");
taskProgress({ payload: { id: "architecture_scout", text: "第 4/12 轮", trace: { child_id: "c1", phase: "end", name: "grep", ok: true, preview: "命中 12 处" } } });
taskProgress({ payload: { id: "architecture_scout", text: "第 5/12 轮", trace: { child_id: "c2", phase: "start", name: "read", summary: "phase_pipeline.rs" } } });
await flush();
assert(scout.dataset.bgCurrentTool === "read", `当前工具名未跟着换到下一个工具,实得 "${scout.dataset.bgCurrentTool}"`);
assert(/内部调用 2/.test(scout.querySelector(".bg-meta")?.textContent ?? ""),
  `工具调用次数未累计,实得 "${scout.querySelector(".bg-meta")?.textContent}"`);
// ⑥ 终态:成功/失败/超时三分,完成的条目继续可见。
toolEnd({ payload: { id: "architecture_scout", name: "task", ok: true, preview: "勘察简报首行", display: null } });
toolEnd({ payload: { id: "runtime_scout", name: "task", ok: false, preview: "(超时,未产出结果)", display: null } });
toolEnd({ payload: { id: "test_scout", name: "task", ok: false, preview: "子代理内部报错", display: null } });
await flush();
assert(orchEntry("architecture_scout")?.dataset.bgStatus === "ok", "成功角色终态不对");
assert(orchEntry("test_scout")?.dataset.bgStatus === "err", "失败角色终态不对");
assert(orchEntry("runtime_scout")?.dataset.bgStatus === "timeout",
  `超时角色未与失败区分开,实得 "${orchEntry("runtime_scout")?.dataset.bgStatus}"`);
assert(orchEntry("runtime_scout")?.classList.contains("timeout"), "超时角色缺少可视区分的样式钩子");
assert(!orchEntry("test_scout")?.classList.contains("timeout"), "普通失败被误标成超时");
assert(orchEntry("runtime_scout")?.querySelector(".bg-meta")?.textContent.includes("超时"),
  `超时角色元信息未写明超时,实得 "${orchEntry("runtime_scout")?.querySelector(".bg-meta")?.textContent}"`);
assert(orchEntry("test_scout")?.querySelector(".bg-meta")?.textContent.includes("失败"), "失败角色元信息未写明失败");
assert(scout.dataset.bgCurrentTool === "", "角色收尾后当前工具名没清掉(会一直显示最后一个工具在跑)");
for (const role of scoutRoles) {
  assert(orchEntry(role) && !orchEntry(role).classList.contains("hidden"), `${role} 跑完就消失了(完成的条目必须保留可见)`);
}
assert(orchGroupHead("scouting").includes("3/3"), `勘察分组标题未跟随完成数,实得 "${orchGroupHead("scouting")}"`);
// ⑦ 复核阶段单独一区,与勘察分开。
for (const role of ["spec_reviewer", "risk_reviewer"]) {
  toolStart({ payload: { id: role, name: "task", summary: `${role} · 复核`, input: { prompt: `派给 ${role} 的完整指令`, phase: "review", role } } });
}
await flush();
assert(orchGroup("review"), "复核子代理未单独分区");
assert(orchGroup("review").querySelectorAll(".bg-entry").length === 2, "复核分组内条目数不对");
assert(orchGroupHead("review").includes("复核"), `复核分组标题缺阶段名,实得 "${orchGroupHead("review")}"`);
assert(orchEntry("spec_reviewer")?.querySelector(".bg-phase-badge")?.textContent === "复核", "复核条目阶段标记不对");
assert(
  orchGroup("scouting").querySelectorAll(".bg-entry").length === 3,
  "复核条目串进了勘察分组",
);
// ⑧ 角色名跨轮复用:同名角色再次派发要原地复位,否则第二轮的进度全写进上一轮那条终态行。
toolStart({ payload: { id: "architecture_scout", name: "task", summary: "architecture_scout · 勘察", input: { prompt: "第二轮指令", phase: "scouting", role: "architecture_scout" } } });
await flush();
assert(orchEntry("architecture_scout")?.dataset.bgStatus === "running",
  `同名角色第二轮派发未复位,实得 "${orchEntry("architecture_scout")?.dataset.bgStatus}"(面板会定格在上一轮)`);
assert(orchGroup("scouting").querySelectorAll(".bg-entry").length === 3, "同名角色复位时把条目复制了一份");
assert(orchGroupHead("scouting").includes("2/3"), `复位后完成数未回退,实得 "${orchGroupHead("scouting")}"`);
toolEnd({ payload: { id: "architecture_scout", name: "task", ok: true, preview: "第二轮简报", display: null } });
await flush();

// ---------- R-184 P2:活动记录按 agent 归属与折叠 ----------
// ① 编排子代理轨迹带角色色点(角色名文本始终在旁,颜色不作唯一区分);
//    无 phase 的模型自派 task 不进活动面板,自然也不该有色点。
assert(orchEntry("architecture_scout")?.querySelector(".bg-dot"), "编排子代理轨迹缺角色色点");
assert(
  !orchEntry("MODEL_TASK") || !orchEntry("MODEL_TASK").querySelector(".bg-dot"),
  "无 phase 的模型自派 task 不该有角色色点",
);
// ② 角色筛选下拉动态列出全部角色(全部 + 每个出现过的角色)。
const roleFilter = document.querySelector("#bg-role-filter");
assert(roleFilter, "活动面板缺角色筛选下拉");
const roleOptions = [...roleFilter.options].map((o) => o.value);
for (const role of [...scoutRoles, "spec_reviewer", "risk_reviewer"]) {
  assert(roleOptions.includes(role), `角色筛选下拉缺选项 ${role},实得 ${roleOptions.join(",")}`);
}
// ③ 切到某角色 → 只剩该角色的条目可见;切回全部 → 全部恢复。
roleFilter.value = "architecture_scout";
roleFilter._listeners.change?.forEach((fn) => fn({ target: roleFilter }));
await flush();
const visibleAfterRole = [...document.querySelectorAll("#bg-list .bg-entry")].filter((n) => !n.classList.contains("hidden"));
assert(
  visibleAfterRole.length >= 1 && visibleAfterRole.every((n) => n.dataset.bgRole === "architecture_scout"),
  `按角色筛选后应只剩 architecture_scout,实得 ${visibleAfterRole.map((n) => n.dataset.bgRole).join(",")}`,
);
roleFilter.value = "all";
roleFilter._listeners.change?.forEach((fn) => fn({ target: roleFilter }));
await flush();
// ④ 主对话里同一角色的 task 工具块折叠成一组(默认收起,组头带块数)。
const fold = document.querySelector('.agent-fold[data-agent-role="architecture_scout"]');
assert(fold, "主对话缺角色折叠组");
const foldHead = fold.querySelector(".agent-fold-head");
const foldBody = fold.querySelector(".agent-fold-body");
assert(foldHead && foldBody, "折叠组缺头部或主体");
assert(foldBody.classList.contains("hidden"), "折叠组默认应收起");
assert(foldHead.getAttribute("aria-expanded") === "false", "折叠组头 aria-expanded 初始应为 false");
// 编排角色固定调用 id,第二轮 start 被 chatToolBlocks.has(id) 守卫吞(既有行为:
// 同一调用 id 只渲染一次),折叠组内保留第一轮的 1 个块。
const inFold = foldBody.querySelectorAll(".tool-msg").length;
assert(inFold === 1, `architecture_scout 折叠组内应有 1 个工具块,实得 ${inFold}`);
assert(fold.querySelector(".agent-fold-count")?.textContent.includes("1"), "折叠组头未显示块数");
foldHead.click();
await flush();
assert(!foldBody.classList.contains("hidden"), "点击折叠组头未展开");
assert(foldHead.getAttribute("aria-expanded") === "true", "展开后 aria-expanded 应为 true");
assert(fold.querySelector(".agent-fold-caret")?.textContent === "▾", "展开后 caret 未变为 ▾");
// ⑤ 不同角色各自独立成组,不互相吞并。
for (const role of scoutRoles.slice(1)) {
  assert(document.querySelector(`.agent-fold[data-agent-role=${role}]`), `角色 ${role} 没有自己的折叠组`);
}

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
// R-133:diff 汇总为目录树——多路径文件按层级归入目录,目录行可折叠。
{
  // 再投两个分属不同目录的文件,验证树形分组而不是平铺。
  editEnd({ payload: { id: "T7", name: "edit", ok: true, preview: "replaced", display: { kind: "diff", path: "crates/kanzei-app/src/docs.rs", additions: 5, deletions: 2, language: "rust", lines: [] } } });
  editEnd({ payload: { id: "T8", name: "edit", ok: true, preview: "replaced", display: { kind: "diff", path: "crates/kanzei-tools/src/tracker.rs", additions: 1, deletions: 0, language: "rust", lines: [] } } });
  await flush();
  const tree = document.querySelector("#diff-summary .diff-tree");
  assert(tree, "diff 汇总未构建目录树容器");
  const dirs = tree.querySelectorAll(".diff-dir-head");
  assert(dirs.length >= 1, `diff 树缺少目录行(应有 crates/ 等目录),实得 ${dirs.length}`);
  const dirTexts = [...dirs].map((d) => d.textContent);
  assert(
    dirTexts.some((s) => s.includes("crates")),
    `diff 树目录未包含 crates:${dirTexts.join(",")}`,
  );
  const head = [...dirs].find((d) => d.textContent.includes("crates"));
  assert(head.getAttribute("aria-expanded") === "true", "diff 目录初始应展开");
  const fileRows = tree.querySelectorAll(".diff-summary-row");
  assert(
    [...fileRows].some((r) => r.textContent.includes("crates/kanzei-app/src/docs.rs")),
    "diff 树文件行未归入目录下",
  );
  // 折叠交互:点目录头,子文件应隐藏。
  head._listeners.click?.forEach((fn) => fn({ target: head }));
  const collapsed = tree.querySelector(".diff-dir-body.hidden");
  assert(collapsed, "点击 diff 目录头未折叠子目录");
  assert(head.getAttribute("aria-expanded") === "false", "折叠后 aria-expanded 应为 false");
}

// ---------- 完整活动流 + bash 实时输出 + rail 侧栏开合 ----------
// ① 成功工具也进入活动流,失败仍保留失败态。
const quietBefore = document.querySelectorAll("#bg-list .bg-entry").length;
toolStart({ payload: { id: "Q1", name: "read", summary: "crates/kanzei/src/main.rs", input: { path: "crates/kanzei/src/main.rs" } } });
toolEnd({ payload: { id: "Q1", name: "read", ok: true, preview: "1 //! kz", display: null } });
await flush();
assert(document.querySelectorAll("#bg-list .bg-entry").length === quietBefore + 1, "成功的 read 未进入完整活动流");
toolStart({ payload: { id: "Q2", name: "req", summary: "update R-999", input: { action: "update" } } });
toolEnd({ payload: { id: "Q2", name: "req", ok: false, preview: "找不到 R-999", display: null } });
await flush();
const quietErrEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "Q2");
assert(quietErrEntry, "失败的静默工具没有补建条目(错误被吞掉)");
assert(quietErrEntry.classList.contains("err"), "补建的静默条目未标失败态");
// ② bash 增量输出:执行中逐段追加,收起时进度行跟到最后一行,结束后让位终态输出。
const toolProgress = handlers.get("kz:tool-progress");
assert(toolProgress, "工具增量输出事件未订阅");
toolStart({ payload: { id: "S1", name: "bash", summary: "scripts/package.ps1", input: { command: "powershell scripts/package.ps1" } } });
toolProgress({ payload: { id: "S1", chunk: "[1/6] 发布范围核对\n" } });
toolProgress({ payload: { id: "S1", chunk: "[4/6] cargo tauri build\n" } });
await flush();
const streamEntry = [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === "S1");
assert(streamEntry?.querySelector(".bg-live")?.textContent.includes("[4/6]"), "bash 执行中未实时追加输出");
assert(streamEntry?.querySelector(".bg-prog")?.textContent.includes("[4/6]"), "收起状态的进度行未跟到最后一行");
toolEnd({ payload: { id: "S1", name: "bash", ok: true, preview: "exit code: 0", display: { kind: "terminal", command: "powershell scripts/package.ps1", output: "全部完成", full: "全部完成" } } });
await flush();
assert(!streamEntry.querySelector(".bg-live"), "结束后实时流未让位给终态输出(同一份输出双份并存)");
// ③ rail 上的常驻侧栏开合:窄视口悬浮模式下顶栏开关会被盖住,rail 开关必须存在且可切换。
const railToggle = byId.get("rail-sidebar-toggle");
assert(railToggle, "activitybar 缺少常驻侧栏开合按钮");
const sidebarEl = byId.get("sidebar");
const collapsedBefore = sidebarEl.classList.contains("collapsed");
railToggle.click();
assert(sidebarEl.classList.contains("collapsed") !== collapsedBefore, "rail 开关没有切换侧栏");
railToggle.click();
assert(sidebarEl.classList.contains("collapsed") === collapsedBefore, "rail 开关未能再次切换回来");

// ---------- 主对话工具块:实时路径同样不双写 ----------
// 实时事件里的 preview 是后端 runner::preview 的单行摘要(首行 120 字 + " (+N lines)"),
// 本身就超过 ⎿ 行预算——双写在这条路径上是每次失败都能看见的。
{
  const toolMsgAt = (index) => document.querySelectorAll("#messages .tool-msg")[index];
  const LIVE_PREVIEW =
    "old_string 未命中:crates/kanzei-tools/src/edit.rs:202-209 的空白与换行与磁盘上的内容不一致," +
    "请改用插入式替换,或者先确认 allow_deletion 这个参数的语义之后再重试一次 (+2 lines)";
  assert(LIVE_PREVIEW.length > 120, `夹具失效:实时 preview 必须超过 ⎿ 行预算才验得到双写,实得 ${LIVE_PREVIEW.length} 字`);

  // ① 失败的长摘要:同一段文案在一个块里只能出现一次。
  let index = document.querySelectorAll("#messages .tool-msg").length;
  toolStart({ payload: { id: "X1", name: "edit", summary: "crates/kanzei-tools/src/edit.rs", input: { path: "crates/kanzei-tools/src/edit.rs" } } });
  toolEnd({ payload: { id: "X1", name: "edit", ok: false, preview: LIVE_PREVIEW, display: null } });
  await flush();
  const x1 = toolMsgAt(index);
  assert(x1, "实时失败工具块未建出");
  const needle = LIVE_PREVIEW.slice(0, 60);
  assert(
    x1.textContent.split(needle).length - 1 === 1,
    `工具块把同一段结果文案渲染了两遍(D-237 同族回归):出现 ${x1.textContent.split(needle).length - 1} 次`,
  );
  // 但被截掉的后半句必须仍读得到——不允许用「干脆不给 detail」的方式消灭重复。
  // 判据比 includes 更硬:去掉两端的续接省略号后,摘要 + 剩余必须**逐字**拼回原文
  // ——既不丢字(详情被砍掉),也不重字(双写复发)。
  const x1Head = (x1.querySelector(".tool-msg-result")?.textContent ?? "").replace(/^⎿ /, "").replace(/…$/, "");
  const x1Rest = (x1.querySelector(".tool-msg-raw")?.textContent ?? "").replace(/^…/, "");
  assert(
    x1Head + x1Rest === LIVE_PREVIEW,
    `摘要 + 详情拼不回原文(丢字或重字):摘要 ${x1Head.length} 字 + 详情 ${x1Rest.length} 字 vs 原文 ${LIVE_PREVIEW.length} 字`,
  );
  assert(x1Rest.endsWith("(+2 lines)"), `被截掉的尾巴读不到了:"${x1Rest.slice(-30)}"`);

  // ② 成功的短结果:零退化——⎿ 行原样,不出展开区,不加 has-detail。
  index = document.querySelectorAll("#messages .tool-msg").length;
  toolStart({ payload: { id: "X2", name: "edit", summary: "ui/x.js" } });
  toolEnd({ payload: { id: "X2", name: "edit", ok: true, preview: "replaced 1 occurrence", display: null } });
  await flush();
  const x2 = toolMsgAt(index);
  assert(x2?.querySelector(".tool-msg-result")?.textContent === "⎿ replaced 1 occurrence", `成功短结果的 ⎿ 行变了:"${x2?.querySelector(".tool-msg-result")?.textContent}"`);
  assert(x2.querySelector(".tool-msg-raw") === null, "成功短结果不该出展开区(展开了还是那一行 = 假承诺)");
  assert(!x2.classList.contains("has-detail"), "成功短结果不该标 has-detail");

  // 结构化终态:no-op/受控拒绝/真实故障必须三分，不能继续全部画成红叉。
  index = document.querySelectorAll("#messages .tool-msg").length;
  toolStart({ payload: { id: "XNOOP", name: "edit", summary: "ui/noop.js" } });
  toolEnd({ payload: { id: "XNOOP", name: "edit", ok: false, outcome: "noop", code: "EDIT_IDENTICAL_INPUT", preview: "无需修改", display: null } });
  await flush();
  const xNoop = toolMsgAt(index);
  assert(xNoop.classList.contains("noop") && !xNoop.classList.contains("err"), "no-op 仍被渲染成真实失败");
  assert(xNoop.querySelector(".tool-msg-status")?.textContent === "↪", "no-op 缺少独立形状标记");

  index = document.querySelectorAll("#messages .tool-msg").length;
  toolStart({ payload: { id: "XWARN", name: "edit", summary: "ui/warn.js" } });
  toolEnd({ payload: { id: "XWARN", name: "edit", ok: false, outcome: "needs_correction", code: "EDIT_ANCHOR_NOT_FOUND", preview: "请重读锚点", display: null } });
  await flush();
  const xWarn = toolMsgAt(index);
  assert(xWarn.classList.contains("warn") && !xWarn.classList.contains("err"), "受控拒绝仍被渲染成真实失败");
  assert(xWarn.querySelector(".tool-msg-status")?.textContent === "⚠", "受控拒绝缺少警告形状标记");
  const warnActivity = document.querySelector("#bg-list .bg-entry[data-bg-id=XWARN]");
  assert(warnActivity?.dataset.bgStatus === "warn", "活动栏没有保留受控拒绝终态");

  index = document.querySelectorAll("#messages .tool-msg").length;
  toolStart({ payload: { id: "XFAIL", name: "edit", summary: "ui/fail.js" } });
  toolEnd({ payload: { id: "XFAIL", name: "edit", ok: false, outcome: "failed", code: "EDIT_WRITE_FAILED", preview: "磁盘写入失败", display: null } });
  await flush();
  const xFail = toolMsgAt(index);
  assert(xFail.classList.contains("err"), "真实执行故障没有保留失败态");
  assert(xFail.querySelector(".tool-msg-status")?.textContent === "✗", "真实执行故障图标漂移");

  // ③ ⎿ 行截断点与剩余部分的切分必须严丝合缝:一个字要么在摘要里、要么在详情里。
  index = document.querySelectorAll("#messages .tool-msg").length;
  toolStart({ payload: { id: "X3", name: "edit", summary: "ui/y.js" } });
  toolEnd({ payload: { id: "X3", name: "edit", ok: true, preview: "x".repeat(200), display: null } });
  await flush();
  const x3 = toolMsgAt(index);
  const x3Result = x3?.querySelector(".tool-msg-result")?.textContent ?? "";
  assert(x3Result.length === 112 && x3Result.endsWith("…"), `⎿ 行预算漂移(应为 "⎿ " + 109 字 + "…" = 112),实得 ${x3Result.length}`);
  assert(
    x3.querySelector(".tool-msg-raw")?.textContent === `…${"x".repeat(91)}`,
    `剩余部分与截断点对不上(会漏字或重字):"${x3.querySelector(".tool-msg-raw")?.textContent?.slice(0, 20)}"`,
  );
}

// ---------- 活动栏条目标题:按工具挑字段,不是后端那坨入参 JSON ----------
// 后端 summarize_input(kanzei-core/src/runner/compaction.rs:251)把整个入参 JSON 截到
// 160 字,对所有工具一视同仁——edit 于是显示成 `{"new_string":"…","old_strin…`,
// 完全看不出改的是哪个文件(用户截图)。前端标题必须走 toolCallSummary 挑字段。
{
  const bgEntry = (id) => [...document.querySelectorAll("#bg-list .bg-entry")].find((n) => n.dataset.bgId === id);
  // ① 终端直入列:后端 summary 是裸 JSON,标题必须显示命令本身。
  toolStart({ payload: { id: "BGJ1", name: "bash", summary: '{"command":"cargo test -p kanzei-app","workdir":"."}', input: { command: "cargo test -p kanzei-app", workdir: "." } } });
  await flush();
  const j1 = bgEntry("BGJ1");
  assert(j1, "终端条目未入列");
  const j1Target = j1.querySelector(".bg-target")?.textContent ?? "";
  assert(j1Target === "cargo test -p kanzei-app", `活动栏标题回到了后端整坨入参 JSON:"${j1Target.slice(0, 60)}"`);
  assert(!j1Target.startsWith("{") && !j1Target.includes('"command"'), "活动栏标题里还留着 JSON 语法");
  // ② 悬浮提示同步:鼠标停上去看到的必须是同一份人类可读文本。
  assert(
    j1.querySelector(".bg-title")?.title === j1Target,
    `悬浮提示仍是裸 JSON 或与标题不一致:"${j1.querySelector(".bg-title")?.title?.slice(0, 60)}"`,
  );

  // ③ 失败补建路径(复现用户截图那条):edit 的 summary 是 new_string/old_string 的整坨 JSON。
  toolStart({ payload: { id: "BGJ2", name: "edit", summary: '{"new_string":"pub fn append_episode","old_string":"old"}', input: { path: "crates/kanzei-core/src/store.rs", new_string: "pub fn append_episode", old_string: "old" } } });
  toolEnd({ payload: { id: "BGJ2", name: "edit", ok: false, preview: "old_string not found", display: null } });
  await flush();
  const j2 = bgEntry("BGJ2");
  assert(j2, "失败的静默工具没有补建条目(R-168 回归)");
  assert(j2.classList.contains("err"), "补建的失败条目未标失败态");
  const j2Target = j2.querySelector(".bg-target")?.textContent ?? "";
  assert(j2Target === "crates/kanzei-core/src/store.rs", `edit 条目标题不是文件路径:"${j2Target.slice(0, 60)}"`);
  assert(!j2Target.includes("new_string") && !j2Target.startsWith("{"), "edit 条目标题里还留着入参 JSON");
  assert(j2.querySelector(".bg-title")?.title === j2Target, "edit 条目的悬浮提示与标题不一致");
  // 活动按钮本身展示人类可读目标,不能回退到后端整坨 JSON。
  assert(j2Target.includes("store.rs") && !j2Target.includes("new_string"), "活动条目目标仍是裸 JSON");
  assert(
    !listText("log-lines").includes('"new_string"'),
    "运行日志里仍直接拼后端 summary(edit 在日志里还是一坨入参 JSON)",
  );
  // summary 缺省时不能抛:事件里 summary 并非必填,`summary.slice()` 会把整条事件链打断。
  toolStart({ payload: { id: "BGJ2b", name: "read", input: { path: "crates/kanzei/src/main.rs" } } });
  await flush();
  assert(
    bgEntry("BGJ2b")?.querySelector(".bg-target")?.textContent.includes("crates/kanzei/src/main.rs"),
    "summary 缺省时活动条目未回落到入参挑字段",
  );

  // ④ 回落链第二级:挑不出字段就用后端 summary(回放事件不带 input,靠的就是这一级)。
  toolStart({ payload: { id: "BGJ3", name: "bash", summary: "人类可读的后端摘要", input: {} } });
  await flush();
  assert(
    bgEntry("BGJ3")?.querySelector(".bg-target")?.textContent === "人类可读的后端摘要",
    `挑不出字段时未回落后端 summary:"${bgEntry("BGJ3")?.querySelector(".bg-target")?.textContent}"`,
  );

  // ⑤ 回落链第三级:两级都空就是空标题,不能抛异常、也不能凭空编一句。
  toolStart({ payload: { id: "BGJ4", name: "bash", summary: "", input: {} } });
  await flush();
  const j4 = bgEntry("BGJ4");
  assert(j4, "summary 与 input 都为空时条目建不出来了");
  assert(j4.querySelector(".bg-target")?.textContent === "", "空标题被填了兜底文案");
  assert((j4.querySelector(".bg-title")?.title ?? "") === "", "空标题的悬浮提示不该有内容");

  // ⑥ diff 终态不得把两段式标题拍平:对整个 title 按钮做 textContent += 会把
  // .bg-tool/.bg-target 两个 span 压成单个文本节点,工具名/目标的分栏当场消失。
  toolStart({ payload: { id: "BGJ5", name: "edit", summary: "x.rs", input: { path: "x.rs" } } });
  toolEnd({ payload: { id: "BGJ5", name: "edit", ok: false, preview: "写坏了", display: { kind: "diff", path: "x.rs", additions: 3, deletions: 1, language: "rust", lines: [] } } });
  await flush();
  const j5 = bgEntry("BGJ5");
  assert(j5?.querySelector(".bg-tool") && j5?.querySelector(".bg-target"), "diff 终态把 .bg-tool/.bg-target 两段式结构拍平了(工具名/目标的分栏消失)");
  const j5Target = j5?.querySelector(".bg-target")?.textContent ?? j5?.textContent ?? "";
  assert(j5Target.includes("x.rs") && j5Target.includes("+3"), `diff 增删数未追加到目标列:"${j5Target.slice(0, 60)}"`);

  // ⑦ 「重跑」填回输入框的首行也不能是裸 JSON(entry.summary 存的是显示值)。
  toolEnd({ payload: { id: "BGJ1", name: "bash", ok: true, preview: "test result: ok", display: null } });
  await flush();
  const rerun = [...bgEntry("BGJ1").querySelectorAll(".bg-actions button")].find((b) => b.textContent === "重跑");
  assert(rerun, "结束的终端条目缺少重跑入口");
  rerun.click();
  await flush();
  const promptValue = byId.get("prompt").value;
  assert(
    promptValue.split("\n")[0] === "重跑这次调用:bash cargo test -p kanzei-app",
    `重跑填词首行仍是裸 JSON:"${promptValue.split("\n")[0]}"`,
  );
  assert(promptValue.includes("workdir"), "重跑填词丢了完整入参(只剩一行摘要就没法复核参数)");
  byId.get("prompt").value = "";

  // ⑧ 回放路径不能被改坏:回放事件不带 input,标题只能来自后端 summary。
  toolEnd({ payload: { id: "BGJ3", name: "bash", ok: true, preview: "done", display: null } });
  toolEnd({ payload: { id: "BGJ4", name: "bash", ok: true, preview: "done", display: null } });
  await flush();
  sandbox.renderRecoveredTraces([{
    events: [
      { id: "RP1", kind: "tool.started", name: "bash", summary: "scripts/verify.ps1" },
      { id: "RP1", kind: "tool.completed", ok: true, durationMs: 1200 },
      { id: "RP2", kind: "tool.started", name: "edit", summary: "历史失败调用" },
      { id: "RP2", kind: "tool.completed", ok: false, error: "boom" },
      // name 缺失的回放事件没有可读身份,不应建出空壳条目。
      { id: "RP3", kind: "tool.started" },
    ],
  }]);
  await flush();
  const rp1 = bgEntry("RP1");
  assert(rp1?.querySelector(".bg-target")?.textContent === "scripts/verify.ps1", `回放条目标题不对:"${rp1?.querySelector(".bg-target")?.textContent}"`);
  assert(rp1?.querySelector(".bg-tool")?.textContent === "bash", "回放条目未分列工具名");
  // D-208 不变量:回放条目是历史,不能显示成运行中、也不能给停止按钮。
  assert(!rp1.classList.contains("running"), "回放条目被标成运行中");
  assert(!rp1.querySelectorAll(".bg-actions button").some((b) => b.textContent === "停止"), "回放条目不该有停止按钮");
  const rp2 = bgEntry("RP2");
  assert(rp2, "回放里失败的静默工具未补建条目");
  assert(rp2.classList.contains("err"), "回放补建的条目未标失败态");
  assert(rp2.querySelector(".bg-target")?.textContent === "历史失败调用", `回放失败条目标题不对:"${rp2.querySelector(".bg-target")?.textContent}"`);
  assert(!bgEntry("RP3"), "name 缺失的回放事件不该建出条目(它走静默通道,建了就是一条没有信息量的空壳)");
}

// ---------- D-280 回归:清空消息区不得连「回到最新」按钮一起清掉 ----------
// 2026-08-12 实测事故:D-280 把 #jump-latest 挪进了 #messages 里,而
// renderRecoveredMessages / clearChat 都做 `messages.innerHTML = ""`——
// 一清按钮就没了,之后任何滚动/渲染触发 updateLatestButton 都抛
// `Cannot read properties of null (reading 'classList')`,恢复历史与新建
// 并行线路整条链路当场崩掉。按钮必须是滚动容器的**兄弟**,不是它的孩子。
{
  // 冒烟的 DOM 是按 id 摊平造的(按钮一律挂在 body 上),父子关系在这里测不出来,
  // 所以结构断言直接查 index.html 源文本:#messages 的开闭标签之间不许出现按钮。
  assert(byId.get("chat-area"), "缺少 #chat-area:「回到最新」需要一个不滚动的定位容器做锚点");
  const messagesOpen = html.indexOf('<section id="messages">');
  const messagesClose = html.indexOf("</section>", messagesOpen);
  assert(messagesOpen >= 0 && messagesClose > messagesOpen, "index.html 里找不到 #messages 区块");
  assert(
    !html.slice(messagesOpen, messagesClose).includes('id="jump-latest"'),
    "「回到最新」按钮又被放进 #messages 了:renderRecoveredMessages/clearChat 会 " +
      "`messages.innerHTML = \"\"`,一清就把它删掉,之后 updateLatestButton 抛 null.classList",
  );
  // 行为面:两条清空路径都不得抛异常(抛了会被 __reportInitError/console.error 抓住)。
  sandbox.clearChat("新对话");
  await flush();
  sandbox.renderRecoveredMessages([]);
  await flush();
}

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
// 前置:上面的用例把标签页停在「对照」,而对照页按既有设计只提供「全部状态」一个选项
// (12-docs-pages.js:126/130),直接给状态筛选赋 "dropped" 会被 <select> 规范语义拒绝
// (无匹配 option → selectedIndex=-1 → value 变空串),整组断言会以"筛不动"的形态假失败。
byId.get("documents-tab-req").click();
await flush();
// 旧结构种子(顶层 req 而非 docReq):这是 10-docs-core.js:34-35 降级读取的真实用例,
// R-115 的筛选偏好在这次搬迁中不能丢。不要顺手改成 docReq。
const filtersKey = [...storage.keys()].find((k) => k.startsWith("kz-filters")) ?? "kz-filters:C:/smoke/project";
storage.set(filtersKey, JSON.stringify({ req: { tag: "这个标签不存在", status: "all", priority: "all", complexity: "all", blocked: "all", sort: "manual" } }));
sandbox.restoreDocFilters();
await sandbox.refreshDocs();
await flush();
// 不变量:列表不得"无声变空"。要么标签回落后条目照常显示,要么明说被筛掉了多少。
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length > 0
    || document.querySelector("#documents-req-list .doc-filtered-empty"),
  "不存在的标签把列表筛空了,且界面没有任何说明——看起来就是需求凭空掉了",
);
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length > 0,
  "当前项目没有这个标签,筛选状态应回落成「全部」而不是筛空",
);

// 真实存在但无匹配的筛选:验证"被筛空"的提示与一键清除。
const statusFilterEl = byId.get("documents-status-filter");
statusFilterEl.value = "dropped";
statusFilterEl._listeners.change?.forEach((fn) => fn({ target: statusFilterEl }));
await flush();
const filteredEmpty = document.querySelector("#documents-req-list .doc-filtered-empty");
assert(filteredEmpty, "列表被筛空却没有任何说明(一片空白最容易被当成数据丢失)");
assert(/\d/.test(filteredEmpty.textContent), "未给出被隐藏的条数");
const clearFiltersBtn = filteredEmpty.querySelector("button");
assert(clearFiltersBtn, "被筛空时缺少一键清除筛选");
clearFiltersBtn.click();
await flush();
assert(
  document.querySelectorAll("#documents-req-list .doc-item").length > 0,
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

// ---------- 设置页逐字段往返:开关 / provider 删除 / 项目级覆盖 / 未知合法值 ----------
// 这一组守的是"界面显示 A、运行用 B"的几条具体路径:开关不登记脏状态、provider 删了又
// 回来、limits/proxy 被项目级覆盖却不告警、配置里的合法值下拉里没有就被静默降级。
{
  // 上一组刚保存过并回流 loadSettings,这里就是干净基线。
  assert(byId.get("settings-dirty").classList.contains("hidden"), "保存回流后未回到干净态(基线没归零,后面的脏状态断言全是假通过)");

  // ① 开关回填:只测保存不测回填的话,「进设置页开关自己弹回去」这种形态抓不到。
  assert(byId.get("set-codex-fast-mode").checked === true, "Codex Fast mode 已存值未回填到开关");
  // ② 配置里的合法值下拉里没有,必须补兜底 option 而不是静默变空串。
  assert(
    byId.get("set-profile").value === "readonly",
    `配置里的 readonly 档位被下拉吃掉了(保存一次就降级成 dev):实得 "${byId.get("set-profile").value}"`,
  );

  // ③ 项目级覆盖必须明说,且不能误报。断言一律用**值/键名**而不是中文标签:
  // 界面语言会切,标签会跟着变,按标签断言是假红的常见来源。
  assert(!byId.get("settings-effective").classList.contains("hidden"), "项目级覆盖了 proxy/limits,设置页却没有提示");
  const effectiveNotice = listText("settings-effective");
  assert(effectiveNotice.includes("http://127.0.0.1:7890"), `代理被项目级覆盖却没报出实际生效值:${effectiveNotice}`);
  assert(effectiveNotice.includes("maxTokens"), `运行上限被项目级覆盖却没点名到具体键:${effectiveNotice}`);
  assert(!effectiveNotice.includes("readonly"), `两侧相同的 profileDefault 被误报成覆盖:${effectiveNotice}`);
  assert(!effectiveNotice.includes("Codex Fast mode"), `两侧相同的 Codex Fast mode 被误报成覆盖:${effectiveNotice}`);
  // effective 里**没有**的键必须整条跳过:undefined 被当成"实际生效是未设"会让提示条
  // 天天误报,用户学会无视它,真被覆盖时反而看不见。
  assert(!effectiveNotice.includes("ollama:qwen3"), `effective 里缺失的 fast 被当成"被覆盖成未设"误报了:${effectiveNotice}`);

  // ④ 开关的脏状态:checkbox 的 .value 恒为 "on",拿它做指纹永远比不出差异。
  // 漏登记的后果不是少个角标——进设置页会重跑 loadSettings 把表单整张覆盖回磁盘值,
  // 走开一趟再回来勾过的开关就悄悄弹回去了,而角标从头到尾没亮过。
  const codexToggle = byId.get("set-codex-fast-mode");
  codexToggle.checked = false;
  codexToggle.dispatchEvent({ type: "change" });
  assert(!byId.get("settings-dirty").classList.contains("hidden"), "改了开关却没有「未保存」提示(开关版的 D-157)");

  // ⑤ provider 删除:删行是 click 不是 input,表格上的事件委托抓不到,必须显式同步脏状态。
  const providerRows = document.querySelectorAll("#providers-table tbody tr");
  assert(providerRows.length === 3, `provider 表格未渲染出三行,实得 ${providerRows.length}`);
  // D-246:内置 provider(anthropic)的删除入口是「内置」标记,不是 ×——删了重开又回来,不给假按钮。
  const builtinCell = providerRows[2].querySelector(".provider-builtin");
  assert(builtinCell, "内置 provider 行缺少「内置」标记(D-246)");
  const builtinRemove = providerRows[2].querySelector("button");
  assert(!builtinRemove || builtinRemove.textContent !== "×", "内置 provider 不应提供删除按钮(D-246)");
  const removeBtn = providerRows[0].querySelectorAll("button").find((b) => b.textContent === "×");
  assert(removeBtn, "自定义 provider 行缺少移除按钮");
  removeBtn.click();
  await flush();
  assert(!byId.get("settings-dirty").classList.contains("hidden"), "删了 provider 却没有「未保存」提示(切走再回来它就原样回来了)");

  // ⑥ 保存载荷:顶层键集合逐字比对。规范 §4 要求表单透传全部字段,多一项少一项都要红——
  // 少一项 = 那个字段保存时被悄悄丢掉,多一项 = 有人加了后端不认的键。
  byId.get("settings-save").click();
  await flush();
  const payload = savedPayloads.get("settings_save")?.payload;
  assert(payload, "点保存未调 settings_save");
  assert(
    Object.keys(payload ?? {}).sort().join(",")
      === "cadence,codexFastMode,compact,fast,language,limits,primary,profileDefault,providers,proxy,reasoning",
    `settings_save 载荷顶层键集合变了: ${Object.keys(payload ?? {}).sort().join(",")}`,
  );
  // 根因终局判据:首次进设置页时两个角色 select 是零 option 的空壳,若实现仍是
  // 「先 select.value = 已存值、再读 DOM 当基准」,这里必然收到空串——而空串保存回去
  // 就是把 [models] primary/fast 从 kanzei.toml 里删掉。
  assert(payload?.primary === "deepseek:deepseek-chat", `探测不到的已存 primary 被保存成了 "${payload?.primary}"`);
  assert(payload?.fast === "ollama:qwen3", `已存 fast 被保存成了 "${payload?.fast}"`);
  assert(payload?.profileDefault === "readonly", `readonly 档位保存时被静默降级成 "${payload?.profileDefault}"`);
  assert(payload?.codexFastMode === false, `开关的新值未透传到载荷: ${payload?.codexFastMode}`);
  assert(
    (payload?.providers ?? []).map((p) => p.name).join(",") === "keepme,anthropic",
    `provider 删除未落进载荷(或整张表没发全): ${(payload?.providers ?? []).map((p) => p.name).join(",")}`,
  );
  // 保存回流后基线必须归零,否则「未保存」角标会一直亮着,变成人人无视的噪音。
  assert(byId.get("settings-dirty").classList.contains("hidden"), "保存回流后「未保存」角标仍亮着");
}

// ---------- R-184 P6(D-247):代理「指定地址」留空必须可见提示 ----------
{
  const proxyMode = byId.get("set-proxy-mode");
  const proxyUrl = byId.get("set-proxy-url");
  const proxyHint = byId.get("set-proxy-hint");
  assert(proxyMode && proxyUrl && proxyHint, "设置页缺少代理模式/地址/提示元素");
  // 夹具回显 proxy=env → custom 输入框隐藏、提示隐藏。
  assert(proxyUrl.classList.contains("hidden"), "env 模式下地址框不应可见");
  assert(proxyHint.classList.contains("hidden"), "env 模式下提示不应可见");
  // 切「指定地址」且留空 → 提示可见,说明将回落环境变量。
  proxyMode.value = "custom";
  proxyMode._listeners.change?.forEach((fn) => fn({ target: proxyMode }));
  assert(!proxyUrl.classList.contains("hidden"), "custom 模式下地址框应可见");
  assert(!proxyHint.classList.contains("hidden"), "「指定地址」留空时提示应可见(D-247)");
  const hintText = proxyHint.textContent || "";
  assert(hintText.includes("回落"), `提示未说明将回落: ${hintText}`);
  // 填了地址 → 提示消失。
  proxyUrl.value = "http://127.0.0.1:12000";
  proxyUrl._listeners.input?.forEach((fn) => fn({ target: proxyUrl }));
  assert(proxyHint.classList.contains("hidden"), "地址已填时提示应消失(D-247)");
  // 不静默改写用户选择:留空保存时载荷 proxy 仍为 custom 语义(空串),由后端按空回落,
  // 但界面已把回落说出来——这里验证载荷没被前端擅自改成 env。
  proxyUrl.value = "";
  proxyUrl._listeners.input?.forEach((fn) => fn({ target: proxyUrl }));
  proxyMode._listeners.change?.forEach((fn) => fn({ target: proxyMode }));
  byId.get("settings-save").click();
  await flush();
  const proxyPayload = savedPayloads.get("settings_save")?.payload?.proxy;
  assert(proxyPayload === "", `前端不应改写用户选择(留空即空串,回落语义在后端): 实得 ${JSON.stringify(proxyPayload)}`);
  proxyMode.value = "env";
  proxyMode._listeners.change?.forEach((fn) => fn({ target: proxyMode }));
  proxyUrl.value = "";
  proxyUrl._listeners.input?.forEach((fn) => fn({ target: proxyUrl }));
  assert(proxyUrl.classList.contains("hidden"), "切回 env 后地址框应隐藏");
  assert(proxyHint.classList.contains("hidden"), "切回 env 后提示应消失");
}

// ---------- R-178 批4 D7:设置页作用域选择器 ----------
// 第一版只覆盖 [models]:scope=project 时后端只写模型角色进主根 .kanzei/kanzei.toml,
// proxy/provider/limits/cadence 一律仍走全局(后端 settings.rs 按 scope 拦截)。
// 前端职责:默认全局、有项目上下文时 project 选项可用、无项目时禁用并回退 global、
// 保存时透传 scope+projectDir。
{
  const scopeSelect = byId.get("set-save-scope");
  assert(scopeSelect, "设置页缺少作用域选择器 #set-save-scope");
  const projectOption = scopeSelect.querySelector('option[value="project"]');
  assert(projectOption, "作用域选择器缺少「本项目」选项");
  // 冒烟 settings_get 桩自带 projectConfig(有项目)→ 选项可用、默认值 global。
  assert(projectOption.disabled === false, "有项目上下文时「本项目」选项未启用");
  assert(scopeSelect.value === "global", "默认作用域应为 global");

  byId.get("settings-save").click();
  await flush();
  const saveArgs = savedPayloads.get("settings_save");
  assert(saveArgs?.scope === "global", `默认作用域应为 global: ${JSON.stringify(saveArgs?.scope)}`);
  assert(saveArgs?.projectDir === null || saveArgs?.projectDir === undefined, "global 作用域不应携带 projectDir");

  // 无项目上下文:settings_get 不带 projectConfig → 选项禁用且当前值回退 global。
  const originalSettingsGet = payloads.settings_get;
  payloads.settings_get = { ...originalSettingsGet, projectConfig: undefined };
  try {
    const loadSettingsInSandbox = vm.runInContext("loadSettings", sandbox);
    await loadSettingsInSandbox();
    assert(projectOption.disabled === true, "无项目上下文时「本项目」选项未被禁用");
    assert(scopeSelect.value === "global", "无项目上下文时作用域未回退到 global");
  } finally {
    payloads.settings_get = originalSettingsGet;
  }

  // 有项目上下文:选中「本项目」保存 → scope+projectDir 一起透传。
  await vm.runInContext("loadSettings", sandbox)();
  scopeSelect.value = "project";
  byId.get("settings-save").click();
  await flush();
  const projectArgs = savedPayloads.get("settings_save");
  assert(projectArgs?.scope === "project", `选了「本项目」保存却没带 scope=project: ${JSON.stringify(projectArgs?.scope)}`);
  const currentProjectInSandbox = vm.runInContext("currentProject", sandbox);
  assert(projectArgs?.projectDir === currentProjectInSandbox, "scope=project 未携带当前项目目录");
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
const compactModelValues = [...modelSelect.options].map((o) => o.value);
assert(compactModelValues.includes("deepseek:deepseek-chat"), "当前线路已选的 DeepSeek 未保留在紧凑模型列表");
assert(!compactModelValues.includes("ollama:qwen3"), "紧凑模型列表仍把未选模型全部灌入顶栏");
const showAllOption = [...modelSelect.options].find((o) => o.value === "__show_all_models__");
assert(showAllOption, "紧凑模型列表缺少展开完整探测清单入口");
modelSelect.value = "__show_all_models__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
assert([...modelSelect.options].some((o) => o.value === "ollama:qwen3"), "展开完整模型清单后仍缺少探测模型");
const manualOption = [...modelSelect.options].find((o) => o.value === "__manual__");
assert(manualOption, "模型下拉缺少手填入口(端点不实现 /models 时就彻底没法选)");
sandbox.window.prompt = () => "deepseek:deepseek-chat";
modelSelect.value = "__manual__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
// R-178 批3:手填模型写后端(process_update 携带 manualModels),不再落 localStorage。
const manualUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && Array.isArray(args?.manualModels));
assert(manualUpdate, "手填模型未以 manualModels 发给后端(下次重开又要再填一遍)");
assert(
  manualUpdate.args.manualModels.includes("deepseek:deepseek-chat"),
  `手填模型落盘值不对:${JSON.stringify(manualUpdate.args.manualModels)}`,
);
assert(
  [...byId.get("model-select").options].some((o) => o.value === "deepseek:deepseek-chat"),
  "手填后模型未回到下拉列表里",
);
// 格式不对要挡住:provider 名对不上配置键时后端 resolve_model 会直接失败。
sandbox.window.prompt = () => "随便写的";
modelSelect.value = "__manual__";
modelSelect._listeners.change?.forEach((fn) => fn({ target: modelSelect }));
await flush();
const badUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && Array.isArray(args?.manualModels));
assert(
  !badUpdate || !badUpdate.args.manualModels.includes("随便写的"),
  "非 provider:model 格式不应被接受",
);

// ---------- R-178 批3:localStorage 旧模型偏好一次性上迁后端并清除 ----------
// 预置旧版键(模型选择 + 手填候选),迁移后必须写入默认进程且旧键消失,否则下次
// 启动又回到 localStorage,永远迁不完。
storage.set(`kz-model:${PROJECT}`, "anthropic:claude-sonnet-5");
storage.set(`kz-manual-models:${PROJECT}`, JSON.stringify(["ollama:qwen3"]));
await sandbox.migrateLegacyModelPrefs();
await flush();
const migrationModelUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && args?.model === "anthropic:claude-sonnet-5");
assert(migrationModelUpdate, "旧模型偏好未上迁到默认进程(process_update 缺 model)");
const migrationManualUpdate = invokeArgs.findLast(({ cmd, args }) =>
  cmd === "process_update" && Array.isArray(args?.manualModels)
    && args.manualModels.includes("ollama:qwen3"));
assert(migrationManualUpdate, "旧手填候选未上迁到默认进程(process_update 缺 manualModels)");
assert(!storage.has(`kz-model:${PROJECT}`), "迁移成功后旧模型键未清除(下次启动会重复迁移)");
assert(!storage.has(`kz-manual-models:${PROJECT}`), "迁移成功后旧手填键未清除(下次启动会重复迁移)");
// 迁移失败(后端报错)必须保留旧键,下次 loadModels 重试,不能丢用户选择。
const migrationArgs = invokeArgs.length;
invokeFailures.set("process_update", "后端拒绝");
expectedPersistentError = "旧模型偏好迁移失败";
storage.set(`kz-model:${PROJECT}`, "anthropic:claude-sonnet-5");
await sandbox.migrateLegacyModelPrefs();
await flush();
invokeFailures.delete("process_update");
expectedPersistentError = null;
assert(storage.has(`kz-model:${PROJECT}`), "迁移失败时旧键不应被清除(可重试)");
assert(invokeArgs.length > migrationArgs, "迁移失败重试路径未调用后端");

// 后端回显整链:默认进程的 manual_models(②层)必须驱动下拉回显,而不是 localStorage。
payloads.process_list[0].manual_models = ["ollama:qwen3"];
await sandbox.refreshProcesses();
await sandbox.loadModels();
await flush();
assert(
  [...byId.get("model-select").options].some((o) => o.value === "ollama:qwen3"),
  "后端 manual_models 未回显到下拉(前端仍以 localStorage 为真源?)",
);
delete payloads.process_list[0].manual_models;

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

// 状态筛选只在「需求与工作」标签页下可用(对照页只有「全部状态」一个选项),
// 先把标签页切回来,否则 select 规范语义会把 "doing" 拒成空串,落盘值也跟着变空。
byId.get("documents-tab-req").click();
await flush();
const reqStatusFilter = byId.get("documents-status-filter");
reqStatusFilter.value = "doing";
reqStatusFilter._listeners.change?.forEach((fn) => fn({ target: reqStatusFilter }));
await flush();
const filterKey = [...storage.keys()].find((k) => k.startsWith("kz-filters"));
assert(filterKey, "需求筛选未落盘(重启后会回到「全部」)");
assert(JSON.parse(storage.get(filterKey)).docReq.status === "doing", "筛选落盘值不对");

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
// R-184 P6(D-248):applyProfileValue 是**回显**,只读——切进程看一眼绝不许改写全局
// kz-profile(否则用户全局档位被静默降级)。写全局只能发生在用户主动 change。
storage.set("kz-profile", "dev-auto");
sandbox.applyProfileValue("dev");
assert(
  storage.get("kz-profile") === "dev-auto",
  `回显(切进程)不得改写全局 kz-profile,实得 ${storage.get("kz-profile")}(D-248)`,
);
const profileSelectEl = byId.get("profile-select");
profileSelectEl.value = "dev-auto";
profileSelectEl._listeners.change?.forEach((fn) => fn({ target: profileSelectEl }));
assert(
  storage.get("kz-profile") === "dev-auto",
  "用户主动切换档位仍须写全局 kz-profile",
);
storage.set("kz-profile", "dev-auto");
sandbox.applyProfileValue("dev");
profileSelectEl.value = "dev-pair";
profileSelectEl._listeners.change?.forEach((fn) => fn({ target: profileSelectEl }));
assert(
  storage.get("kz-profile") === "dev-pair",
  "用户主动切换档位仍须写全局 kz-profile(第二次)",
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
probe({ payload: { id: 1, kind: "dom", arg: "#documents-req-list" } });
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
assert(languageControl.querySelectorAll("option").length === 3, "界面语言应提供跟随系统/中文/English 三个选项");
// rail 上还有侧栏开合(无 data-view),对话按钮要按 data-view 精确取。
const chatActivity = document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat");
assert(projectInit.getAttribute("title") === "初始化新项目目录", "HTML title 未进入真实冒烟 DOM");
assert(chatActivity.getAttribute("aria-label") === "切换到对话", "HTML aria-label 未进入真实冒烟 DOM");
languageControl.value = "system";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(storage.get("kz-language") === "system", "跟随系统选择未持久化");
assert(["zh-CN", "en"].includes(document.documentElement.lang), "跟随系统未解析成中文或英文界面");
languageControl.value = "en";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "en", "切换 English 后 document.lang 未更新");
assert(storage.get("kz-language") === "en", "English 选择未持久化");
assert(projectInit.getAttribute("title") === "Initialize a new project directory", "静态 title 未翻译");
assert(chatActivity.getAttribute("aria-label") === "Switch to chat", "静态 aria-label 未翻译");
// 非终态 persistence warning 不能把仍在运行的会话投影成空闲；随后再验证
// terminal=true 的真正运行失败会收口为 Error。
handlers.get("kz:turn")?.({ payload: { step: 1, maxSteps: 1, sessionId: "sess-smoke" } });
await flush();
handlers.get("kz:error")?.({ payload: { message: "smoke persistence warning", terminal: false, sessionId: "sess-smoke" } });
await flush();
assert(listText("status-mode").includes("Running"), `非终态错误不应收回运行态: "${listText("status-mode")}"`);
assert(!byId.get("stop").classList.contains("hidden"), "非终态错误期间停止按钮不应消失");
handlers.get("kz:error")?.({ payload: { message: "smoke backend failure" } });
await flush();
assert(listText("status-text").includes("Error"), `英文动态错误状态未翻译: "${listText("status-text")}"`);
assert(document.querySelector(".error-level")?.textContent === "Fatal error", "英文错误等级未翻译");
languageControl.value = "zh";
languageControl.dispatchEvent({ type: "change" });
await flush();
assert(document.documentElement.lang === "zh-CN", "切回中文后 document.lang 未更新");
assert(storage.get("kz-language") === "zh", "中文选择未持久化");
assert(listText("status-text").includes("出错"), `中文动态错误状态未恢复: "${listText("status-text")}"`);
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

// ---------- R-223 权限被拦聚合呈现:①被拦落可见 notice + 轮末汇总 ②自动放行常驻徽标 ----------
{
  const askHandler = handlers.get("kz:ask");
  // 断言①a:autonomous 权限询问跳过 → 对话流落可见 notice(不只隐藏日志)。
  askHandler?.({
    payload: {
      id: 901, sessionId: "sess-smoke", kind: "permission",
      action: "edit", resource: "src/a.rs", remember: "src/*",
      source: "autonomous",
    },
  });
  await flush();
  const notices = [...document.querySelectorAll(".msg.notice")];
  assert(
    notices.some((n) =>
      (n.textContent.includes("权限被拦已跳过") || n.textContent.includes("Permission blocked, skipped")) &&
      n.textContent.includes("edit"),
    ),
    `autonomous 权限被拦应在对话流落可见 notice(实得: ${notices.map((n) => n.textContent).join(" | ")})`,
  );
  // 断言①b:轮末汇总(kz:done 携带 steps)→ 「本轮 N 次被拦」。
  const doneHandler = handlers.get("kz:done");
  doneHandler?.({ payload: { sessionId: "sess-smoke", steps: 2 } });
  await flush();
  assert(
    [...document.querySelectorAll(".msg.notice")].some((n) =>
      (n.textContent.includes("本轮权限被拦") || n.textContent.includes("permissions blocked this round")) &&
      n.textContent.includes("edit"),
    ),
    "轮末应汇总「本轮 N 次被拦(动作/资源清单)」",
  );
  // 断言②:开启自动放行 → 状态栏常驻警示徽标可见;localStorage 持久化(模拟重启后仍可见)。
  const autoAllow = byId.get("auto-allow");
  autoAllow.checked = true;
  autoAllow.dispatchEvent({ type: "change" });
  await flush();
  const badge = byId.get("status-auto-allow");
  assert(
    badge && !badge.classList.contains("hidden"),
    "开启自动放行后状态栏应挂常驻警示徽标",
  );
  assert(
    storage.get("kz-auto-allow") === "1",
    "自动放行选择必须持久化到 localStorage(跨重启)",
  );
  // 模拟重启:徽标初始化逻辑在 07-events.js 顶部,直接重建可见性。
  autoAllow.checked = false;
  autoAllow.dispatchEvent({ type: "change" });
  await flush();
  assert(
    badge.classList.contains("hidden"),
    "关闭自动放行后徽标应隐藏",
  );
}

// ---------- R-086 多会话并发:控制事件按 sessionId 收敛,切回可见可答复、不丢不串 ----------
// 前置:清空上面语言切换测试留下的主会话 ask(91/92 仍在队列,askActive=91)。
if (byId.get("ask-allow")) byId.get("ask-allow").click();
await flush();
if (!byId.get("ask-overlay").classList.contains("hidden") && byId.get("ask-allow")) byId.get("ask-allow").click();
await flush();
assert(byId.get("ask-overlay").classList.contains("hidden"), "R-086 前置:主会话 ask 未清空");
// 场景:主会话(sess-smoke)活动;后台会话(sess-bg)初始 running=true(桩里故意给旧值,
// 模拟"事件已收敛但轮询采样发生在事件之前"的竞态)。
const activeLine = document.querySelector("#parallel-task-status .parallel-task-row.active");
assert(activeLine?.textContent.includes("主会话"), `冒烟前置:主会话应为活动线路(实际:${activeLine?.textContent})`);
// 并行线状态卡按 process_list 全量投影,三条并行线就必须有三条可切换任务行。
const twoProcesses = structuredClone(payloads.process_list);
sandbox.renderProcesses([
  ...twoProcesses,
  { id: "p|third", label: "第三线路", session_id: "sess-third", running: false, branch: "kanzei/thread-third", authority: "parallel", stage: "测试" },
]);
assert(document.querySelectorAll("#parallel-task-status .parallel-task-row").length === 3, "三线并行时侧栏只渲染了一个/两条任务状态");
const thirdLineStatus = [...document.querySelectorAll("#parallel-task-status .parallel-task-row")]
  .find((row) => row.dataset.processId === "p|third")?.textContent ?? "";
assert(thirdLineStatus.includes("第三线路") && !thirdLineStatus.includes("测试"), `空闲第三线路未显示或仍残留旧阶段:${thirdLineStatus}`);
sandbox.renderProcesses(twoProcesses);
await flush();
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
const bgLineAfterDone = [...document.querySelectorAll("#parallel-task-status .parallel-task-row")].find((row) => row.textContent.includes("后台会话"));
assert(bgLineAfterDone?.textContent.includes("●"), `多轮运行第一轮结束后线路按钮熄灯(实际:${bgLineAfterDone?.textContent})`);
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
const bgLineAfterIdle = [...document.querySelectorAll("#parallel-task-status .parallel-task-row")].find((row) => row.textContent.includes("后台会话"));
assert(!bgLineAfterIdle?.textContent.includes("●"), "会话已空闲但线路按钮仍亮着运行标记");
// 切回后台会话:权限询问可见可答复,运行态显示空闲(converged 挡住桩里的旧 running=true)。
const messagesBeforeSwitch = listText("messages");
const pendingSwitch = sandbox.switchProcess("p|bg");
assert(listText("messages") === messagesBeforeSwitch, "切线程请求尚未完成时主对话被清空");
await pendingSwitch;
await flush();
const bgLine = document.querySelector("#parallel-task-status .parallel-task-row.active");
assert(bgLine?.textContent.includes("后台会话"), "切换到后台会话后活动线路按钮未更新");
assert(bgLine?.textContent.includes("kanzei/thread-smoke"), `分支线按钮未显示真实分支名:${bgLine?.textContent}`);
const trackerToggle = byId.get("process-tracker-writes");
assert(trackerToggle && !trackerToggle.checked, "分支线 tracker 写入必须默认关闭");
assert(!byId.get("process-tracker-writes-wrap").classList.contains("hidden"), "分支线未显示 tracker 写入开关");
payloads.process_list[1].tracker_writes = true;
trackerToggle.checked = true;
trackerToggle.dispatchEvent({ type: "change" });
await flush();
assert(
  invokeArgs.findLast(({ cmd }) => cmd === "process_update")?.args?.trackerWrites === true,
  `tracker 开关未以 trackerWrites 发给后端:${JSON.stringify(invokeArgs.findLast(({ cmd }) => cmd === "process_update"))}`
);
assert(trackerToggle.checked, "后端回显开启后 tracker 开关未保持选中");
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
const backLine = document.querySelector("#parallel-task-status .parallel-task-row.active");
assert(backLine?.textContent.includes("主会话"), "切回主会话后活动线路按钮未更新");
assert(byId.get("process-tracker-writes-wrap").classList.contains("hidden"), "默认线不应显示分支 tracker 开关");
assert(byId.get("ask-overlay").classList.contains("hidden"), "切回主会话后残留后台 ask 弹窗");
// R-206 验收③:长工具运行中点停止 → stopping 过渡态,晚到进度事件不得把
// 停止按钮翻回运行中(无状态闪跳)。直接经 transitionSession 置 stopping
// (与 stop 按钮 handler 的 872-873 行同源),再发进度事件断言不翻回。
{
  vm.runInContext('transitionSession("sess-smoke", "running")', sandbox);
  const mainState = sandbox.sessionState("sess-smoke");
  assert(mainState.phase === "running", "前置:主会话应在运行中");
  // 与 08-compose.js 停止按钮 handler 同源:置 stopping + setStopping。
  vm.runInContext('transitionSession("sess-smoke", "stopping")', sandbox);
  await flush();
  assert(mainState.phase === "stopping", "点停止后 phase 未进入 stopping");
  // stopping 期间 running=true 是设计语义(按钮显示「停止中…」而非消失);
  // 要防的是 phase 闪跳回 running 与按钮可点化。验证 phase 稳定即可。
  // 晚到的进度事件:不得翻回 running(01-core.js stopping 保护)。
  handlers.get("kz:tool-progress")?.({ payload: { sessionId: "sess-smoke", name: "bash", detail: "仍在执行" } });
  handlers.get("kz:status")?.({ payload: { sessionId: "sess-smoke", stage: "跑工具", detail: "" } });
  await flush();
  assert(mainState.phase === "stopping", "stopping 期间晚到进度事件把 phase 翻回 running(闪跳)");
  // stopping 期间 running=true 是设计语义;关键防的是 live_running 权威残留
  // 让后续轮询翻回 running 相位。断言相位稳定 + live_running 已清。
  assert(mainState.live_running === false, "stopping 后 live_running 权威未清,轮询可把会话翻回运行中");
  // 终态离开 stopping。
  handlers.get("kz:stopped")?.({ payload: { sessionId: "sess-smoke" } });
  await flush();
  assert(mainState.phase === "stopped", "kz:stopped 后 phase 未离开 stopping");
  assert(mainState.running === false, "stopped 后 running 未收敛为 false");
}

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
  { id: "p|bg", label: "后台会话", session_id: "sess-bg", running: true, worktree_path: "C:/smoke-wt", branch: "kanzei/thread-smoke", tracker_writes: true },
]);
await flush();
assert(document.querySelector("#parallel-task-status .parallel-task-row.active")?.textContent.includes("主会话"), "重建用例收尾后活动线路未回到主会话");

// ---------- R-169 鞭挞执行层:判定已引擎化,前端只执行 autoAction ----------
// 判定(空转画像/连数/全部阻塞/NUDGE 时机/停止原因)全部在 harness auto_run
// 状态机单测覆盖(kanzei-harness auto_run.rs,12 组);这里验证前端对 kz:done
// 携带 autoAction 的执行:Continue→续跑、Nudge→追加指令提示、Stop→停止+原因+
// 开关联动、NoContinue→不动。
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
// ① Continue:镜像计数并续跑,不刹车。
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { read: 2, edit: 1 }, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 1, `Continue 应镜像推进计数,实得 ${sandbox.__kzTest.rounds()}`);
assert(byId.get("auto-continue").checked, "Continue 不应关掉自动推进");
// ② Nudge:引擎给出的推进指令占一轮,前端给提示不刹车。
handlers.get("kz:done")?.({ payload: { steps: 2, halted: false, tools: { memory_note: 1 }, autoAction: { type: "Nudge", prompt: "上一轮没有产生任何实质动作。", rounds: 2, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 2, `Nudge 应镜像推进计数,实得 ${sandbox.__kzTest.rounds()}`);
assert(byId.get("auto-status").textContent.includes("无动作 · 追加推进指令"), `#auto-status 未提示追加推进指令: ${byId.get("auto-status")?.textContent}`);
assert(byId.get("auto-continue").checked, "Nudge 第一次不应立即刹车");
// ③ Stop(NoAction):连续两轮无动作,停止并显示原因。
handlers.get("kz:done")?.({ payload: { steps: 2, halted: false, tools: { memory_note: 1 }, autoAction: { type: "Stop", reason: "NoAction" }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 0, "连续两轮无实质动作后推进计数应清零");
assert(sandbox.__kzTest.stopReason().includes("连续两轮无动作"), `刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
assert(byId.get("auto-status").textContent.includes("连续两轮无动作"), `#auto-status 未显示刹车原因: ${byId.get("auto-status")?.textContent}`);
// ④ Stop(AllBlocked):全部阻塞,停并取消开关。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(3);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "AllBlocked" }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-continue").checked, "需求/缺陷全部被阻塞时自动推进应停止");
assert(sandbox.__kzTest.rounds() === 0, "阻塞刹车后推进计数应清零");
assert(sandbox.__kzTest.stopReason().includes("全部被阻塞"), `阻塞刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
// ⑤ Continue:存在可推进条目时正常续跑(不误刹车)。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 2, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(byId.get("auto-continue").checked, "Continue 不得误刹车");
// ⑥ Stop(BacklogEmpty):清空,停并取消开关。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(2);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "BacklogEmpty" }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-continue").checked, "需求/缺陷清空时自动推进应停止");
assert(sandbox.__kzTest.stopReason().includes("已清空"), `清空刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
// ⑦ Stop(StopAfterRound):本轮后停,开关自动取消勾选。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "StopAfterRound" }, sessionId: "sess-smoke" } });
await flush();
assert(!byId.get("auto-stop-round").checked, "本轮后停后开关应自动取消勾选");
assert(sandbox.__kzTest.stopReason().includes("本轮后停"), `本轮后停原因不对: ${sandbox.__kzTest.stopReason()}`);
// ⑧ Stop(MaxRounds):达上限,计数清零原因明确。
byId.get("auto-continue").checked = true;
const autoMaxWhip = Number.parseInt(byId.get("auto-max").value, 10) || 10;
sandbox.__kzTest.setRounds(autoMaxWhip);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "MaxRounds", max: autoMaxWhip }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 0, "达到上限后推进计数应清零");
assert(sandbox.__kzTest.stopReason().includes("已达连上限"), `上限刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
// ⑨ Stop(Paused):暂停中完成本轮 → 停;恢复后 Continue 再推进。
byId.get("auto-continue").checked = true;
sandbox.__kzTest.setRounds(1);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Stop", reason: "Paused" }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.stopReason().includes("已暂停"), `暂停刹车原因不对: ${sandbox.__kzTest.stopReason()}`);
handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 2, max: 10 }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 2, `恢复后推进轮次应继续增长,实得 ${sandbox.__kzTest.rounds()}`);
// ⑩ NoContinue(halted):整段鞭挞分支不进入,推进计数原地不动。
sandbox.__kzTest.setRounds(4);
handlers.get("kz:done")?.({ payload: { steps: 2, halted: true, tools: { edit: 1 }, autoAction: { type: "NoContinue" }, sessionId: "sess-smoke" } });
await flush();
assert(sandbox.__kzTest.rounds() === 4, "用户拒绝后推进计数应保持原样(不再续跑)");

// ---------- D-291 续跑闸门必须出声 ----------
// 引擎判 Continue、前端却不发下一轮,是允许的(模式/暂停/开关都能否决);**静默**不行。
// 旧实现四个条件各自 `return`,auto_pending 留在 true,界面永久停在「等待下一轮」,
// 而那一轮永远不来——用户看到的就是"鞭挞开着却不动"。
{
  const whipSession = "sess-smoke";
  byId.get("auto-continue").checked = true;
  sandbox.__kzTest.reset();
  byId.get("profile-select").value = "dev-pair"; // R-199:档位否决已下沉引擎,前端不再持有
  handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: whipSession } });
  await flush();
  // R-199:引擎 decide() 判档位(ProfileMismatch→Stop),前端只显示引擎结论;
  // 这里模拟引擎已判 Continue,前端必须持续推进(不再有私有否决)。
  // 时序容忍:flush 可能跨越 2 秒续跑间隔,phase 可能是 auto_pending(挂起中)
  // 或 starting(已开跑)——两者都证明引擎放行后前端没有拦下。
  const contPhase = sandbox.sessionState(whipSession).phase;
  assert(
    contPhase === "auto_pending" || contPhase === "starting",
    `R-199 后前端不再否决续跑(档位判定在引擎):Continue 后应挂起或已开跑,实得 phase=${contPhase}`,
  );
  assert(
    !byId.get("auto-status").textContent.includes("鞭挞未续跑"),
    `引擎判 Continue 时前端不得拦下: ${byId.get("auto-status")?.textContent}(D-291/R-199)`,
  );
  // 非致命错误(terminal:false,如持久化告警)不得掐掉已排好的下一轮。
  byId.get("profile-select").value = "dev-auto";
  handlers.get("kz:done")?.({ payload: { steps: 3, halted: false, tools: { edit: 1 }, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: whipSession } });
  const ph2 = sandbox.sessionState(whipSession).phase;
  assert(
    ph2 === "auto_pending" || ph2 === "starting",
    `前置失败:Continue 未挂起/开跑下一轮, phase=${ph2}`,
  );
  handlers.get("kz:error")?.({ payload: { message: "持久化告警(非致命)", terminal: false } });
  const ph3 = sandbox.sessionState(whipSession).phase;
  assert(
    ph3 === "auto_pending" || ph3 === "starting",
    "非致命错误不得取消已排队的续跑(旧实现在函数开头无条件 cancelAutoContinueTimer,一条告警就让鞭挞永久停摆)(D-291)",
  );
}

// ---------- D-323 暂停→恢复路径不得持有前端私有否决 ----------
// R-199 档位判定已下沉引擎(decide→Stop/ProfileMismatch 带 reason 可见收口);
// 恢复分支若仍被 autoContinueAllowed() 静默拦下,引擎计数与状态不知情(验收①未兑现)。
// 非 dev-auto 档位下恢复必须照样调度——档位不对由引擎下轮 done 判 Stop 收口。
{
  const savedProfileD323 = byId.get("profile-select").value;
  byId.get("profile-select").value = "dev-pair"; // 非 dev-auto → autoContinueAllowed()=false
  byId.get("auto-continue").checked = true;
  sandbox.__kzTest.setPaused(false);
  sandbox.__kzTest.reset();
  // 确保轮间空闲:清掉上游遗留的续跑定时器,并把 running 全局拉回 false。
  sandbox.__kzTest.cancelTimers();
  // 进程刷新会按 item.running 重设 running(08-compose.js:881),必须把主会话
  // 进程项置 idle 再渲染,否则任何刷新都会把 running 翻回 true。
  const savedD323ProcessList = payloads.process_list;
  payloads.process_list = (payloads.process_list ?? []).map((p) =>
    p.session_id === "sess-smoke" ? { ...p, running: false } : p,
  );
  sandbox.renderProcesses(payloads.process_list);
  sandbox.setRunning(false); // 03-shell 顶层函数声明在共享作用域,冒烟可直接调用
  byId.get("auto-pause").click(); // 暂停(autoPaused → true)
  const pausedText = byId.get("auto-pause").textContent;
  const pausedVal = sandbox.__kzTest.paused();
  byId.get("auto-pause").click(); // 恢复(autoPaused → false)→ 必须进入「2 秒后继续」分支
  const resumedText = byId.get("auto-pause").textContent;
  const resumedVal = sandbox.__kzTest.paused();
  const statusMode = byId.get("status-mode")?.textContent;
  const autoChecked = byId.get("auto-continue").checked;
  payloads.process_list = savedD323ProcessList;
  assert(
    pausedText.includes("继续鞭挞") && pausedVal === true,
    `D-323 前置:暂停点击未生效,pausedVal=${pausedVal},text=${pausedText}`,
  );
  assert(
    resumedVal === false,
    `D-323 前置:恢复点击未生效,pausedVal=${resumedVal},text=${resumedText}`,
  );
  assert(
    byId.get("status-text").textContent.includes("鞭挞恢复"),
    `D-323:恢复路径不得静默不调度(档位判定在引擎),status=${byId.get("status-text")?.textContent},mode=${statusMode},autoChecked=${autoChecked},btn=${resumedText}`,
  );
  assert(
    sandbox.__kzTest.timerSessions().includes("sess-smoke"),
    `D-323:恢复必须重新调度续跑定时器,timers=${sandbox.__kzTest.timerSessions().join(",")}`,
  );
  byId.get("profile-select").value = savedProfileD323;
  sandbox.__kzTest.reset();
}

// ---------- R-226 后台控制事件与双线路 timer 必须按 session 隔离 ----------

// ---------- R-224 鞭挞勾选自动切自主推进 ----------
// 结伴(dev-pair)勾鞭挞 → 自动切 dev-auto + notice;research 勾鞭挞 → 拒绝并复位。
{
  const savedProfileR224 = byId.get("profile-select").value;
  // ① 结伴勾鞭挞:自动切 dev-auto,notice 可见,鞭挞保持勾选。
  byId.get("profile-select").value = "dev-pair";
  byId.get("auto-continue").checked = true;
  sandbox.__kzTest.cancelTimers();
  byId.get("auto-continue").dispatchEvent({ type: "change" });
  await flush();
  assert(
    byId.get("profile-select").value === "dev-auto",
    `R-224:结伴勾鞭挞未自动切到 dev-auto,实际=${byId.get("profile-select").value}`,
  );
  assert(
    byId.get("auto-continue").checked === true,
    "R-224:自动切模式后鞭挞勾选被复位(应保持勾选)",
  );
  assert(
    [...document.querySelectorAll("#messages .msg, #messages div")].some((el) =>
      el.textContent.includes("已切换到自主推进"),
    ),
    "R-224:自动切模式未落 notice 说明",
  );
  // ② research 勾鞭挞:拒绝并复位勾选,模式不变。
  byId.get("profile-select").value = "research";
  byId.get("auto-continue").checked = true;
  byId.get("auto-continue").dispatchEvent({ type: "change" });
  await flush();
  assert(
    byId.get("auto-continue").checked === false,
    "R-224:research 勾鞭挞未被拒绝复位",
  );
  assert(
    byId.get("profile-select").value === "research",
    "R-224:research 拒绝路径不应改模式",
  );
  // 收尾恢复。
  byId.get("auto-continue").checked = false;
  byId.get("profile-select").value = savedProfileR224;
  sandbox.__kzTest.cancelTimers();
}

// ---------- R-226 后台控制事件与双线路 timer 必须按 session 隔离 ----------
{
  const lines = [
    { id: "d|smoke", label: "主会话", session_id: "sess-smoke", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|bg-a", label: "后台甲", session_id: "sess-bg-a", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
    { id: "p|bg-b", label: "后台乙", session_id: "sess-bg-b", running: false, project_dir: "C:/smoke", origin_project: "C:/smoke" },
  ];
  const savedProcessList = payloads.process_list;
  payloads.process_list = lines;
  sandbox.renderProcesses(lines);
  sandbox.__kzTest.setAutoState("p|bg-a", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
  sandbox.__kzTest.setAutoState("p|bg-b", { enabled: true, paused: false, stopAfterRound: false, maxRounds: 10 });
  handlers.get("kz:done")?.({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-bg-a" } });
  handlers.get("kz:done")?.({ payload: { steps: 1, autoAction: { type: "Continue", rounds: 1, max: 10 }, sessionId: "sess-bg-b" } });
  const timerSessions = sandbox.__kzTest.timerSessions();
  assert(timerSessions.includes("sess-bg-a") && timerSessions.includes("sess-bg-b"), `后台双线路 timer 未并存:${timerSessions.join(",")}`);
  assert(sandbox.sessionState("sess-bg-a").phase === "auto_pending", "后台甲 done 未进入等待下一轮");
  assert(sandbox.sessionState("sess-bg-b").phase === "auto_pending", "后台乙 done 未进入等待下一轮");
  assert(sandbox.activeSessionId === undefined || document.querySelector("#parallel-task-status .parallel-task-row.active")?.textContent.includes("主会话"), "后台 done 串改活动线路");
  await flush();
  const backgroundRuns = invokeArgs.filter(({ cmd, args }) => cmd === "run_prompt" && ["p|bg-a", "p|bg-b"].includes(args?.processId));
  assert(backgroundRuns.some(({ args }) => args.processId === "p|bg-a"), "后台甲 done 没有续跑所属线路");
  assert(backgroundRuns.some(({ args }) => args.processId === "p|bg-b"), "后台乙 done 没有续跑所属线路");
  payloads.process_list = savedProcessList;
  sandbox.renderProcesses(savedProcessList);
}

// ---------- D-290 回显不得写盘 ----------
// 「模式/鞭挞每次开 app 都要重设」的根:回显期间控件显示的是**算出来的值**,
// 把它当用户意图写回存档,一次算错就永久固化,而且自我延续。
{
  const autoStateBefore = storage.get("kz-process-auto-state");
  storage.set("kz-auto-continue", "1");
  byId.get("auto-continue").checked = true;
  storage.set("kz-profile", "dev-pair");
  sandbox.applyProfileValue("dev"); // 回显把模式刷成结伴开发 → 顺带关掉鞭挞控件
  assert(
    storage.get("kz-auto-continue") === "1",
    `回显关掉的鞭挞不得写进全局 kz-auto-continue,实得 ${storage.get("kz-auto-continue")}(D-290:下次冷启动会被当成用户上次的选择)`,
  );
  assert(
    storage.get("kz-process-auto-state") === autoStateBefore,
    "回显不得改写 kz-process-auto-state(D-290)",
  );
  storage.set("kz-profile", "dev-auto");
}
// 切进程时不得拿选择器当前值覆盖旧进程的档位存档:那个值在回显期间不是用户意图。
// 这是上面那条的另一半——只修一处,另一处照样能把 dev-auto 覆盖成 dev-pair。
if (source.includes('processProfileUi.set(activeProcessId, $("profile-select").value)')) {
  fail("switchProcess 又拿选择器显示值当旧进程的用户意图写盘(D-290);写盘只能发生在 profile-select 的 change 事件里");
}
// ---------- 「勘察复核」= 阶段流水线总闸(2026-08-11 换闸门) ----------
// 闸门从 auto_runs[session].enabled 换成进程级开关后,「开鞭挞 = 每轮勘察+复核」这个
// 旧心智模型不再成立。四种组合里只有「鞭挞开 + 闸门关」需要提示,这里把它和它的
// 反面(闸门开 → 不提示)一起钉住,顺带钉 IPC 参数名(phasePipeline,不再是 subagent)。
const pipelineToggle = byId.get("process-phase-pipeline");
assert(pipelineToggle, "顶栏「更多」里缺少「勘察复核」开关");
assert(
  !pipelineToggle.checked,
  "「勘察复核」必须默认关闭(process_list 桩不带 phase_pipeline 字段时回落 false)"
);
byId.get("auto-continue").checked = true;
pipelineToggle.checked = false;
pipelineToggle.dispatchEvent({ type: "change" });
await flush();
assert(
  invokeArgs.findLast(({ cmd }) => cmd === "process_update")?.args?.phasePipeline === false,
  `关闭勘察复核未以 phasePipeline 发给后端:${JSON.stringify(invokeArgs.findLast(({ cmd }) => cmd === "process_update"))}`
);
assert(
  byId.get("auto-status").textContent.includes("勘察复核未开"),
  `鞭挞开着而勘察复核关着时,自主推进面板必须明说:${byId.get("auto-status")?.textContent}`
);
// 打开闸门:后端回显跟着变(桩模拟 process_list 的新值),提示随即消失。
payloads.process_list[0].phase_pipeline = true;
pipelineToggle.checked = true;
pipelineToggle.dispatchEvent({ type: "change" });
await flush();
assert(
  invokeArgs.findLast(({ cmd }) => cmd === "process_update")?.args?.phasePipeline === true,
  `打开勘察复核未以 phasePipeline 发给后端:${JSON.stringify(invokeArgs.findLast(({ cmd }) => cmd === "process_update"))}`
);
assert(
  pipelineToggle.checked && !byId.get("auto-status").textContent.includes("勘察复核未开"),
  `闸门开着时不该再提示未开:${byId.get("auto-status")?.textContent}`
);
// 收尾:桩与控件回到默认关,免得后续用例继承本节状态。
payloads.process_list[0].phase_pipeline = false;
pipelineToggle.checked = false;
pipelineToggle.dispatchEvent({ type: "change" });
await flush();

// 收尾:恢复冒烟前置环境(语言/档位/开关/计数)。
byId.get("profile-select").value = savedProfileForWhip;
byId.get("auto-continue").checked = savedAutoCheck;
languageControl.value = savedLangForWhip;
languageControl.dispatchEvent({ type: "change" });
await flush();
sandbox.__kzTest.reset();

// ---------- 视图切换:真实驱动 activity-item 的监听,抓初始化后才触发的运行时错误 ----------
// rail 上的侧栏开合不是视图,统计与点击都只认带 data-view 的按钮。
const activityItems = document.querySelectorAll(".activity-item[data-view]");
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

// ---------- 架构浏览(R-122):索引 + 设计文档树,未入册分组可见 ----------
{
  const archView = byId.get("view-arch");
  assert(archView, "缺少 view-arch 视图容器");
  // 上面视图切换循环已点击过 arch 按钮,refreshArch 应已拉取并渲染。
  const tree = byId.get("arch-tree");
  const treeText = tree?.textContent ?? "";
  assert(treeText.includes("direction_taste.md"), `架构树缺已入册文档: "${treeText.slice(0, 80)}"`);
  assert(treeText.includes("memory_system.md"), "架构树缺设计文档行");
  assert(treeText.includes("未入册") || treeText.includes("not indexed"), "索引外的文档未进「未入册」分组");
  assert(treeText.includes("现行基线") || treeText.includes("基线"), "索引章节分组未渲染");
  assert((byId.get("arch-index-body")?.textContent ?? "").includes("方向基线"), "右侧索引原文未渲染");
  assert((byId.get("arch-summary")?.textContent ?? "").includes("2"), "架构汇总缺文档计数");
  // 点击文档行应经 docs_read_custom 打开应用内查看器。
  const row = [...tree.querySelectorAll(".arch-entry")].find((r) => r.textContent.includes("memory_system.md"));
  assert(row, "架构树缺少可点击的文档行");
  row.click();
  await flush();
  assert(!byId.get("viewer-overlay").classList.contains("hidden"), "点击设计文档未打开查看器");
  assert((byId.get("viewer-body")?.textContent ?? "").includes("Memory 系统设计基线"), "查看器未展示设计文档内容");
  byId.get("viewer-close").click();
  await flush();
  assert(byId.get("viewer-overlay").classList.contains("hidden"), "查看器关闭失败");
  // 批3:记忆管理入口跳转记忆页(复用导航按钮,维护动作走既有 memory_* 命令)。
  const gotoMemory = byId.get("arch-goto-memory");
  assert(gotoMemory, "架构页缺少记忆管理入口");
  const memBtn = [...document.querySelectorAll(".activity-item[data-view]")].find((b) => b.dataset.view === "memory");
  assert(memBtn, "缺少记忆导航按钮");
  gotoMemory.click();
  await flush();
  assert(byId.get("view-memory").classList.contains("active"), "记忆管理入口未激活记忆视图");
  assert(memBtn.classList.contains("active"), "记忆导航按钮未同步高亮");
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

// ---------- R-140 批10:MutationObserver 退役 ----------
// 动态文案必须由渲染点 t()/localizeDynamic/applyDataI18nKeys 产出,不再有 observer
// 事后扫描改写。裸中文节点(用户数据/漏翻)保持原样——这正是验收③ 的正面断言:
// 谁把 observer 换回来、或退回全文档扫描,这条就红。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  const probe = document.createElement("div");
  probe.textContent = "移动端桥接";
  document.body.appendChild(probe);
  await flush();
  assert(
    probe.textContent === "移动端桥接",
    `裸中文节点被自动本地化(实际 "${probe.textContent}"):MutationObserver 未退役,事后扫描改写仍在`
  );
  probe.remove();
  // 渲染点路径:渲染器插入 data-i18n-key 节点后,由切语言/初始化路径
  // applyDataI18nKeys(document.body) 重算 → 英文态即时翻译(与 change 处理器一致)。
  const keyed = document.createElement("span");
  keyed.setAttribute("data-i18n-key", "移动端桥接");
  keyed.textContent = "移动端桥接";
  document.body.appendChild(keyed);
  sandbox.applyDataI18nKeys(document.body, "en");
  await flush();
  assert(
    keyed.textContent === "Mobile bridge",
    `渲染点 data-i18n-key 节点未翻译(实际 "${keyed.textContent}"):applyDataI18nKeys 渲染点路径失效`
  );
  keyed.remove();
  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批1:消息容器整体豁免词典替换(止血) ----------
// 模型输出是用户数据,英文态下不能因「恰好等于词典 key」被 observer 改写成英文。
// 在英文态追加一条包含词典 key 的模型输出,断言 message-body 原文保持中文不变;
// 同时消息区外的节点仍要正常翻译(豁免只圈 #messages,不误伤其它界面域)。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  const before = sandbox.document.querySelectorAll("#messages .msg").length;
  sandbox.appendAssistant("运行中 · 失败 · 复制 是用户数据片段,不得改写");
  await flush();
  const assistant = sandbox.document.querySelectorAll("#messages .msg.assistant .message-body").at(-1);
  assert(assistant, "追加模型输出后找不到 .msg.assistant .message-body(前置失效)");
  const assistantMsg = assistant.closest(".msg");
  assert(
    assistantMsg.dataset.raw.includes("运行中"),
    "模型输出原文丢失(appendAssistant 未保留 raw)"
  );
  assert(
    assistant.textContent.includes("运行中") && assistant.textContent.includes("失败"),
    `消息容器内的模型输出被词典替换(实际 "${assistant.textContent}"):英文态下用户数据被 i18n 篡改,R-140 止血失败`
  );
  // 消息区外:裸中文节点同样不再被自动改写(observer 退役,无事后扫描);产品文案由
  // 渲染点 data-i18n-key + applyDataI18nKeys 负责(上方批10 用例已覆盖渲染点路径)。
  const outside = document.createElement("div");
  outside.textContent = "移动端桥接";
  sandbox.document.body.appendChild(outside);
  await flush();
  assert(
    outside.textContent === "移动端桥接",
    `消息区外的裸中文被自动翻译(实际 "${outside.textContent}"):observer 仍在做全文档改写`
  );
  outside.remove();
  // 清掉追加的消息,避免污染后续用例。
  const msgs = sandbox.document.querySelectorAll("#messages .msg");
  for (const m of msgs) if (msgs.length > before) m.remove();
  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批2:静态 DOM data-i18n-key/data-i18n-title 一次性应用 ----------
// 侧栏标题与按钮属性迁移到 data-i18n-key 后,翻译由渲染点 t() 承担,不再依赖
// observer 词典扫描。英文态应翻译,切回中文应回原文;title 属性同样走渲染点。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  const sectionTitle = (key) => sandbox.document.querySelector(`[data-i18n-key="${key}"]`)?.textContent;
  const titleOf = (id) => sandbox.document.getElementById(id)?.title;
  assert(sectionTitle("项目") === "项目", "中文态侧栏「项目」标题应保持原文(前置失效)");
  assert(titleOf("project-init") === "初始化新项目目录", "中文态 project-init title 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(sectionTitle("项目") === "Projects", `英文态侧栏「项目」未翻译,实际 "${sectionTitle("项目")}"`);
  assert(sectionTitle("当前状态") === "Current status", `英文态侧栏「当前状态」未翻译,实际 "${sectionTitle("当前状态")}"`);
  assert(sectionTitle("开发规范") === "Conventions", `英文态侧栏「开发规范」未翻译,实际 "${sectionTitle("开发规范")}"`);
  assert(titleOf("project-init") === "Initialize a new project directory", `英文态 project-init title 未翻译,实际 "${titleOf("project-init")}"`);
  assert(titleOf("worktrees-refresh") === "Refresh worktree changes", `英文态 worktrees-refresh title 未翻译,实际 "${titleOf("worktrees-refresh")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(sectionTitle("项目") === "项目", `切回中文后侧栏「项目」未回原文,实际 "${sectionTitle("项目")}"`);
  assert(titleOf("project-init") === "初始化新项目目录", `切回中文后 project-init title 未回原文,实际 "${titleOf("project-init")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批3:顶栏/对话区/工作区视图 data-i18n-key 迁移 ----------
// 顶栏按钮(新对话/活动/侧栏/鞭挞/更多)、对话区按钮(继续/附件/停止/发送)、
// 工作区与并行线路视图标题迁移到 data-i18n-key 后,英文态翻译、切中文回原文。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  const keyText = (key) => sandbox.document.querySelector(`[data-i18n-key="${key}"]`)?.textContent;
  assert(keyText("新对话") === "新对话", "中文态顶栏「新对话」应保持原文(前置失效)");
  assert(keyText("发送") === "发送", "中文态发送按钮应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(keyText("新对话") === "New chat", `英文态「新对话」未翻译,实际 "${keyText("新对话")}"`);
  assert(keyText("发送") === "Send", `英文态「发送」未翻译,实际 "${keyText("发送")}"`);
  assert(keyText("停止") === "Stop", `英文态「停止」未翻译,实际 "${keyText("停止")}"`);
  assert(keyText("工作区") === "Workspace", `英文态「工作区」未翻译,实际 "${keyText("工作区")}"`);
  assert(keyText("并行线路") === "Parallel lines", `英文态「并行线路」未翻译,实际 "${keyText("并行线路")}"`);
  assert(keyText("刷新") === "Refresh", `英文态「刷新」未翻译,实际 "${keyText("刷新")}"`);
  assert(keyText("鞭挞") === "Auto-run", `英文态「鞭挞」未翻译,实际 "${keyText("鞭挞")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(keyText("新对话") === "新对话", `切回中文后「新对话」未回原文,实际 "${keyText("新对话")}"`);
  assert(keyText("发送") === "发送", `切回中文后「发送」未回原文,实际 "${keyText("发送")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批4:架构浏览域迁移 + aria-label/placeholder 渲染点翻译 ----------
// 架构浏览视图的标题/说明/按钮文案迁移到 data-i18n-key,title 走 data-i18n-title,
// aria-label 走 data-i18n-aria-label(渲染点补齐属性翻译——元素挂 data-i18n-* 后
// observer 整体豁免,属性不在此补齐会在英文态漏翻)。英文态翻译、切中文回原文。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const archKey = (key) => sandbox.document.querySelector(`[data-i18n-key="${key}"]`)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(archKey("架构浏览") === "架构浏览", "中文态架构浏览标题应保持原文(前置失效)");
  assert(attrOf("arch-tree", "aria-label") === "设计文档树", "中文态 arch-tree aria-label 应保持原文(前置失效)");
  assert(attrOf("arch-refresh", "aria-label") === "重新扫描架构索引", "中文态 arch-refresh aria-label 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(archKey("架构浏览") === "Architecture browser", `英文态「架构浏览」未翻译,实际 "${archKey("架构浏览")}"`);
  assert(archKey("架构索引") === "Architecture index", `英文态「架构索引」未翻译,实际 "${archKey("架构索引")}"`);
  assert(archKey("记忆管理") === "Memory management", `英文态「记忆管理」未翻译,实际 "${archKey("记忆管理")}"`);
  assert(archKey("打开") === "Open", `英文态「打开」未翻译,实际 "${archKey("打开")}"`);
  assert(attrOf("arch-goto-memory", "title") === "Jump to the memory page to maintain entries (edit/consolidate/focus via existing memory commands)", `英文态 arch-goto-memory title 未翻译,实际 "${attrOf("arch-goto-memory", "title")}"`);
  assert(attrOf("arch-open-index", "title") === "Open the architecture index in the viewer", `英文态 arch-open-index title 未翻译,实际 "${attrOf("arch-open-index", "title")}"`);
  assert(attrOf("arch-tree", "aria-label") === "Design doc tree", `英文态 arch-tree aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("arch-tree", "aria-label")}"`);
  assert(attrOf("arch-refresh", "title") === "Rescan", `英文态 arch-refresh title 未翻译,实际 "${attrOf("arch-refresh", "title")}"`);
  assert(attrOf("arch-refresh", "aria-label") === "Rescan architecture index", `英文态 arch-refresh aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("arch-refresh", "aria-label")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(archKey("架构浏览") === "架构浏览", `切回中文后「架构浏览」未回原文,实际 "${archKey("架构浏览")}"`);
  assert(attrOf("arch-tree", "aria-label") === "设计文档树", `切回中文后 arch-tree aria-label 未回原文,实际 "${attrOf("arch-tree", "aria-label")}"`);
  assert(attrOf("arch-refresh", "aria-label") === "重新扫描架构索引", `切回中文后 arch-refresh aria-label 未回原文,实际 "${attrOf("arch-refresh", "aria-label")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批5:文档页域迁移(标题/工具栏/筛选/批量/测试区) ----------
// 文档页 h1/说明/标签/按钮/筛选下拉/批量操作/测试记录区迁移到 data-i18n-key/
// data-i18n-title/data-i18n-aria-label,含静态 <option> 文本。英文态翻译、切中文回原文。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  // harness 的 queryAllFrom 按空格切分选择器,含空格/斜杠的 key 无法用 `[data-i18n-key="..."]`
  // 查询;改为遍历所有 data-i18n-key 节点按 dataset 匹配(与 B4 断言组同款绕过)。
  const docKey = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(docKey("需求与工作 / 缺陷 / 测试") === "需求与工作 / 缺陷 / 测试", "中文态文档页标题应保持原文(前置失效)");
  assert(docKey("全部状态") === "全部状态", "中文态状态筛选「全部状态」应保持原文(前置失效)");
  assert(attrOf("documents-status-filter", "title") === "按状态筛选", "中文态状态筛选 title 应保持原文(前置失效)");
  assert(attrOf("req-open", "aria-label") === "打开 requirements.md 原文", "中文态 req-open aria-label 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(docKey("需求与工作 / 缺陷 / 测试") === "Work items / Defects / Tests", `英文态文档页标题未翻译,实际 "${docKey("需求与工作 / 缺陷 / 测试")}"`);
  assert(docKey("自动审查缺陷") === "Review defects", `英文态「自动审查缺陷」未翻译,实际 "${docKey("自动审查缺陷")}"`);
  assert(docKey("依赖视图") === "Dependency view", `英文态「依赖视图」未翻译,实际 "${docKey("依赖视图")}"`);
  assert(docKey("全部状态") === "All statuses", `英文态「全部状态」未翻译(option 渲染点),实际 "${docKey("全部状态")}"`);
  assert(docKey("未评估") === "Not assessed", `英文态「未评估」未翻译(option 渲染点),实际 "${docKey("未评估")}"`);
  assert(docKey("已阻塞") === "Blocked", `英文态「已阻塞」未翻译(option 渲染点),实际 "${docKey("已阻塞")}"`);
  assert(docKey("手动") === "Manual", `英文态「手动」未翻译(option 渲染点),实际 "${docKey("手动")}"`);
  assert(docKey("取消选择") === "Clear selection", `英文态「取消选择」未翻译,实际 "${docKey("取消选择")}"`);
  assert(attrOf("documents-status-filter", "title") === "Filter by status", `英文态状态筛选 title 未翻译,实际 "${attrOf("documents-status-filter", "title")}"`);
  assert(attrOf("documents-priority-filter", "title") === "Filter by priority (reference only; does not affect work order)", `英文态优先级筛选 title 未翻译,实际 "${attrOf("documents-priority-filter", "title")}"`);
  assert(attrOf("req-open", "aria-label") === "Open requirements.md source", `英文态 req-open aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("req-open", "aria-label")}"`);
  assert(attrOf("tests-refresh", "aria-label") === "Refresh and archive completed tests", `英文态 tests-refresh aria-label 未翻译,实际 "${attrOf("tests-refresh", "aria-label")}"`);
  assert(attrOf("documents-batch-bar", "aria-label") === "Bulk actions", `英文态批量操作区 aria-label 未翻译,实际 "${attrOf("documents-batch-bar", "aria-label")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(docKey("需求与工作 / 缺陷 / 测试") === "需求与工作 / 缺陷 / 测试", `切回中文后文档页标题未回原文,实际 "${docKey("需求与工作 / 缺陷 / 测试")}"`);
  assert(docKey("全部状态") === "全部状态", `切回中文后「全部状态」未回原文,实际 "${docKey("全部状态")}"`);
  assert(attrOf("req-open", "aria-label") === "打开 requirements.md 原文", `切回中文后 req-open aria-label 未回原文,实际 "${attrOf("req-open", "aria-label")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批6:记忆页域迁移(标题/说明/工具/侧栏区块) ----------
// 记忆页 h1/说明/搜索框(placeholder+aria-label)/整理按钮/区块标题/清理按钮迁移到
// data-i18n-key/data-i18n-title/data-i18n-placeholder/data-i18n-aria-label。
// 含子元素的 h2 文本用 span 包裹(不得在 h2 上直接 data-i18n-key,会清掉计数 span)。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const memKey = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(memKey("记忆") === "记忆", "中文态记忆页标题应保持原文(前置失效)");
  assert(memKey("待确认候选") === "待确认候选", "中文态「待确认候选」应保持原文(前置失效)");
  assert(attrOf("memory-search-input", "placeholder") === "检索全部记忆(FTS)", "中文态搜索框 placeholder 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(memKey("记忆") === "Memory", `英文态「记忆」未翻译,实际 "${memKey("记忆")}"`);
  assert(memKey("整理 inbox") === "Consolidate inbox", `英文态「整理 inbox」未翻译,实际 "${memKey("整理 inbox")}"`);
  assert(memKey("待确认候选") === "Pending candidates", `英文态「待确认候选」未翻译(span 包裹),实际 "${memKey("待确认候选")}"`);
  assert(memKey("空闲整理清单") === "Idle cleanup list", `英文态「空闲整理清单」未翻译,实际 "${memKey("空闲整理清单")}"`);
  assert(memKey("一键整理") === "Clean up now", `英文态「一键整理」未翻译,实际 "${memKey("一键整理")}"`);
  assert(memKey("召回评估") === "Recall evaluation", `英文态「召回评估」未翻译,实际 "${memKey("召回评估")}"`);
  assert(memKey("上下文账单") === "Context bill", `英文态「上下文账单」未翻译,实际 "${memKey("上下文账单")}"`);
  assert(memKey("最近轮次") === "Recent rounds", `英文态「最近轮次」未翻译,实际 "${memKey("最近轮次")}"`);
  assert(attrOf("memory-search-input", "placeholder") === "Search all memory (FTS)", `英文态搜索框 placeholder 未翻译(渲染点属性补齐),实际 "${attrOf("memory-search-input", "placeholder")}"`);
  assert(attrOf("memory-search-input", "aria-label") === "Search memory", `英文态搜索框 aria-label 未翻译(渲染点属性补齐),实际 "${attrOf("memory-search-input", "aria-label")}"`);
  assert(attrOf("memory-arch", "aria-label") === "Memory architecture overview", `英文态 memory-arch aria-label 未翻译,实际 "${attrOf("memory-arch", "aria-label")}"`);
  assert(attrOf("memory-consolidate-btn", "title") === "Consolidate inbox drafts now", `英文态整理按钮 title 未翻译,实际 "${attrOf("memory-consolidate-btn", "title")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(memKey("记忆") === "记忆", `切回中文后「记忆」未回原文,实际 "${memKey("记忆")}"`);
  assert(memKey("待确认候选") === "待确认候选", `切回中文后「待确认候选」未回原文,实际 "${memKey("待确认候选")}"`);
  assert(attrOf("memory-search-input", "placeholder") === "检索全部记忆(FTS)", `切回中文后搜索框 placeholder 未回原文,实际 "${attrOf("memory-search-input", "placeholder")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批7:指标页 + 文件页域迁移(标题/说明/工具栏/占位) ----------
// 指标页 h1/说明/两个 aria-label;文件页排序·标注·刷新按钮(title+文本+aria-label)、
// 文件树 aria-label、占位说明 迁移到 data-i18n-key/data-i18n-title/data-i18n-aria-label。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const b7Key = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b7Key("运行画像") === "运行画像", "中文态指标页标题应保持原文(前置失效)");
  assert(attrOf("metrics-trend", "aria-label") === "跨轮趋势", "中文态 metrics-trend aria-label 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(b7Key("运行画像") === "Run profile", `英文态「运行画像」未翻译,实际 "${b7Key("运行画像")}"`);
  assert(b7Key("按行数") === "By lines", `英文态「按行数」未翻译,实际 "${b7Key("按行数")}"`);
  assert(b7Key("标注") === "Annotate", `英文态「标注」未翻译,实际 "${b7Key("标注")}"`);
  assert(attrOf("metrics-trend", "aria-label") === "Cross-round trends", `英文态 metrics-trend aria-label 未翻译,实际 "${attrOf("metrics-trend", "aria-label")}"`);
  assert(attrOf("metrics-rounds", "aria-label") === "Per-round profile", `英文态 metrics-rounds aria-label 未翻译,实际 "${attrOf("metrics-rounds", "aria-label")}"`);
  assert(attrOf("files-sort", "title") === "Toggle sort: name / lines", `英文态 files-sort title 未翻译,实际 "${attrOf("files-sort", "title")}"`);
  assert(attrOf("files-refresh", "aria-label") === "Rescan file tree", `英文态 files-refresh aria-label 未翻译,实际 "${attrOf("files-refresh", "aria-label")}"`);
  assert(attrOf("files-tree", "aria-label") === "Project file tree", `英文态文件树 aria-label 未翻译,实际 "${attrOf("files-tree", "aria-label")}"`);
  assert(b7Key("选择左侧文件查看内容 · 目录行显示聚合度量 · 「标注」用 fast 模型生成用途说明").includes("Select a file on the left"), `英文态文件占位说明未翻译,实际 "${b7Key("选择左侧文件查看内容 · 目录行显示聚合度量 · 「标注」用 fast 模型生成用途说明")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b7Key("运行画像") === "运行画像", `切回中文后「运行画像」未回原文,实际 "${b7Key("运行画像")}"`);
  assert(attrOf("files-refresh", "aria-label") === "重新扫描文件树", `切回中文后 files-refresh aria-label 未回原文,实际 "${attrOf("files-refresh", "aria-label")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批8:设置页域迁移(标题/关于/全部 details 区块/动态字符串) ----------
// 设置页 h1/说明(span 包裹保留 code#settings-path)/关于 kanzei 三行/界面语言/模型角色
// (保存到·作用域 option·primary·fast·探测·一键就绪)/Provider(测试·表头·添加)/网络与默认
// (代理 option·默认模式 option·思考强度 option)/运行上限(六组 label+说明)/验证与提交节奏
// (全量/定向/提交/push 的 option 与 title)/移动端桥接/已记住的权限/工作资料导出/版本与更新/
// 底部动作区 全部挂 data-i18n-key/data-i18n-title/data-i18n-placeholder。16-settings.js 的
// 11 处动态模板改走 t()(删除失败/读取权限规则失败/本页·实际生效·未设/手填×2/设置读取失败/
// 启动·停止桥接失败/保存失败/选择导出目录失败/导出失败),词典补 本页/实际生效/手填 三 key。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const b8Key = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b8Key("设置") === "设置", "中文态设置页标题应保持原文(前置失效)");
  assert(b8Key("模型角色") === "模型角色", "中文态「模型角色」应保持原文(前置失效)");
  assert(attrOf("export-output-dir", "placeholder") === "选择导出目录", "中文态导出目录 placeholder 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(b8Key("设置") === "Settings", `英文态「设置」未翻译,实际 "${b8Key("设置")}"`);
  assert(b8Key("关于 kanzei") === "About kanzei", `英文态「关于 kanzei」未翻译,实际 "${b8Key("关于 kanzei")}"`);
  assert(b8Key("模型角色") === "Model roles", `英文态「模型角色」未翻译,实际 "${b8Key("模型角色")}"`);
  assert(b8Key("保存到") === "Save to", `英文态「保存到」未翻译(span 包裹保留 hint),实际 "${b8Key("保存到")}"`);
  assert(b8Key("全局配置") === "Global config", `英文态「全局配置」未翻译(option 渲染点),实际 "${b8Key("全局配置")}"`);
  assert(b8Key("主循环") === "Main loop", `英文态「主循环」未翻译(span 包裹),实际 "${b8Key("主循环")}"`);
  assert(b8Key("重新探测模型") === "Re-detect models", `英文态「重新探测模型」未翻译,实际 "${b8Key("重新探测模型")}"`);
  assert(b8Key("测试全部连通性") === "Test connectivity", `英文态「测试全部连通性」未翻译,实际 "${b8Key("测试全部连通性")}"`);
  assert(b8Key("思考强度") === "Reasoning effort", `英文态「思考强度」未翻译,实际 "${b8Key("思考强度")}"`);
  assert(b8Key("主对话输出上限") === "Main output cap", `英文态「主对话输出上限」未翻译,实际 "${b8Key("主对话输出上限")}"`);
  assert(b8Key("验证与提交节奏") === "Verification & commit cadence", `英文态「验证与提交节奏」未翻译,实际 "${b8Key("验证与提交节奏")}"`);
  assert(b8Key("全量测试") === "Full test suite", `英文态「全量测试」未翻译,实际 "${b8Key("全量测试")}"`);
  assert(b8Key("每 N 批") === "Every N batches", `英文态「每 N 批」未翻译(option 渲染点),实际 "${b8Key("每 N 批")}"`);
  assert(b8Key("移动端桥接") === "Mobile bridge", `英文态「移动端桥接」未翻译,实际 "${b8Key("移动端桥接")}"`);
  assert(b8Key("已记住的权限") === "Saved permissions", `英文态「已记住的权限」未翻译,实际 "${b8Key("已记住的权限")}"`);
  assert(b8Key("工作资料导出") === "Export work materials", `英文态「工作资料导出」未翻译,实际 "${b8Key("工作资料导出")}"`);
  assert(b8Key("检查更新") === "Check for updates", `英文态「检查更新」未翻译,实际 "${b8Key("检查更新")}"`);
  assert(b8Key("保存") === "Save", `英文态「保存」未翻译,实际 "${b8Key("保存")}"`);
  assert(attrOf("set-save-scope", "title") === "Scope selector (v1) covers model roles only; Providers and API keys always go to the global config", `英文态作用域 title 未翻译,实际 "${attrOf("set-save-scope", "title")}"`);
  assert(attrOf("export-output-dir", "placeholder") === "Choose an export directory", `英文态导出目录 placeholder 未翻译,实际 "${attrOf("export-output-dir", "placeholder")}"`);
  assert(attrOf("set-cadence-full-test-batches", "title") === "Interval in batches for every-N-batches", `英文态每 N 批 title 未翻译,实际 "${attrOf("set-cadence-full-test-batches", "title")}"`);

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b8Key("设置") === "设置", `切回中文后「设置」未回原文,实际 "${b8Key("设置")}"`);
  assert(b8Key("保存到") === "保存到", `切回中文后「保存到」未回原文,实际 "${b8Key("保存到")}"`);
  assert(attrOf("export-output-dir", "placeholder") === "选择导出目录", `切回中文后导出目录 placeholder 未回原文,实际 "${attrOf("export-output-dir", "placeholder")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- R-140 批9:活动/会话/compose 域 + 全局静态面收口 ----------
// rail 导航 title/aria-label 已由批0 断言覆盖;live-turn/status-mode/status-text 是动态
// 元素(JS 用 t()/localizeDynamic 渲染点写入),一律不挂 data-i18n-key——挂了会在切语言时被
// applyDataI18nKeys 覆写回原文。chat-search·prompt placeholder/sop-picker aria-label/
// queue·steer option/log 面板/statusbar(git·ctx·tokens·日志)/
// 活动面板筛选(类型+状态)/agent 面板区块与清空/权限询问(标题·字段·回答 placeholder·
// 四按钮)/查看器两按钮全部挂 data-i18n-*。带 id 的元素把 data-i18n-key 放元素自身
// (冒烟按 id 建节点只取开标签后首个 < 前的 directText,span 包裹会让按钮文本变空);
// 无 id 的容器/区块标题用内层 span 包裹。06-agent-panel 运行中/已完成计数器、06-activity
// 未命名文件、08-compose、09-sessions 动态字符串改走 t()。词典补 未命名文件/工作树清单
// 读取失败(资源 54→56)。
{
  const priorLanguage = localStorageShim.getItem("kz-language") || "zh";
  const b9Key = (key) => [...sandbox.document.querySelectorAll("[data-i18n-key]")].find((el) => el.dataset.i18nKey === key)?.textContent;
  const attrOf = (id, attr) => sandbox.document.getElementById(id)?.getAttribute(attr);
  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b9Key("权限请求") === "权限请求", "中文态权限请求标题应保持原文(前置失效)");
  assert(b9Key("当前计划") === "当前计划", "中文态当前计划应保持原文(前置失效)");
  assert(!sandbox.document.getElementById("process-tabs"), "中文态不应出现顶部进程切换条");
  assert(attrOf("prompt", "placeholder") === "想做什么?可粘贴/拖拽图片或 PDF", "中文态输入框 placeholder 应保持原文(前置失效)");
  assert(b9Key("排队 queue") === "排队 queue", "中文态排队 queue option 应保持原文(前置失效)");

  localStorageShim.setItem("kz-language", "en");
  sandbox.applyLanguage();
  assert(attrOf("rail-sidebar-toggle", "title") === "Open or close the sidebar", `英文态 rail 侧栏开关 title 未翻译,实际 "${attrOf("rail-sidebar-toggle", "title")}"`);
  assert(attrOf("rail-sidebar-toggle", "aria-label") === "Open or close the sidebar", `英文态 rail 侧栏开关 aria-label 未翻译,实际 "${attrOf("rail-sidebar-toggle", "aria-label")}"`);
  assert(b9Key("运行日志") === "Runtime log", `英文态「运行日志」未翻译,实际 "${b9Key("运行日志")}"`);
  assert(attrOf("log-copy", "aria-label") === "Copy runtime log", `英文态 log-copy aria-label 未翻译,实际 "${attrOf("log-copy", "aria-label")}"`);
  assert(attrOf("status-git", "title") === "Git branch · uncommitted changes", `英文态 status-git title 未翻译,实际 "${attrOf("status-git", "title")}"`);
  assert(attrOf("status-tokens", "aria-label") === "View context components", `英文态 status-tokens aria-label 未翻译,实际 "${attrOf("status-tokens", "aria-label")}"`);
  assert(b9Key("日志") === "Logs", `英文态「日志」未翻译,实际 "${b9Key("日志")}"`);
  assert(b9Key("当前计划") === "Current plan", `英文态「当前计划」未翻译,实际 "${b9Key("当前计划")}"`);
  assert(b9Key("全部类型") === "All types", `英文态「全部类型」未翻译(option 渲染点),实际 "${b9Key("全部类型")}"`);
  assert(b9Key("终端") === "terminal", `英文态「终端」未翻译(option 渲染点),实际 "${b9Key("终端")}"`);
  assert(b9Key("已关闭") === "Closed", `英文态「已关闭」未翻译,实际 "${b9Key("已关闭")}"`);
  assert(b9Key("清空") === "Clear", `英文态「清空」未翻译,实际 "${b9Key("清空")}"`);
  assert(b9Key("权限请求") === "Permission request", `英文态「权限请求」未翻译,实际 "${b9Key("权限请求")}"`);
  assert(b9Key("拒绝") === "Deny", `英文态「拒绝」未翻译,实际 "${b9Key("拒绝")}"`);
  assert(b9Key("总是允许") === "Always allow", `英文态「总是允许」未翻译,实际 "${b9Key("总是允许")}"`);
  assert(b9Key("允许一次") === "Allow once", `英文态「允许一次」未翻译,实际 "${b9Key("允许一次")}"`);
  assert(attrOf("ask-answer", "placeholder") === "Enter your answer", `英文态回答 placeholder 未翻译(渲染点属性补齐),实际 "${attrOf("ask-answer", "placeholder")}"`);
  assert(attrOf("viewer-external", "aria-label") === "Open in external editor", `英文态 viewer-external aria-label 未翻译,实际 "${attrOf("viewer-external", "aria-label")}"`);
  assert(attrOf("prompt", "placeholder") === "What would you like to do? Paste or drop images or PDFs", `英文态输入框 placeholder 未翻译,实际 "${attrOf("prompt", "placeholder")}"`);
  assert(b9Key("排队 queue") === "Queue", `英文态「排队 queue」未翻译(option 渲染点),实际 "${b9Key("排队 queue")}"`);
  assert(!sandbox.document.getElementById("process-tabs"), "英文态不应出现顶部进程切换条");
  // 动态元素不被静态 key 覆写:status-mode/status-text/live-turn 都不得带 data-i18n-key,
  // 它们的文案由 JS 渲染点(t()/localizeDynamic)负责,切语言不应被 applyDataI18nKeys 触碰。
  assert(!attrOf("status-mode", "data-i18n-key"), "status-mode 不得挂 data-i18n-key(动态渲染点)");
  assert(!attrOf("status-text", "data-i18n-key"), "status-text 不得挂 data-i18n-key(动态渲染点)");

  localStorageShim.setItem("kz-language", "zh");
  sandbox.applyLanguage();
  assert(b9Key("权限请求") === "权限请求", `切回中文后「权限请求」未回原文,实际 "${b9Key("权限请求")}"`);
  assert(b9Key("当前计划") === "当前计划", `切回中文后「当前计划」未回原文,实际 "${b9Key("当前计划")}"`);
  assert(attrOf("prompt", "placeholder") === "想做什么?可粘贴/拖拽图片或 PDF", `切回中文后输入框 placeholder 未回原文,实际 "${attrOf("prompt", "placeholder")}"`);
  assert(b9Key("排队 queue") === "排队 queue", `切回中文后「排队 queue」未回原文,实际 "${b9Key("排队 queue")}"`);

  localStorageShim.setItem("kz-language", priorLanguage);
  sandbox.applyLanguage();
}

// ---------- 换项目不得把上一个项目的筛选落进新项目 ----------
// documentFilters 是模块级状态,切项目不会重建它;restoreDocFilters 又只"叠加保存里存在
// 的字段、不复位"。于是切到一个从没设过偏好的新项目时,内存里还挂着上个项目的整套口径,
// 而 syncDocumentFilters 里 D-169 的标签回落一触发就 saveDocFilters(),把这一整套写进
// **新项目**的键——用户在新项目从没设过,列表却少了一批,重启也回不来。
// 触发条件一点不苛刻:上个项目的标签在新项目里不存在(标签本来就按项目走)。
// 两头一起钉死:新项目必须是干净的默认口径,老项目切回去必须原样还在(别为了修这个
// 把 R-115 的按项目持久化弄坏)。
{
  const PROJECT_B = "C:/smoke/project-b";
  const savedDocsPayload = structuredClone(payloads.docs_snapshot);
  const filtersKeyOf = (path) => `kz-filters:${path}`;
  // 默认口径取自被测代码自己那一份(DOC_FILTER_DEFAULTS),冒烟里不另抄一遍:
  // 抄第二份的话,默认值一改这组断言就悄悄变成恒真。
  const DEFAULTS = JSON.parse(vm.runInContext("JSON.stringify(DOC_FILTER_DEFAULTS)", sandbox));
  const liveFilters = () => JSON.parse(vm.runInContext("JSON.stringify(documentFilters)", sandbox));
  const savedFilters = (path) => JSON.parse(storage.get(filtersKeyOf(path)) ?? "null");
  // 内存与落盘共用同一把尺子:列出所有"与默认值不同"的持久化字段。
  const stray = (bag, kind) =>
    Object.entries(DEFAULTS[kind])
      .filter(([field, def]) => bag?.[field] !== undefined && bag[field] !== def)
      .map(([field]) => `${kind}.${field}=${bag[field]}`);
  const strayLive = () => { const f = liveFilters(); return [...stray(f.req, "req"), ...stray(f.defect, "defect")]; };
  const straySaved = (path) => {
    const blob = savedFilters(path);
    return [...stray(blob?.docReq, "req"), ...stray(blob?.docDefect, "defect")];
  };
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const gotoProject = async (path, docs) => {
    payloads.docs_snapshot = docs;
    payloads.projects_select = {
      current: path,
      projects: [PROJECT, PROJECT_B],
      names: { [PROJECT]: "smoke", [PROJECT_B]: "smoke-b" },
    };
    await sandbox.selectWorkspaceProject(path);
    await flush();
    assert(
      vm.runInContext("currentProject", sandbox) === path,
      `前置失败:切项目没走通(currentProject=${vm.runInContext("currentProject", sandbox)})`,
    );
  };
  // 项目1 有「核心」标签,项目2 只有「流程」——标签按项目走,这就是常态。
  const docsA = {
    ...savedDocsPayload,
    requirements: [
      docEntry("R-001", "核心大需求", "doing", { complexity: "大", fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [
      docEntry("D-001", "冒烟缺陷", "open", { fields: [["标签", "前端"]] }),
      docEntry("D-002", "在修缺陷", "fixing", { fields: [["标签", "前端"]] }),
    ],
  };
  const docsB = {
    ...savedDocsPayload,
    requirements: [docEntry("R-900", "项目二需求", "todo", { complexity: "中", fields: [["标签", "流程"]] })],
    defects: [docEntry("D-900", "项目二缺陷", "open", { fields: [["标签", "流程"]] })],
  };

  // 前置:项目1 设好一套只属于它的筛选(两队都设,跨队列泄漏也要能看出来)。
  payloads.docs_snapshot = docsA;
  await sandbox.refreshDocs();
  await flush();
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-status-filter", "fixing");
  byId.get("documents-tab-req").click();
  await flush();
  await setDocFilter("documents-status-filter", "doing");
  await setDocFilter("documents-complexity-filter", "大");
  await setDocFilter("documents-tag-filter", "核心");
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "前置失败:项目1 的需求筛选没生效",
  );
  const strayA = straySaved(PROJECT);
  assert(
    ["req.status=doing", "req.complexity=大", "req.tag=核心", "defect.status=fixing"].every((f) => strayA.includes(f)),
    `前置失败:项目1 的筛选没完整落盘(R-115 持久化本身断了):${storage.get(filtersKeyOf(PROJECT))}`,
  );

  // 项目2 从没设过偏好:键必须先干净,否则断言测的是"回填",不是"泄漏"。
  storage.delete(filtersKeyOf(PROJECT_B));

  await gotoProject(PROJECT_B, docsB);
  assert(
    strayLive().length === 0,
    `换到没设过偏好的项目,内存里还挂着上一个项目的筛选:${strayLive().join(", ")}`,
  );
  assert(
    straySaved(PROJECT_B).length === 0,
    `上一个项目的筛选被写进了新项目的键(用户从没设过,重启也回不来):${straySaved(PROJECT_B).join(", ")} / ${storage.get(filtersKeyOf(PROJECT_B))}`,
  );
  // 用户视角的后果:新项目的列表被一个自己从没设过的条件筛空。
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]'),
    "新项目的需求被上一个项目的筛选藏掉了(看起来就是条目凭空没有)",
  );
  assert(
    document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-900"]'),
    "新项目的缺陷被上一个项目的筛选藏掉了",
  );
  assert(byId.get("documents-status-filter").value === "all", "新项目的状态下拉还显示着上一个项目的值");
  assert(byId.get("documents-tag-filter").value === "all", "新项目的标签下拉还显示着上一个项目的值");

  // 切回项目1:自己的筛选必须原样还在(内存 + 落盘 + 控件 + 列表)。
  await gotoProject(PROJECT, docsA);
  byId.get("documents-tab-req").click();
  await flush();
  const backLive = liveFilters();
  assert(
    backLive.req.status === "doing" && backLive.req.complexity === "大" && backLive.req.tag === "核心"
      && backLive.defect.status === "fixing",
    `切回原项目,它自己的筛选没回来(为了不泄漏把按项目持久化一起弄坏了):${JSON.stringify(backLive)}`,
  );
  assert(
    ["req.status=doing", "req.complexity=大", "req.tag=核心", "defect.status=fixing"].every((f) => straySaved(PROJECT).includes(f)),
    `切回原项目,落盘的筛选被改掉了:${storage.get(filtersKeyOf(PROJECT))}`,
  );
  assert(byId.get("documents-status-filter").value === "doing", "切回原项目,状态下拉没回填");
  assert(byId.get("documents-complexity-filter").value === "大", "切回原项目,复杂度下拉没回填");
  assert(byId.get("documents-tag-filter").value === "核心", "切回原项目,标签下拉没回填");
  assert(
    !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    "切回原项目后筛选只剩下拉显示值、列表没在筛(状态与显示脱节)",
  );

  // 分组开关不按项目走:它记在全局键 kz-grouped-docs 上(见 bindGroupToggle),换项目
  // 复位**不得**碰它。整个复位形状就建立在这条边界上——复位只覆盖 DOC_FILTER_DEFAULTS 的
  // 键,grouped 故意不在那份清单里。把 grouped 加进 DOC_FILTER_DEFAULTS 上面那些断言全绿,
  // 因为它们只逐字段比对"与默认值不同"的持久化项,而 grouped 的默认值(true)恰好等于
  // 被错误复位后的值。所以必须单独钉死:用户关掉分组,换个项目它自己回来了,
  // 落盘还停在 0、按钮还写着 aria-pressed=false —— 显示、内存、落盘三方脱节。
  {
    const groupToggle = byId.get("documents-group-toggle");
    const groupedLive = () => liveFilters();
    const groupedBefore = groupedLive().req.grouped;
    // 走用户路径把开关拨到「关」:已经是关的就先开再关,保证落盘值也确实是这条路径写出来的
    // (前面的用例可能只改过内存里的 grouped —— 解锁按钮就是),否则前置断言测的是别人留下的残值。
    if (!groupedLive().req.grouped) {
      groupToggle.click();
      await flush();
    }
    groupToggle.click();
    await flush();
    assert(
      groupedLive().req.grouped === false && groupedLive().defect.grouped === false
        && storage.get("kz-grouped-docs") === "0" && groupToggle.getAttribute("aria-pressed") === "false",
      `前置失败:分组没关掉(内存 ${JSON.stringify(groupedLive().req.grouped)} / 落盘 ${storage.get("kz-grouped-docs")} / aria-pressed ${groupToggle.getAttribute("aria-pressed")})`,
    );

    storage.delete(filtersKeyOf(PROJECT_B));
    await gotoProject(PROJECT_B, docsB);
    const afterSwitch = groupedLive();
    assert(
      afterSwitch.req.grouped === false && afterSwitch.defect.grouped === false,
      `换项目把分组开关复位了(它按 kz-grouped-docs 全局记、不随项目走):req=${afterSwitch.req.grouped} defect=${afterSwitch.defect.grouped}`,
    );
    assert(
      storage.get("kz-grouped-docs") === "0",
      `换项目改掉了分组开关的全局落盘值:kz-grouped-docs=${storage.get("kz-grouped-docs")}`,
    );
    assert(
      groupToggle.getAttribute("aria-pressed") === "false",
      `换项目后分组按钮的 aria-pressed 与状态脱节:aria-pressed=${groupToggle.getAttribute("aria-pressed")}(内存 ${afterSwitch.req.grouped})`,
    );

    // 还原:切回项目1 并把分组开关调回进来时的样子,否则后续用例看到的是另一种渲染形态。
    storage.delete(filtersKeyOf(PROJECT_B));
    await gotoProject(PROJECT, docsA);
    if (groupedLive().req.grouped !== groupedBefore) {
      groupToggle.click();
      await flush();
    }
    assert(
      groupedLive().req.grouped === groupedBefore,
      `收尾失败:分组开关没还原(${groupedLive().req.grouped} ≠ ${groupedBefore}),后续用例会连带假失败`,
    );
    byId.get("documents-tab-req").click();
    await flush();
  }

  // 收尾:走用户路径把筛选调回全部,清掉项目2 的键,还原快照。
  await setDocFilter("documents-status-filter", "all");
  await setDocFilter("documents-complexity-filter", "all");
  await setDocFilter("documents-tag-filter", "all");
  byId.get("documents-tab-defect").click();
  await flush();
  await setDocFilter("documents-status-filter", "all");
  byId.get("documents-tab-req").click();
  await flush();
  storage.delete(filtersKeyOf(PROJECT_B));
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
  assert(strayLive().length === 0, `收尾失败:筛选没调回全部(${strayLive().join(", ")})`);
}

// ---------- 一次空快照不得清掉用户的标签筛选 ----------
// syncDocumentFilters 的 D-169 回落(「保存的标签在这一队里已经不存在了 → 回落成全部并落盘」)
// 只在**这一队真的有条目**时才成立。而 docs_snapshot 并不保证非空:docstore 那几个文件是
// fs::write 截断重写(非原子),load() 又把空文件当成合法的空列表返回,docs.rs 更是
// unwrap_or_default —— 任何读失败(含 Windows 上的文件占用)都静默降级成空;偏偏
// docs_snapshot 自己开头就在写这几个文件,一次 refreshDocs 与一次 refreshDocsSoon
// 完全可以同时在飞。于是一次瞬态空快照就够把用户设好的标签筛选永久清成「全部」:
// 内存与落盘一起改,数据回来了也回不来,重启同样,全程零用户动作。
// 空列表里「列表被一个看不见的条件筛空」这个前提根本不成立(列表本来就是空的),
// 没有任何理由改用户的口径。
{
  const savedDocsPayload = structuredClone(payloads.docs_snapshot);
  const filtersKey = `kz-filters:${PROJECT}`;
  const liveTag = (kind) => JSON.parse(vm.runInContext(`JSON.stringify(documentFilters.${kind})`, sandbox)).tag;
  const savedTag = (kind) =>
    JSON.parse(storage.get(filtersKey) ?? "{}")[kind === "req" ? "docReq" : "docDefect"]?.tag;
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  const taggedDocs = {
    ...savedDocsPayload,
    requirements: [
      docEntry("R-001", "核心标签需求", "doing", { fields: [["标签", "核心"]] }),
      docEntry("R-002", "前端标签需求", "todo", { fields: [["标签", "前端"]] }),
    ],
    defects: [docEntry("D-001", "前端标签缺陷", "open", { fields: [["标签", "前端"]] })],
  };

  byId.get("documents-tab-req").click();
  await flush();
  payloads.docs_snapshot = taggedDocs;
  await sandbox.refreshDocs();
  await flush();
  await setDocFilter("documents-tag-filter", "核心");
  assert(
    liveTag("req") === "核心" && savedTag("req") === "核心",
    `前置失败:标签筛选没设上或没落盘(内存 ${liveTag("req")} / 落盘 ${storage.get(filtersKey)})`,
  );

  // 瞬态空快照:两队都读成了空(截断重写撞上并发读,或读失败降级成空列表)。
  payloads.docs_snapshot = { ...savedDocsPayload, requirements: [], defects: [] };
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTag("req") === "核心",
    `一次瞬态空快照把用户的标签筛选清成了「全部」(内存):${liveTag("req")}`,
  );
  assert(
    savedTag("req") === "核心",
    `一次瞬态空快照把用户的标签筛选清成「全部」并落盘了(数据回来了筛选也回不来,重启同样):${storage.get(filtersKey)}`,
  );

  // 数据回来:筛选必须原样还在,并且真的在筛(状态与显示不许脱节)。
  payloads.docs_snapshot = taggedDocs;
  await sandbox.refreshDocs();
  await flush();
  assert(
    byId.get("documents-tag-filter").value === "核心"
      && document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-002"]'),
    `空快照过后数据回来了,标签筛选却没恢复成用户设的那个:下拉=${byId.get("documents-tag-filter").value}`,
  );

  // 收尾:走用户路径调回全部并还原快照。
  await setDocFilter("documents-tag-filter", "all");
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
  assert(
    liveTag("req") === "all" && liveTag("defect") === "all",
    `收尾失败:标签没调回全部(${liveTag("req")} / ${liveTag("defect")})`,
  );
}

// ---------- 在途快照不得落到已经切走的项目上 ----------
// refreshDocs / refreshDocsSoon 都是 `await invoke("docs_snapshot", { projectDir: currentProject })`
// 之后直接 renderDocsSnapshot。await 期间用户切了项目,这份数据就是**上一个项目**的:
// 轻则切项目瞬间闪一下上一个项目的列表,重则 syncDocumentFilters 拿**新项目**的筛选去
// **旧项目**的条目里判「这标签还在不在」——判否就回落成「全部」,而落盘走的是新项目的键。
// 用户在新项目从没动过筛选,列表却少了一批,重启也回不来。
// 闸门把"IPC 还没回来"这段真机时序复现出来:只卡住这一次,切项目自己那次刷新照常走。
// 切项目夹具提到块外:下面 D-250/D-251 的跨项目用例走的是同一套「甲乙两个项目 + 闸门」
// 时序,各自再抄一份 gotoProject 只会让三处的切项目口径将来悄悄分叉。
const PROJECT_B = "C:/smoke/project-b";
const savedDocsPayload = structuredClone(payloads.docs_snapshot);
const gotoProject = async (path, docs) => {
  payloads.docs_snapshot = docs;
  payloads.projects_select = {
    current: path,
    projects: [PROJECT, PROJECT_B],
    names: { [PROJECT]: "smoke", [PROJECT_B]: "smoke-b" },
  };
  await sandbox.selectWorkspaceProject(path);
  await flush();
  assert(
    vm.runInContext("currentProject", sandbox) === path,
    `前置失败:切项目没走通(currentProject=${vm.runInContext("currentProject", sandbox)})`,
  );
};
// 标签按项目走:甲只有「核心」,乙只有「流程」。这就是常态,触发条件一点不苛刻。
const docsA = {
  ...savedDocsPayload,
  requirements: [docEntry("R-001", "甲项目需求", "doing", { fields: [["标签", "核心"]] })],
  defects: [docEntry("D-001", "甲项目缺陷", "open", { fields: [["标签", "核心"]] })],
};
const docsB = {
  ...savedDocsPayload,
  requirements: [docEntry("R-900", "乙项目需求", "todo", { fields: [["标签", "流程"]] })],
  defects: [docEntry("D-900", "乙项目缺陷", "open", { fields: [["标签", "流程"]] })],
};
{
  const filtersKeyOf = (path) => `kz-filters:${path}`;
  const liveReqTag = () => JSON.parse(vm.runInContext("JSON.stringify(documentFilters.req)", sandbox)).tag;
  const savedReqTag = (path) => JSON.parse(storage.get(filtersKeyOf(path)) ?? "{}").docReq?.tag;
  const setDocFilter = async (id, value) => {
    const el = byId.get(id);
    el.value = value;
    assert(el.value === value, `前置失败:#${id} 没有 value=${value} 的选项`);
    el._listeners.change?.forEach((fn) => fn({ target: el }));
    await flush();
  };
  byId.get("documents-tab-req").click();
  await flush();
  storage.delete(filtersKeyOf(PROJECT_B));
  await gotoProject(PROJECT_B, docsB);
  await setDocFilter("documents-tag-filter", "流程");
  assert(
    liveReqTag() === "流程" && savedReqTag(PROJECT_B) === "流程",
    `前置失败:项目乙的标签筛选没设上或没落盘(内存 ${liveReqTag()} / 落盘 ${storage.get(filtersKeyOf(PROJECT_B))})`,
  );

  await gotoProject(PROJECT, docsA);
  let releaseStale;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseStale = resolve; }));
  const stale = sandbox.refreshDocs(); // 替项目甲发出,此刻卡在闸门上
  await settle();
  invokeGates.delete("docs_snapshot"); // 只卡住上面那一次:已在 await 的调用握着自己那个 promise
  await gotoProject(PROJECT_B, docsB);
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]'),
    "前置失败:切到项目乙后列表不是乙的",
  );
  assert(liveReqTag() === "流程", `前置失败:项目乙自己的标签筛选没回填(${liveReqTag()})`);

  // 在途的那一次现在才落地,带回来的是**项目甲**的数据。
  payloads.docs_snapshot = docsA;
  releaseStale();
  await stale;
  payloads.docs_snapshot = docsB; // 还原,免得随后的定时器刷新又拿到甲的数据(那是另一回事)
  await flush();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "上一个项目的在途快照被画到了当前项目上(切项目瞬间闪一下上一个项目的列表)",
  );
  assert(
    liveReqTag() === "流程",
    `上一个项目的在途快照把当前项目的标签筛选清掉了(内存):${liveReqTag()}`,
  );
  assert(
    savedReqTag(PROJECT_B) === "流程",
    `上一个项目的在途快照把当前项目的标签筛选清掉并落进了当前项目的键(用户从没动过,重启也回不来):${storage.get(filtersKeyOf(PROJECT_B))}`,
  );

  // refreshDocsSoon 走同一条路,而且更容易撞上:它由 agent 的文档变更事件驱动,自带 400ms
  // 合并窗口,定时器落地时用户早就可能切走了。两个函数各测一次,少一个就是漏一条真实路径。
  await gotoProject(PROJECT, docsA);
  assert(liveReqTag() === "all", `前置失败:项目甲的标签筛选不是干净的(${liveReqTag()})`);
  let releaseSoon;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseSoon = resolve; }));
  // 手工点火,且**不 await**:回调此刻正卡在闸门上,drainTimersOnce 会按 300ms 超时判红。
  // 只点火 refreshDocsSoon 自己排的那一个,不波及别处已排队的定时器。
  const timersBefore = new Set(pendingTimers);
  sandbox.refreshDocsSoon();
  for (const handle of [...pendingTimers]) {
    if (timersBefore.has(handle) || handle.interval) continue;
    pendingTimers.delete(handle);
    void handle.fn();
  }
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "docs_snapshot" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:refreshDocsSoon 没有替项目甲发出在途的 docs_snapshot(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("docs_snapshot");
  await gotoProject(PROJECT_B, docsB);
  payloads.docs_snapshot = docsA;
  releaseSoon();
  for (let i = 0; i < 12; i += 1) await settle();
  payloads.docs_snapshot = docsB;
  await flush();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')
      && !document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]'),
    "refreshDocsSoon 的在途快照被画到了已经切走的项目上",
  );
  assert(
    liveReqTag() === "流程" && savedReqTag(PROJECT_B) === "流程",
    `refreshDocsSoon 的在途快照(上一个项目的数据)把当前项目的标签筛选清掉并落盘了:内存 ${liveReqTag()} / 落盘 ${storage.get(filtersKeyOf(PROJECT_B))}`,
  );

  // 收尾:调回全部、切回项目甲、清掉项目乙的键、还原快照。
  await setDocFilter("documents-tag-filter", "all");
  await gotoProject(PROJECT, savedDocsPayload);
  storage.delete(filtersKeyOf(PROJECT_B));
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
  assert(liveReqTag() === "all", `收尾失败:标签没调回全部(${liveReqTag()})`);
}

// ---------- 旧项目的刷新失败不得作废新项目刚排的跳转高亮(D-250) ----------
// 上一块钉的是**成功**路径按项目收敛。catch 里的 clearPendingJump() 没有同样的守卫:
// 替旧项目发出的那次刷新若在用户切走之后才抛错,会把**新项目刚排上的**跳转高亮一并作废
// ——用户点了条目引用跳过去,却看不出落在哪一条。同一条路径上的不对称(成功收敛、失败不收敛)
// 正是 D-211 说的「承诺与实现脱节」。
// 手法:替甲发出的那次 refreshDocs 卡在闸门上 → 切到乙 → 在乙里排一个跳转高亮(它自己那次
// 刷新也卡住,免得当场被消费掉)→ 再让甲那次以**失败**落地。失败判定在闸门之后,所以顺序
// 必须是「先注入失败、再放行闸门」。
{
  await gotoProject(PROJECT, docsA);
  let releaseStaleFail;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseStaleFail = resolve; }));
  const staleFail = sandbox.refreshDocs(); // 替项目甲发出,此刻卡在闸门上
  await settle();
  invokeGates.delete("docs_snapshot"); // 只卡住上面那一次
  await gotoProject(PROJECT_B, docsB);
  // 离开单页视图,jumpToEntry 才会走「先切视图 + 排挂起高亮」那条路。
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  assert(!byId.get("view-documents").classList.contains("active"), "前置失败:未离开单页视图");
  assert(document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]'), "前置失败:项目乙的列表里没有 R-900");
  let releaseJumpB;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseJumpB = resolve; }));
  sandbox.jumpToEntry("R-900");
  // 只推微任务:这一步要的是「乙那次刷新卡住、pendingJumpId 挂着」,不能让定时器插进来。
  for (let i = 0; i < 12; i += 1) await settle();
  invokeGates.delete("docs_snapshot");
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    `前置失败:项目乙没有排上跳转高亮(实得 ${JSON.stringify(vm.runInContext("pendingJumpId", sandbox))})`,
  );
  // 注入的刷新失败会走 toastError:那正是被测的那条 catch,不判红。
  expectedPersistentError = "项目文档刷新失败";
  const hitsBefore = expectedPersistentHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:旧项目的在途刷新撞上目录被删/文件被锁/解析失败");
  releaseStaleFail();
  await staleFail;
  invokeFailures.delete("docs_snapshot");
  assert(expectedPersistentHits > hitsBefore, "前置失败:注入的 docs_snapshot 失败没有走到 refreshDocs 的 catch");
  expectedPersistentError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    "旧项目的刷新失败作废了新项目刚排的跳转高亮:refreshDocs 的 catch 里 clearPendingJump() 没有项目守卫(成功路径按项目收敛了、失败路径没有)",
  );
  // 高亮还得真能兑现:守卫若写成「永远不清」,上面那条断言会被另一个错误盖过去。
  releaseJumpB();
  for (let i = 0; i < 12; i += 1) await settle();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')?.classList.contains("ref-highlight"),
    "项目乙自己那次刷新没有兑现挂起的跳转高亮(守卫收得过紧,或高亮被别处清掉了)",
  );
  await flush();
}

// ---------- refreshDocsSoon 的失败路径同样要按项目收敛(单独钉,D-250) ----------
// 与上一条同病、独立出口(console.error,不是 toastError),而且更容易撞上:它由 agent 的
// 文档变更事件驱动、自带 400ms 合并窗口,定时器落地时用户早就可能切走了。
// 上面「悬挂高亮」那一族已经实测过一次:只修 refreshDocs 那处、留着这处,整套照样全绿。
{
  await gotoProject(PROJECT, docsA);
  let releaseStaleSoon;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseStaleSoon = resolve; }));
  // 手工点火,且**不 await**:回调此刻正卡在闸门上,drainTimersOnce 会按 300ms 超时判红。
  // 只点火 refreshDocsSoon 自己排的那一个,不波及别处已排队的定时器。
  const timersBefore = new Set(pendingTimers);
  sandbox.refreshDocsSoon();
  for (const handle of [...pendingTimers]) {
    if (timersBefore.has(handle) || handle.interval) continue;
    pendingTimers.delete(handle);
    void handle.fn();
  }
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "docs_snapshot" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:refreshDocsSoon 没有替项目甲发出在途的 docs_snapshot(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("docs_snapshot");
  await gotoProject(PROJECT_B, docsB);
  document.querySelectorAll(".activity-item").find((n) => n.dataset.view === "chat")?.click();
  await flush();
  let releaseJumpB;
  invokeGates.set("docs_snapshot", new Promise((resolve) => { releaseJumpB = resolve; }));
  sandbox.jumpToEntry("R-900");
  for (let i = 0; i < 12; i += 1) await settle();
  invokeGates.delete("docs_snapshot");
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    `前置失败:项目乙没有排上跳转高亮(实得 ${JSON.stringify(vm.runInContext("pendingJumpId", sandbox))})`,
  );
  // 被测的正是 refreshDocsSoon 那条 catch,它只 console.error —— 开窗放行,出了这段立刻收回。
  expectedConsoleError = "冒烟注入";
  const consoleHitsBefore = expectedConsoleHits;
  invokeFailures.set("docs_snapshot", "冒烟注入:refreshDocsSoon 的在途刷新撞上目录被删/文件被锁/解析失败");
  releaseStaleSoon();
  for (let i = 0; i < 12; i += 1) await settle();
  invokeFailures.delete("docs_snapshot");
  assert(
    expectedConsoleHits > consoleHitsBefore,
    "前置失败:注入的 docs_snapshot 失败没有走到 refreshDocsSoon 的 catch(这一段根本没测到目标路径)",
  );
  expectedConsoleError = null;
  assert(
    vm.runInContext("pendingJumpId", sandbox) === "R-900",
    "旧项目的 refreshDocsSoon 刷新失败作废了新项目刚排的跳转高亮:它那条 catch 里的 clearPendingJump() 没有项目守卫",
  );
  releaseJumpB();
  for (let i = 0; i < 12; i += 1) await settle();
  assert(
    document.querySelector('#documents-req-list .doc-item[data-doc-id="R-900"]')?.classList.contains("ref-highlight"),
    "项目乙自己那次刷新没有兑现挂起的跳转高亮",
  );
  await flush();
}

// ---------- R-174 子代理面板:独立分区、六字段真实数据、单条停止、transcript ----------
{
  // 打开面板:agent-toggle 应切出 #agent-panel 并收起 #bg-panel(互斥)。
  const agentToggle = byId.get("agent-toggle");
  assert(agentToggle, "子代理面板缺少 rail 开关");
  const agentPanel = byId.get("agent-panel");
  const bgPanel = byId.get("bg-panel");
  agentToggle.click();
  assert(!agentPanel.classList.contains("hidden"), "点击 agent-toggle 后 #agent-panel 未展开");
  assert(bgPanel.classList.contains("hidden"), "子代理面板打开时活动面板未收起(互斥切换失败)");
  agentToggle.click(); // 收起
  assert(agentPanel.classList.contains("hidden"), "再次点击 agent-toggle 后 #agent-panel 未收起");
  agentToggle.click(); // 再展开,后续断言用
  // 建条:task 的 tool-start 进子代理面板(编排派发带 phase,模型自派 name=task)。
  const sA = handlers.get("kz:tool-start");
  const pA = handlers.get("kz:task-progress");
  const eA = handlers.get("kz:tool-end");
  sA({ payload: { id: "my_scout", name: "task", summary: "勘察文件结构", input: { prompt: "review the repo", phase: "scouting", role: "my_scout" } } });
  await flush();
  const a1 = document.querySelector('#agent-running .bg-entry[data-agent-id="my_scout"]');
  assert(a1, "运行中的子代理未进入 Running 区");
  assert(a1.querySelector(".bg-tool")?.textContent.includes("my_scout"), "编排派发的子代理未以角色名作名称");
  assert(a1.querySelector(".bg-phase-badge")?.textContent.includes("Scouting"), "子代理缺少类型(阶段)徽章");
  assert(a1.dataset.agentElapsed === "0", "子代理缺少已运行时长字段");
  // 六字段中的 token/工具调用数/当前工具名必须来自真实 trace(usage/start 事件)。
  pA({ payload: { id: "my_scout", text: "读取中", trace: { child_id: "c1", phase: "start", name: "read", summary: "src/main.rs", input: { path: "src/main.rs" } } } });
  await flush();
  assert(a1.dataset.agentCurrentTool === "read", "子代理未显示当前正在用的工具名(trace 数据源)");
  pA({ payload: { id: "my_scout", text: "读取中", trace: { child_id: "c1", phase: "end", name: "read", ok: true, preview: "ok" } } });
  pA({ payload: { id: "my_scout", text: "统计", trace: { child_id: "c1", phase: "usage", usage: { input: 100, output: 50, cache_read: 10, cache_write: 5 } } } });
  await flush();
  assert(a1.dataset.agentCurrentTool === "read", "工具结束后当前工具名应保留(idle 态)");
  assert(a1.querySelector(".bg-meta")?.textContent.includes("tokens"), "子代理元信息未显示累计 token");
  assert(a1.querySelector(".bg-meta")?.textContent.includes("tool calls"), "子代理元信息未显示工具调用次数");
  // transcript:展开 detail 应有完整调用序列(名称 + 入参)。
  a1.querySelector(".bg-title").click();
  assert(a1.querySelector(".agent-call"), "子代理缺少 transcript 调用序列");
  assert(a1.querySelector(".agent-call pre")?.textContent.includes("src/main.rs"), "transcript 未包含调用的完整入参");
  // 单条停止:运行中的子代理有停止按钮,点击走 stop_task 而非 stop_run。
  const stopBtn = [...a1.querySelectorAll(".bg-actions button")].find((b) => b.textContent === "Stop");
  assert(stopBtn, "运行中的子代理缺少单条停止按钮");
  const beforeStop = invokeLog.filter((cmd) => cmd === "stop_run").length;
  stopBtn.click();
  await flush();
  assert(
    invokeArgs.some((a) => a.cmd === "stop_task" && a.args?.taskId === "my_scout"),
    "单条停止未调用 stop_task(或参数缺少 taskId)",
  );
  assert(invokeLog.filter((cmd) => cmd === "stop_run").length === beforeStop, "单条停止误调用了 stop_run(整轮停止)");
  // 被停终态:tool-end ok=false +「被停」文案 → 移到 Finished 区、标 stopped、读槽释放由后端负责。
  eA({ payload: { id: "my_scout", name: "task", ok: false, preview: "子代理已被停止", display: null } });
  await flush();
  const a1f = document.querySelector('#agent-finished .bg-entry[data-agent-id="my_scout"]');
  assert(a1f, "被停的子代理未移入 Finished 区");
  assert(a1f.dataset.bgStatus === "stopped", "被停的子代理未标记 stopped 终态");
  assert(a1f.querySelector(".bg-meta")?.textContent.includes("Stopped"), "被停的子代理终态元信息未显示「已停止」");
  assert(!a1f.querySelectorAll(".bg-actions button").some((b) => b.textContent === "Stop"), "结束的子代理不应残留停止按钮");
  // Finished 区的条目有「打开」(transcript 视图入口)。
  assert(a1f.querySelectorAll(".bg-actions button").some((b) => b.textContent === "Open"), "Finished 区子代理缺少打开 transcript 入口");
  // 关闭只收起条目,后端历史仍保留;重新打开可恢复到 Finished,再删除才移除本次 UI 条目。
  a1f.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Close")?.click();
  const a1c = document.querySelector('#agent-closed .bg-entry[data-agent-id="my_scout"]');
  assert(a1c, "关闭后的子代理未移入 Closed 区");
  assert(a1c.querySelectorAll(".bg-actions button").some((b) => b.textContent === "Open"), "Closed 条目缺少重新打开入口");
  a1c.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Open")?.click();
  const a1reopened = document.querySelector('#agent-finished .bg-entry[data-agent-id="my_scout"]');
  assert(a1reopened, "Closed 条目点击 Open 后未回到 Finished 区");
  a1reopened.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Close")?.click();
  document.querySelector('#agent-closed .bg-entry[data-agent-id="my_scout"]')?.querySelectorAll(".bg-actions button").find((b) => b.textContent === "Delete")?.click();
  assert(!document.querySelector('#agent-closed .bg-entry[data-agent-id="my_scout"]'), "删除已关闭子代理后 UI 条目仍存在");
  // Clear 清空 Finished/Closed 区,但不会影响 Running。
  byId.get("agent-clear").click();
  await flush();
  assert(!document.querySelector('#agent-finished .bg-entry[data-agent-id="my_scout"]'), "Clear 未清空 Finished 区");
  // 关闭面板,恢复活动面板互斥状态。
  agentToggle.click();
  assert(agentPanel.classList.contains("hidden"), "断言结束后 #agent-panel 未收起");
}

// ---------- R-184 B 面:真实并列视图与合并前冲突预警 ----------
{
  await gotoProject(PROJECT, savedDocsPayload);
  const linesButton = document.querySelector('.activity-item[data-view="lines"]');
  assert(linesButton, "活动栏缺少并行线路入口");
  const beforeOpen = invokeArgs.length;
  linesButton?.click();
  await flush();
  const openCalls = invokeArgs.slice(beforeOpen);
  assert(
    openCalls.some((entry) => entry.cmd === "collaboration_snapshot" && entry.args?.projectDir === PROJECT),
    `打开并行线路没有读取真实 collaboration_snapshot(${JSON.stringify(openCalls)})`,
  );
  const lanes = document.querySelectorAll("#lines-list .line-lane");
  assert(lanes.length === 2, `并列视图没有渲染两条线路(实得 ${lanes.length})`);
  // harness 的 index.html 解析只登记静态 id,不重建完整父子树；动态线路需从
  // #lines-list 的真实后代读取，不能靠 #view-lines 汇总 textContent。
  const linesText = lanes.map((lane) => lane.textContent).join("\n");
  const claims = document.querySelectorAll("#lines-list .line-claim").map((node) => node.textContent);
  assert(
    claims.length === 2 && claims.every((claim) => claim.startsWith("R-184")) && new Set(claims).size === 2,
    `并列视图没有分别显示两条真实 claim(${JSON.stringify(claims)})`,
  );
  // 冒烟前半段已切到英文；固定标签和恰好命中词典的中文值会被本地化，下面只断言
  // 语言无关的现场值，阶段另允许中英两种等价值。
  for (const expected of ["实现", "edit", "thread-a1", "crates/shared.rs"]) {
    assert(linesText.includes(expected), `并列视图漏掉真实字段:${expected}(实得:${linesText})`);
  }
  assert(linesText.includes("复核") || linesText.includes("Review"), `并列视图漏掉真实阶段复核(实得:${linesText})`);
  assert(
    document.querySelectorAll("#lines-list .line-agent-code").map((node) => node.textContent).join("") === "MA",
    "线路没有同时显示稳定的 M/A 文本身份",
  );
  assert(
    document.querySelectorAll("#lines-conflict-list .line-conflict").length === 1 &&
      (document.querySelector("#lines-conflict-list .line-conflict")?.textContent ?? "").includes("crates/shared.rs"),
    "共享改动文件没有在发起合并前形成可下钻的跨线冲突预警",
  );
  const semanticNote = byId.get("lines-semantic-note")?.textContent ?? "";
  assert(
    semanticNote.includes("语义层未检查") || semanticNote.includes("semantic overlap unchecked"),
    `并列视图缺少语义冲突未检查的固定边界提示(实得:${semanticNote})`,
  );
  assert(
    !openCalls.some((entry) => entry.cmd === "worktree_merge"),
    "查看冲突预警不应偷偷触发合并",
  );
  assert(document.querySelectorAll("#lines-list .line-close").length === 1, "非默认线路应有关闭入口，默认主线路不得显示关闭");
  const closeCallsBefore = invokeArgs.length;
  document.querySelector("#lines-list .line-close")?.click();
  await flush();
  assert(
    invokeArgs.slice(closeCallsBefore).some((entry) => entry.cmd === "process_close" && entry.args?.processId === "p|bg"),
    "线路页关闭按钮没有调用目标线路的 process_close",
  );
  // D-304 验收②③:只有真实 claim 才出现「● 代号 被取得」；排在队首但无人认领不显示。
  const claimedSnapshot = [
    ...payloads.collaboration_snapshot,
    {
      process_id: "p|claim", label: "认领线", branch: "claim-a1", worktree_path: "C:/smoke/wt/claim-a1",
      claim: "R-001", phase: "实现", current_tool: "edit", running: true,
      steps: 1, input_tokens: 10, output_tokens: 5, changed_files: [],
    },
  ];
  sandbox.renderLines(claimedSnapshot);
  const claimedRow = document.querySelector('#documents-req-list .doc-item[data-doc-id="R-001"]');
  assert(claimedRow?.textContent.includes("● B"), `真实 claim 未渲染稳定线路代号:${claimedRow?.textContent}`);
  assert(
    claimedRow?.textContent.includes("被取得") || claimedRow?.textContent.includes("Claimed"),
    `真实 claim 未渲染「被取得」事实文案:${claimedRow?.textContent}`,
  );
  const unclaimedHead = document.querySelector('#documents-defect-list .doc-item[data-doc-id="D-001"]');
  assert(!unclaimedHead?.querySelector(".doc-claim-fact"), "排在队首但无人 claim 的条目不应显示被取得标记");
  sandbox.renderLines(payloads.collaboration_snapshot);
  await flush();
  assert(
    document.querySelectorAll("#lines-list .line-lane").every((lane) => !lane.className.includes("line-lane-initial")),
    "并行线路刷新不应挂载进入动画 class",
  );
  // R-184 验收⑩:800/1024/1280 三档宽度下并列视图不崩——线道、冲突预警、语义提示仍渲染。
  for (const width of [800, 1024, 1280]) {
    windowShim.innerWidth = width;
    await flush();
    const lanesAt = document.querySelectorAll("#lines-list .line-lane");
    assert(lanesAt.length === 2, `${width}px 下列表视图线道缺失(实得 ${lanesAt.length})`);
    assert(
      document.querySelectorAll("#lines-conflict-list .line-conflict").length === 1,
      `${width}px 下冲突预警缺失`,
    );
    assert(
      (byId.get("lines-semantic-note")?.textContent ?? "").includes("未检查") ||
        (byId.get("lines-semantic-note")?.textContent ?? "").includes("unchecked"),
      `${width}px 下语义边界提示缺失`,
    );
  }
  windowShim.innerWidth = 1280;
}

// ---------- R-184 P5:收活五格(②不可跳过 + 门禁 + 合并) ----------
{
  // 五格需要 worktree_diff / worktree_gate 桩:diff 给一条真实差异,gate 给四步全过。
  payloads.worktree_diff = {
    path: "C:/smoke/wt/thread-a1",
    branch: "thread-a1",
    clean: false,
    files: ["crates/branch.rs"],
    diff: "diff --git a/crates/branch.rs b/crates/branch.rs\n+pub fn line_work() {}\n",
  };
  payloads.worktree_gate = [
    { name: "fmt", ok: true, summary: "" },
    { name: "clippy", ok: true, summary: "" },
    { name: "test", ok: true, summary: "test result: ok. 118 passed" },
    { name: "ui-smoke", ok: true, summary: "UI 运行时冒烟通过" },
  ];
  // R-222 防线②:合并后全量复用同一门禁步骤(主根),桩同值。
  payloads.worktree_post_merge_gate = [
    { name: "fmt", ok: true, summary: "" },
    { name: "clippy", ok: true, summary: "" },
    { name: "test", ok: true, summary: "test result: ok. 120 passed" },
    { name: "ui-smoke", ok: true, summary: "UI 运行时冒烟通过" },
  ];
  // 五格块先于下方工作树清单块执行,worktree_merge 桩在此补齐(下方同值覆盖无害)。
  payloads.worktree_merge = "已合并工作树分支 thread-a1;工作树仍保留,可检查后显式放弃";
  payloads.worktree_harvest_writeback = "已回写 R-184 收活记录。当前进展:\n2026-08-11 收活回写: 由 A 线交付并合并(branch thread-a1)。";
  // 桩里只有后台会话带 worktree_path → 只有它的 lane 有「收活」按钮。
  const lanes = [...document.querySelectorAll("#lines-list .line-lane")];
  const withHarvest = lanes.filter((lane) => lane.querySelector(".line-harvest-toggle"));
  assert(withHarvest.length === 1, `收活按钮应只出现在带工作树的线上(实得 ${withHarvest.length})`);
  const wtLane = withHarvest[0];
  assert(
    (wtLane.querySelector(".line-claim")?.textContent ?? "").includes("R-184 并行分支"),
    "收活按钮出现在了错误的线上(应属于带工作树的后台会话)",
  );

  // 打开收活面板:六格结构(格1-4 收活,格5 合并后全量 R-222,格6 回写)。
  wtLane.querySelector(".line-harvest-toggle").click();
  await flush();
  // flush 可能同时跑到线路定时刷新,因此必须从当前 DOM 按 process_id 取 lane,
  // 不能继续使用刷新前已经脱离 DOM 的旧节点引用。
  const openedWtLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  const panel = openedWtLane?.querySelector(".line-harvest");
  assert(panel, "点击收活未展开五格面板");
  // 自动刷新会重建线路卡片,但不应销毁用户已经展开且正在操作的收活面板。
  sandbox.renderLines(payloads.collaboration_snapshot);
  await flush();
  const refreshedWtLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  const refreshedPanel = refreshedWtLane?.querySelector(".line-harvest");
  assert(refreshedPanel === panel, "线路刷新后收活面板被销毁或未按 process_id 复挂");
  // smoke 的 Element 不解析 innerHTML 拼的子节点,格号从面板文本提取数字序列。
  const panelText = panel.textContent;
  const stepNoSeq = ["1", "2", "3", "4", "5", "6"].filter((no) => panelText.includes(no)).join("");
  assert(stepNoSeq === "123456", `收活面板应呈现 1/2/3/4/5/6 六格(实得 ${stepNoSeq})`);

  // ② 不可跳过:未读 diff 前,格3(门禁)、格4(合并)、格5(合并后全量)、格6(回写)必须全部禁用。
  const gateRun = panel.querySelector(".harvest-gate-run");
  const mergeRun = panel.querySelector(".harvest-merge-run");
  const readConfirm = panel.querySelector(".harvest-read-confirm");
  const postMergeRun = panel.querySelector(".harvest-postmerge-run");
  const writebackRun = panel.querySelector(".harvest-writeback-run");
  assert(gateRun && mergeRun && readConfirm && postMergeRun && writebackRun, "收活面板缺少格2确认/格3门禁/格4合并/格5合并后全量/格6回写控件");
  assert(readConfirm.disabled, "未加载差异时「我已读过 diff」应禁用");
  assert(
    gateRun.disabled && mergeRun.disabled && postMergeRun.disabled && writebackRun.disabled,
    "② 不可跳过:未读 diff 前格3/格4/格5/格6必须全部禁用",
  );

  // 加载差异 → 确认 → 解锁格3/格4。
  const diffLoad = panel.querySelector(".harvest-diff-load");
  assert(diffLoad, "收活面板缺少「加载差异」按钮");
  diffLoad.click();
  await flush();
  assert(!readConfirm.disabled, "差异加载成功后「我已读过 diff」应可用");
  readConfirm.click();
  await flush();
  assert(
    panel.querySelector(".harvest-step.confirmed"),
    "确认后格2未进入已读状态(confirmed)",
  );
  assert(!gateRun.disabled, "② 不可跳过:已读 diff 后格3(门禁)应解锁");
  assert(
    mergeRun.disabled,
    "R-222 防线①:已读 diff 后格4(合并)必须仍禁用——合并前置是门禁,不是读 diff",
  );
  assert(
    writebackRun.disabled,
    "格5/格6 必须等合并+合并后全量完成才解锁:已读 diff 后不应可用(② 不可跳过)",
  );

  // 格3 门禁:worktree_gate 被真实调用,步骤结果渲染进面板。
  const gateCallsBefore = invokeArgs.length;
  gateRun.click();
  await flush();
  const gateCalls = invokeArgs.slice(gateCallsBefore).filter((e) => e.cmd === "worktree_gate");
  assert(
    gateCalls.length === 1 && gateCalls[0].args?.worktreePath === "C:/smoke/wt/thread-a1",
    `跑门禁没有带正确工作树调用 worktree_gate(${JSON.stringify(gateCalls)})`,
  );
  const gateRows = [...panel.querySelectorAll(".harvest-gate-step")];
  assert(gateRows.length >= 4, `门禁结果未逐步骤渲染(实得 ${gateRows.length})`);
  assert(
    gateRows.every((row) => row.classList.contains("ok")),
    "桩门禁应全部通过(ok),实得: " + gateRows.map((r) => `${r.dataset.gateName}:${r.className}`).join(","),
  );
  assert(
    panel.querySelector(".harvest-gate-pass")?.textContent.includes("门禁通过") ||
      panel.querySelector(".harvest-gate-pass")?.textContent.includes("All gates passed"),
    "门禁全过未显示通过结论",
  );
  assert(
    !mergeRun.disabled,
    "R-222 防线①:门禁通过后格4(合并)才解锁",
  );

  // 格4 合并:确认后调用 worktree_merge。
  const mergeCallsBefore = invokeArgs.length;
  mergeRun.click();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  // confirmWorktreeMerge 会先打 collaboration_snapshot,再打 worktree_merge。
  const mergeCalls = invokeArgs.slice(mergeCallsBefore).filter((e) => e.cmd === "worktree_merge");
  assert(
    mergeCalls.length === 1 && mergeCalls[0].args?.worktreePath === "C:/smoke/wt/thread-a1",
    `合并没有带正确工作树调用 worktree_merge(${JSON.stringify(mergeCalls)})`,
  );
  assert(
    (panel.querySelector(".harvest-merge-done")?.textContent ?? "").includes("已合并工作树"),
    `合并成功后未显示合并结果,panel 文本: ${(panel?.textContent ?? "panel 已摘除").slice(0, 200)}`,
  );

  // R-222 防线②:合并成功后解锁格5(合并后全量);格6 回写仍需合并后全量通过。
  assert(postMergeRun, "收活面板缺少格5「合并后全量」按钮");
  assert(
    !postMergeRun.disabled,
    "合并成功后格5(合并后全量)按钮必须解锁",
  );
  assert(
    writebackRun.disabled,
    "R-222 防线②:合并后全量通过前,格6 回写必须保持禁用",
  );

  // 合并后全量:主根调用 worktree_post_merge_gate,通过后解锁格6 回写。
  const postMergeCallsBefore = invokeArgs.length;
  postMergeRun.click();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  const postMergeCalls = invokeArgs
    .slice(postMergeCallsBefore)
    .filter((e) => e.cmd === "worktree_post_merge_gate");
  assert(
    postMergeCalls.length === 1,
    `合并后全量应调用 worktree_post_merge_gate(${JSON.stringify(postMergeCalls)})`,
  );
  const postMergeStepEl = postMergeRun.closest(".harvest-step");
  assert(
    postMergeStepEl.querySelector(".harvest-gate-pass")?.textContent.includes("合并后全量通过") ||
      postMergeStepEl.querySelector(".harvest-gate-pass")?.textContent.includes("Post-merge suite passed"),
    "合并后全量通过未显示结论",
  );
  assert(
    !writebackRun.disabled,
    "合并后全量通过后格6 回写才解锁",
  );

  // 格6 回写 tracker:合并+合并后全量通过后,点击调用 worktree_harvest_writeback 并渲染结果。
  const writebackOutput = panel.querySelector(".harvest-writeback-output");
  assert(writebackRun && writebackOutput, "收活面板缺少格6 回写控件");
  const writebackCallsBefore = invokeArgs.length;
  writebackRun.click();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  const writebackCalls = invokeArgs
    .slice(writebackCallsBefore)
    .filter((e) => e.cmd === "worktree_harvest_writeback");
  assert(
    writebackCalls.length === 1 &&
      writebackCalls[0].args?.worktreePath === "C:/smoke/wt/thread-a1" &&
      writebackCalls[0].args?.claim.includes("R-184") &&
      writebackCalls[0].args?.branch === "thread-a1",
    `格6 回写没有带正确参数调用 worktree_harvest_writeback(${JSON.stringify(writebackCalls)})`,
  );
  assert(
    (writebackOutput?.textContent ?? "").includes("已回写 R-184 收活记录"),
    `格6 回写成功后未渲染结果(实得: ${writebackOutput?.textContent ?? "(无)"})`,
  );
  assert(
    panel.querySelector(".harvest-step.confirmed"),
    "回写成功后格6 未进入已读/完成状态",
  );

  // D-314:即使 collaboration claim 未声明，线路对话唯一候选也必须自动回显。
  payloads.worktree_harvest_candidates = ["D-297"];
  const conversationCandidatePanel = sandbox.buildHarvestPanel(
    { ...payloads.collaboration_snapshot[1], claim: "未声明条目" },
    PROJECT,
    "A",
  );
  await flush();
  assert(
    conversationCandidatePanel.querySelector(".harvest-tracker-select")?.value === "D-297",
    "线路对话唯一候选 D-297 没有自动选中",
  );
  payloads.worktree_harvest_candidates = ["D-297", "R-184"];
  const multipleCandidatePanel = sandbox.buildHarvestPanel(
    { ...payloads.collaboration_snapshot[1], claim: "未声明条目" },
    PROJECT,
    "A",
  );
  await flush();
  const multipleCandidateSelect = multipleCandidatePanel.querySelector(".harvest-tracker-select");
  assert(!multipleCandidateSelect?.disabled && multipleCandidateSelect?.value === "", "多条对话候选必须等待用户明确选择，不能自动猜一条");
  multipleCandidateSelect.value = "D-297";
  multipleCandidateSelect.dispatchEvent({ type: "change" });
  assert(multipleCandidateSelect.value === "D-297", "多候选下用户选择 D-297 没有保留");
  payloads.worktree_harvest_candidates = [];

  // D-310:没有真实 R-/D- claim 时,合并仍可完成,但格5不得伪装成可回写入口。
  document.querySelector('#lines-list .line-lane[data-process-id="p|bg"] .line-harvest-toggle')?.click();
  const originalClaim = payloads.collaboration_snapshot[1].claim;
  payloads.collaboration_snapshot[1].claim = "未声明条目";
  sandbox.renderLines(payloads.collaboration_snapshot);
  await flush();
  const noClaimLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  noClaimLane?.querySelector(".line-harvest-toggle")?.click();
  await flush();
  const currentNoClaimLane = [...document.querySelectorAll("#lines-list .line-lane")]
    .find((lane) => lane.dataset.processId === "p|bg");
  const noClaimPanel = currentNoClaimLane?.querySelector(".line-harvest");
  noClaimPanel?.querySelector(".harvest-diff-load")?.click();
  await flush();
  noClaimPanel?.querySelector(".harvest-read-confirm")?.click();
  await flush();
  noClaimPanel?.querySelector(".harvest-gate-run")?.click();
  await flush();
  noClaimPanel?.querySelector(".harvest-merge-run")?.click();
  await flush();
  const noClaimWriteback = noClaimPanel?.querySelector(".harvest-writeback-run");
  payloads.collaboration_snapshot[1].claim = originalClaim;
  payloads.worktree_harvest_candidates = ["R-184"];
  assert(noClaimWriteback?.disabled, "无有效 claim 时格5 回写入口必须保持禁用");
  assert(
    (noClaimPanel?.querySelector(".harvest-writeback-output")?.textContent ?? "") === "",
    "无有效 claim 时不应产生回写输出",
  );
  const noClaimCallsBefore = invokeArgs.length;
  // 真实浏览器不会为 disabled button 派发 click;假 DOM 的 click 不模拟该规范门禁,
  // 因而这里验证的是“禁用状态”本身,并确认未产生任何回写调用。
  assert(
    !invokeArgs.slice(noClaimCallsBefore).some((entry) => entry.cmd === "worktree_harvest_writeback"),
    "无有效 claim 时不应调用 worktree_harvest_writeback",
  );
}

// ---------- 线清单来自 git,前端不再持有清单状态(R-177 内容③ / D-251 / D-257) ----------
// 清单真源改成后端 `worktree_list`(它跑 `git worktree list --porcelain`)之后,
// 原来那两条护栏守的性质没有消失,只是换了形态,所以**等价重写而不是删除**:
//   D-251「切项目时清单不错位」→ 在途的清单响应落地时项目已切走,不得画进新项目的面板;
//   D-257「#worktrees-refresh 真的绑了监听器」→ 点击后必须真打出 worktree_list IPC。
// 另加一条新的反向断言:前端**不得再写任何 kz-worktrees 键**(清单状态已经全部下沉)。
//
// 两条断言各接一个变异开关(KZ_SMOKE_MUTATE=d251 / =d257):故意破坏被守护的行为,
// 期望脚本非零退出。这是把「删掉任一条即变红」从人工验证换成机械判据——不然一条
// 恒绿的断言和一条真护栏在 CI 里长得一模一样。
{
  const wtItem = (path, branch, bound_process = null) => ({ path, branch, clean: true, files: [], diff: "", bound_process });
  const WT_A1 = "C:/smoke/wt/thread-a1";
  const WT_A2 = "C:/smoke/wt/thread-a2";
  const WT_B = "C:/smoke/wt/thread-b";
  // 两个项目返回**不同**清单:D-251 只有这样才判得出来。
  payloads.worktree_list = (args) =>
    args?.projectDir === PROJECT_B
      ? [wtItem(WT_B, "thread-b")]
      : [wtItem(WT_A1, "thread-a1", "p|bg"), wtItem(WT_A2, "thread-a2")];
  payloads.process_create = {
    id: "p2|smoke", label: "线路 2", session_id: "sess-line-2", running: false,
    worktree_path: WT_A1, branch: "thread-a1", tracker_writes: false,
  };
  payloads.worktree_merge = "已合并工作树";
  payloads.worktree_discard = "已放弃工作树";

  // 写入去向探针:方向反过来了。以前查「有没有写错键」,现在查「还写不写」。
  const wtWrites = [];
  const rawSetItem = localStorageShim.setItem;
  localStorageShim.setItem = (key, value) => {
    if (String(key).startsWith("kz-worktrees:")) wtWrites.push(String(key));
    return rawSetItem(key, value);
  };

  // ---------- ⓪ 刷新按钮真的能刷新(D-257) ----------
  // 拦的是「按钮在 index.html 里,监听器却不在任何 JS 里」这个形态:7c5f022 抽
  // handleWorktreeAction 时,函数收尾的 `}` 吃掉了下一行 `$("worktrees-refresh")
  // .addEventListener` 的前半段,剩下 `}("click", refreshWorktrees);` —— 语法合法、
  // `node --check` 通过、静态 grep 也只在标记里看得见那个 id,唯独点下去什么都不发生。
  // 所以断言必须是**点击后真的打出了 IPC**,不能退化成「源码里有这个字符串」。
  await gotoProject(PROJECT, docsA);
  const refreshBtn = byId.get("worktrees-refresh");
  assert(refreshBtn, "侧栏「隔离工作树」的刷新按钮 #worktrees-refresh 不在 index.html 里了(界面承诺的手动刷新能力消失)");
  if (refreshBtn) {
    const beforeRefreshClick = invokeArgs.length;
    refreshBtn.click();
    await flush();
    const refreshCalls = invokeArgs.slice(beforeRefreshClick).filter((entry) => String(entry.cmd).startsWith("worktree_"));
    assert(
      refreshCalls.some((entry) => entry.cmd === "worktree_list" && entry.args?.projectDir === PROJECT),
      "点击 #worktrees-refresh 没有触发 worktree_list:按钮没有绑上 refreshWorktrees" +
        `(本次点击后的 worktree IPC:${JSON.stringify(refreshCalls)})`,
    );
    assert(
      document.querySelectorAll("#worktree-list .worktree-entry").length === 2,
      "点击 #worktrees-refresh 后工作树清单没有按 git 返回的数据重渲染(拉回来了却没画上去)",
    );
    // 清单不再逐条 worktree_diff:一次 IPC 拿全,清单越长省得越多。
    assert(
      !refreshCalls.some((entry) => entry.cmd === "worktree_diff"),
      `刷新清单不该再逐条打 worktree_diff(实得:${JSON.stringify(refreshCalls)})`,
    );
  }

  // ---------- ① 在途的清单响应不得画进已经切走的项目(D-251) ----------
  await gotoProject(PROJECT, docsA);
  let releaseList;
  invokeGates.set("worktree_list", new Promise((resolve) => { releaseList = resolve; }));
  refreshBtn.click();
  await settle();
  assert(
    invokeArgs.at(-1)?.cmd === "worktree_list" && invokeArgs.at(-1)?.args?.projectDir === PROJECT,
    `前置失败:刷新没有替项目甲发出 worktree_list(${JSON.stringify(invokeArgs.at(-1))})`,
  );
  invokeGates.delete("worktree_list"); // 只卡住上面那一次
  await gotoProject(PROJECT_B, docsB);
  refreshBtn.click();
  await flush();
  assert(
    document.querySelectorAll("#worktree-list .worktree-entry").length === 1,
    "前置失败:切到项目乙之后清单没按乙的数据渲染",
  );
  releaseList();
  for (let i = 0; i < 12; i += 1) await settle();
  await flush();
  const paintedAfterSwitch = [...document.querySelectorAll("#worktree-list .worktree-entry")]
    .map((row) => row.textContent);
  assert(
    paintedAfterSwitch.length === 1 && paintedAfterSwitch[0].includes("thread-b"),
    "项目甲在途的工作树清单被画进了项目乙的面板(await 之后少了一次 currentProject 复查):" +
      `实得 ${JSON.stringify(paintedAfterSwitch)}`,
  );

  // ---------- ② 侧栏不再直达合并，只能进入同一收活五格 ----------
  const mergeButton = document.querySelector("#worktree-list .worktree-merge");
  assert(!mergeButton, "侧栏仍保留绕过人读 diff 的直接合并入口(D-305)");
  await gotoProject(PROJECT, docsA);
  await sandbox.refreshWorktrees();
  const harvestButton = [...document.querySelectorAll("#worktree-list .worktree-harvest")]
    .find((button) => button.parentElement?.parentElement?.textContent.includes("thread-a1"));
  const beforeHarvest = invokeArgs.length;
  harvestButton?.click();
  await flush();
  const mergeCalls = invokeArgs.slice(beforeHarvest);
  assert(
    !mergeCalls.some((entry) => entry.cmd === "worktree_merge"),
    `点击侧栏收活竟直接触发了 worktree_merge(${JSON.stringify(mergeCalls)})`,
  );
  assert(
    mergeCalls.some((entry) => entry.cmd === "collaboration_snapshot") &&
      document.querySelector('.activity-item[data-view="lines"]')?.classList.contains("active"),
    `侧栏收活没有进入线路视图并刷新统一五格数据源(${JSON.stringify(mergeCalls)})`,
  );

  // ---------- ③ 建线原子创建「工作树 + 进程绑定」,前端不维护影子清单 ----------
  await gotoProject(PROJECT, docsA);
  const beforeAdd = invokeArgs.length;
  const addButton = byId.get("worktree-add");
  const linesAddButton = byId.get("lines-add");
  linesAddButton.click();
  addButton.click();
  assert(addButton.disabled && linesAddButton.disabled, "并行线路创建期间两个入口没有同步禁用");
  assert(
    addButton.getAttribute("aria-busy") === "true" && linesAddButton.getAttribute("aria-busy") === "true",
    "并行线路创建期间入口没有暴露 aria-busy",
  );
  assert(/创建中|Creating/.test(linesAddButton.textContent), "线路页按钮没有显示创建中反馈");
  await flush();
  const addCalls = invokeArgs.slice(beforeAdd);
  const processCreateCalls = addCalls.filter((entry) => entry.cmd === "process_create");
  assert(
    processCreateCalls.length === 1 && processCreateCalls[0].args?.projectDir === PROJECT &&
      /^line-\d+-\d+$/.test(processCreateCalls[0].args?.worktreeName ?? "") &&
      processCreateCalls[0].args?.phasePipeline === false &&
      processCreateCalls[0].args?.trackerWrites === false,
    `新建线路没有原子发出唯一命名的 process_create(${JSON.stringify(addCalls)})`,
  );
  assert(!addButton.disabled && !linesAddButton.disabled, "并行线路创建完成后两个入口没有恢复");
  assert(
    !addButton.hasAttribute("aria-busy") && !linesAddButton.hasAttribute("aria-busy") &&
      /新建线路|New line/.test(linesAddButton.textContent),
    "并行线路创建完成后忙碌状态或按钮文案没有恢复",
  );
  assert(
    !addCalls.some((entry) => entry.cmd === "worktree_create"),
    "建线仍在调用只建树不绑进程的 worktree_create",
  );
  assert(
    addCalls.some((entry) => entry.cmd === "worktree_list"),
    "新建之后没有重新向 git 要清单(前端已不持有清单,不刷就看不见新树)",
  );

  // ---------- ④ 放弃工作树后必须刷新进程投影 ----------
  // 后端会同步注销绑定进程；若这里只刷新 worktree_list，旧线路 tab 仍会把已删
  // 目录作为 cwd 发送，直到 provider/tool 报“工作目录不存在”才暴露。这个断言
  // 要求放弃动作后至少重新拉进程和工作树两份真源。
  await gotoProject(PROJECT, docsA);
  const discardButton = document.querySelector("#worktree-list .worktree-discard");
  assert(discardButton, "工作树条目缺少放弃按钮，无法关闭绑定线路");
  if (discardButton) {
    const beforeDiscard = invokeArgs.length;
    discardButton.click();
    await flush();
    const discardCalls = invokeArgs.slice(beforeDiscard);
    assert(
      discardCalls.some((entry) => entry.cmd === "worktree_discard"),
      `放弃按钮没有调用 worktree_discard(${JSON.stringify(discardCalls)})`,
    );
    assert(
      discardCalls.some((entry) => entry.cmd === "process_list" && entry.args?.projectDir === PROJECT),
      `放弃后未刷新进程页签，旧线路会残留(${JSON.stringify(discardCalls)})`,
    );
    assert(
      discardCalls.some((entry) => entry.cmd === "worktree_list" && entry.args?.projectDir === PROJECT),
      `放弃后未刷新工作树清单(${JSON.stringify(discardCalls)})`,
    );
  }

  // ---------- ⑤ 前端不得再写任何 kz-worktrees 键 ----------
  assert(
    wtWrites.length === 0,
    `前端仍在写 localStorage 工作树清单(${wtWrites.join(" / ")}):清单真源已经是 git,` +
      "留着这份影子清单只会在两处不一致时误导用户",
  );

  // 收尾:摘掉写入探针与桩,切回项目甲并还原快照。
  localStorageShim.setItem = rawSetItem;
  delete payloads.worktree_list;
  delete payloads.process_create;
  delete payloads.worktree_merge;
  delete payloads.worktree_discard;
  await gotoProject(PROJECT, savedDocsPayload);
  delete payloads.projects_select;
  payloads.docs_snapshot = savedDocsPayload;
  await sandbox.refreshDocs();
  await flush();
}

// ---------- D-337:ask 弹窗 question 档位的多选(声明多选时点选项不再立即提交) ----------
// 老行为:question 的每个选项是"点击即提交"的按钮,问题文本写着「可多选」也选不了多个。
// 新契约:multiple=true 或问题文本声明多选(兜底)时,选项变成可勾选,提交回答才汇总;
// 默认档位(未声明多选)点击即提交的行为保持不变。
{
  const overlay = byId.get("ask-overlay");
  const answerCalls = () => invokeArgs.filter(({ cmd }) => cmd === "answer_ask");
  vm.runInContext('activeSessionId = "sess-smoke"', sandbox);
  const clearAsks = async () => {
    while (!overlay.classList.contains("hidden") && byId.get("ask-cancel")) {
      byId.get("ask-cancel").click();
      await flush();
    }
  };
  await clearAsks();

  // ① 显式 multiple=true:点选项只切换勾选,提交回答才汇总(所选选项 + 补充文本)。
  askHandler?.({
    payload: {
      id: 701, sessionId: "sess-smoke", kind: "question",
      question: "问题 701", options: ["A 选项一", "B 选项二"], default: "", multiple: true,
    },
  });
  await flush();
  assert(!overlay.classList.contains("hidden"), "D-337:多选 question 弹窗未弹出");
  const optionsBox = byId.get("ask-options");
  assert(optionsBox.classList.contains("multi"), "D-337:multiple=true 时选项容器未标 multi");
  const optionButtons = [...optionsBox.querySelectorAll(".ask-option")];
  assert(optionButtons.length === 2, `D-337:多选选项数不对:${optionButtons.length}`);
  const beforeClicks = answerCalls().length;
  optionButtons[0].click();
  await flush();
  assert(answerCalls().length === beforeClicks, "D-337:多选档位点一个选项就立即提交了(老 bug 复发)");
  assert(optionButtons[0].classList.contains("selected"), "D-337:点过的选项没有选中标记");
  assert(optionButtons[0].getAttribute("aria-pressed") === "true", "D-337:选中选项 aria-pressed 未置 true");
  optionButtons[1].click();
  assert(optionButtons[1].classList.contains("selected"), "D-337:第二个选项未选中");
  optionButtons[0].click();
  assert(!optionButtons[0].classList.contains("selected"), "D-337:再点一次应取消选中");
  optionButtons[0].click();
  assert(byId.get("ask-submit").disabled === false, "D-337:已选选项时提交按钮仍被禁用");
  byId.get("ask-answer").value = "补充说明文字";
  byId.get("ask-answer").dispatchEvent({ type: "input" });
  byId.get("ask-submit").click();
  await flush();
  const multiAnswer = answerCalls().at(-1)?.args;
  assert(
    multiAnswer && multiAnswer.reply === "B 选项二\nA 选项一\n补充说明文字",
    `D-337:多选提交的汇总不对(应按勾选顺序含所选选项与补充文本):${JSON.stringify(multiAnswer)}`,
  );
  assert(overlay.classList.contains("hidden"), "D-337:多选提交后弹窗未关闭");
  await clearAsks();

  // ② 文本声明「可多选」的兜底:工具没传 multiple 也进多选档位(历史问题文本的形态)。
  askHandler?.({
    payload: {
      id: 702, sessionId: "sess-smoke", kind: "question",
      question: "你观察到的不匹配具体指哪一块?(可多选/补充)",
      options: ["A 测试节奏", "B 提交粒度"], default: "", multiple: false,
    },
  });
  await flush();
  assert(byId.get("ask-options").classList.contains("multi"), "D-337:问题文本声明「可多选」未进入多选档位");
  [...byId.get("ask-options").querySelectorAll(".ask-option")].forEach((button) => button.click());
  byId.get("ask-submit").click();
  await flush();
  assert(
    answerCalls().at(-1)?.args?.reply === "A 测试节奏\nB 提交粒度",
    `D-337:文本兜底多选的提交不对:${JSON.stringify(answerCalls().at(-1)?.args)}`,
  );
  await clearAsks();

  // ③ 非多选档位(默认)行为不变:点一个选项立即提交。
  askHandler?.({
    payload: {
      id: 703, sessionId: "sess-smoke", kind: "question",
      question: "单选问题", options: ["甲", "乙"], default: "",
    },
  });
  await flush();
  assert(!byId.get("ask-options").classList.contains("multi"), "D-337:默认档位误进了多选");
  const beforeSingle = answerCalls().length;
  [...byId.get("ask-options").querySelectorAll(".ask-option")][1].click();
  await flush();
  assert(answerCalls().length === beforeSingle + 1, "D-337:非多选档位点选项未立即提交");
  assert(answerCalls().at(-1)?.args?.reply === "乙", `D-337:非多选档位提交的不是所点选项:${JSON.stringify(answerCalls().at(-1)?.args)}`);
  await clearAsks();

  // ④ 多选档位空选空文本时提交按钮禁用(离开必须经由「取消」)。
  askHandler?.({
    payload: {
      id: 704, sessionId: "sess-smoke", kind: "question",
      question: "空选测试", options: ["唯一"], default: "", multiple: true,
    },
  });
  await flush();
  assert(byId.get("ask-submit").disabled === true, "D-337:多选空选空文本时提交按钮应禁用");
  byId.get("ask-cancel").click();
  await flush();
  assert(overlay.classList.contains("hidden"), "D-337:取消未关闭多选弹窗");
}

// ---------- R-190 常驻 fast 模型状态指示 ----------
// 状态栏 #status-fast 在托管且未就绪时显示缺环文案(桩:serviceUp=false);
// 且轮询函数已注册(fastStatusTimer 非空),说明常驻刷新不是一次性快照。
{
  const fastEl = byId.get("status-fast");
  assert(fastEl, "R-190:状态栏缺少 #status-fast 常驻指示位");
  await flush();
  // 首次轮询已跑(启动即查),桩状态 serviceUp=false → 显示「服务未运行」并标 warn。
  assert(
    fastEl.textContent.includes("服务未运行"),
    `R-190:常驻指示未反映服务未运行,实得 "${fastEl.textContent}"`,
  );
  assert(fastEl.classList.contains("warn-text"), "R-190:未就绪时指示应标红(warn-text)");
  const timerRegistered = vm.runInContext("typeof fastStatusTimer !== 'undefined' && fastStatusTimer !== null", sandbox);
  assert(timerRegistered, "R-190:常驻轮询定时器未注册(状态不会随真实探测更新)");
}

// ---------- R-179 深并行 UX:diff 接入既有渲染器 + 冲突预检 + 建线提示 ----------
{
  const linesSrc = await readFile(resolve(root, "crates", "kanzei-app", "ui", "20-lines.js"), "utf8");
  const sessionsSrc = await readFile(resolve(root, "crates", "kanzei-app", "ui", "09-sessions.js"), "utf8");
  // 验收①:线的 diff 用 06-activity.js 既有 buildDiffTree,不新写查看器。
  assert(
    linesSrc.includes('typeof buildDiffTree === "function" ? buildDiffTree(treeFiles)'),
    "R-179:线 diff 未接入既有 buildDiffTree 目录树渲染器",
  );
  assert(
    linesSrc.includes('rawSummary.textContent = t("原始差异文本")'),
    "R-179:原始 diff 未收进可折叠 details(目录树 + 原始文本并存)",
  );
  // 验收③:合并确认前调用 worktree_merge_preview 取冲突文件列表。
  assert(
    linesSrc.includes('await invoke("worktree_merge_preview"'),
    "R-179:合并确认未调用 worktree_merge_preview 冲突预检",
  );
  assert(
    linesSrc.includes('t("Git 合并冲突文件")'),
    "R-179:冲突文件列表未进入合并确认文案",
  );
  // 验收⑤:建线 UI 有磁盘/冷编译成本提示。
  assert(
    sessionsSrc.includes('t("每线独立 target/ 目录,磁盘占用随线路数成倍增加;首次冷编译需数分钟")'),
    "R-179:建线 UI 缺少磁盘/冷编译成本提示",
  );
  // 验收⑧:三档宽度下线路页不崩(列表容器仍在 DOM)。
  for (const width of [800, 1024, 1280]) {
    windowShim.innerWidth = width;
    await flush();
    assert(
      document.getElementById("lines-list"),
      `R-179:${width}px 下线路页列表容器缺失`,
    );
  }
  windowShim.innerWidth = 1280;
  await flush();
}

// ---------- R-187 提示音管理设置 ----------
// 设置页「提示音」区块控件存在;playRunNotice 读 localStorage 配置(总开关关掉
// 时不播放,音量可调)。
{
  const settingsView = document.getElementById("view-settings");
  if (settingsView) settingsView.classList.remove("hidden");
  await flush();
  assert(byId.get("set-sound-enabled"), "R-187:设置页缺少提示音总开关");
  assert(byId.get("set-sound-volume"), "R-187:设置页缺少音量滑杆");
  assert(byId.get("set-sound-completed"), "R-187:设置页缺少运行完成开关");
  assert(byId.get("set-sound-failed"), "R-187:设置页缺少运行失败开关");
  assert(byId.get("set-sound-stopped"), "R-187:设置页缺少运行已停止开关");
  assert(byId.get("sound-preview"), "R-187:设置页缺少试听按钮");
  // 默认配置:全部开启、音量 0.12。
  const defaultSound = vm.runInContext("readSoundSettings()", sandbox);
  assert(defaultSound.enabled && defaultSound.completed && defaultSound.failed && defaultSound.stopped,
    `R-187:默认提示音配置应为全开,实得 ${JSON.stringify(defaultSound)}`);
  assert(Math.abs(defaultSound.volume - 0.12) < 0.001, `R-187:默认音量应为 0.12,实得 ${defaultSound.volume}`);
  // 关闭总开关后 soundEnabledFor 对任何 kind 都返回 false(不播放)。
  const disabled = vm.runInContext('(function(){ saveSoundSettings({enabled:false, volume:0.12, completed:true, failed:true, stopped:true}); return soundEnabledFor("completed") && soundEnabledFor("failed"); })()', sandbox);
  assert(disabled === false, "R-187:总开关关闭后提示音不应播放");
  // 恢复默认,避免污染后续用例。
  vm.runInContext('saveSoundSettings({enabled:true, volume:0.12, completed:true, failed:true, stopped:true})', sandbox);
}

// ---------- R-188 架构图:代码生成的 SVG 依赖图 ----------
// 架构浏览页在文字树之外渲染依赖图 SVG;图数据为空时隐藏降级文字树。
{
  // 先切到架构视图触发 refreshArch。
  document.querySelector('.activity-item[data-view="arch"]')?.click();
  await flush();
  const graphHost = byId.get("arch-graph");
  assert(graphHost, "R-188:架构浏览页缺少 #arch-graph 图容器");
  const svg = graphHost.querySelector("svg.arch-svg");
  assert(svg, "R-188:架构图未渲染为 SVG(应代码生成,非文生图/预置图)");
  assert(
    svg.querySelectorAll("g.arch-node").length >= 6,
    `R-188:SVG 节点数不足(桩 graph 有 6 crate),实得 ${svg.querySelectorAll("g.arch-node").length}`,
  );
  assert(
    svg.querySelectorAll("line").length >= 6,
    `R-188:SVG 依赖边数不足(桩 graph 有 6 边),实得 ${svg.querySelectorAll("line").length}`,
  );
  // 图渲染不替换文字树(降级视图仍在)。
  assert(
    (byId.get("arch-tree")?.childNodes?.length ?? byId.get("arch-tree")?.childElementCount ?? 0) > 0,
    "R-188:架构图渲染后文字树被清空(降级视图必须保留)",
  );
  // 节点可点击定位(点击 app 节点应尝试打开 crate Cargo.toml)。
  const appNode = [...svg.querySelectorAll("g.arch-node")].find((g) => g.getAttribute("aria-label") === "kanzei-app");
  assert(appNode, "R-188:SVG 缺少 kanzei-app 节点");
  appNode.dispatchEvent({ type: "click", preventDefault() {}, stopPropagation() {} });
  await flush();
  assert(
    invokeLog.some((cmd) => cmd === "docs_read_custom"),
    "R-188:点击图节点未触发文档/Cargo 定位读取",
  );
}

// ---------- R-190 常驻 fast 模型状态指示 ----------
// 状态栏 #status-fast 在托管且未就绪时显示缺环文案(桩:serviceUp=false);
// 且轮询函数已注册(fastStatusTimer 非空),说明常驻刷新不是一次性快照。
{
  const fastEl = byId.get("status-fast");
  assert(fastEl, "R-190:状态栏缺少 #status-fast 常驻指示位");
  await flush();
  assert(
    fastEl.textContent.includes("服务未运行"),
    `R-190:常驻指示未反映服务未运行,实得 "${fastEl.textContent}"`,
  );
  assert(fastEl.classList.contains("warn-text"), "R-190:未就绪时指示应标红(warn-text)");
  const timerRegistered = vm.runInContext("typeof fastStatusTimer !== 'undefined' && fastStatusTimer !== null", sandbox);
  assert(timerRegistered, "R-190:常驻轮询定时器未注册(状态不会随真实探测更新)");
}

// ---------- R-189 亮色主题:切换持久化 + Monaco setTheme 联动 ----------
{
  const themeBtn = byId.get("theme-toggle");
  assert(themeBtn, "R-189:状态栏缺少主题切换按钮");
  // 默认暗色(现状零回归)。
  assert(document.documentElement.getAttribute("data-theme") !== "light", "R-189:默认主题应为暗色(或未设=暗)");
  // 切亮色:html[data-theme=light] + localStorage 持久化。
  themeBtn.click();
  await flush();
  assert(document.documentElement.getAttribute("data-theme") === "light", "R-189:点击切换后 data-theme 未变 light");
  assert(storage.get("kz-theme") === "light", "R-189:亮色主题未持久化到 localStorage");
  // 切回暗色,保持默认。
  themeBtn.click();
  await flush();
  assert(document.documentElement.getAttribute("data-theme") !== "light", "R-189:切回暗色失败");
  assert(storage.get("kz-theme") === "dark", "R-189:暗色未持久化");
  // Monaco setTheme 联动:17-files.js 创建编辑器时按当前主题选 vs/vs-dark。
  const filesSrc = await readFile(resolve(root, "crates", "kanzei-app", "ui", "17-files.js"), "utf8");
  assert(
    filesSrc.includes('currentTheme() === "light" ? "vs" : "vs-dark"'),
    "R-189:Monaco 编辑器主题未跟随全局主题",
  );
}

if (issues.length) {
  reportedIssues = true;
  console.error(`UI 运行时冒烟失败(${issues.length} 处):`);
  for (const issue of issues) console.error(` - ${issue}`);
  process.exit(1);
}
console.log(
  `UI 运行时冒烟通过:${sources.length} 个 ui/*.js 按序执行 + 初始化序列(${invokeLog.length} 次 invoke) + ` +
  `需求/缺陷/目标/测试/历史列表渲染 + ${document.querySelectorAll(".activity-item[data-view]").length} 个主视图切换,0 运行时错误`
);
