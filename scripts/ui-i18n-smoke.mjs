import assert from "node:assert/strict";
import { loadUiSources } from "./ui-sources.mjs";

const { html: htmlSource, joined: source } = loadUiSources();
const dictionaryBody = source.match(/const I18N_EN = \{([\s\S]*?)\n\};/)?.[1] ?? "";
const dynamicDictionaryBody = source.match(/const I18N_DYNAMIC_EN = \{([\s\S]*?)\n\};/)?.[1] ?? "";
const dictionaryKeys = new Set(
  [...`${dictionaryBody}\n${dynamicDictionaryBody}`.matchAll(/"((?:\\.|[^"])*)"\s*:/g)].map(([, key]) => key)
);
const dynamicKeys = [...source.matchAll(/\bt\("([^"]+)"\)/g)].map(([, key]) => key);
const missingKeys = [...new Set(dynamicKeys)].filter((key) => !dictionaryKeys.has(key));
assert.deepEqual(missingKeys, [], `动态 i18n key 未进入资源表: ${missingKeys.join(", ")}`);
// D-509：静态资源表之外，JS 运行时入口也必须经过翻译层。保留 liveIdle 的源文案
// 是为了让它在语言切换时由 localizeDynamic 重新计算；其余直接把中文送入用户可见
// toast/status/log 的调用点一律报错，避免新增调用绕过冒烟。
const directJsI18nLiterals = [];
for (const [lineNumber, rawLine] of source.split(/\r?\n/).entries()) {
  const line = rawLine.replace(/\/\/.*$/, "");
  const match = line.match(/\b(setStatus|setRunning|toast|toastError|log|liveTurn|liveIdle|notifyRunState)\(\s*(["'`])([^"'`]*[\u3400-\u9fff][^"'`]*)/);
  if (!match || /\bt\(/.test(match[0]) || /localizeDynamic/.test(match[0]) || match[1] === "liveIdle") continue;
  if ((match[1] === "setStatus" || match[1] === "setRunning") && dictionaryKeys.has(match[3].trim())) continue;
  directJsI18nLiterals.push(`${lineNumber + 1}:${match[1]}(${match[3]}`);
}
assert.deepEqual(directJsI18nLiterals, [], `JS 用户可见中文未经过 t()/localizeDynamic(): ${directJsI18nLiterals.join(" | ")}`);

// 状态机/启动步骤会先保存中文 source key，再在消费点调用 t；这些动态 key 也必须入表。
const deferredSourceKeys = [...source.matchAll(/\b(?:abortAutoContinue|runStep)\([^\n]*?["'`]([^"'`]*[\u3400-\u9fff][^"'`]*)["'`]/g)]
  .map(([, key]) => key)
  .filter((key) => !dictionaryKeys.has(key));
assert.deepEqual([...new Set(deferredSourceKeys)], [], `延迟翻译 source key 未进入资源表: ${[...new Set(deferredSourceKeys)].join(", ")}`);

const han = /[\u3400-\u9fff]/;
const htmlCandidates = [];
for (const match of htmlSource.matchAll(/\b(?:title|placeholder|aria-label)="([^"]*[\u3400-\u9fff][^"]*)"/g)) {
  htmlCandidates.push(match[1].replace(/\s+/g, " ").trim());
}
for (const match of htmlSource.matchAll(/>([^<>]*[\u3400-\u9fff][^<>]*)</g)) {
  const value = match[1].replace(/\s+/g, " ").trim();
  if (value) htmlCandidates.push(value);
}
const htmlAllowlist = new Set(["简体中文", "English"]);
const missingHtmlKeys = [...new Set(htmlCandidates)]
  .filter((key) => han.test(key) && !htmlAllowlist.has(key) && !dictionaryKeys.has(key));
assert.deepEqual(missingHtmlKeys, [], `HTML 静态文案未进入资源表: ${missingHtmlKeys.join(" | ")}`);
// R-140 批10:静态 DOM 必须走 data-i18n-* 一次性应用(验收②/④ 的机械保证)——每个含中文
// 文本/属性的元素都必须带 data-i18n-key/title/aria-label/placeholder 之一,否则英文态
// 永远不翻译。动态状态元素(live-turn/live-action/live-note/live-focus/status-mode/status-text)由 JS 渲染点负责,白名单放行;
// 其余都是漏网,必须在 HTML 里补 data-i18n-*。
const staticDynamicId = new Set(["live-turn", "live-action", "live-note", "live-focus", "status-mode", "status-text"]);
const uncoveredStatic = [];
for (const match of htmlSource.matchAll(/<(\w+)((?:[^<>"]|"[^"]*")*?)(?<![-\w])id="([\w-]+)"((?:[^<>"]|"[^"]*")*?)>/g)) {
  const [, , before, id, after] = match;
  const attributes = `${before} ${after}`;
  const tail = htmlSource.slice(match.index + match[0].length);
  const directText = tail.match(/^([^<]*)</)?.[1].replace(/\s+/g, " ").trim() ?? "";
  const hasHan = han.test(directText) || han.test(attributes) && /(?:title|placeholder|aria-label)="/.test(attributes);
  if (staticDynamicId.has(id)) continue;
  if (!hasHan) continue;
  if (/\bdata-i18n-(?:key|title|aria-label|placeholder)="/.test(attributes)) continue;
  uncoveredStatic.push(id);
}
assert.deepEqual(uncoveredStatic, [], `静态中文元素未挂 data-i18n-*: ${uncoveredStatic.join(", ")}`);
const untranslatedValues = [...`${dictionaryBody}\n${dynamicDictionaryBody}`.matchAll(/"((?:\\.|[^"])*)"\s*:\s*"((?:\\.|[^"])*)"/g)]
  .filter(([, key, value]) => han.test(value) || (han.test(key) && key === value))
  .map(([, key]) => key);
assert.deepEqual(untranslatedValues, [], `英文资源仍含中文或等于源文案: ${untranslatedValues.join(" | ")}`);
assert.match(htmlSource, /class="activity-item[^>]*data-view="settings"/, "缺少设置页真实入口");
assert.match(htmlSource, /id="view-settings"[\s\S]*id="about-kanzei"[\s\S]*<h2[^>]*>关于 kanzei<\/h2>[\s\S]*<p[^>]*>[\s\S]*<p[^>]*>/, "关于页面缺少设置页内的真实标题与两段内容");
for (const id of ["ask-queue-preview", "ask-action", "ask-resource", "ask-remember", "ask-question", "ask-options"]) {
  assert.match(htmlSource, new RegExp(`id="${id}"[^>]*data-i18n-raw`), `${id} 未保护权限/用户原始数据`);
}
const required = [
  ["关于 kanzei", "关于页面英文标题"],
  ["kanzei 是文件优先的日常开发工具", "关于页面英文正文"],
  ["const I18N_EN", "英文资源"],
  ["function t(key)", "动态翻译入口"],
  ["I18N_DYNAMIC_EN[key]", "动态资源可由 t 直接消费"],
  ["function applyLanguage()", "静态节点翻译入口"],
  ["function applyDataI18nKeys(root, language)", "data-i18n-* 静态+动态渲染点翻译入口"],
  ["I18N_EN[key] || I18N_DYNAMIC_EN[key] || key", "漏译回落中文原文(t 与 applyDataI18nKeys 共用)"],
  ["I18N_LOCALIZE_ENTRIES", "静态与动态资源统一处理复合文案"],
  ["for (const el of root.querySelectorAll(\"[data-i18n-aria-label]\"))", "可访问属性渲染点翻译入口"],
  ["运行中\": \"Running", "运行中翻译键"],
  ["运行完成\": \"Run completed", "运行完成翻译键"],
  ["运行失败\": \"Run failed", "运行失败翻译键"],
  ["运行已停止\": \"Run stopped", "运行已停止翻译键"],
  ["工具执行中\": \"Tool running", "工具状态翻译键"],
  ["成功\": \"Succeeded", "工具成功翻译键"],
  ["失败\": \"Failed", "工具失败翻译键"],
  ["需要你的回答\": \"Your answer is needed", "问题弹窗翻译键"],
  ["权限请求\": \"Permission request", "权限弹窗翻译键"],
  ["function updateAskQueueStatus()", "权限队列动态入口"],
  ["当前对话\": \"Current chat", "工作区对话翻译键"],
  ["最近活动\": \"Recent activity", "工作区活动翻译键"],
  ["function renderWorkspace(snapshot)", "工作区动态入口"],
  ["function localizedDocStatus(status)", "文档状态翻译入口"],
  ["function renderDocList(", "文档动态入口"],
  ["function renderPermissionRules(data)", "权限设置动态入口"],
  ["测试中\": \"Testing", "设置测试状态翻译键"],
  ["连通性检查完成\": \"Connectivity check complete", "设置测试结果翻译键"],
  ["function renderProviders()", "Provider 动态入口"],
  ["if (document.querySelector(\"#providers-table tbody\")?.children.length) renderProviders()", "语言切换刷新 Provider"],
  ["let lastWorkspaceSnapshot = null", "工作区语言刷新缓存"],
  ["if (lastWorkspaceSnapshot) renderWorkspace(lastWorkspaceSnapshot)", "语言切换刷新工作区"],
  ["if (document.body.classList.contains(\"documents-active\")) refreshDocs()", "语言切换刷新文档"],
  ["setStatus(statusText ?? (value ? t(\"运行中\") : t(\"空闲\")), value)", "运行状态翻译入口"],
  ["点击查看上下文成分\": \"Click to view context details", "上下文属性翻译键"],
  ["连接中断\": \"Connection interrupted", "重连状态翻译键"],
  ["总结中\": \"Summarizing", "总结状态翻译键"],
  ["当前没有可总结的对话\": \"No conversation to summarize", "总结空状态翻译键"],
  ["自主推进\": \"Self-directed progress", "自动推进翻译键"],
  ["等待下一轮\": \"Waiting for next round", "自动推进等待翻译键"],
  ["已停止\": \"Stopped", "停止状态翻译键"],
  ["完成\": \"Completed", "完成状态翻译键"],
  ["function notifyRunState(kind, text)", "运行通知动态入口"],
  ["没有可复制的内容\": \"Nothing to copy", "复制空状态翻译键"],
  ["当前没有可复制的对话\": \"No conversation to copy", "对话复制空状态翻译键"],
  ["当前任务还在运行，自动鞭挞将在本轮完成后继续\": \"The current task is still running; auto-run will continue after this round", "运行中自动推进提示翻译键"],
  ["先在左侧「项目」里添加并选择一个目录\": \"Add and select a directory under Projects first", "项目缺失提示翻译键"],
  ["已撤销排队输入\": \"Queued input cancelled", "队列撤销翻译键"],
  ["暂无测试记录\": \"No test runs", "测试记录空状态翻译键"],
  ["环境变量名(可选)\": \"Environment variable name (optional)", "Provider 表单翻译键"],
  ["订阅登录态\": \"Subscription login", "Provider 登录态翻译键"],
  ["思考中\": \"Thinking", "思考状态翻译键"],
  // status-mode 由布尔值每次重算,用 t() 是正确的(天然可逆,无需存源);
  // 需要存源的是 setStatus/liveIdle 这类保存文案本身的路径——那里禁止先 t() 再存,
  // 否则英文会被烤进 source,切回中文就冻住(D-136 同族)。
  ["status-mode\").textContent = statusRunning ? t(\"运行中\")", "动态状态使用翻译入口"],
  ["statusTextSource = String(text ?? \"\")", "状态文案存源"],
  ["localizeDynamic(statusTextSource)", "状态文案渲染时本地化"],
  // 侧栏焦点卡片:切语言要跟着重渲(renderDocsSnapshot 末尾调 applyLanguage),
  // 「凭什么指这一条」的依据字段也不能被后人悄悄删掉。
  ["function renderFocusPanel(", "侧栏焦点面板动态入口"],
  ["focus.activeSource", "焦点依据字段"],
];
const missing = required.filter(([needle]) => !source.includes(needle));
if (missing.length) {
  throw new Error(`UI i18n 静态契约缺失: ${missing.map(([, label]) => label).join(", ")}`);
}
console.log(
  `UI i18n 静态冒烟通过：${dictionaryKeys.size} 个资源 key、${new Set(htmlCandidates).size} 项 HTML 文案、${required.length} 项动态契约已覆盖`
);
