/* Interaction prototype. All records and mutations stay in this page's memory. */
"use strict";
const $ = id => document.getElementById(id);
const esc = value => String(value ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const reduced = matchMedia("(prefers-reduced-motion: reduce)");
const viewNames = { overview: "总控台", deliveries: "成果与验收", decisions: "决策与偏好", memory: "记忆关系" };
let state, graph, graphObserver, graphTimer, playbackTimers = [], toastTimer;
const seed = () => ({
  view: "overview", scope: "all", selection: { kind: "decision", id: "d1" }, feedbackOpen: false,
  selected: new Set(["d1", "d2", "d3"]), node: null,
  projects: [
    { id: "kanzei", name: "Kanzei", kind: "开发工具", goal: "让三个项目在一页持续推进", current: "总控台 · 项目状态聚合", task: "正在连接跨项目事件", active: true, lines: 2, changes: "刚刚 · 项目切换不再影响后台任务", rework: [] },
    { id: "md", name: "MD 文件保存", kind: "应用", goal: "打开文件、保留附件、搜索内容", current: "Reader · 搜索与附件", task: "正在验证 PDF 检索", active: true, lines: 2, changes: "2 分钟前 · Reader 已形成可试用版本", rework: [] },
    { id: "agentos", name: "AgentOS", kind: "平台", goal: "每次运行都能看到结果和依据", current: "运行结果 · 证据展示", task: "正在补全结果详情", active: true, lines: 1, changes: "5 分钟前 · API 路由方案已采用", rework: [] }
  ],
  decisions: [
    { id: "d1", code: "DEC · 024", project: "md", title: "附件应该怎样保存？", question: "原附件保留在原位，还是复制到应用自己的目录？", choice: "复制到应用目录，同时保留源文件", alternative: "直接引用原始路径；移动到应用目录", reason: "原文件改名或移动后，笔记仍可打开附件。保留源文件也符合这个项目已经确认的删除偏好。", source: "项目偏好 · 删除条目时保留源文件", impact: "可调整 · 影响附件存储与导入流程", delivery: "reader", status: "pending", time: "8 分钟前", feedback: "" },
    { id: "d2", code: "DEC · 025", project: "kanzei", title: "关系图继续用现有库吗？", question: "为了关系动画，是否需要换成新的图形技术栈？", choice: "保留 force-graph，先改关系与交互", alternative: "切到 Sigma.js；单独手写 Canvas", reason: "当前离线库已经支持动态关系、镜头聚焦和粒子。沿用共享渲染器，可以先交付可审阅的决策关系。", source: "代码事实 · 已有离线 force-graph", impact: "可调整 · 影响记忆页与共享图形视图", delivery: "console", status: "pending", time: "12 分钟前", feedback: "" },
    { id: "d3", code: "DEC · 026", project: "agentos", title: "前端默认请求哪个地址？", question: "未显式配置 API 地址时，前端应如何选择服务地址？", choice: "默认使用同源 API 地址", alternative: "默认固定本机端口；使用预设外部地址", reason: "让部署环境提供实际服务地址，避免把开发机地址写进浏览器产物。显式配置仍然有效。", source: "示例项目约定 · 部署地址随环境变化", impact: "可调整 · 影响默认请求路由", delivery: "result", status: "pending", time: "19 分钟前", feedback: "" }
  ],
  deliveries: [
    { id: "reader", project: "md", title: "Reader：打开、附件与搜索", description: "能打开已导入文件，并从正文搜索到对应位置。", version: "reader · demo-r3", evidence: "附件保存 / 搜索路径通过", boundary: "真机 OCR 另行验证", state: "ready", steps: ["进入 Reader，打开一份已导入的文档。", "选择附件，再输入正文中的关键词。", "确认附件可见，搜索结果能回到正文。"] },
    { id: "console", project: "kanzei", title: "总控台：多个项目的运行状态", description: "在一个页面查看任务、待试用成果和自动决策。", version: "console · demo-r2", evidence: "项目范围 / 状态聚合通过", boundary: "安装包尚未发布", state: "ready", steps: ["同时打开两个示例项目。", "切换项目筛选，确认其他项目仍在运行。", "从决定清单打开关联成果和依据。"] },
    { id: "result", project: "agentos", title: "运行结果：证据与版本", description: "每次结果都附带产生它的运行版本和来源。", version: "result · demo-r4", evidence: "结果读取 / 来源链接通过", boundary: "生产环境尚未验证", state: "ready", steps: ["进入运行记录，选择已完成的运行。", "打开结果，查看版本和证据入口。", "核对示例来源与本次运行是否匹配。"] }
  ],
  preferences: [{ id: "pref0", scope: "md", text: "删除条目时保留源文件，只移除索引。", source: "上一个示例波次 · 用户纠正", version: 1 }]
});
function project(id) { return state.projects.find(p => p.id === id); }
function visibleProject(id) { return state.scope === "all" || state.scope === id; }
function visibleDecisions() { return state.decisions.filter(d => visibleProject(d.project)); }
function pending() { return state.decisions.filter(d => d.status === "pending"); }
function ready() { return state.deliveries.filter(d => d.state === "ready"); }
function showToast(message) { clearTimeout(toastTimer); $("toast").textContent = message; $("toast").classList.add("show"); toastTimer = setTimeout(() => $("toast").classList.remove("show"), 3600); }
function cleanupGraph() {
  clearTimeout(graphTimer); playbackTimers.forEach(clearTimeout); playbackTimers = [];
  graphObserver?.disconnect(); graphObserver = null;
  if (graph) { graph.pauseAnimation(); graph._destructor?.(); graph = null; }
}
function render() {
  cleanupGraph();
  const meta = {
    overview: ["多项目总控", "查看各项目进展、可用成果和自主决定。"],
    deliveries: ["本波次成果", "按成果试用，分别查看实现、验证和你的反馈。"],
    decisions: ["决策与偏好", "认可一次，或纠正并记住；决定会跟随成果一起更新。"],
    memory: ["决策关系", "沿着一条关系，看到选择、反馈与下一次采用。"]
  }[state.view];
  $("page-title").textContent = meta[0]; $("page-description").textContent = meta[1];
  $("breadcrumb-view").textContent = viewNames[state.view];
  document.querySelectorAll("[data-view]").forEach(el => { el.classList.toggle("selected", el.dataset.view === state.view); el.setAttribute("aria-current", el.dataset.view === state.view ? "page" : "false"); });
  $("nav-decisions").textContent = pending().length; $("nav-deliveries").textContent = ready().length;
  $("summary").innerHTML = [[state.projects.filter(p => p.active).length, "项目推进中"], [ready().length, "成果待试用"], [pending().length, "决定待审阅"], [1, "单元缺信息"]].map(([number, label]) => `<div class="metric"><strong>${number.toString().padStart(2, "0")}</strong><span>${label}</span></div>`).join("");
  $("project-nav").innerHTML = state.projects.map(p => `<button class="project-button ${state.scope === p.id ? "active" : ""}" data-scope="${p.id}"><span class="project-dot ${p.active ? "running" : ""}"></span>${esc(p.name)}</button>`).join("");
  $("scope-tabs").innerHTML = [{ id: "all", name: "全部项目" }, ...state.projects].map(p => `<button class="scope-tab ${state.scope === p.id ? "active" : ""}" data-scope="${p.id}" aria-pressed="${state.scope === p.id}">${esc(p.name)}</button>`).join("");
  $("main-content").innerHTML = ({ overview: overviewHTML, deliveries: deliveriesHTML, decisions: decisionsHTML, memory: memoryHTML })[state.view]();
  renderInspector();
  if (state.view === "memory") requestAnimationFrame(createGraph);
}
function outputRows() {
  const items = state.deliveries.filter(d => visibleProject(d.project));
  return items.map(d => `<div class="output-row"><span class="output-mark">${d.state === "stale" ? "↻" : "↗"}</span><div class="output-main"><h3>${esc(d.title)}</h3><p>${esc(project(d.project).name)} · <span class="${d.state === "stale" ? "warn" : "verified"}">${d.state === "stale" ? "决定已修改，等待新版本" : d.state === "accepted" ? "你已认可这一版" : "机器验证通过 · 等你试用"}</span></p></div><button data-delivery="${d.id}">查看成果 →</button></div>`).join("");
}
function overviewHTML() {
  return `${visibleProject("md") ? `<div class="notice"><span>MD 文件保存 · 真机 OCR 缺少测试设备<small>只等待这一项，Reader 和其他项目继续推进。</small></span><button data-blocked="true">查看 →</button></div>` : ""}
    <div class="section-title"><h2>正在推进</h2><span>按结果组织工作</span></div>
    <div class="column-labels"><span>项目 / 当前结果</span><span>运行状态</span><span style="text-align:right">操作</span></div>
    ${state.projects.filter(p => visibleProject(p.id)).map((p, i) => `<section class="project-row" style="--i:${i}"><div class="project-row-head"><div><button class="project-name" data-project="${p.id}">${esc(p.name)}<span class="project-type">${esc(p.kind)}</span></button><p class="project-goal">${esc(p.rework.length ? `返工 · ${p.rework[0].title}` : p.current)}</p></div><span class="run-state ${p.active ? "" : "paused"}">${p.active ? `● ${p.lines} 条线推进` : "Ⅱ 已暂停"}</span><div class="row-actions"><button class="icon-button" data-toggle="${p.id}" aria-label="${p.active ? "暂停" : "继续"}${esc(p.name)}">${p.active ? "Ⅱ" : "▷"}</button><button class="icon-button" data-project="${p.id}" aria-label="查看${esc(p.name)}详情">↗</button></div></div>
    <div class="stage-track"><span class="stage complete">✓ 已形成成果</span><span class="stage-arrow">→</span><span class="stage current">${p.rework.length ? "↻ 按反馈调整" : "定向验证"}</span><span class="stage-arrow">→</span><span class="stage">等你试用</span></div>
    <div class="project-foot"><small>${esc(p.rework.length ? `${p.rework.length} 项修改已加入当前项目` : p.changes)}</small><div class="inline-actions"><button data-project-deliveries="${p.id}">${state.deliveries.filter(d => d.project === p.id && d.state === "ready").length} 项待试用</button><button data-project-decisions="${p.id}">${pending().filter(d => d.project === p.id).length} 个自主决定</button></div></div></section>`).join("")}
    <section class="output-section"><div class="section-title"><h2>这一波的成果</h2><button class="text-button" data-view="deliveries">全部成果 →</button></div>${outputRows()}</section>`;
}
function deliveriesHTML() {
  return `<div class="section-title"><h2>按实际结果验收</h2><span>当前筛选范围</span></div><p class="list-intro">机器验证通过后先给你使用。真机、生产和发布的边界分别保留。</p>${state.deliveries.filter(d => visibleProject(d.project)).map(d => `<section class="delivery-full"><div class="delivery-top"><h3>${esc(d.title)}</h3><span class="tag ${d.state === "stale" ? "stale" : ""}">${d.state === "stale" ? "待更新" : d.state === "accepted" ? "已认可" : "待试用"}</span></div><p>${esc(d.description)}</p><div class="evidence-line"><span class="${d.state === "stale" ? "warn" : "verified"}">${d.state === "stale" ? "↻ 原验证已过期" : "✓ " + esc(d.evidence)}</span><span>◷ ${esc(d.boundary)}</span></div><div class="delivery-actions"><button class="secondary" data-delivery="${d.id}">查看使用路径</button><button class="primary" data-accept-delivery="${d.id}" ${d.state !== "ready" ? "disabled" : ""}>${d.state === "accepted" ? "你已认可" : "验收通过"}</button></div></section>`).join("")}`;
}
function decisionsHTML() {
  const decisions = visibleDecisions();
  const selected = decisions.filter(d => d.status === "pending" && state.selected.has(d.id));
  return `<div class="section-title"><h2>本波次自主决定</h2><span>${decisions.length} 个决定</span></div><p class="list-intro">它先作出选择、记录依据并继续。你可以逐条调整，或集中认可。</p>
  ${decisions.map(d => `<div class="decision-row ${state.selection.id === d.id ? "selected" : ""}"><input type="checkbox" aria-label="选择${esc(d.title)}" data-select-decision="${d.id}" ${state.selected.has(d.id) && d.status === "pending" ? "checked" : ""} ${d.status !== "pending" ? "disabled" : ""}><button class="decision-open" data-decision="${d.id}"><strong>${esc(d.title)}</strong><p>${esc(d.choice)}</p><small>${esc(project(d.project).name)} · ${esc(d.code)} · ${esc(d.time)}</small></button><span class="decision-status ${d.status !== "pending" ? "reviewed" : ""}">${d.status === "pending" ? "待审阅" : d.status === "corrected" ? "已纠正" : "已认可"}</span></div>`).join("")}
  <div class="batchbar"><span>已选 ${selected.length} 项 · 只认可本次选择</span><button class="primary" data-accept-batch="true" ${!selected.length ? "disabled" : ""}>认可选中的决定</button></div>
  <div class="section-title"><h2>已确认的偏好</h2><span>你的反馈形成的规则</span></div>${preferencesHTML()}`;
}
function preferencesHTML() {
  const prefs = state.preferences.filter(p => !p.superseded && (p.scope === "global" || visibleProject(p.scope)));
  return prefs.length ? prefs.map(p => `<div class="preference-row"><p><span class="scope-label">${p.scope === "global" ? "所有项目" : esc(project(p.scope).name)}</span>${esc(p.text)}</p><small>${esc(p.source)} · v${p.version}</small></div>`).join("") : `<div class="empty">这个项目还没有已确认的偏好。</div>`;
}
function graphData() {
  const p = state.scope === "all" ? "md" : state.scope;
  const current = state.decisions.find(d => d.project === p);
  // A saved global preference is not evidence that another decision adopted it.
  const pref = current.status === "corrected"
    ? current.feedbackScope !== "once" && state.preferences.find(x => !x.superseded && x.decision === current.id)
    : p === "md" && state.preferences.find(x => x.id === "pref0");
  const nodes = [
    { id: "project", label: project(p).name, kind: "source", x: -170, y: -95, detail: "关系范围绑定项目。切换筛选不改变后台运行。" },
    { id: "question", label: "遇到一个选择", kind: "source", x: -195, y: 24, detail: current.question },
    { id: "decision", label: "自主决定", kind: "decision", x: -63, y: 0, detail: current.choice, decision: current.id },
    { id: "result", label: current.status === "corrected" ? "成果待更新" : "形成可用成果", kind: "result", x: 80, y: -86, detail: state.deliveries.find(d => d.id === current.delivery).description, delivery: current.delivery },
    { id: "review", label: current.status === "corrected" ? "你的本次纠正" : pref ? "你曾经的纠正" : "等待你的审阅", kind: "decision", x: 78, y: 65, detail: current.feedback || (pref ? pref.text : "当前选择已记录，尚未形成用户偏好。") }
  ];
  const links = [{ source: "project", target: "question" }, { source: "question", target: "decision" }, { source: "decision", target: "result" }];
  if (current.status === "corrected") links.push({ source: "result", target: "review" });
  if (pref) {
    nodes.push({ id: "preference", label: pref.scope === "global" ? "全局偏好" : "项目偏好", kind: "preference", x: 220, y: 18, detail: pref.text });
    links.push({ source: "review", target: "preference" });
    if (current.status !== "corrected" && p === "md") links.push({ source: "preference", target: "decision", adopted: true });
  }
  return { nodes, links };
}
function memoryHTML() {
  const data = graphData();
  return `<div class="graph-head"><div><h2>决定的来处与去处</h2><p>${esc(project(state.scope === "all" ? "md" : state.scope).name)} · 当前决定的关系邻域</p></div><button class="secondary" data-playback="true">▷ 回放关系</button></div><div class="graph-container" id="graph" role="img" aria-label="决策关系图；下方节点列表提供等价操作"></div><div class="graph-legend"><span><i class="legend-dot source"></i>问题与来源</span><span><i class="legend-dot"></i>决定与反馈</span><span><i class="legend-dot result"></i>成果</span><span><i class="legend-dot pref"></i>已确认偏好</span></div><p class="playback-caption" id="playback-caption">点选节点，查看选择与依据。</p><div class="node-list">${data.nodes.map(n => `<button data-node="${n.id}">${esc(n.label)}</button>`).join("")}</div><p class="graph-note">仅展示已记录的关系。偏好只有在用户明确确认后出现；“采用”来自独立的执行记录。这里展示示例事件。</p>`;
}
function renderInspector() {
  const selection = state.selection;
  if (selection.kind === "decision") {
    const d = state.decisions.find(x => x.id === selection.id);
    $("inspector").innerHTML = `<div class="inspector-top"><strong>决策详情</strong><span>${d.status === "pending" ? "已自主采用 · 待审阅" : d.status === "corrected" ? "已纠正 · 返工中" : "用户已认可"}</span></div><div class="inspector-body"><div class="detail-meta">${esc(project(d.project).name)} / ${esc(d.code)}</div><h2>${esc(d.title)}</h2><p class="detail-question">${esc(d.question)}</p><span class="field-label">${d.status === "corrected" ? "按你的反馈调整为" : "它作出的选择"}</span><div class="chosen-option">${esc(d.choice)}</div><div class="alternative">${d.status === "corrected" ? "原选择已保留在示例历史，相关成果将重新验证。" : "其他方案：" + esc(d.alternative)}</div><span class="field-label">为什么这样选</span><p class="reason">${esc(d.reason)}</p><span class="source">↳ ${esc(d.source)}</span><div class="impact"><span>↶</span><span>${esc(d.impact)}</span></div></div><div class="inspector-actions">${d.status === "pending" ? `<button class="primary" data-accept-decision="${d.id}">认可这次选择</button>` : `<p class="reviewed-label">✓ ${d.status === "corrected" ? "反馈已记录，关联成果等待更新" : "这次选择已认可"}</p>`}<button class="secondary" data-feedback="${d.id}">${d.status === "corrected" ? "再次调整" : "修改选择与偏好"}</button><p class="quiet-note">认可一次选择，不会自动变成全局偏好。</p></div>${state.feedbackOpen ? `<form class="feedback-form" id="feedback-form" data-decision-id="${d.id}"><label for="feedback-text">希望改成什么？</label><textarea id="feedback-text" placeholder="例如：附件先保留引用，只有导出时才复制。" required></textarea><label for="feedback-scope">这次反馈的作用范围</label><select id="feedback-scope"><option value="once">只修改这次选择</option><option value="project" selected>修改并记为本项目偏好</option><option value="global">修改并记为所有项目偏好</option></select><button class="primary" type="submit">保存反馈并加入返工</button><p class="feedback-error" id="feedback-error"></p></form>` : ""}`;
  } else if (selection.kind === "delivery") {
    const d = state.deliveries.find(x => x.id === selection.id);
    $("inspector").innerHTML = `<div class="inspector-top"><strong>成果详情</strong><span>${d.state === "stale" ? "待更新" : d.state === "accepted" ? "已认可" : "待试用"}</span></div><div class="inspector-body"><div class="detail-meta">${esc(project(d.project).name)} / ${esc(d.version)}</div><h2>${esc(d.title)}</h2><p class="detail-question">${esc(d.description)}</p><span class="field-label">使用路径 · 示例</span><ol class="facts-list">${d.steps.map((step, i) => `<li><span class="step-number">0${i + 1}</span>${esc(step)}</li>`).join("")}</ol><span class="field-label">机器验证</span><p class="reason ${d.state === "stale" ? "warn" : "verified"}">${d.state === "stale" ? "决定改变后，原版本证据已过期。等待返工完成后重新验证。" : "✓ " + esc(d.evidence)}</p><span class="field-label">仍需验证</span><p class="reason">${esc(d.boundary)}</p><div class="detail-divider"></div><p class="quiet-note" style="text-align:left">这里展示成果包的操作方式，没有连接真实产物。</p></div><div class="inspector-actions"><button class="primary" data-accept-delivery="${d.id}" ${d.state !== "ready" ? "disabled" : ""}>${d.state === "accepted" ? "你已认可这一版" : "这一版验收通过"}</button><button class="secondary" data-decision="${state.decisions.find(x => x.delivery === d.id).id}">查看关联决定</button></div>`;
  } else if (selection.kind === "blocked") {
    $("inspector").innerHTML = `<div class="inspector-top"><strong>需要的信息</strong><span>仅影响一个单元</span></div><div class="inspector-body"><div class="detail-meta">MD 文件保存 / OCR 真机验证</div><h2>需要一台可测试的设备</h2><p class="detail-question">当前已形成 Reader 版本。真机 OCR 需要设备与真实图片，示例中的机器环境没有这些输入。</p><span class="field-label">当前处理</span><p class="reason">已挂起 OCR 真机验证。附件、搜索和其他项目仍继续推进。</p><span class="field-label">恢复条件</span><p class="reason">设备可用后，运行真实图片解码与 OCR 路径，再补充对应版本的证据。</p><div class="detail-divider"></div><p class="quiet-note" style="text-align:left">这是缺少客观条件；无法通过模型自问自答证明测试通过。</p></div><div class="inspector-actions"><button class="secondary" data-delivery="reader">先看已经可用的 Reader</button></div>`;
  } else if (selection.kind === "node") {
    const n = graphData().nodes.find(x => x.id === selection.id) || graphData().nodes[0];
    $("inspector").innerHTML = `<div class="inspector-top"><strong>关系详情</strong><span>示例事实</span></div><div class="inspector-body"><div class="detail-meta">${esc(project(state.scope === "all" ? "md" : state.scope).name)} / 决策邻域</div><h2>${esc(n.label)}</h2><p class="detail-question">${esc(n.detail)}</p><span class="field-label">如何读这条关系</span><p class="reason">从来源看到一次选择，再看到成果、反馈以及偏好的实际使用。每一条边都能回到对应记录。</p></div><div class="inspector-actions">${n.decision ? `<button class="primary" data-decision="${n.decision}">打开决定</button>` : n.delivery ? `<button class="primary" data-delivery="${n.delivery}">打开成果</button>` : `<button class="secondary" data-view="decisions">查看决定与偏好</button>`}</div>`;
  } else {
    const p = project(selection.id);
    $("inspector").innerHTML = `<div class="inspector-top"><strong>项目详情</strong><span>${p.active ? "自主推进中" : "已暂停"}</span></div><div class="inspector-body"><div class="detail-meta">${esc(p.kind)} / 本机项目</div><h2>${esc(p.name)}</h2><p class="detail-question">${esc(p.goal)}</p><span class="field-label">当前工作</span><p class="reason">${esc(p.rework.length ? `按用户反馈修改：${p.rework[0].text}` : p.task)}</p><div class="detail-divider"></div><div class="project-stat"><span>执行线路</span><strong>${p.lines} 条</strong></div><div class="project-stat"><span>待审阅决定</span><strong>${pending().filter(d => d.project === p.id).length} 个</strong></div><div class="project-stat"><span>反馈返工</span><strong>${p.rework.length} 项</strong></div><div class="project-stat"><span>等待你的信息</span><strong>${p.id === "md" ? "1 个单元" : "无"}</strong></div></div><div class="inspector-actions"><button class="primary" data-project-decisions="${p.id}">查看这个项目的决定</button><button class="secondary" data-toggle="${p.id}">${p.active ? "暂停" : "继续"}此项目</button></div>`;
  }
}
function setView(view) { state.view = view; state.feedbackOpen = false; if (view === "memory") state.selection = { kind: "node", id: "decision" }; render(); }
function setScope(id) {
  state.scope = id; state.feedbackOpen = false;
  if (id !== "all") state.selection = { kind: "project", id };
  render();
}
function revealInspector() { if (matchMedia("(max-width:860px)").matches) $("inspector").scrollIntoView({ block: "start", behavior: reduced.matches ? "instant" : "smooth" }); }
function openDecision(id) { state.selection = { kind: "decision", id }; state.feedbackOpen = false; renderInspector(); revealInspector(); }
function acceptDecision(id) { const d = state.decisions.find(x => x.id === id); if (d.status !== "pending") return; d.status = "accepted"; state.selected.delete(id); render(); showToast("已认可这次选择；偏好范围保持不变。"); }
function acceptDelivery(id) { const d = state.deliveries.find(x => x.id === id); if (d.state !== "ready") return; d.state = "accepted"; render(); showToast("这一版已标为用户认可，发布与未验证路径仍单独保留。"); }
document.addEventListener("click", event => {
  const target = event.target.closest("button, a"); if (!target) return;
  if (target.classList.contains("brand")) { event.preventDefault(); setView("overview"); return; }
  const d = target.dataset;
  if (d.view) { setView(d.view); return; }
  if (d.scope) { setScope(d.scope); return; }
  if (d.decision) { openDecision(d.decision); return; }
  if (d.delivery) { state.selection = { kind: "delivery", id: d.delivery }; state.feedbackOpen = false; renderInspector(); revealInspector(); return; }
  if (d.project) { state.selection = { kind: "project", id: d.project }; renderInspector(); revealInspector(); return; }
  if (d.blocked) { state.selection = { kind: "blocked" }; renderInspector(); revealInspector(); return; }
  if (d.toggle) { const p = project(d.toggle); p.active = !p.active; render(); showToast(`${p.name}已${p.active ? "继续" : "暂停"}（示例）。`); return; }
  if (d.projectDecisions || d.projectDeliveries) { state.scope = d.projectDecisions || d.projectDeliveries; const first = visibleDecisions()[0]; state.selection = d.projectDecisions ? { kind: "decision", id: first.id } : { kind: "delivery", id: state.deliveries.find(x => x.project === state.scope).id }; setView(d.projectDecisions ? "decisions" : "deliveries"); return; }
  if (d.acceptDecision) { acceptDecision(d.acceptDecision); return; }
  if (d.acceptDelivery) { acceptDelivery(d.acceptDelivery); return; }
  if (d.feedback) { state.selection = { kind: "decision", id: d.feedback }; state.feedbackOpen = !state.feedbackOpen; renderInspector(); if (state.feedbackOpen) $("feedback-text").focus(); return; }
  if (d.acceptBatch) {
    const chosen = visibleDecisions().filter(x => state.selected.has(x.id) && x.status === "pending");
    chosen.forEach(x => { x.status = "accepted"; state.selected.delete(x.id); });
    render(); showToast(`已认可选中的 ${chosen.length} 个决定，没有新增全局偏好。`); return;
  }
  if (d.node) { selectNode(d.node); return; }
  if (d.playback) { playGraph(); return; }
  if (target.id === "review-wave") { state.scope = "all"; const first = pending()[0] || state.decisions[0]; state.selection = { kind: "decision", id: first.id }; setView("decisions"); return; }
  if (target.id === "reset-demo") { state = seed(); render(); showToast("已恢复初始示例。"); }
});
document.addEventListener("change", event => {
  const id = event.target.dataset.selectDecision;
  if (id) { if (event.target.checked) state.selected.add(id); else state.selected.delete(id); $("main-content").innerHTML = decisionsHTML(); }
});
document.addEventListener("submit", event => {
  if (event.target.id !== "feedback-form") return;
  event.preventDefault();
  const text = $("feedback-text").value.trim(), scope = $("feedback-scope").value;
  if (!text) { $("feedback-error").textContent = "写下希望调整的内容。"; return; }
  const d = state.decisions.find(x => x.id === event.target.dataset.decisionId);
  const previous = d.choice;
  d.status = "corrected"; d.feedback = text; d.choice = text; d.feedbackScope = scope;
  d.history = [...(d.history || []), previous];
  d.reason = "用户明确纠正了这次选择。执行端应按反馈调整，并重新验证受影响的成果。";
  d.source = scope === "once" ? "用户反馈 · 仅这次选择" : scope === "global" ? "用户反馈 · 所有项目" : "用户反馈 · 本项目";
  project(d.project).rework.push({ decision: d.id, title: d.title, text });
  state.deliveries.find(x => x.id === d.delivery).state = "stale";
  if (scope !== "once") {
    const prefScope = scope === "global" ? "global" : d.project;
    const old = state.preferences.filter(x => x.decision === d.id && !x.superseded);
    old.forEach(x => { x.superseded = true; });
    state.preferences.push({ id: `pref${state.preferences.length + 1}`, scope: prefScope, text, source: `${d.code} · 用户纠正`, version: old.length ? Math.max(...old.map(x => x.version)) + 1 : 1, decision: d.id });
  }
  state.selected.delete(d.id); state.feedbackOpen = false; render();
  showToast(scope === "once" ? "反馈已加入返工；这次不新增偏好。" : `已加入返工，并记为${scope === "global" ? "所有项目" : "本项目"}偏好（示例）。`);
});
function createGraph() {
  const host = $("graph"); if (!host || state.view !== "memory") return;
  if (typeof window.ForceGraph !== "function") { host.innerHTML = `<div class="empty">图形资源未加载。仍可从下方列表查看全部关系。</div>`; return; }
  const data = graphData(); data.nodes.forEach(n => { n.fx = n.x; n.fy = n.y; });
  const colors = { source: "#b2b4bd", decision: "#ffad66", preference: "#77dae5", result: "#a4afd5" };
  graph = window.ForceGraph()(host).width(host.clientWidth).height(host.clientHeight).backgroundColor("#00000000")
    .nodeLabel(n => esc(n.label)).linkColor(l => l.adopted ? "#77dae57a" : "#555964")
    .linkWidth(1).linkDirectionalArrowLength(4).linkDirectionalArrowRelPos(.9)
    .linkDirectionalParticleColor(() => "#ffad66").linkDirectionalParticleWidth(3).linkDirectionalParticleSpeed(.016)
    .warmupTicks(0).cooldownTicks(30).enableNodeDrag(true)
    .nodeCanvasObject((node, ctx, scale) => {
      const hot = state.node === node.id; const r = node.kind === "preference" ? 10 : 7;
      ctx.beginPath(); ctx.arc(node.x, node.y, hot ? r + 10 : r + 5, 0, Math.PI * 2); ctx.fillStyle = hot ? colors[node.kind] + "22" : "#ffffff04"; ctx.fill();
      ctx.beginPath(); ctx.arc(node.x, node.y, r, 0, Math.PI * 2); ctx.fillStyle = "#1b1c1f"; ctx.fill(); ctx.strokeStyle = colors[node.kind]; ctx.lineWidth = hot ? 2 : 1.4; ctx.stroke();
      ctx.beginPath(); ctx.arc(node.x, node.y, 2.3, 0, Math.PI * 2); ctx.fillStyle = colors[node.kind]; ctx.fill();
      const size = 11 / Math.max(.84, scale); ctx.font = `${size}px "Segoe UI", "Microsoft YaHei", sans-serif`; ctx.textAlign = "center"; ctx.textBaseline = "middle";
      const width = ctx.measureText(node.label).width; ctx.fillStyle = "#151618ed"; ctx.fillRect(node.x - width / 2 - 4, node.y + 16, width + 8, size + 9); ctx.fillStyle = hot ? "#f6ede4" : "#bfc2cc"; ctx.fillText(node.label, node.x, node.y + 21 + size / 2);
    })
    .nodePointerAreaPaint((node, color, ctx) => { ctx.fillStyle = color; ctx.beginPath(); ctx.arc(node.x, node.y, 19, 0, Math.PI * 2); ctx.fill(); })
    .onNodeClick(node => selectNode(node.id))
    .onZoom(() => wakeGraph(500))
    .graphData(data);
  graph.zoom(Math.min(host.clientWidth / 540, 1.15), 0); graph.centerAt(12, 0, 0);
  graphObserver = new ResizeObserver(() => { if (!graph || !$("graph")) return; graph.width(host.clientWidth).height(host.clientHeight); graph.zoom(Math.min(host.clientWidth / 540, 1.15), 0); wakeGraph(350); });
  graphObserver.observe(host); wakeGraph(1000);
}
function wakeGraph(ms = 750) {
  if (!graph || state.view !== "memory" || document.hidden) return;
  graph.resumeAnimation(); clearTimeout(graphTimer); graphTimer = setTimeout(() => graph?.pauseAnimation(), reduced.matches ? 80 : ms);
}
function selectNode(id) {
  state.node = id; state.selection = { kind: "node", id }; renderInspector();
  document.querySelectorAll("[data-node]").forEach(el => el.classList.toggle("selected", el.dataset.node === id));
  const n = graphData().nodes.find(x => x.id === id);
  if (n && graph) { graph.centerAt(n.x * .22, n.y * .22, reduced.matches ? 0 : 300); wakeGraph(); }
}
function playGraph() {
  if (!graph) { showToast("图形资源尚未就绪，可先查看节点列表。"); return; }
  playbackTimers.forEach(clearTimeout); playbackTimers = [];
  const data = graph.graphData();
  const stages = data.links.map(l => ({ from: typeof l.source === "string" ? l.source : l.source.id, to: typeof l.target === "string" ? l.target : l.target.id, link: l }));
  const captions = { question: "01 · 工作中遇到选择", decision: "02 · 模型选择方案并继续推进", result: "03 · 形成有版本与证据的成果", review: "04 · 你的纠正关联到受影响成果", preference: "05 · 你明确确认的反馈成为偏好" };
  if (reduced.matches) { state.node = "decision"; $("playback-caption").textContent = "关系已展示 · 系统已启用减少动态效果"; wakeGraph(); return; }
  stages.forEach((stage, i) => playbackTimers.push(setTimeout(() => {
    if (!graph || state.view !== "memory" || document.hidden) return;
    state.node = stage.to;
    $("playback-caption").textContent = stage.link.adopted ? "06 · 已确认偏好在后续决定中被采用" : captions[stage.to] || "已记录的关系";
    graph.emitParticle(stage.link); wakeGraph(1250);
  }, i * 850)));
  playbackTimers.push(setTimeout(() => { if (state.view === "memory") { state.node = null; $("playback-caption").textContent = "回放结束 · 点选节点可以查看记录。"; wakeGraph(350); } }, stages.length * 850 + 800));
}
document.addEventListener("visibilitychange", () => { if (document.hidden) { graph?.pauseAnimation(); playbackTimers.forEach(clearTimeout); } else wakeGraph(); });
reduced.addEventListener("change", () => { playbackTimers.forEach(clearTimeout); wakeGraph(); });
state = seed();
render();
