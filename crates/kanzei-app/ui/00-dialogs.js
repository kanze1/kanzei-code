import { confirmDialog as surfaceConfirmDialog, inputDialog as surfaceInputDialog, setSurfaceTranslator } from "./00-surface.js";
import { t } from "./02-i18n.js";
// D-418:统一确认弹窗(替代浏览器原生 window.confirm)。
// options: { title, message, list?: string[], okText?, safeText?, danger?: boolean }
// 返回 Promise<boolean|string>;确认 resolve(true),safeText 按钮 resolve("safe"),
// 取消/Esc/点外 resolve(false)。可承载清单与风险分级(R-245 删除弹窗规范)。
// 实现归 00-surface.js(<dialog> 模态:原生惰性化背景、Esc 只关栈顶、并发调用排队不互相覆盖);
// 这里保留导出与冒烟接缝 setConfirmDialog,全部调用点零改动。
export let confirmDialog = (options) => surfaceConfirmDialog(options);
export function setConfirmDialog(value) { confirmDialog = value; }
// D-420:WebView2 不提供 window.prompt,统一使用应用内输入弹窗(实现同样归 00-surface.js)。
// options: { title, message?, value?, placeholder?, okText? }
// 返回 Promise<string|null>;确认返回输入值,取消/Esc/点外返回 null。
export let inputDialog = (options) => surfaceInputDialog(options);
export function setInputDialog(value) { inputDialog = value; }
// 弹层模块零 import,翻译函数从这里注入(包一层:02-i18n 在循环依赖里可能尚未求值完)。
setSurfaceTranslator((key) => t(key));
