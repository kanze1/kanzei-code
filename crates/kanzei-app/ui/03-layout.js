// UI2-0926 #14/#4 界面布局偏好:可调框几何、分隔条宽高、后台任务侧栏的两个开关。
//
// 真源是 ~/.kanzei/app.json 的 ui_layout(经 ui_prefs_get/ui_prefs_set):本机 WebView2 的 localStorage
// 重启即丢(D-404),只拿它当首屏缓存(读写都包 try/catch)。启动时先按缓存同步落地,后端值到达后
// 覆盖并重放一次;写入按「分区 → 键」合并(后端 merge_ui_layout 同一口径),400ms 去抖,页面隐藏时立即写。
// 形如 { side_panel: { auto_open, auto_close }, frames: { <框 id>: {v,l|r,t|b,w?,h?,…} }, splits: { <id>: px } }。
//
// 几何的读写经 00-frame.js 的可注入存储(setFrameStore):00-frame 零 import,不直接碰 IPC。
import { bindFrames, installSplit, refreshFrames, setFrameStore } from "./00-frame.js";
import { $, defer, uiPrefsLoad, uiPrefsSave } from "./01-core.js";
import { t } from "./02-i18n.js";

const CACHE_KEY = "kz-ui-layout";
const FLUSH_MS = 400;
const listeners = new Set();
let layout = readCache();
let pending = null;
let flushTimer = null;

function isObject(value) {
  return Boolean(value) && typeof value === "object" && !Array.isArray(value);
}
function readCache() {
  try {
    const value = JSON.parse(localStorage.getItem(CACHE_KEY) || "null");
    return isObject(value) ? value : {};
  } catch {
    return {};
  }
}
function writeCache() {
  try {
    localStorage.setItem(CACHE_KEY, JSON.stringify(layout));
  } catch {
    /* 隐私模式、配额:缓存丢了只是首屏晚一拍,后端才是真源 */
  }
}
function notify(section) {
  for (const fn of listeners) {
    try { fn(section); } catch (error) { console.warn(error); }
  }
}

/// 读一个布局偏好;没有就是 null。
export function layoutPref(section, key) {
  const bucket = layout[section];
  return isObject(bucket) && Object.prototype.hasOwnProperty.call(bucket, key) ? bucket[key] : null;
}
/// 写一个布局偏好(null = 删除),立即进缓存,去抖后合并写后端。
export function setLayoutPref(section, key, value) {
  const next = value === undefined ? null : value;
  if (!isObject(layout[section])) layout[section] = {};
  if (next === null) delete layout[section][key];
  else layout[section][key] = next;
  writeCache();
  pending ??= {};
  if (!isObject(pending[section])) pending[section] = {};
  pending[section][key] = next;
  clearTimeout(flushTimer);
  flushTimer = setTimeout(flushLayout, FLUSH_MS);
}
export function flushLayout() {
  clearTimeout(flushTimer);
  flushTimer = null;
  if (!pending) return;
  const patch = pending;
  pending = null;
  void uiPrefsSave({ ui_layout: patch });
}
/// 偏好整体变化(后端值到达、设置页改开关)的订阅;回调参数是分区名或 "*"。
export function onLayoutChange(fn) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/// 后端值到达:以后端为准,启动后、到达前用户刚改过而还没写出去的键保留本地值。
export function adoptLayout(remote) {
  const next = {};
  if (isObject(remote)) {
    for (const [section, bucket] of Object.entries(remote)) next[section] = isObject(bucket) ? { ...bucket } : bucket;
  }
  for (const [section, bucket] of Object.entries(pending ?? {})) {
    if (!isObject(next[section])) next[section] = {};
    for (const [key, value] of Object.entries(bucket)) {
      if (value === null) delete next[section][key];
      else next[section][key] = value;
    }
  }
  layout = next;
  writeCache();
  refreshFrames();
  notify("*");
}

// ---------- 后台任务侧栏的两个开关(设置页「后台任务侧栏」组) ----------
export function sidePanelPrefs() {
  return {
    autoOpen: layoutPref("side_panel", "auto_open") !== false,
    autoClose: layoutPref("side_panel", "auto_close") !== false,
  };
}
export function setSidePanelPref(key, value) {
  setLayoutPref("side_panel", key, Boolean(value));
  notify("side_panel");
}

// 00-frame 的存储接到这里(模块求值时就换,早于任何 defer 里的 installFrame/installSplit)。
// 分隔条的旧 localStorage 键(侧栏 kz-sidebar-width)只在 ui_layout 里还没有值时读一次。
export const layoutFrameStore = {
  get(kind, id, hint) {
    const value = layoutPref(kind, id);
    if (value !== null) return value;
    if (kind === "splits" && hint) {
      try { return localStorage.getItem(hint); } catch { return null; }
    }
    return null;
  },
  set(kind, id, value, hint) {
    setLayoutPref(kind, id, value);
    // 旧键只读一次:写过新值(或复位)之后就删掉,免得复位后又被旧值顶回来。
    if (kind === "splits" && hint) {
      try { localStorage.removeItem(hint); } catch { /* 忽略 */ }
    }
  },
};
setFrameStore(layoutFrameStore);

defer(() => {
  bindFrames(document);
  // 布局分隔条:尺寸写成 <html> 上的 --kz-split-<id>,style.css 的默认值与引用点在同名变量上。
  // 文案按当前语言译好传入,词条键一并传入(切语言时经 data-i18n-* 重译)。
  installSplit($("sidebar"), {
    id: "sidebar", side: "right", min: 220, max: 460, key: "kz-sidebar-width",
    title: t("拖动调整面板宽度"), titleKey: "拖动调整面板宽度", ariaLabel: t("调整面板宽度"), ariaKey: "调整面板宽度",
  });
  installSplit($("log-panel"), {
    id: "log", side: "top", min: 80, max: () => Math.round(window.innerHeight * 0.6),
    title: t("拖动调整面板高度"), titleKey: "拖动调整面板高度", ariaLabel: t("调整面板高度"), ariaKey: "调整面板高度",
  });
  // UI2-0926 #6:文件树与编辑器之间。上限按文件页自身宽度算,给编辑器留至少 360px(侧栏开合、后台任务侧栏
  // 停靠都会改变文件页宽度);文件页隐藏时量不到宽度,退回窗口一半。
  installSplit($("files-side"), {
    id: "files", side: "right", min: 200,
    max: () => {
      const layoutWidth = $("files-layout")?.getBoundingClientRect?.().width || 0;
      return layoutWidth > 0 ? Math.max(200, Math.round(layoutWidth - 360)) : Math.round(window.innerWidth * 0.5);
    },
    title: t("拖动调整文件树宽度 · 双击复位"), titleKey: "拖动调整文件树宽度 · 双击复位",
    ariaLabel: t("调整文件树宽度"), ariaKey: "调整文件树宽度",
  });
  installSplit(document.querySelector("#view-memory .memory-list-pane"), {
    id: "memory", side: "right", min: 200, max: 520,
    title: t("拖动调整面板宽度"), titleKey: "拖动调整面板宽度", ariaLabel: t("调整面板宽度"), ariaKey: "调整面板宽度",
  });
  void uiPrefsLoad().then((prefs) => adoptLayout(prefs?.ui_layout));
  window.addEventListener("pagehide", flushLayout);
});
