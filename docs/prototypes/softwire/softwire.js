/* Interaction trial: example state only. Browser-local persistence; no model or project writes. */
const $ = (id) => document.getElementById(id);
const escapeHtml = (value) => String(value).replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const settingsLabel = (settings) => `${settings.model === 'primary' ? '主模型' : '快速模型'} · ${settings.tools === 'all' ? '全部工具' : '只读工具'} · ${settings.subagents ? '子代理可用' : '停止新派发'}`;
const glyphs = {
  grid: '<rect x="4" y="4" width="6" height="6" rx="1"/><rect x="14" y="4" width="6" height="6" rx="1"/><rect x="4" y="14" width="6" height="6" rx="1"/><rect x="14" y="14" width="6" height="6" rx="1"/>',
  inbox: '<path d="M4 5h16v14H4zM4 13h5l1 3h4l1-3h5"/>',
  mail: '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="m3 6 9 7 9-7"/>',
  step: '<path d="m6 5 10 7-10 7zM19 5v14"/>',
  target: '<circle cx="12" cy="12" r="8"/><circle cx="12" cy="12" r="3"/>',
  chevron: '<path d="m9 6 6 6-6 6"/>',
  commit: '<circle cx="12" cy="12" r="4"/><path d="M3 12h5m8 0h5"/>',
  network: '<circle cx="12" cy="12" r="3"/><circle cx="4" cy="5" r="2"/><circle cx="20" cy="5" r="2"/><circle cx="12" cy="21" r="2"/><path d="m6 7 4 3m4 0 4-3m-6 8v4"/>',
  chat: '<path d="M20 15a3 3 0 0 1-3 3H9l-5 3V6a3 3 0 0 1 3-3h10a3 3 0 0 1 3 3z"/><path d="M8 8h8m-8 5h5"/>',
  history: '<path d="M4 10a8 8 0 1 1 1 7M4 4v6h6m2-3v5l3 2"/>',
  warning: '<path d="m12 3 10 18H2zM12 9v5m0 3v.2"/>',
  document: '<path d="M6 3h8l4 4v14H6zM14 3v5h4M9 12h6m-6 4h6"/>',
  archive: '<path d="M4 8h16v12H4zM3 4h18v4H3zM9 12h6m-5 4h4"/>',
  agent: '<circle cx="12" cy="12" r="3"/><path d="M12 3v4m0 10v4M3 12h4m10 0h4M5.6 5.6l2.8 2.8m7.2 7.2 2.8 2.8M5.6 18.4l2.8-2.8m7.2-7.2 2.8-2.8"/>',
  child: '<circle cx="12" cy="8" r="3"/><path d="M6 20v-3a6 6 0 0 1 12 0v3"/>',
  tool: '<path d="m4 6 6 6-6 6m9 0h7"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  verify: '<rect x="5" y="4" width="14" height="17" rx="2"/><path d="M9 4V2h6v2M9 10h6m-6 5h6"/>',
  clock: '<circle cx="12" cy="12" r="9"/><path d="M12 6v6l4 2"/>',
  layers: '<path d="m12 3 10 6-10 6L2 9zm-10 10 10 6 10-6M2 17l10 6 10-6"/>',
  sliders: '<path d="M4 6h6m4 0h6M4 12h12m4 0h1M4 18h2m4 0h10"/><circle cx="12" cy="6" r="2"/><circle cx="18" cy="12" r="2"/><circle cx="8" cy="18" r="2"/>',
  float: '<path d="M14 3h7v7m0-7-9 9M9 4H4v16h16v-5"/>',
  dock: '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 15h18m-9-9v6m-3-3 3 3 3-3"/>',
  up: '<path d="M12 20V4m-6 6 6-6 6 6"/>',
  back: '<path d="M20 12H4m6-6-6 6 6 6"/>',
  reply: '<path d="m9 4-7 7 7 6m-7-6h12a7 7 0 0 1 7 7v2"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 10v7m0-10v.1"/>',
  grip: '<path d="M8 5v.2m8-.2v.2M8 12v.2m8-.2v.2M8 19v.2m8-.2v.2" stroke-width="3"/>',
};
const icon = name => `<svg class="icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${glyphs[name] || glyphs.document}</svg>`;
const links = [['work', 'main'], ['memory', 'main'], ['archive', 'main'], ['scout', 'main'], ['main', 'fetch'], ['main', 'test'], ['fetch', 'candidate']];
let inspectedNode = null;
function hydrateIcons() { document.querySelectorAll('[data-icon]').forEach(el => { el.innerHTML = icon(el.dataset.icon); }); }
const owners = {
  work: { id: 'plan', name: '需求管理', module: '需求与计划', initial: '这里管理需求、验收和批次安排。你可以直接补充目标或调整范围。' },
  batch: { id: 'plan', name: '需求管理', module: '批次管理', initial: '本批范围已经明确。需要收口、调整验收或拆出下一批，直接告诉我。' },
  memory: { id: 'memory', name: '记忆管理', module: '记忆管理', initial: '我会核对记忆的版本、来源和适用范围。修订先形成候选，实际生效会另给回执。' },
  candidate: { id: 'memory', name: '记忆管理', module: '记忆管理', initial: '这条候选还在等待来源核对。你可以补充依据或提出修订。' },
  archive: { id: 'context', name: '上下文管理', module: '历史与压缩', initial: '我负责摘要与原文的对应关系。查看原文和装入下一次请求是两件事。' },
  main: { id: 'main', name: '主执行', module: '执行管理', initial: '我负责整合当前工作并形成交付。其他模块的消息会发给它们自己的负责人。' },
  scout: { id: 'scout', name: '资料核对', module: '子任务管理', initial: '已交回三个来源。发给我的新消息会续聊这个子任务，仍保留原来的目标与身份。' },
  fetch: { id: 'tools', name: '工具管理', module: '工具管理', initial: '这里查看调用目的、参数与结果。可以安排重查或调整后续调用，已经执行的记录保留。' },
  test: { id: 'verification', name: '验证管理', module: '验证管理', initial: '这里管理验证计划和检查结果。我有一个关于验证目标的问题等你回复。' },
};
const projects = {
  kanzei: makeProject('kanzei code', '搜索与引用链路', '01', '检索约定.md', 18, 'v3'),
  reader: makeProject('阅读器', '附件全文检索', '02', '附件索引约定.md', 7, 'v4'),
};
let activeId = 'kanzei';
let overview = false;
let composing = false;
let lastTrigger = null;
let inboxReturn = null;
let toastId = null;
let interactionSequence = 3;
const manualExperience = new URLSearchParams(location.search).has('manual');
let automatic = !manualExperience;
let storageReady = false;
let persistTimer = null;
let previousExperience = null;
const deliveryTimers = new Map();
const STORAGE_KEY = 'kanzei-softwire-experience-v4';
const interactions = [
  { id: 'decision-1', projectId: 'kanzei', node: 'main', kind: '决策复核', title: '无法取证的引用怎么处理', body: '我已保留无法取证的引用，并标记为“待核对”。你可以沿用这个决定，也可以让我调整。', impact: '本批继续推进，复核不阻塞运行', blocking: false, choices: ['沿用这个决定', '改为从本批移除'], status: 'open', revision: 1, read: false },
  { id: 'question-2', projectId: 'kanzei', node: 'test', kind: '提问', title: '先验证哪个使用场景', body: '引用回溯已经可以验证。这个批次先验证桌面路径，还是浏览器路径？', impact: '只等待本批验证目标；其他工作继续', blocking: true, choices: ['先验证桌面路径', '先验证浏览器路径'], status: 'open', revision: 1, read: false },
  { id: 'delivery-3', projectId: 'reader', node: 'main', kind: '交付反馈', title: '附件检索试用版已准备好', body: '这一版可以按内容找回附件并定位到原文。你试用之后，可以认可这一版，也可以直接说哪里需要调整。', impact: '等待用户试用，不影响独立工作', blocking: false, choices: ['已试用，可以继续', '需要调整'], status: 'open', revision: 1, read: false },
];

function makeProject(name, work, id, memoryName, step, memoryVersion) {
  const state = {
    name, work, id, memoryName, step, memoryVersion,
    selected: 'main', reference: 'main',
    view: 'flow', panel: null, draft: '', scope: 'work',
    routeKind: 'workbench', moduleTab: 'manage', interactionId: null, shownRevision: null, drafts: new Map(), paused: false,
    panels: [], evidence: null, evidenceReturn: null,
    pending: [], settings: { model: 'primary', tools: 'all', subagents: true },
    settingsDraft: null, archiveRecalled: false, snapshots: [], replayStep: step - 1,
    messages: [], notice: '',
    batch: { phase: 'implementing', edits: 18, artifact: null },
  };
  state.snapshots = [snapshot(state, step - 1), snapshot(state, step)];
  return state;
}
function project() { return projects[activeId]; }
function currentInteraction() { return project().routeKind === 'inbox' ? interactions.find(item => item.id === project().interactionId) : null; }
function currentOwner() { return owners[currentInteraction()?.node || project().selected]; }
function threadKey() { const p = project(); return currentInteraction() ? `${activeId}/interaction/${p.interactionId}` : `${activeId}/${currentOwner().id}/${p.selected}`; }
function saveDraft() { if (!overview) project().drafts.set(threadKey(), { text: $('instruction').value, scope: $('scope').value }); schedulePersist(); }
function restoreDraft() { const d = project().drafts.get(threadKey()) || { text: '', scope: 'work' }; $('instruction').value = d.text; $('scope').value = d.scope; project().scope = d.scope; }
function waitingForVerification(p) { return interactions.some(item => item.projectId === Object.keys(projects).find(id => projects[id] === p) && item.node === 'test' && item.blocking && item.status !== 'resolved'); }
function interactionState(item) { return { open: '待回复', replied: '已回复 · 等待处理', resolved: '已处理', failed: '回复未生效' }[item.status]; }
function snapshot(p, step = p.step) {
  return { step, memoryVersion: p.memoryVersion, archiveRecalled: p.archiveRecalled, settings: { ...p.settings } };
}
function memoryStatus(p) {
  return p.memoryVersion === 'v3' ? '旧版本仍在本步上下文' : p.memoryVersion ? '当前依据版本已核对' : '本步已排除这条记忆';
}
function manifest(p, snap = snapshot(p)) {
  return [
    { type: '约束', name: '项目约定.md · v2', detail: '项目范围 · 约束片段 8–16 行', state: '已装入', size: 680, node: 'work' },
    { type: '工作', name: `${p.work} · 检查点 06`, detail: '目标、验收、当前进展与未解决项', state: '已装入', size: 2450, node: 'work' },
    { type: '近期对话', name: '最近 3 轮明确要求', detail: '当前工作 · 保留用户最近指令', state: '已装入', size: 2100 },
    { type: '记忆', name: `${p.memoryName} · ${snap.memoryVersion || 'v4'}`, detail: snap.memoryVersion === 'v3' ? '已装入 12–18 行 · 文件已更新到 v4' : snap.memoryVersion ? '已装入 12–22 行 · 版本与文件一致' : '本次排除 · 文件和过去请求保留', state: snap.memoryVersion ? '已装入' : '未纳入', size: snap.memoryVersion ? (snap.memoryVersion === 'v4' ? 360 : 320) : 0, node: 'memory' },
    { type: '工具证据', name: '页面取证与代码读取 · 4 个片段', detail: '完整输出在产物中 · 不等于全部正文', state: '已装入', size: 4820, node: 'fetch' },
    { type: '协作交接', name: '资料核对 → 主执行', detail: '引用索引、取证结论、1 项待核对事实', state: '已装入', size: 310, node: 'scout' },
    { type: '折叠历史', name: snap.archiveRecalled ? '历史摘要 03 + 原文片段' : '历史摘要 03.md · v1', detail: snap.archiveRecalled ? '请求 10 的原文片段已重新装入' : '原文请求 1–15 已归档 · 仅摘要装入', state: snap.archiveRecalled ? '已召回原文' : '摘要已装入', size: snap.archiveRecalled ? 8120 : 1720, node: 'archive' },
  ];
}
function nodes(p) {
  const version = p.memoryVersion || '未纳入';
  return {
    batch: { title: activeId === 'kanzei' ? 'B2 · 引用回溯' : 'B1 · 附件检索', kind: '批次交付', version: p.batch.phase === 'delivered' ? '已交付' : p.batch.phase === 'verifying' ? '验证中' : '实施中', description: '实施、验证、提交与回写', lane: 'batch' },
    work: { title: '目标与项目约定', kind: '工作上下文', version: '检查点 06', description: '目标、约束与验收 · 已装入', lane: 'source' },
    memory: { title: p.memoryName, kind: '记忆 · 正文片段', version: p.memoryVersion === 'v3' ? 'v3 · 旧版本' : version, description: p.memoryVersion ? '检索 → 装入 → 工具返回正文' : '后续装配排除 · 原文件保留', lane: 'source', warning: p.memoryVersion === 'v3' },
    archive: { title: '历史摘要 03.md', kind: '折叠历史', version: p.archiveRecalled ? '原文已召回' : 'v1', description: p.archiveRecalled ? '请求 10 的片段已重新装入' : '15 次请求 → 摘要与原文索引', lane: 'source', muted: !p.archiveRecalled },
    main: { title: '主执行', kind: '项目负责人', version: p.paused ? '已暂停' : `请求 ${p.step}`, description: p.paused ? '等待继续指令' : p.batch.phase === 'delivered' ? '本批已交付 · 等待试用' : p.batch.phase === 'verifying' ? '等待本批验证结果' : '运行中 · 整合当前工作', lane: 'agent', agent: true, running: !p.paused && p.batch.phase === 'implementing' },
    scout: { title: '资料核对', kind: '子代理', version: '已交回', description: '交接 3 个来源 · 等待主执行核对', lane: 'agent', agent: true },
    fetch: { title: activeId === 'reader' ? '读取附件索引' : '获取原始页面', kind: '工具 · 读取', version: '已返回', description: activeId === 'reader' ? '核对索引条目与原文位置' : '核对引用与来源原文是否对应', lane: 'action' },
    test: { title: '验证引用链路', kind: '验证模块', version: p.batch.phase === 'delivered' ? '示例通过' : waitingForVerification(p) ? '等你回复' : p.settings.tools === 'all' ? '可执行' : '只读范围', description: p.batch.phase === 'delivered' ? '本批快照验证完成' : waitingForVerification(p) ? '1 条提问 · 选择验证目标' : '按已确定的验证目标继续', lane: 'action', running: p.batch.phase === 'verifying' && !waitingForVerification(p) && p.settings.tools === 'all', waiting: waitingForVerification(p) },
    candidate: { title: '取证记录.md', kind: '记忆候选', version: 'candidate', description: '等待管理者核对来源', lane: 'action', muted: true },
  };
}
function renderNodes() {
  const p = project();
  const all = nodes(p);
  for (const lane of ['source', 'agent', 'action']) {
    for (const [id, node] of Object.entries(all).filter(([, value]) => value.lane === lane)) {
      let button = $(`${lane}-nodes`).querySelector(`[data-select="${id}"]`);
      if (!button) { button = document.createElement('button'); button.type = 'button'; button.dataset.select = id; button.innerHTML = '<span class="node-symbol"></span><span class="node-copy"><strong></strong><span class="node-version"></span></span><span class="node-status"></span>'; $(`${lane}-nodes`).append(button); }
      button.className = `flow-node ${node.agent ? 'agent-node' : ''} ${node.warning ? 'warning' : ''} ${node.muted ? 'muted-node' : ''} ${node.running ? 'is-running' : ''} ${node.waiting ? 'is-waiting' : ''}`;
      button.dataset.state = node.running ? 'running' : node.waiting ? 'waiting' : node.warning ? 'warning' : node.muted ? 'folded' : ['scout', 'fetch'].includes(id) ? 'done' : 'ready';
      button.setAttribute('aria-pressed', String(p.routeKind === 'module' && p.selected === id));
      button.setAttribute('aria-label', `${node.title}，${node.kind}，${node.description}，打开${owners[id].module}`);
      button.title = `${node.description} · ${owners[id].module}`;
      button.querySelector('.node-symbol').innerHTML = icon(({ work: 'target', memory: 'document', archive: 'archive', main: 'agent', scout: 'child', fetch: 'tool', test: 'verify', candidate: 'document' })[id]);
      const versions = { work: '', memory: p.memoryVersion || '已排除', archive: p.archiveRecalled ? '已召回' : '15 → 1', main: `#${p.step}`, scout: '3 份来源', fetch: '', test: node.version === '可执行' ? '' : node.version, candidate: '候选' };
      button.querySelector('.node-version').textContent = versions[id];
      button.querySelector('strong').textContent = node.title;
      button.querySelector('.node-status').innerHTML = node.waiting ? icon('chat') : node.warning ? icon('warning') : ['scout', 'fetch'].includes(id) || (id === 'test' && p.batch.phase === 'delivered') ? icon('check') : '';
    }
  }
  requestAnimationFrame(drawWires);
}
function drawWires() {
  if (overview || $('flow-view').hidden || window.innerWidth <= 620) return;
  const area = $('flow-map').getBoundingClientRect();
  $('wires').setAttribute('viewBox', `0 0 ${area.width} ${area.height}`);
  $('wires').innerHTML = '<defs><marker id="signal-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="5" markerHeight="5" orient="auto"><path d="m1 1 5 3-5 3" fill="none" stroke="#9aa88c" stroke-width="1.3"/></marker></defs>' + links.map(([source, target]) => {
    const a = document.querySelector(`.flow-node[data-select="${source}"]`).getBoundingClientRect();
    const b = document.querySelector(`.flow-node[data-select="${target}"]`).getBoundingClientRect();
    const vertical = source === 'scout' || source === 'fetch';
    let path;
    if (vertical) {
      const x = a.left + a.width / 2 - area.left;
      const y1 = (source === 'scout' ? a.top : a.bottom) - area.top;
      const y2 = (source === 'scout' ? b.bottom : b.top) - area.top;
      // The output branch goes around the test node instead of through it.
      path = source === 'fetch' ? `M ${a.right - area.left} ${a.top + a.height / 2 - area.top} C ${a.right - area.left + 14} ${a.top + a.height / 2 - area.top}, ${b.right - area.left + 14} ${b.top + b.height / 2 - area.top}, ${b.right - area.left} ${b.top + b.height / 2 - area.top}` : `M ${x} ${y1} L ${x} ${y2}`;
    } else {
      const x1 = a.right - area.left; const y1 = a.top + a.height / 2 - area.top;
      const x2 = b.left - area.left; const y2 = b.top + b.height / 2 - area.top;
      const mid = (x1 + x2) / 2;
      path = `M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`;
    }
    return `<path id="wire-${source}-${target}" data-source="${source}" data-target="${target}" class="wire ${source === 'memory' && project().memoryVersion === 'v3' ? 'attention-wire' : ''} ${source === 'archive' ? 'folded' : ''}" marker-end="url(#signal-arrow)" d="${path}"/>`;
  }).join('');
  highlightNode(inspectedNode);
}
function highlightNode(id) {
  inspectedNode = id;
  const related = new Set(id ? links.filter(pair => pair.includes(id)).flat() : []);
  document.querySelectorAll('.flow-node').forEach(button => { button.classList.toggle('is-dimmed', Boolean(id) && !related.has(button.dataset.select)); button.classList.toggle('is-inspected', button.dataset.select === id); });
  $('wires').querySelectorAll('.wire').forEach(path => { path.classList.toggle('is-inspected', Boolean(id) && [path.dataset.source, path.dataset.target].includes(id)); path.classList.toggle('is-dimmed', Boolean(id) && ![path.dataset.source, path.dataset.target].includes(id)); });
}
function pulseSignal(edges) {
  if (overview || $('flow-view').hidden || window.matchMedia('(prefers-reduced-motion: reduce)').matches || document.hidden) return;
  drawWires();
  for (const [source, target] of edges) { const path = $(`wire-${source}-${target}`); if (!path) continue; const pulse = path.cloneNode(); pulse.removeAttribute('id'); pulse.removeAttribute('marker-end'); pulse.setAttribute('class', 'wire signal-pulse'); $('wires').append(pulse); pulse.addEventListener('animationend', () => pulse.remove(), { once: true }); }
}
function focusDescription(p) {
  const descriptions = {
    batch: p.batch.phase === 'delivered' ? '本批已形成示例提交和检查点；交付计数现在才增加。' : p.batch.phase === 'verifying' ? '已冻结示例快照，正在验证；当前批次不再扩展范围。' : '改动仍在增加，但尚无交付。收口会冻结本批，再验证、提交和回写。',
    work: '这项工作的目标、约束和验收随检查点装入；项目切换不会共享。',
    memory: p.memoryVersion === 'v3' ? '本步装入了 v3；文件已更新到 v4。纠正从下次请求开始生效。' : p.memoryVersion ? '本步已装入 v4 的片段。读取记录只能证明工具返回了正文。' : '这条记忆已从本次工作的后续装配中排除；历史引用仍保留。',
    archive: p.archiveRecalled ? '请求 10 的原文片段已装入本步；摘要和原文都能追溯。' : '15 次请求折叠为一份摘要；展开原文只供查看，召回才进入下一步。',
    main: `当前依据属于请求 ${p.step}。给它补充要求，可在下次请求送达。`,
    scout: '它只收到资料核对的交接包；主执行没有共享整段历史。',
    fetch: '目的：验证引用能否回到原始内容。调用已完成，可展开参数与证据。',
    test: p.settings.tools === 'all' ? '准备执行定向验证，使用主执行的当前工作和版本依据。' : '本工作已限制为只读。验证调用尚未派发。',
    candidate: '来自本次取证的草稿，尚未成为有效记忆；保留来源后交管理者核对。',
  };
  return descriptions[p.selected];
}
function renderFocus() {
  const p = project(); const node = nodes(p)[p.selected];
  document.querySelector('.batch-entry').setAttribute('aria-pressed', String(p.selected === 'batch'));
  const interaction = currentInteraction();
  if (interaction) {
    $('focus-kind').textContent = `${interaction.kind} · ${interactionState(interaction)}`;
    $('focus-title').textContent = interaction.title;
    $('focus-version').textContent = `问题 v${interaction.revision}`;
    $('focus-description').textContent = interaction.impact;
    $('focus-actions').innerHTML = `<button type="button" class="quiet-button" data-action="interaction-source">${icon('back')}来源模块</button>`;
    renderReference(); return;
  }
  if (p.view === 'replay') {
    const snap = p.snapshots.find((item) => item.step === p.replayStep) || p.snapshots[0];
    $('focus-kind').textContent = '历史回放 · 只读';
    $('focus-title').textContent = `请求 ${snap.step} 的快照`;
    $('focus-version').textContent = snap.memoryVersion || '该记忆未纳入';
    $('focus-description').textContent = '这里保留当时的依据。返回当前后，才能调整后续工作。';
    $('focus-actions').innerHTML = '<button type="button" class="outline-button" data-action="replay-context">查看当时上下文</button>';
    renderReference();
    return;
  }
  $('focus-kind').textContent = `${currentOwner().module} · ${p.routeKind === 'module' ? '当前会话对象' : '当前工作'}`;
  $('focus-title').textContent = node.title;
  $('focus-version').textContent = node.version;
  $('focus-description').textContent = focusDescription(p);
  let actions = '';
  if (p.selected === 'batch' && p.batch.phase === 'implementing') actions += '<button type="button" class="primary" data-action="close-batch">收口本批</button>';
  else if (p.selected === 'memory') {
    if (p.memoryVersion === 'v3') actions += '<button type="button" class="primary" data-action="use-latest" title="下次请求使用新版" aria-label="下次请求使用新版">使用 v4</button>';
    if (p.memoryVersion) actions += '<button type="button" class="quiet-button" data-action="exclude">本次排除</button>';
    else actions += '<button type="button" class="primary" data-action="use-latest" title="下次请求加入新版">加入 v4</button>';
  } else if (p.selected === 'archive' && !p.archiveRecalled) actions += '<button type="button" class="primary" data-action="recall" title="下次请求召回原文片段">召回原文</button>';
  else if (p.selected === 'candidate') actions += '<button type="button" class="quiet-button" data-action="propose-edit">提出修订</button>';
  if (p.selected === 'test' && waitingForVerification(p)) actions += '<button type="button" class="primary" data-action="answer-question">回复验证提问</button>';
  $('focus-actions').innerHTML = actions;
  const readOnly = p.view === 'replay';
  $('focus-actions').querySelectorAll('[data-action]').forEach((button) => { if (!['detail', 'agent-context'].includes(button.dataset.action)) button.disabled = readOnly; });
  renderReference();
}
function renderReference() {
  const p = project(); const node = nodes(p)[p.selected]; const owner = currentOwner(); const interaction = currentInteraction();
  $('receiver-label').textContent = `发给：${owner.name} agent`;
  $('reference-label').textContent = interaction ? `${interaction.title} · v${interaction.revision}` : p.selected === 'main' ? '' : `${node.title} · ${node.version}`;
  $('reference-label').hidden = p.selected === 'main' && !interaction;
  $('dock-recipient').innerHTML = `<span class="recipient-project">${escapeHtml(p.name)} <span aria-hidden="true">/</span> </span><strong>${owner.name}</strong>`;
  $('send').setAttribute('aria-label', `${interaction ? '回复' : '发给'}${owner.name}`);
  $('send').title = `${interaction ? '回复' : '发给'}${owner.name} · Ctrl + Enter`;
  $('instruction').placeholder = interaction ? '回复…' : '补充要求，或安排下一步…';
  const delivery = p.view === 'replay' ? '回放中 · 需返回当前' : interaction?.status === 'replied' ? '已回复 · 等待接收方确认' : interaction?.status === 'resolved' ? '接收方已确认处理' : p.selected === 'scout' ? '续聊这个子任务' : '下次请求送达';
  $('delivery-label').title = delivery; $('delivery-label').setAttribute('aria-label', delivery);
  $('delivery-label').innerHTML = icon(interaction?.status === 'resolved' ? 'check' : 'clock');
  $('focus-strip').classList.toggle('has-actions', $('focus-actions').childElementCount > 0);
}
function renderContext() {
  const p = project();
  const snap = p.view === 'replay' ? p.snapshots.find((item) => item.step === p.replayStep) || p.snapshots[0] : snapshot(p);
  const items = manifest(p, snap);
  $('manifest-title').textContent = `主执行 · ${p.view === 'replay' ? '历史' : ''}请求 ${snap.step} 的上下文`;
  $('context-label').textContent = currentOwner().id === 'main' ? '本步上下文' : '关联工作上下文';
  $('context-caption').textContent = currentOwner().id === 'main' ? '约束 · 工作 · 记忆 · 工具 · 历史' : '关联工作：主执行的请求依据';
  $('context-total').textContent = `${(items.reduce((sum, item) => sum + item.size, 0) / 1000).toFixed(1)}k`;
  $('context-button').title = `${$('context-label').textContent} · 主执行 · 估算 ${$('context-total').textContent} tokens`;
  $('context-items').innerHTML = items.map((item) => `<div class="manifest-row"><strong>${item.type}</strong><div>${item.node && p.view !== 'replay' ? `<button type="button" data-detail-node="${item.node}">${escapeHtml(item.name)} ↗</button>` : escapeHtml(item.name)}<small>${escapeHtml(item.detail)}</small></div><span class="manifest-state">${item.state}</span><span class="manifest-size">${item.size ? item.size.toLocaleString() : '—'}</span></div>`).join('') + '<p class="manifest-note">token 为演示估算。此处显示实际请求的装配清单；检索候选、已折叠原文和未纳入内容不会被算成正在使用。</p>';
}
function messageHtml(message) {
  return `<article class="message user-message"><div class="message-byline">你 → ${escapeHtml(message.recipientName)}<span class="message-state ${message.status}">${message.status === 'queued' ? '排队中 · 等待送达' : message.status === 'failed' ? '未生效 · 问题版本已变' : `已送达 · ${escapeHtml(message.receipt)}`}</span></div><p>${escapeHtml(message.text)}</p><span class="message-reference">${escapeHtml(message.reference)}</span></article>${message.reply ? `<article class="message agent-receipt"><div class="message-byline">${icon('check')}${escapeHtml(message.recipientName)}<span>演示回执</span></div><p>${escapeHtml(message.reply)}</p></article>` : ''}`;
}
function renderMessages() {
  const p = project(); const owner = currentOwner();
  const initial = `<article class="message agent-message"><div class="message-byline">${owner.name} agent<span>示例会话</span></div><p>${owner.initial}</p></article>`;
  const html = initial + p.messages.filter(message => message.threadKey === threadKey()).map(messageHtml).join('');
  $('messages').innerHTML = html; $('module-messages').innerHTML = html;
}
function renderDetail() {
  const p = project(); const node = nodes(p)[p.selected];
  $('detail-kind').textContent = currentOwner().module;
  $('detail-title').textContent = node.title;
  $('module-owner').innerHTML = `${icon('agent')}${currentOwner().name}`;
  const related = Object.keys(owners).filter(id => owners[id].id === currentOwner().id);
  $('module-resources').innerHTML = related.map(id => `<button type="button" data-select="${id}" aria-pressed="${id === p.selected}">${escapeHtml(nodes(p)[id].title)}</button>`).join('');
  $('module-resources').hidden = related.length <= 1;
  const meta = `<div class="document-meta"><span class="detail-version">${escapeHtml(node.version)}</span></div>`;
  const detail = {
    batch: `<h3>所属需求与运行</h3><p>${activeId === 'kanzei' ? 'R-365 · 网页搜索与抓取升级 / B2 · 引用回溯' : 'R-001 · 附件全文检索 / B1 · 索引读取'}<br>负责人：主执行。${waitingForVerification(p) ? '验证目标等待回复，实施继续。' : '验证目标已确定。'}</p><h3>本批范围</h3><p>形成可复查的来源记录，验证内容与原文位置对应。</p><h3>持久进展</h3><p>${p.batch.phase === 'delivered' ? '示例提交 demo-commit-01 与检查点 07 已形成。机器验证：示例通过。用户试用：待试用。' : p.batch.phase === 'verifying' ? '已保存示例快照 demo-tree-01；验证阶段，尚未记为交付。' : '已有文件修改，尚未形成当前批次的交付版本。'}</p>`,
    memory: `<div class="provenance"><span>${icon('child')}资料核对</span><span>${icon('document')}${p.memoryVersion || '未纳入'}</span><span>${icon('agent')}请求 ${p.step}</span></div><div class="version-compare"><section class="version-old"><h3>v3 <small>${p.memoryVersion === 'v3' ? '本步使用' : '旧版'}</small></h3><p>搜索结果有可访问链接，即可进入引用列表。</p><p class="quiet">页面内容留待后续人工核对。</p></section><section class="version-new"><h3>v4 <small>${p.memoryVersion === 'v4' ? '本步使用' : '新版'}</small></h3><p>引用必须能回到具体原文片段。</p><p>无法取证的页面保留链接，标记“待核对”。</p><p class="quiet">搜索摘要不能直接作为原文证据。</p></section></div>`,
    work: `<h3>${activeId === 'kanzei' ? 'R-365 · 网页搜索与抓取升级' : 'R-001 · 附件全文检索'}</h3><p>需求状态：进行中。当前批次：${activeId === 'kanzei' ? 'B2 / 5 · 引用回溯' : 'B1 / 3 · 附件检索'}。</p><h3>目标与验收</h3><p>输出中的每条引用能回到具体来源；有内容片段，无法取证的引用明确标识。</p><h3>运行状态</h3><p>${p.paused ? '主执行已暂停。' : '主执行运行中。'}${waitingForVerification(p) ? '验证管理等待一条回复，其他工作继续。' : '验证目标已经确定。'}</p>`,
    archive: `<div class="provenance"><span>请求 1–15</span>→<span>历史摘要 03.md · v1</span>→<span>${p.archiveRecalled ? '请求 ' + p.step + ' 已召回原文' : '原文仍在归档'}</span></div><h3>摘要</h3><p>已排除无法取得正文的来源，保留待核对条目。下一步检查引用与原文之间的对应关系。</p><details><summary>展开请求 10 的原文片段 · 查看不会装入上下文</summary><pre>用户：没有取得原文时请明确写待核对。
工具：来源 B 返回访问失败，正文不可取得。
检查点：保留链接，并记录证据缺口。</pre></details><p class="manifest-note">该原文片段估算 6.4k tokens；只有“下次召回原文片段”会改变后续请求。</p>`,
    main: `<h3>当前请求 ${p.step}</h3><p>持有当前工作、项目约束、${p.memoryVersion ? `${escapeHtml(p.memoryName)} ${p.memoryVersion}` : '排除该记忆后的上下文'}、工具证据与折叠历史。</p><h3>收件方式</h3><p>操作台指令进入本工作队列，在下一次模型请求中装入并返回回执。</p><h3>运行边界</h3><p>${settingsLabel(p.settings)}。模型和工具范围变更在下一次请求使用。</p>`,
    scout: '<h3>收到的交接包</h3><p>目标：核对三条来源能否支持引用。范围：只读检索。输入：来源链接和引用片段。</p><h3>返回给主执行</h3><p>三个来源、可回溯片段与一项待核对事实。主执行选择交接结论纳入自己的下一次请求。</p><h3>没有共享</h3><p>其他项目的记忆、主执行完整历史和无关工具结果。</p>',
    fetch: `<div class="provenance"><span>${icon('agent')}请求 ${p.step}</span><span>${icon('tool')}tool_demo_018</span><span>${icon('document')}artifact_demo_018</span></div><div class="tool-result">${icon('check')}<div><h3>正文片段已返回</h3><p>核对引用句与来源正文，原文位置已保留。</p></div></div><details class="tool-parameters"><summary>调用参数</summary><pre>{ "source_ref": "source-demo-03", "requested_range": "引用附近正文" }</pre></details>`,
    test: `<h3>验证计划</h3><p>针对引用与来源对应关系运行定向验证。</p><h3>运行状态</h3><p>${waitingForVerification(p) ? '等待你确定验证目标。回复后恢复本批验证，其他模块继续工作。' : p.settings.tools === 'all' ? '验证目标已确定，可以继续。' : '当前工具范围为只读，执行尚未派发。'}</p><h3>依据</h3><p>检查点 06 · 请求 ${p.step} · 绑定本批快照。</p>`,
    candidate: '<div class="provenance"><span>页面取证 · tool_demo_018</span>→<span>memory_note</span>→<span>取证记录.md · candidate</span></div><h3>候选内容</h3><p>引用需附可追溯原文片段；取证失败需明确保留缺口。</p><h3>下一步</h3><p>由记忆管理者核对来源与重复内容。候选不会仅因出现在界面就进入有效记忆。</p>',
  };
  const evidence = {
    work: [['requirements', '目标与验收原文']], batch: [['checkpoint', '批次检查点'], ['verification', '验证记录']],
    memory: [['memory-v3', 'v3 原文'], ['memory-v4', 'v4 原文'], ['request', `请求 ${p.step}`]],
    candidate: [['candidate', '候选原文'], ['tool-output', '来源片段']],
    archive: [['archive-original', '原文 · 请求 10'], ['request', '装配记录']],
    main: [['request', `请求 ${p.step} 的依据`], ['checkpoint', '检查点']],
    scout: [['handoff', '交接原文']], fetch: [['tool-output', '返回的正文片段'], ['tool-input', '调用参数']],
    test: [['verification', '验证记录'], ['checkpoint', '绑定的检查点']],
  }[p.selected];
  const evidenceLinks = `<div class="evidence-links" aria-label="查看具体依据">${evidence.map(([key, title]) => `<button type="button" data-evidence="${key}">${icon('document')}<span>${escapeHtml(title)}</span>${icon('chevron')}</button>`).join('')}</div>`;
  $('detail-content').innerHTML = `<div class="document-text">${meta}${detail[p.selected]}${evidenceLinks}</div>`;
  renderModuleTab();
}
const EVIDENCE_KEYS = new Set(['requirements', 'checkpoint', 'verification', 'memory-v3', 'memory-v4', 'request', 'candidate', 'tool-output', 'tool-input', 'archive-original', 'handoff']);
function openEvidence(key) {
  if (!EVIDENCE_KEYS.has(key)) return;
  const p = project();
  p.evidenceReturn = { panel: p.panel, scroll: $('work-surface').scrollTop, key };
  p.evidence = { key, snap: snapshot(p), batchPhase: p.batch.phase, waiting: waitingForVerification(p) };
  p.panel = 'evidence'; showSurface(); $('work-surface').scrollTop = 0;
  $('evidence-title').focus({ preventScroll: true });
}
function closeEvidence() {
  const p = project(), prior = p.evidenceReturn;
  p.panel = prior?.panel || 'detail'; p.evidence = null; p.evidenceReturn = null;
  renderAll(); $('work-surface').scrollTop = prior?.scroll || 0;
  if (prior?.key) document.querySelector(`[data-evidence="${prior.key}"]`)?.focus({ preventScroll: true });
}
function renderEvidence() {
  const p = project(), proof = p.evidence;
  if (!proof || !EVIDENCE_KEYS.has(proof.key)) return;
  const n = proof.snap.step;
  const source = (start, lines) => `<div class="source-lines" role="region" aria-label="原文片段">${lines.map((line, i) => `<div class="source-line"><span aria-hidden="true">${start + i}</span><code>${escapeHtml(line)}</code></div>`).join('')}</div>`;
  const records = {
    'memory-v3': { kind: '记忆原文', title: p.memoryName, version: 'v3', path: `.kanzei/memory/${p.memoryName}`, body: source(12, ['## 引用规则', '', '搜索结果有可访问链接，即可进入引用列表。', '保留来源标题和链接。', '', '页面内容留待后续人工核对。', '无法访问的页面仍保留检索记录。']) },
    'memory-v4': { kind: '记忆原文', title: p.memoryName, version: 'v4', path: `.kanzei/memory/${p.memoryName}`, body: source(12, ['## 引用规则', '', '引用必须能回到具体原文片段。', '保留来源标题、链接和原文位置。', '', '无法取证的页面保留链接，标记“待核对”。', '搜索摘要不能直接作为原文证据。', '', '适用范围：当前项目的检索与引用。', '来源：页面取证记录与人工纠正。', '旧引用需在下一次请求时重新核对。']) },
    requirements: { kind: '需求原文', title: p.work, version: '检查点 06', path: '.kanzei/project/requirements.md', body: source(1, [`# ${p.work}`, '', '目标：每条引用能回到具体来源。', '验收：引用内容与原文位置一致。', '缺少原文时明确保留证据缺口。', '', '本批：引用回溯与定向验证。']) },
    checkpoint: { kind: '批次记录', title: '检查点', version: proof.batchPhase === 'delivered' ? '07' : '06', path: '工作记录 / 当前批次', body: `<div class="evidence-flow"><span>${icon('tool')}实施</span><span>${icon('chevron')}</span><span>${icon('verify')}验证</span><span>${icon('chevron')}</span><span>${icon('commit')}交付</span></div><p class="evidence-description">${proof.batchPhase === 'delivered' ? '定向验证完成，示例提交 demo-commit-01 与检查点 07 已形成。' : proof.batchPhase === 'verifying' ? '示例快照 demo-tree-01 已冻结，等待验证完成。' : '本批尚在实施，没有交付版本。'}</p>` },
    verification: { kind: '验证证据', title: '引用与来源一致性', version: proof.batchPhase === 'delivered' ? '示例通过' : proof.waiting ? '等待目标' : '待执行', path: '验证记录 / 当前批次', body: source(1, [proof.batchPhase === 'delivered' ? '示例结果：通过' : '结果：尚无完成记录', `关联请求：${n}`, `目标：${proof.waiting ? '等待回复' : '已确定'}`, '检查项：引用能回到原文；无法取证的引用保留缺口。']) },
    candidate: { kind: '候选原文', title: '取证记录.md', version: 'candidate', path: '.kanzei/memory/inbox/取证记录.md', body: source(1, ['# 取证记录', '', '引用需附可追溯原文片段。', '取证失败需明确保留缺口。', '', '状态：待记忆管理核对来源与适用范围。']) },
    'tool-output': { kind: '工具结果', title: '来源 03 · 正文片段', version: 'artifact_demo_018', path: '取证记录 / source-demo-03', body: source(42, ['[示例来源正文]', '这段内容是引用对应的原文片段。', '取证记录保留了来源标识和正文位置。', '不能取得原文的来源只保留链接与缺口说明。']) },
    'tool-input': { kind: '工具调用', title: '获取原始页面', version: 'tool_demo_018', path: `主执行 / 请求 ${n}`, body: source(1, ['{', '  "source_ref": "source-demo-03",', '  "requested_range": "引用附近正文"', '}']) },
    'archive-original': { kind: '归档原文', title: '请求 10 的原文片段', version: '原文', path: '历史摘要 03.md → 请求 10', body: source(1, ['用户：没有取得原文时请明确写待核对。', '工具：来源 B 返回访问失败，正文不可取得。', '检查点：保留链接，并记录证据缺口。']) },
    handoff: { kind: '子任务交接', title: '资料核对 → 主执行', version: '3 份来源', path: '资料核对 / 交接记录', body: source(1, ['目标：核对三条来源能否支持引用。', '范围：只读检索。', '', '来源 A：正文可回溯。', '来源 B：访问失败，需要标记待核对。', '来源 C：保留原文片段与位置。', '', '交接只包含结论与引用，不共享完整上下文。']) },
    request: { kind: '请求依据', title: `请求 ${n} 的装配记录`, version: '只读快照', path: '主执行 / 上下文', body: `<div class="request-proof">${manifest(p, proof.snap).map(item => `<div class="request-proof-row"><span>${escapeHtml(item.type)}</span><strong>${escapeHtml(item.name)}</strong><span>${escapeHtml(item.state)}</span></div>`).join('')}</div>` },
  };
  const record = records[proof.key];
  $('evidence-parent').textContent = currentOwner().module;
  $('evidence-kind').textContent = record.kind;
  $('evidence-title').textContent = record.title;
  $('evidence-version').textContent = record.version;
  $('evidence-content').innerHTML = `<div class="evidence-source">${icon('document')}<span>${escapeHtml(record.path)}</span><small>示例记录</small></div>${record.body}`;
}
function renderModuleTab() { const chat = project().moduleTab === 'chat'; $('module-management').hidden = chat; $('module-messages').hidden = !chat; document.querySelectorAll('[data-module-tab]').forEach(button => button.setAttribute('aria-pressed', String((button.dataset.moduleTab === 'chat') === chat))); schedulePersist(); }
function renderReplay() {
  const p = project();
  $('replay-step').innerHTML = p.snapshots.map((s) => `<option value="${s.step}" ${s.step === p.replayStep ? 'selected' : ''}>请求 ${s.step}</option>`).join('');
  const snap = p.snapshots.find((s) => s.step === p.replayStep) || p.snapshots[0];
  $('replay-content').innerHTML = `<div class="replay-event"><time>01 装配</time><div><p>${escapeHtml(p.memoryName)} · ${snap.memoryVersion || '已排除'}<br>${settingsLabel(snap.settings)}</p><small>这是请求 ${snap.step} 发出时的快照，后来修订不会重写它。</small></div></div><div class="replay-event"><time>02 读取</time><div><p>${snap.memoryVersion ? '工具返回所引用文件的正文片段。' : '本请求未装入该记忆。'}</p><small>没有记录的行为保持未知。</small></div></div><div class="replay-event"><time>03 归档</time><div><p>${snap.archiveRecalled ? '归档原文片段重新装入该请求。' : '仅保留历史摘要，原文通过引用定位。'}</p><small>回放只查看记录；要安排新工作，请返回当前。</small></div></div>`;
}
function showSurface() {
  const p = project();
  for (const mode of ['flow', 'conversation', 'replay', 'context', 'detail', 'settings', 'inbox', 'evidence']) $(mode + '-view').hidden = mode !== (p.panel || p.view);
  document.querySelectorAll('[data-view]').forEach((button) => button.setAttribute('aria-pressed', String(button.dataset.view === p.view)));
  $('context-button').setAttribute('aria-expanded', String(p.panel === 'context'));
  $('settings-button').setAttribute('aria-expanded', String(p.panel === 'settings'));
  const replay = p.view === 'replay';
  document.body.dataset.surface = p.panel || p.view;
  const answered = ['replied', 'resolved'].includes(currentInteraction()?.status);
  $('operation-dock').dataset.reply = currentInteraction()?.status || '';
  $('reply-complete').hidden = currentInteraction()?.status !== 'resolved';
  $('instruction').disabled = replay || answered;
  $('scope').disabled = replay || Boolean(currentInteraction());
  $('send').disabled = replay || answered;
  $('simulate').disabled = replay;
  $('settings-button').disabled = replay;
  $('alert-button').disabled = replay;
  $('return-live').hidden = !replay;
  renderReference();
  renderContext();
  if (p.panel === 'evidence') renderEvidence();
  if (!p.panel && p.view === 'flow') requestAnimationFrame(drawWires);
  if (floating) requestAnimationFrame(clampFloating);
  schedulePersist();
}
function renderChrome() {
  const p = project();
  $('project-name').textContent = overview ? '全部项目' : p.name;
  $('work-title').textContent = p.work;
  $('work-id').textContent = `当前工作 · 演示 ${p.id}`;
  $('step-label').textContent = `#${p.step}`;
  $('step-label').title = `主执行 · 请求 ${p.step}`;
  $('alert-text').textContent = '旧版记忆';
  $('alert-button').title = memoryStatus(p);
  $('alert-button').hidden = p.memoryVersion !== 'v3';
  $('alert-button').classList.toggle('resolved', p.memoryVersion !== 'v3');
  $('effective-settings').textContent = settingsLabel(p.settings);
  $('model-label').textContent = `${p.settings.model === 'primary' ? '主模型' : '快速模型'}${p.settings.tools === 'readonly' ? ' · 只读' : ''}`;
  $('settings-button').title = `${settingsLabel(p.settings)} · 当前工作执行设置`;
  const phase = p.batch.phase;
  $('requirement-label').textContent = activeId === 'kanzei' ? 'R-365 · 网页搜索与抓取升级' : 'R-001 · 附件全文检索';
  $('batch-id').textContent = activeId === 'kanzei' ? 'B2 / 5' : 'B1 / 3';
  $('batch-name').textContent = activeId === 'kanzei' ? '引用回溯' : '附件检索';
  $('batch-status').textContent = phase === 'delivered' ? '已交付 · 待试用' : phase === 'verifying' ? '验证中' : '实现中';
  $('batch-runtime').textContent = phase === 'delivered' ? '本批运行结束 · 等待试用' : phase === 'verifying' ? waitingForVerification(p) ? '验证暂停 · 等你回复' : p.settings.tools === 'readonly' ? '验证暂停 · 当前仅允许读取' : '验证管理运行中' : `${p.paused ? '主执行已暂停' : '主执行运行中'}${waitingForVerification(p) ? ' · 验证待回复' : ' · 验证目标已确定'}`;
  const phaseIndex = ['implementing', 'verifying', 'delivered'].indexOf(phase);
  $('batch-stages').innerHTML = [['tool','实施'],['verify','验证'],['commit','交付']].map(([symbol, label], i) => `${i ? '<i class="stage-line" aria-hidden="true"></i>' : ''}<span class="stage ${i === phaseIndex ? 'current' : i < phaseIndex ? 'done' : ''}" ${i === phaseIndex ? 'aria-current="step"' : ''}><i>${icon(i < phaseIndex ? 'check' : symbol)}</i><span>${label}</span></span>`).join('');
  $('batch-delivery').textContent = phase === 'delivered' ? 'demo-commit-01' : phase === 'verifying' ? '已留快照' : '未交付';
  $('batch-delivery').title = phase === 'delivered' ? '检查点 07 · 已交付，待试用' : phase === 'verifying' ? 'demo-tree-01 · 冻结本批，等待验证' : '尚未形成交付';
  $('notice').textContent = p.notice;
  $('dock-footer').hidden = !p.notice;
  const queued = p.messages.filter((m) => m.status === 'queued').length + p.pending.length;
  $('queue-label').innerHTML = queued ? `${icon('clock')}<span>${queued}</span>` : '';
  $('queue-label').title = `项目 ${queued} 项排队中`;
  $('queue-label').setAttribute('aria-label', `项目 ${queued} 项排队中`);
  $('run-state').textContent = phase === 'delivered' ? '已交付' : p.paused ? '已暂停' : phase === 'verifying' ? waitingForVerification(p) ? '等待回复' : p.settings.tools === 'readonly' ? '等待执行' : '验证中' : '运行中';
  document.querySelector('.live-state').dataset.state = phase === 'delivered' ? 'done' : (phase === 'verifying' && (waitingForVerification(p) || p.settings.tools === 'readonly')) || p.paused ? 'waiting' : 'running';
  document.querySelector('.live-state').title = $('batch-runtime').textContent;
  const attention = interactions.filter(item => ['open', 'failed'].includes(item.status)).length;
  $('inbox-count').textContent = String(attention);
  $('inbox-count').hidden = !attention;
  $('inbox-button').setAttribute('aria-pressed', String(!overview && p.routeKind === 'inbox'));
  const next = sortedInteractions().find(item => item.projectId === activeId && item.status !== 'resolved');
  $('attention-shortcut').hidden = !next;
  if (next) {
    $('attention-shortcut').dataset.interaction = next.id;
    $('attention-owner').textContent = owners[next.node].name;
    $('attention-question').textContent = next.title;
    $('attention-shortcut').title = `${interactionState(next)} · ${next.impact}`;
    $('attention-shortcut').setAttribute('aria-label', `${interactionState(next)}：${next.title}，${next.impact}`);
  }
  for (const button of $('overview').querySelectorAll('[data-project]')) {
    const state = projects[button.dataset.project];
    button.children[1].textContent = state.batch.phase === 'delivered' ? '本批已交付 · 等待试用' : state.batch.phase === 'verifying' ? waitingForVerification(state) ? '本批验证 · 等你回复' : '本批验证中' : state.memoryVersion === 'v3' ? '主执行运行中 · 1 条旧版本引用待核对' : '主执行运行中 · 本批实施中';
  }
  document.querySelectorAll('.project[data-project]').forEach((button) => {
    const active = !overview && button.dataset.project === activeId;
    button.classList.toggle('active', active);
    if (active) button.setAttribute('aria-current', 'page'); else button.removeAttribute('aria-current');
  });
}
function renderAll() { renderChrome(); renderNodes(); renderFocus(); renderContext(); renderMessages(); renderReplay(); renderDetail(); if (currentInteraction()) renderInbox(); showSurface(); }
function setNotice(text) { project().notice = text; $('notice').textContent = text; $('dock-footer').hidden = !text; schedulePersist(); }
function selectNode(id) {
  if (!owners[id]) return;
  saveDraft();
  const p = project(); p.selected = id; p.reference = id; p.routeKind = 'module';
  p.panels = []; p.evidence = null; p.evidenceReturn = null;
  p.interactionId = null; p.shownRevision = null; p.moduleTab = 'manage'; p.panel = 'detail'; p.view = 'flow';
  restoreDraft(); renderAll(); $('work-surface').scrollTop = 0;
  $('detail-title').focus({ preventScroll: true });
}
function returnNetwork(view = 'flow') {
  saveDraft();
  const previousNode = project().selected;
  const p = project(); p.routeKind = 'workbench'; p.selected = 'main'; p.reference = 'main';
  p.panels = []; p.evidence = null; p.evidenceReturn = null;
  p.interactionId = null; p.shownRevision = null; p.view = view; p.panel = null;
  restoreDraft(); renderAll(); $('work-surface').scrollTop = 0;
  if (view === 'flow') document.querySelector(`.flow-node[data-select="${previousNode}"]`)?.focus({ preventScroll: true });
}
function openPanel(panel) {
  const p = project();
  lastTrigger = document.activeElement;
  if (p.panel === panel) { closePanel(); return; }
  p.panels.push({ panel: p.panel, scroll: $('work-surface').scrollTop });
  p.panel = panel;
  if (p.panel === 'context') renderContext();
  if (p.panel === 'detail') renderDetail();
  if (p.panel === 'settings') {
    const values = p.settingsDraft || p.pending.find((item) => item.kind === 'settings')?.value || p.settings;
    $('model-select').value = values.model; $('tools-select').value = values.tools; $('subagents-toggle').checked = values.subagents;
  }
  showSurface(); $('work-surface').scrollTop = 0;
}
function closePanel() { const p = project(); if (p.panel === 'evidence') { closeEvidence(); return; } const prior = p.panels.pop(); p.panel = prior ? prior.panel : p.routeKind === 'module' ? 'detail' : p.routeKind === 'inbox' ? 'inbox' : null; renderAll(); $('work-surface').scrollTop = prior?.scroll || 0; if (lastTrigger?.isConnected && lastTrigger.getClientRects().length) lastTrigger.focus({ preventScroll: true }); }
function enqueue(kind, value, label) {
  const p = project();
  p.pending = p.pending.filter((item) => item.kind !== kind);
  p.pending.push({ kind, value, label });
  setNotice(`${label}已排队${automatic ? '。' : '；模拟下一步后生效。'}`);
  renderChrome();
  scheduleDelivery(activeId);
}
function action(name) {
  const p = project();
  if (name === 'replay-context') { openPanel('context'); return; }
  if (name === 'detail') { selectNode(p.selected); return; }
  if (name === 'interaction-source') { selectNode(currentInteraction().node); return; }
  if (name === 'answer-question') { openInbox(interactions.find(item => item.projectId === activeId && item.node === 'test' && item.status !== 'resolved')?.id); return; }
  if (name === 'agent-context') { openPanel(p.selected === 'main' ? 'context' : 'detail'); return; }
  if (p.view === 'replay') return;
  if (name === 'use-latest') enqueue('memory', 'v4', '使用新版 v4');
  if (name === 'exclude') enqueue('memory', null, '本次工作排除该记忆');
  if (name === 'recall') enqueue('archive', true, '召回请求 10 的原文片段');
  if (name === 'close-batch') enqueue('batch', 'closing', '收口当前批次');
  if (name === 'propose-edit') {
    if (!$('instruction').value) $('instruction').value = '请核对这条候选记忆的来源，并补充适用范围。';
    saveDraft(); $('instruction').focus();
  }
}
function switchProject(id) {
  if (!projects[id]) return;
  saveDraft();
  activeId = id; overview = false;
  $('overview').hidden = true; $('project-workspace').hidden = false; $('simulate').hidden = false;
  restoreDraft();
  renderAll();
  if (project().panel === 'settings') { project().panel = null; openPanel('settings'); }
}
function simulate(projectId = activeId, background = false) {
  if (!Object.hasOwn(projects, projectId)) return;
  const p = projects[projectId]; if (!background && (overview || p.view === 'replay')) return;
  const area = $('work-surface');
  const following = projectId === activeId && (p.panel === 'inbox' || (!p.panel && p.view === 'conversation') || (p.panel === 'detail' && p.moduleTab === 'chat')) && area.scrollHeight - area.clientHeight - area.scrollTop < 48;
  const verifying = p.batch.phase === 'verifying';
  const pendingCount = p.pending.length + p.messages.filter((m) => m.status === 'queued').length;
  for (const item of p.pending) {
    if (item.kind === 'memory') p.memoryVersion = item.value;
    if (item.kind === 'settings') p.settings = { ...item.value };
    if (item.kind === 'archive') p.archiveRecalled = item.value;
    if (item.kind === 'batch') p.batch.phase = 'verifying';
  }
  p.step += 1;
  for (const message of p.messages.filter(item => item.status === 'queued')) {
    const interaction = interactions.find(item => item.id === message.interactionId);
    if (interaction && interaction.revision !== message.expectedRevision) {
      message.status = 'failed'; interaction.status = 'failed'; continue;
    }
    message.status = 'applied'; message.appliedStep = p.step;
    message.receipt = `${message.recipientName} / 请求 ${p.step}`;
    message.reply = interaction ? '回复已送达，关联事项已处理。' : `这条要求已送达${message.recipientName}，关联对象保持为“${message.objectTitle || message.reference}”。`;
    if (interaction) { interaction.status = 'resolved'; interaction.receipt = message.receipt; }
  }
  if (verifying && !waitingForVerification(p) && p.settings.tools === 'all') { p.batch.phase = 'delivered'; p.batch.artifact = 'demo-commit-01'; }
  else if (p.batch.phase === 'implementing' && !p.paused) p.batch.edits += 1;
  p.pending = []; p.snapshots.push(snapshot(p));
  p.notice = pendingCount ? `请求 ${p.step} · 回执已更新` : '';
  const signalEdges = [['work', 'main'], ['archive', 'main']];
  if (p.memoryVersion) signalEdges.push(['memory', 'main']);
  if (verifying && !waitingForVerification(p) && p.settings.tools === 'all') signalEdges.push(['main', 'test']);
  if (projectId === activeId) { renderAll(); if (following) area.scrollTop = area.scrollHeight; if (!overview && p.view !== 'replay') requestAnimationFrame(() => pulseSignal(signalEdges)); }
  else { renderChrome(); if (currentInteraction()) renderInbox(); }
  schedulePersist();
  if (p.batch.phase === 'verifying' && !waitingForVerification(p) && p.settings.tools === 'all') scheduleDelivery(projectId);
}

function sortedInteractions() { return [...interactions].sort((a, b) => (a.status === 'resolved') - (b.status === 'resolved') || (b.status === 'failed') - (a.status === 'failed') || Number(b.blocking) - Number(a.blocking)); }

function renderInbox() {
  const interaction = currentInteraction(); if (!interaction) return;
  $('inbox-summary').textContent = `${interactions.filter(item => ['open', 'failed'].includes(item.status)).length} 条待回复`;
  $('interaction-list').innerHTML = sortedInteractions().map(item => `<button type="button" data-interaction="${item.id}" aria-pressed="${item.id === interaction.id}" class="interaction-item ${item.read ? '' : 'unread'}" title="${interactionState(item)} · ${item.impact}"><span class="interaction-avatar">${icon(item.status === 'resolved' ? 'check' : item.blocking ? 'chat' : 'mail')}</span><span class="interaction-copy"><span class="interaction-meta">${escapeHtml(projects[item.projectId].name)}</span><strong>${escapeHtml(item.title)}</strong><span class="interaction-status ${item.status}">${item.status === 'replied' ? '等待处理' : interactionState(item)}</span></span></button>`).join('');
  $('interaction-header').innerHTML = `<span class="eyebrow">${escapeHtml(project().name)} / ${escapeHtml(owners[interaction.node].name)} agent / 问题 v${interaction.revision}</span><h3>${escapeHtml(interaction.title)}</h3>`;
  const impact = interaction.status === 'resolved' ? interaction.blocking ? project().batch.phase === 'delivered' ? '回复已处理，本批已交付。' : '回复已处理，关联步骤继续推进。' : '这条事项已处理。' : interaction.impact;
  $('interaction-content').innerHTML = `<p>${escapeHtml(interaction.body)}</p><p class="interaction-impact ${interaction.blocking && interaction.status !== 'resolved' ? 'waiting' : ''}">${escapeHtml(impact)}</p>`;
  const canReply = ['open', 'failed'].includes(interaction.status);
  $('interaction-choices').innerHTML = canReply ? interaction.choices.map((choice, index) => `<button type="button" data-choice="${index}" title="填入回复" aria-pressed="false">${icon('reply')}${escapeHtml(choice)}</button>`).join('') : `<p class="reply-receipt">${icon(interaction.status === 'resolved' ? 'check' : 'clock')}${interactionState(interaction)}${interaction.receipt ? ` · ${escapeHtml(interaction.receipt)}` : ''}</p>`;
  $('interaction-replies').innerHTML = project().messages.filter(message => message.interactionId === interaction.id).map(messageHtml).join('');
  if (interaction.status === 'failed') $('interaction-content').insertAdjacentHTML('beforeend', '<p class="interaction-impact waiting">问题版本已更新，之前的回复没有生效。重新打开这条消息，核对后再回复。</p>');
}
function routeSnapshot() { const p = project(); return { activeId, routeKind: p.routeKind, selected: p.selected, reference: p.reference, view: p.view, panel: p.panel, moduleTab: p.moduleTab, evidence: p.evidence, evidenceReturn: p.evidenceReturn, panels: [...p.panels], scroll: $('work-surface').scrollTop }; }
function openInbox(id) {
  const item = interactions.find(interaction => interaction.id === id) || sortedInteractions().find(interaction => ['open', 'failed'].includes(interaction.status)) || interactions[0];
  if (!item) return;
  if (project().routeKind !== 'inbox') inboxReturn = routeSnapshot();
  saveDraft(); activeId = item.projectId; overview = false;
  const p = project(); p.routeKind = 'inbox'; p.interactionId = item.id; p.shownRevision = item.revision;
  p.selected = item.node; p.reference = item.node; p.panel = 'inbox'; p.view = 'flow'; item.read = true;
  p.panels = [];
  $('overview').hidden = true; $('project-workspace').hidden = false; $('simulate').hidden = false;
  restoreDraft(); $('scope').value = 'work'; renderAll(); $('work-surface').scrollTop = 0;
  if (toastId === item.id) $('message-toast').hidden = true;
}
function closeInbox() {
  saveDraft();
  if (!inboxReturn) { returnNetwork(); return; }
  const destination = inboxReturn; inboxReturn = null; activeId = destination.activeId;
  const { activeId: ignored, ...route } = destination; Object.assign(project(), route, { interactionId: null, shownRevision: null });
  restoreDraft(); renderAll(); $('work-surface').scrollTop = destination.scroll || 0;
}
function showToast(item) {
  toastId = item.id;
  $('toast-sender').textContent = `${owners[item.node].name} · ${item.kind}`;
  $('toast-body').textContent = item.title;
  $('toast-scope').textContent = `${projects[item.projectId].name} · ${item.blocking ? '等待回复' : '继续运行'}`;
  $('message-toast').hidden = false;
  $('arrival-announcement').textContent = `收到${projects[item.projectId].name}的${item.kind}：${item.title}。可在工作消息中处理。`;
}
function simulateArrival() {
  const item = { id: `question-${++interactionSequence}`, projectId: activeId, node: 'memory', kind: '提问', title: '这条修订适用于整个项目吗', body: '新的取证约定已经整理成候选。请确定它只用于当前批次，还是提交为项目约定候选。', impact: '记忆候选等待确认，当前执行继续', blocking: false, choices: ['只用于当前批次', '作为项目约定候选'], status: 'open', revision: 1, read: false };
  interactions.push(item); renderChrome(); if (currentInteraction()) renderInbox(); showToast(item);
}
function sendMessage() {
  const p = project(); const interaction = currentInteraction();
  if (p.view === 'replay' || composing || ['replied', 'resolved'].includes(interaction?.status)) return;
  const text = $('instruction').value;
  if (!text.trim()) { $('instruction').focus(); setNotice('输入消息后发送。'); return; }
  if (interaction && p.shownRevision !== interaction.revision) { setNotice('问题已经更新。重新打开这条工作消息，核对后再回复；草稿已保留。'); saveDraft(); return; }
  const owner = currentOwner(); const node = nodes(p)[p.selected];
  const key = threadKey();
  p.messages.push({ text, threadKey: key, projectId: activeId, recipientId: owner.id, recipientName: owner.name, node: p.selected, objectTitle: node.title, scope: interaction ? 'work' : $('scope').value, reference: interaction ? `${p.name} / ${interaction.title} · 问题 v${p.shownRevision}` : `${p.name} / ${node.title} · ${node.version}`, interactionId: interaction?.id, expectedRevision: interaction?.revision, status: 'queued' });
  if (interaction) interaction.status = 'replied';
  p.drafts.delete(key); $('instruction').value = '';
  if (p.routeKind === 'module') { p.moduleTab = 'chat'; p.panel = 'detail'; p.panels = []; }
  else if (p.routeKind === 'workbench') { p.view = 'conversation'; p.panel = null; p.panels = []; }
  setNotice(`已为${owner.name}排队。${interaction ? '接收方确认后才记为已处理。' : '下次请求送达后显示回执。'}`);
  renderAll(); $('work-surface').scrollTop = $('work-surface').scrollHeight; if (!$('instruction').disabled) $('instruction').focus({ preventScroll: true });
  scheduleDelivery(activeId);
}

// Floating and docked modes share this one DOM subtree, including the same textarea.
let floating = false;
let floatingPoint = null;
let dragging = null;
function clampFloating() {
  if (!floating) return;
  const rect = $('operation-dock').getBoundingClientRect();
  document.body.style.setProperty('--floating-height', `${rect.height}px`);
  const next = floatingPoint || { x: window.innerWidth - rect.width - 24, y: window.innerHeight - rect.height - 24 };
  floatingPoint = { x: Math.max(12, Math.min(next.x, window.innerWidth - rect.width - 12)), y: Math.max(12, Math.min(next.y, window.innerHeight - rect.height - 12)) };
  $('operation-dock').style.left = `${floatingPoint.x}px`; $('operation-dock').style.top = `${floatingPoint.y}px`;
  schedulePersist();
}
function setFloating(value) {
  floating = value && window.innerWidth > 620;
  $('operation-dock').classList.toggle('floating', floating);
  document.body.classList.toggle('composer-floating', floating);
  $('float-toggle').setAttribute('aria-pressed', String(floating));
  $('float-toggle').innerHTML = icon(floating ? 'dock' : 'float');
  $('float-toggle').setAttribute('aria-label', floating ? '停靠对话框' : '悬浮对话框');
  $('float-toggle').title = floating ? '停靠对话框' : '悬浮对话框';
  if (floating) clampFloating(); else { $('operation-dock').style.left = ''; $('operation-dock').style.top = ''; }
  requestAnimationFrame(drawWires);
  schedulePersist();
}

function scheduleDelivery(projectId) {
  if (!automatic || deliveryTimers.has(projectId)) return;
  const p = projects[projectId];
  if (!p.pending.length && !p.messages.some(m => m.status === 'queued') && !(p.batch.phase === 'verifying' && !waitingForVerification(p) && p.settings.tools === 'all')) return;
  deliveryTimers.set(projectId, setTimeout(() => {
    deliveryTimers.delete(projectId);
    if (automatic) simulate(projectId, true);
  }, 1200));
}
function renderExperience() {
  $('experience-toggle').setAttribute('aria-pressed', String(automatic));
  $('experience-label').textContent = automatic ? '体验中' : '手动演示';
  $('undo-restart').hidden = !previousExperience;
}
function experienceSnapshot() {
  return { version: 4, activeId, overview, automatic, interactionSequence, inboxReturn, floating, floatingPoint,
    projects: Object.fromEntries(Object.entries(projects).map(([id, p]) => [id, { ...p, drafts: [...p.drafts] }])), interactions,
    scroll: $('work-surface').scrollTop };
}
function schedulePersist() {
  if (!storageReady || manualExperience) return;
  clearTimeout(persistTimer); persistTimer = setTimeout(persistNow, 100);
}
function persistNow() {
  if (!storageReady || manualExperience) return;
  clearTimeout(persistTimer);
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(experienceSnapshot())); $('storage-state').textContent = '现场保存在当前浏览器'; }
  catch { $('storage-state').textContent = '浏览器未允许保存，刷新后现场会重置'; }
}
function validExperience(saved) {
  const texts = (value, keys) => value && keys.every(key => typeof value[key] === 'string');
  const owner = key => typeof key === 'string' && Object.hasOwn(owners, key);
  const settings = s => s && ['primary', 'fast'].includes(s.model) && ['all', 'readonly'].includes(s.tools) && typeof s.subagents === 'boolean';
  const snap = s => s && Number.isSafeInteger(s.step) && s.step >= 0 && [null, 'v3', 'v4'].includes(s.memoryVersion) && settings(s.settings);
  const panel = p => [null, 'detail', 'context', 'settings', 'inbox', 'evidence'].includes(p);
  const proof = e => e === null || (e && EVIDENCE_KEYS.has(e.key) && snap(e.snap) && ['implementing', 'verifying', 'delivered'].includes(e.batchPhase));
  if (!saved || saved.version !== 4 || !['kanzei', 'reader'].includes(saved.activeId) || !Number.isSafeInteger(saved.interactionSequence)) return false;
  if (typeof saved.automatic !== 'boolean' || typeof saved.overview !== 'boolean' || typeof saved.floating !== 'boolean') return false;
  if (saved.floatingPoint && ![saved.floatingPoint.x, saved.floatingPoint.y].every(Number.isFinite)) return false;
  if (saved.inboxReturn && (!['kanzei', 'reader'].includes(saved.inboxReturn.activeId) || !owner(saved.inboxReturn.selected) || !panel(saved.inboxReturn.panel) || !proof(saved.inboxReturn.evidence))) return false;
  if (!Array.isArray(saved.interactions) || !saved.interactions.length || !saved.interactions.every(i => texts(i, ['id', 'kind', 'title', 'body', 'impact']) && /^(decision|question|delivery)-\d+$/.test(i.id) && ['kanzei', 'reader'].includes(i.projectId) && owner(i.node) && ['open', 'replied', 'resolved', 'failed'].includes(i.status) && Number.isSafeInteger(i.revision) && Array.isArray(i.choices) && i.choices.every(c => typeof c === 'string'))) return false;
  return ['kanzei', 'reader'].every(id => {
    const p = saved.projects?.[id];
    if (!p || !owner(p.selected) || !owner(p.reference) || !snap(p) || !panel(p.panel) || !proof(p.evidence)) return false;
    if (!['workbench', 'module', 'inbox'].includes(p.routeKind) || !['flow', 'conversation', 'replay'].includes(p.view) || !['manage', 'chat'].includes(p.moduleTab)) return false;
    if (!texts(p, ['name', 'work', 'memoryName', 'notice']) || !Array.isArray(p.panels) || !p.panels.every(v => v && panel(v.panel))) return false;
    if (p.routeKind === 'inbox' && !saved.interactions.some(i => i.id === p.interactionId && i.projectId === id)) return false;
    if (!p.batch || !['implementing', 'verifying', 'delivered'].includes(p.batch.phase) || !Number.isSafeInteger(p.batch.edits)) return false;
    if (!Array.isArray(p.drafts) || !p.drafts.every(d => Array.isArray(d) && d.length === 2 && typeof d[0] === 'string' && texts(d[1], ['text']) && ['work', 'project'].includes(d[1].scope))) return false;
    if (!Array.isArray(p.snapshots) || !p.snapshots.length || !p.snapshots.every(snap)) return false;
    if (!Array.isArray(p.messages) || !p.messages.every(m => texts(m, ['text', 'threadKey', 'recipientId', 'recipientName', 'reference']) && m.projectId === id && owner(m.node) && ['queued', 'applied', 'failed'].includes(m.status))) return false;
    if (!Array.isArray(p.pending) || !p.pending.every(v => v && typeof v.label === 'string' && (v.kind === 'memory' ? [null, 'v4'].includes(v.value) : v.kind === 'settings' ? settings(v.value) : v.kind === 'archive' ? v.value === true : v.kind === 'batch' && v.value === 'closing'))) return false;
    return true;
  });
}
function applyExperience(saved) {
  if (!validExperience(saved)) return false;
  for (const timer of deliveryTimers.values()) clearTimeout(timer);
  deliveryTimers.clear();
  for (const id of ['kanzei', 'reader']) projects[id] = { ...saved.projects[id], drafts: new Map(saved.projects[id].drafts) };
  interactions.splice(0, interactions.length, ...saved.interactions);
  activeId = saved.activeId; overview = saved.overview; automatic = saved.automatic && !manualExperience;
  interactionSequence = saved.interactionSequence; inboxReturn = saved.inboxReturn;
  floatingPoint = saved.floatingPoint; floating = saved.floating;
  $('overview').hidden = !overview; $('project-workspace').hidden = overview; $('simulate').hidden = overview;
  $('message-toast').hidden = true; toastId = null;
  return true;
}
function settleExperience(scroll = 0) {
  restoreDraft(); renderAll(); setFloating(floating); renderExperience();
  if (project().panel === 'settings') { const p = project(); const values = p.settingsDraft || p.settings; $('model-select').value = values.model; $('tools-select').value = values.tools; $('subagents-toggle').checked = values.subagents; }
  $('work-surface').scrollTop = scroll;
  for (const id of Object.keys(projects)) scheduleDelivery(id);
}

document.addEventListener('click', (event) => {
  const evidence = event.target.closest('[data-evidence]');
  if (evidence) { openEvidence(evidence.dataset.evidence); return; }
  const projectButton = event.target.closest('[data-project]');
  if (projectButton) { switchProject(projectButton.dataset.project); return; }
  const select = event.target.closest('[data-select]');
  if (select) { selectNode(select.dataset.select); return; }
  const view = event.target.closest('[data-view]');
  if (view) { returnNetwork(view.dataset.view); return; }
  const detail = event.target.closest('[data-detail-node]');
  if (detail) { selectNode(detail.dataset.detailNode, true); return; }
  if (event.target.closest('[data-panel-close]')) { closePanel(); return; }
  const moduleTab = event.target.closest('[data-module-tab]');
  if (moduleTab) { project().moduleTab = moduleTab.dataset.moduleTab; renderModuleTab(); return; }
  const interactionButton = event.target.closest('[data-interaction]');
  if (interactionButton) { openInbox(interactionButton.dataset.interaction); return; }
  const choice = event.target.closest('[data-choice]');
  if (choice) {
    const text = currentInteraction().choices[Number(choice.dataset.choice)];
    $('instruction').value += `${$('instruction').value ? '\n' : ''}${text}`;
    choice.setAttribute('aria-pressed', 'true'); saveDraft(); $('instruction').focus(); return;
  }
  const control = event.target.closest('[data-action]');
  if (control) action(control.dataset.action);
});
$('instruction').addEventListener('input', saveDraft);
$('instruction').addEventListener('compositionstart', () => { composing = true; });
$('instruction').addEventListener('compositionend', () => { composing = false; saveDraft(); });
$('instruction').addEventListener('keydown', (event) => {
  if (event.key === 'Enter' && (event.ctrlKey || event.metaKey) && !event.isComposing && !composing) { event.preventDefault(); $('composer-form').requestSubmit(); }
});
$('scope').addEventListener('change', saveDraft);
$('composer-form').addEventListener('submit', (event) => {
  event.preventDefault(); sendMessage();
});
$('simulate').addEventListener('click', () => simulate());
$('simulate-message').addEventListener('click', simulateArrival);
$('inbox-button').addEventListener('click', () => openInbox());
$('evidence-back').addEventListener('click', closeEvidence);
$('close-inbox').addEventListener('click', closeInbox);
$('reply-return').addEventListener('click', closeInbox);
$('back-network').addEventListener('click', () => returnNetwork());
$('toast-reply').addEventListener('click', () => openInbox(toastId));
$('toast-dismiss').addEventListener('click', () => { $('message-toast').hidden = true; });
$('context-button').addEventListener('click', () => openPanel('context'));
$('settings-button').addEventListener('click', () => openPanel('settings'));
$('alert-button').addEventListener('click', () => { selectNode('memory', true); });
$('return-live').addEventListener('click', () => { returnNetwork(); $('instruction').focus(); });
$('overview-button').addEventListener('click', () => {
  saveDraft(); overview = true;
  $('overview').hidden = false; $('project-workspace').hidden = true; $('simulate').hidden = true; renderChrome();
  schedulePersist();
});
$('breadcrumb-projects').addEventListener('click', () => $('overview-button').click());
$('experience-toggle').addEventListener('click', () => {
  automatic = !automatic;
  for (const timer of deliveryTimers.values()) clearTimeout(timer);
  deliveryTimers.clear();
  renderExperience(); schedulePersist();
  if (automatic) for (const id of Object.keys(projects)) scheduleDelivery(id);
});
$('experience-menu-toggle').addEventListener('click', () => {
  const open = $('experience-menu').hidden;
  $('experience-menu').hidden = !open; $('experience-menu-toggle').setAttribute('aria-expanded', String(open));
});
$('restart-experience').addEventListener('click', () => {
  saveDraft(); previousExperience = JSON.stringify(experienceSnapshot());
  applyExperience(JSON.parse(initialExperience)); settleExperience(); persistNow();
  $('experience-menu').hidden = false;
});
$('undo-restart').addEventListener('click', () => {
  if (!previousExperience) return;
  const saved = JSON.parse(previousExperience);
  if (applyExperience(saved)) { previousExperience = null; settleExperience(saved.scroll); persistNow(); }
  $('experience-menu').hidden = true; $('experience-menu-toggle').setAttribute('aria-expanded', 'false');
});
document.addEventListener('click', event => {
  if (!event.target.closest('#experience-menu, #experience-menu-toggle')) { $('experience-menu').hidden = true; $('experience-menu-toggle').setAttribute('aria-expanded', 'false'); }
});
$('replay-step').addEventListener('change', () => { project().replayStep = Number($('replay-step').value); renderReplay(); renderFocus(); renderContext(); });
$('settings-form').addEventListener('input', () => { project().settingsDraft = { model: $('model-select').value, tools: $('tools-select').value, subagents: $('subagents-toggle').checked }; });
$('settings-form').addEventListener('submit', (event) => {
  event.preventDefault(); if (project().view === 'replay') { setNotice('请先返回当前，再调整执行设置。'); return; }
  const value = { model: $('model-select').value, tools: $('tools-select').value, subagents: $('subagents-toggle').checked };
  enqueue('settings', value, '当前工作的执行设置'); project().settingsDraft = null; closePanel();
});
$('float-toggle').addEventListener('click', () => setFloating(!floating));
$('drag-handle').addEventListener('pointerdown', (event) => {
  if (event.button !== 0 || window.innerWidth <= 620) return;
  const rect = $('operation-dock').getBoundingClientRect();
  dragging = { pointerId: event.pointerId, startX: event.clientX, startY: event.clientY, offsetX: event.clientX - rect.left, offsetY: event.clientY - rect.top, moved: false, origin: null };
  $('drag-handle').setPointerCapture(event.pointerId);
});
$('drag-handle').addEventListener('pointermove', (event) => {
  if (!dragging || dragging.pointerId !== event.pointerId) return;
  if (!dragging.moved && Math.hypot(event.clientX - dragging.startX, event.clientY - dragging.startY) < 5) return;
  if (!dragging.moved) {
    if (floating) dragging.origin = { ...floatingPoint };
    else {
      setFloating(true);
      const width = $('operation-dock').getBoundingClientRect().width;
      dragging.origin = { x: dragging.startX - Math.min(dragging.offsetX, width - 40), y: dragging.startY - dragging.offsetY };
    }
    dragging.moved = true;
  }
  floatingPoint = { x: dragging.origin.x + event.clientX - dragging.startX, y: dragging.origin.y + event.clientY - dragging.startY };
  clampFloating();
});
for (const event of ['pointerup', 'pointercancel', 'lostpointercapture']) $('drag-handle').addEventListener(event, () => { dragging = null; });
$('drag-handle').addEventListener('keydown', (event) => {
  if (!floating || !['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown'].includes(event.key)) return;
  event.preventDefault(); const distance = event.shiftKey ? 40 : 10;
  if (event.key === 'ArrowLeft') floatingPoint.x -= distance;
  if (event.key === 'ArrowRight') floatingPoint.x += distance;
  if (event.key === 'ArrowUp') floatingPoint.y -= distance;
  if (event.key === 'ArrowDown') floatingPoint.y += distance;
  clampFloating();
});
document.addEventListener('keydown', (event) => { if (event.key === 'Escape' && !event.isComposing) { if (!$('experience-menu').hidden) { $('experience-menu').hidden = true; $('experience-menu-toggle').setAttribute('aria-expanded', 'false'); $('experience-menu-toggle').focus(); } else if (project().panel === 'inbox') closeInbox(); else if (project().panel === 'detail') returnNetwork(); else if (project().panel) closePanel(); } });
document.addEventListener('visibilitychange', () => { document.body.dataset.visibility = document.hidden ? 'hidden' : 'visible'; });
window.addEventListener('resize', () => { if (window.innerWidth <= 620) setFloating(false); else clampFloating(); });
new ResizeObserver(drawWires).observe($('flow-map'));
new ResizeObserver(() => { if (floating) requestAnimationFrame(clampFloating); }).observe($('operation-dock'));
for (const event of ['pointerover', 'focusin']) $('flow-map').addEventListener(event, e => { const node = e.target.closest('.flow-node'); if (node) highlightNode(node.dataset.select); });
$('flow-map').addEventListener('pointerleave', () => highlightNode(null));
$('flow-map').addEventListener('focusout', e => { if (!$('flow-map').contains(e.relatedTarget)) highlightNode(null); });
const initialExperience = JSON.stringify(experienceSnapshot());
let restoredScroll = 0;
if (!manualExperience) {
  try { const stored = localStorage.getItem(STORAGE_KEY); if (stored) { const saved = JSON.parse(stored); if (applyExperience(saved)) restoredScroll = Number(saved.scroll) || 0; } }
  catch { /* A missing or damaged trial record falls back to the initial scene. */ }
}
hydrateIcons();
settleExperience(restoredScroll);
storageReady = true;
window.addEventListener('pagehide', () => { saveDraft(); persistNow(); });
document.addEventListener('visibilitychange', () => { if (document.hidden) { saveDraft(); persistNow(); } });
document.addEventListener('click', schedulePersist);
$('work-surface').addEventListener('scroll', schedulePersist, { passive: true });
document.body.dataset.visibility = document.hidden ? 'hidden' : 'visible';
