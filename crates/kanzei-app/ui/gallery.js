// 弹层样例页脚本(UI-0926 #9,docs/design/ui_surface_stack.md §8)。
// 只从 00-surface.js 引入,不依赖应用其它模块与 Tauri。暴露 window.__gallery 供
// scripts/ui-surface-gallery-smoke.mjs 调用:demos() / open(id) / measure(id) / tokens() /
// stackDepth() / closeAll() / setTheme(theme) / picks()(菜单项 onSelect 调用记录)。
import {
  bindMenus,
  closeSurface,
  confirmDialog,
  hideCard,
  inputDialog,
  installTooltips,
  openDialog,
  openMenu,
  openPopover,
  showCard,
  stackDepth,
  toast,
} from "./00-surface.js";

// 样例页不接 i18n 运行时:文案沿用应用已有词条的中文源文(t 为恒等),i18n 冒烟据此校验词条仍在资源表里。
const t = (key) => key;
const $ = (id) => document.getElementById(id);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

let lastMenu = null;
// 菜单项 onSelect 的调用记录:冒烟据此确认「弹窗里的菜单点得到」(惰性节点上的点击不会走到 onSelect)。
const picks = [];

function openDialogMenu() {
  lastMenu = openMenu($("demo-dialog-menu-js"), [
    { label: t("复制"), onSelect: () => picks.push("复制") },
    { label: t("总结"), desc: t("复制上下文"), onSelect: () => picks.push("总结") },
  ], { label: t("动作") });
  return lastMenu;
}

function hoverTip(id) {
  const el = $(id);
  el.scrollIntoView({ block: "center" });
  el.dispatchEvent(new PointerEvent("pointerover", { bubbles: true }));
  return sleep(520);
}

// kind 决定冒烟对它的外观期望:modal=lg 圆角+shadow-3;tooltip=bg-raised+sm+shadow-1;
// card=md+shadow-3;chip=pill;其余 md+shadow-2。select 类只做下拉列表亮度检查。
const DEMOS = {
  confirm: {
    kind: "modal",
    target: () => $("confirm-overlay"),
    open: () => void confirmDialog({ title: t("确认删除"), message: t("此操作不可撤销") }),
  },
  "confirm-danger": {
    kind: "modal",
    target: () => $("confirm-overlay"),
    open: () => void confirmDialog({
      title: t("确认删除"),
      message: `${t("将删除勾选的")} 2 ${t("份历史对话快照")}`,
      list: [t("会话事件与投影"), t("此操作不可撤销")],
      okText: t("仅删除"),
      danger: true,
    }),
  },
  "confirm-safe": {
    kind: "modal",
    target: () => $("confirm-overlay"),
    open: () => void confirmDialog({ title: t("确认删除"), message: t("此操作不可撤销"), okText: t("仅删除"), safeText: t("删除并安全整理"), danger: true }),
  },
  input: {
    kind: "modal",
    target: () => $("input-overlay"),
    open: () => void inputDialog({ title: t("搜索"), value: "kanzei code", placeholder: t("搜索") }),
  },
  "dialog-lg": {
    kind: "modal",
    target: () => $("demo-dialog-lg"),
    open: () => openDialog($("demo-dialog-lg"), { initialFocus: "#demo-dialog-lg-close" }),
  },
  "dialog-palette": {
    kind: "modal",
    target: () => $("demo-dialog-palette"),
    open: () => openDialog($("demo-dialog-palette"), { initialFocus: "#palette-input" }),
  },
  "menu-static": {
    kind: "menu",
    anchor: "demo-menu-static-trigger",
    target: () => $("demo-menu-static"),
    open: () => openPopover(null, $("demo-menu-static"), { type: "menu" }),
  },
  "menu-js": {
    kind: "menu",
    anchor: "demo-menu-js-anchor",
    target: () => (lastMenu && !lastMenu.closed ? lastMenu.el : null),
    open: () => {
      lastMenu = openMenu($("demo-menu-js-anchor"), [
        { heading: t("动作") },
        { label: t("复制"), kbd: "Ctrl+C", onSelect() {} },
        { label: t("总结"), desc: t("复制上下文"), onSelect() {} },
        { label: t("自动放行"), checked: true, onSelect() {} },
        "separator",
        { label: t("新对话"), disabled: true, onSelect() {} },
        { label: t("删除"), danger: true, onSelect() {} },
      ], { label: t("动作") });
    },
  },
  // 弹窗里的 JS 菜单:openMenu 把它挂进锚点所在的 <dialog>(模态之外的节点是惰性的)。
  "dialog-menu": {
    kind: "menu",
    anchor: "demo-dialog-menu-js",
    target: () => (lastMenu && !lastMenu.closed ? lastMenu.el : null),
    open: () => {
      openDialog($("demo-dialog-menu"), { initialFocus: "#demo-dialog-menu-js" });
      openDialogMenu();
    },
  },
  // 文件补全:锚在宽输入框上,列表与锚点同宽(style.css 的 .file-suggestions 用 anchor-size(width))。
  completion: {
    kind: "popover",
    anchor: "demo-completion-anchor",
    target: () => $("demo-completion"),
    open: () => openPopover($("demo-completion-anchor"), $("demo-completion"), { manual: true, placement: "top-start" }),
  },
  "menu-container": {
    kind: "menu",
    anchor: "demo-menu-container-trigger",
    target: () => $("demo-menu-container"),
    open: () => openPopover(null, $("demo-menu-container"), { type: "menu" }),
  },
  "menu-select": {
    kind: "menu",
    anchor: "demo-menu-select-trigger",
    target: () => $("demo-menu-select"),
    open: () => openPopover(null, $("demo-menu-select"), { type: "menu" }),
  },
  popover: {
    kind: "popover",
    anchor: "demo-popover-anchor",
    target: () => $("demo-popover"),
    open: () => openPopover($("demo-popover-anchor"), $("demo-popover"), { placement: "bottom-start" }),
  },
  card: {
    kind: "card",
    target: () => $("demo-card"),
    open: () => showCard($("demo-card"), { onEscape: () => hideCard($("demo-card")), focus: "none" }),
  },
  chip: {
    kind: "chip",
    target: () => $("demo-chip"),
    open: () => showCard($("demo-chip"), { focus: "none" }),
  },
  toast: {
    kind: "toast",
    target: () => [...$("toast").children].find((node) => node.dataset.kzExpired !== "1") ?? null,
    open: () => {
      toast(t("已复制"));
      toast(t("成功"), { kind: "ok" });
      toast(t("运行已停止"), { kind: "warn" });
      toast(t("失败"), { kind: "err" });
    },
  },
  tooltip: { kind: "tooltip", anchor: "demo-tip-short", target: () => $("kz-tip"), open: () => hoverTip("demo-tip-short") },
  "tooltip-long": { kind: "tooltip", anchor: "demo-tip-long", target: () => $("kz-tip"), open: () => hoverTip("demo-tip-long") },
  panel: { kind: "panel", target: () => $("demo-panel"), open: () => $("demo-panel").scrollIntoView({ block: "center" }) },
  select: { kind: "select", target: () => $("demo-select"), open() {} },
  "select-chip": { kind: "select", target: () => $("demo-select-chip"), open() {} },
  "select-long": { kind: "select", target: () => $("demo-select-long"), open() {} },
};

function open(id) {
  const demo = DEMOS[id];
  if (!demo) throw new Error(`unknown demo ${id}`);
  if (demo.anchor) $(demo.anchor)?.scrollIntoView({ block: "center" });
  return demo.open();
}

function box(el) {
  const r = el.getBoundingClientRect();
  return { left: r.left, top: r.top, right: r.right, bottom: r.bottom, width: r.width, height: r.height };
}

function measure(id) {
  const demo = DEMOS[id];
  const el = demo?.target?.();
  if (!el) return null;
  const s = getComputedStyle(el);
  let open = demo.kind === "panel" || demo.kind === "select" || demo.kind === "toast";
  try { open ||= el.matches(":popover-open") || el.open === true; } catch { /* 不支持 :popover-open 的运行时 */ }
  return {
    id,
    kind: demo.kind,
    open,
    bg: s.backgroundColor,
    fg: s.color,
    border: s.borderTopColor,
    radius: s.borderTopLeftRadius,
    shadow: s.boxShadow,
    appearance: s.appearance,
    box: box(el),
    viewport: { width: innerWidth, height: innerHeight },
  };
}

// 探针元素解析 --surface-* 的实际值(随当前主题)。
function tokens() {
  const probe = document.createElement("div");
  probe.style.position = "absolute";
  probe.style.visibility = "hidden";
  probe.style.borderStyle = "solid";
  document.body.appendChild(probe);
  const read = (prop, value) => {
    probe.style.setProperty(prop, value);
    const resolved = getComputedStyle(probe).getPropertyValue(prop);
    probe.style.removeProperty(prop);
    return resolved;
  };
  const out = {
    bg: read("background-color", "var(--surface-bg)"),
    bgRaised: read("background-color", "var(--surface-bg-raised)"),
    fg: read("color", "var(--surface-fg)"),
    fgStrong: read("color", "var(--surface-fg-strong)"),
    muted: read("color", "var(--surface-muted)"),
    border: read("border-top-color", "var(--surface-border)"),
    attention: read("border-top-color", "var(--surface-attention)"),
    radiusSm: read("border-top-left-radius", "var(--surface-radius-sm)"),
    radiusMd: read("border-top-left-radius", "var(--surface-radius-md)"),
    radiusLg: read("border-top-left-radius", "var(--surface-radius-lg)"),
    radiusPill: read("border-top-left-radius", "var(--r-pill)"),
    shadow1: read("box-shadow", "var(--surface-shadow-1)"),
    shadow2: read("box-shadow", "var(--surface-shadow-2)"),
    shadow3: read("box-shadow", "var(--surface-shadow-3)"),
  };
  probe.remove();
  return out;
}

function closeAll() {
  for (const el of document.querySelectorAll("dialog[open], [popover]")) {
    if (el.id === "kz-tip" || el.id === "toast") continue;
    closeSurface(el);
  }
  lastMenu = null;
}

function setTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme === "light" ? "light" : "dark");
}

// 静态矩阵:把每种弹层按普通块(.k-static)画出来,两套主题同屏对比。
function node(tag, className, text) {
  const el = document.createElement(tag);
  if (className) el.className = className;
  if (text) el.textContent = text;
  return el;
}
// 组件层 token 在 :root 上以 var() 引用语义层,计算值在 :root 就定死了;嵌套的 [data-theme="light"] 区块
// 只换了语义层,组件层仍是暗色的计算值。静态矩阵要同屏对比两套主题,就在亮色区块上按样式表原文
// 把组件层重声明一遍(只取值里带 var( 的——字面量值的是语义层,不能拿暗色值盖掉亮色值)。
function componentTokenDeclarations() {
  const out = [];
  const walk = (rules) => {
    for (const rule of rules) {
      if (rule.styleSheet) walk(rule.styleSheet.cssRules);
      else if (rule.style && rule.selectorText === ":root") {
        for (const name of rule.style) {
          const value = rule.style.getPropertyValue(name);
          if (name.startsWith("--surface-") && value.includes("var(")) out.push([name, value]);
        }
      } else if (rule.cssRules) walk(rule.cssRules);
    }
  };
  for (const sheet of document.styleSheets) walk(sheet.cssRules);
  return out;
}
function renderMatrix(section) {
  if (section.dataset.theme) {
    for (const [name, value] of componentTokenDeclarations()) section.style.setProperty(name, value);
  }
  const dialog = node("div", "k-surface k-dialog k-static");
  dialog.append(node("div", "confirm-title", t("确认删除")), node("div", "", t("此操作不可撤销")));
  const actions = node("div", "confirm-buttons");
  actions.append(node("button", "", t("取消")), node("button", "danger", t("删除")));
  dialog.append(actions);
  const menu = node("div", "k-surface k-menu k-static");
  menu.append(node("div", "k-menu-heading", t("动作")));
  for (const [label, attrs] of [[t("复制"), {}], [t("自动放行"), { "aria-checked": "true" }], [t("新对话"), { "aria-disabled": "true" }], [t("删除"), { "data-tone": "danger" }]]) {
    const item = node("button", "k-menu-item", label);
    for (const [name, value] of Object.entries(attrs)) item.setAttribute(name, value);
    menu.append(item);
  }
  menu.insertBefore(node("div", "k-menu-sep"), menu.lastChild);
  const popover = node("div", "k-surface k-popover k-static", `${t("上下文成分")}: 148,200 tokens`);
  const card = node("div", "k-surface k-card k-static");
  card.dataset.tone = "attention";
  card.append(node("div", "ask-title", t("权限请求")), node("div", "", "git push origin release/2026-09-26-ui"));
  const chip = node("span", "k-surface k-chip-float k-static", t("重新打开询问"));
  const toasts = node("div", "");
  for (const kind of ["info", "ok", "warn", "err"]) {
    const item = node("div", "k-surface k-toast k-static", kind);
    item.dataset.kind = kind;
    toasts.append(item);
  }
  const tip = node("div", "k-surface k-tooltip k-static", t("打开低频操作菜单"));
  const panel = node("div", "k-surface k-panel k-static g-panel", t("活动"));
  section.replaceChildren(dialog, menu, popover, card, chip, toasts, tip, panel);
}

async function openEach() {
  for (const id of Object.keys(DEMOS)) {
    closeAll();
    await open(id);
    await sleep(900);
  }
  closeAll();
}

function init() {
  const long = $("demo-select-long");
  for (let i = 1; i <= 30; i += 1) {
    const option = document.createElement("option");
    option.value = String(i);
    option.textContent = `${t("模型")} ${i}`;
    if (i === 7) option.disabled = true;
    long.appendChild(option);
  }
  $("viewer-body").textContent = Array.from({ length: 40 }, (_, i) => `${i + 1}. ${t("此操作不可撤销")}`).join("\n");
  $("demo-dialog-lg-close").addEventListener("click", () => closeSurface($("demo-dialog-lg")));
  $("demo-dialog-menu-js").addEventListener("click", () => void openDialogMenu());
  $("demo-dialog-menu-close").addEventListener("click", () => closeSurface($("demo-dialog-menu")));
  $("demo-card-deny").addEventListener("click", () => hideCard($("demo-card")));
  $("demo-card-allow").addEventListener("click", () => hideCard($("demo-card")));
  $("demo-chip").addEventListener("click", () => hideCard($("demo-chip")));
  for (const button of document.querySelectorAll("[data-demo]")) {
    button.addEventListener("click", () => {
      closeAll();
      void open(button.dataset.demo);
    });
  }
  for (const button of document.querySelectorAll("[data-theme-set]")) {
    button.addEventListener("click", () => setTheme(button.dataset.themeSet));
  }
  $("demo-open-each").addEventListener("click", () => void openEach());
  $("demo-close-all").addEventListener("click", closeAll);
  for (const section of document.querySelectorAll("[data-matrix]")) renderMatrix(section);
  bindMenus(document);
  installTooltips(document);
  window.__gallery = {
    demos: () => Object.entries(DEMOS).map(([id, demo]) => ({ id, kind: demo.kind })),
    open,
    measure,
    tokens,
    stackDepth,
    closeAll,
    setTheme,
    picks: () => [...picks],
    ready: true,
  };
}

init();
