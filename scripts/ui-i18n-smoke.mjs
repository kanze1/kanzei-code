import { readFileSync } from "node:fs";

const source = readFileSync("crates/kanzei-app/ui/main.js", "utf8");
const required = [
  ["I18N_EN", "英文资源"],
  ["function t(key)", "动态翻译入口"],
  ["function applyLanguage()", "静态节点翻译入口"],
  ["const I18N_ATTR_ZH = new WeakMap()", "属性原文缓存"],
  ["originals.set(attribute, value)", "属性原文稳定保存"],
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
  ["环境变量名(可选)\": \"Environment variable name (optional)", "Provider 表单翻译键"],
  ["订阅登录态\": \"Subscription login", "Provider 登录态翻译键"],
  ["思考中\": \"Thinking", "思考状态翻译键"],
  ["status-mode\").textContent = isRunning ? t(\"运行中\")", "动态状态使用翻译入口"],
];
const missing = required.filter(([needle]) => !source.includes(needle));
if (missing.length) {
  throw new Error(`UI i18n 静态契约缺失: ${missing.map(([, label]) => label).join(", ")}`);
}
console.log(`UI i18n 静态冒烟通过：${required.length} 项资源与动态入口契约已覆盖`);
