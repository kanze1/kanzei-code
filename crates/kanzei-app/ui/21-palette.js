import { defer } from "./01-core.js";
import { $, on, promptBox } from "./01-core.js";
import { localizeDynamic, t } from "./02-i18n.js";
import { log } from "./03-shell.js";
import { state } from "./08-compose.js";

// ---------- 命令面板(Ctrl/Cmd+P) ----------
// 这个应用有 10 个主视图、N 个项目、N 条并行线,外加一批散在各处的动作按钮。
// 想切到「运行画像」得先认出 rail 上第 9 个图标;想切到 p14 得先在侧栏滚到它。
// 命令面板把这三类目标合并成一次键盘检索。
//
// 关键设计:**候选项不持有自己的行为**,一律 `.click()` 既有控件。视图切换、项目
// 选择、线路切换、新对话、停止……每一条都已经有唯一实现和自己的守卫(比如运行中
// 不许开新对话、运行中不许切历史)。面板复制一份行为 = 复制一份要同步的守卫,
// 迟早漂移;点它本人则永远和界面上的按钮同一个语义。
export const PALETTE_LIMIT = 40;
export let paletteEntries = [];
export let paletteIndex = 0;
export let paletteRestoreFocus = null;

export function paletteEl() {
  return $("palette");
}
export function paletteIsOpen() {
  return !paletteEl()?.classList.contains("hidden");
}

/// 采集候选项。每次打开时重新采集——项目/线路是活的,缓存只会给出过期清单。
export function collectPaletteEntries() {
  const entries = [];
  const push = (group, label, detail, run) => {
    if (!label) return;
    entries.push({ group, label, detail: detail || "", run });
  };

  for (const button of document.querySelectorAll("#activitybar .activity-item[data-view]")) {
    if (button.classList.contains("hidden")) continue;
    const label = localizeDynamic(button.dataset.i18nTitle || button.title || button.dataset.view);
    push(t("视图"), label, "", () => button.click());
  }

  for (const item of document.querySelectorAll("#project-list .project-item")) {
    const name = item.querySelector(".name")?.textContent?.trim();
    const path = item.querySelector(".path")?.textContent?.trim();
    push(t("项目"), name, path, () => item.click());
  }

  for (const row of document.querySelectorAll(".parallel-task-row")) {
    const head = row.querySelector(".parallel-task-head")?.textContent?.trim();
    const state = row.querySelector(".parallel-task-state")?.textContent?.trim();
    push(t("线路"), head, state, () => row.click());
  }

  // 动作:只收**当前可用**的(隐藏或禁用的不进清单——面板不该提供点了没反应的条目)。
  // 「可用」必须连祖先一起看:控件自己没有 hidden/disabled,但住在收起的 <details>
  // 里时,.click() 的可见后果是零。这类宿主先展开再点,而不是把它当作不可用剔掉——
  // 剔掉的话「搜索」这类功能就从面板里凭空消失了。
  const action = (label, id) => {
    const el = $(id);
    if (!el || el.disabled || el.classList.contains("hidden")) return;
    // 这里**不替它展开宿主 details**。预先展开会让处理器看到「宿主已开 + 自己没有
    // hidden 类」从而判定为"当前可见"、反手执行关闭——被修的那条高危路径原样复活。
    // 展开责任统一留在各自的 click 处理器里(见 07-events.js chat-search-toggle),
    // 那样无论从菜单点还是从面板点,行为都是同一套。
    push(t("动作"), label, "", () => el.click());
  };
  action(t("新对话"), "new-chat");
  action(t("停止"), "stop");
  action(t("总结"), "summarize-btn");
  action(t("复制上下文"), "copy-context");
  action(t("搜索"), "chat-search-toggle");
  action(t("切换主题"), "theme-toggle");
  action(t("创建隔离 Git 工作树线程"), "worktree-add");
  action(t("记需求"), "req-quick");
  action(t("记缺陷"), "defect-quick");
  push(t("动作"), t("聚焦输入"), "Ctrl/Cmd+K", () => promptBox.focus());
  return entries;
}

/// 子序列匹配:输入的每个字符按顺序出现即命中。比 includes 宽松(「运画」能命中
/// 「运行画像」),又不像全模糊那样什么都命中。空查询返回全部。
export function paletteMatches(entry, query) {
  if (!query) return true;
  const hay = `${entry.group} ${entry.label} ${entry.detail}`.toLowerCase();
  let at = 0;
  for (const ch of query) {
    at = hay.indexOf(ch, at);
    if (at < 0) return false;
    at += 1;
  }
  return true;
}

export function renderPaletteList() {
  const list = $("palette-list");
  if (!list) return;
  const query = ($("palette-input")?.value || "").trim().toLowerCase();
  const matched = paletteEntries.filter((entry) => paletteMatches(entry, query)).slice(0, PALETTE_LIMIT);
  paletteIndex = Math.min(paletteIndex, Math.max(0, matched.length - 1));
  list.replaceChildren();
  if (!matched.length) {
    const empty = document.createElement("div");
    empty.className = "palette-empty";
    empty.textContent = t("没有匹配项");
    list.appendChild(empty);
    paletteEntries.active = null;
    return;
  }
  matched.forEach((entry, index) => {
    const row = document.createElement("div");
    row.className = `palette-row${index === paletteIndex ? " active" : ""}`;
    row.setAttribute("role", "option");
    row.setAttribute("aria-selected", index === paletteIndex ? "true" : "false");
    const group = document.createElement("span");
    group.className = "palette-group";
    group.textContent = entry.group;
    const label = document.createElement("span");
    label.className = "palette-label";
    label.textContent = entry.label;
    const detail = document.createElement("span");
    detail.className = "palette-detail";
    detail.textContent = entry.detail;
    row.append(group, label, detail);
    // mousedown 而不是 click:click 之前会先触发输入框 blur,那条路径会关面板,
    // 于是「点一下候选项」变成「什么都没发生」。
    row.addEventListener("mousedown", (event) => {
      event.preventDefault();
      runPaletteEntry(entry);
    });
    row.addEventListener("mouseenter", () => {
      paletteIndex = index;
      renderPaletteList();
    });
    list.appendChild(row);
  });
  paletteEntries.active = matched[paletteIndex] ?? null;
}

export function runPaletteEntry(entry) {
  closePalette();
  try {
    entry?.run?.();
  } catch (error) {
    log(`${t("命令面板")}:${error}`, "warn");
  }
}

/// aria-modal="true" 只是**声明**,浏览器不会因此把焦点关在里面。实测不加处理时
/// Tab 两下就走到了背后的 rail,回车能在半透明遮罩下真的把视图切走、甚至点到
/// 「新对话」(会清空多轮历史)——而屏幕上什么都看不见。
///
/// 用 inert 把**除面板以外的所有顶层兄弟**整体惰性化:比手写 Tab 循环可靠(不用维护
/// 「哪些算可聚焦」这张永远会漏的表),而且顺带堵住语义泄漏——屏幕阅读器不会在一个
/// 模态对话框里继续读整个应用。按兄弟遍历而不是写死 #app,是因为权限询问弹窗、文档
/// 查看器也在 #app 外面,写死一个 id 就会漏掉它们。
export function setBackgroundInert(on) {
  const panel = paletteEl();
  const siblings = document.body?.children;
  if (!panel || !siblings) return;
  for (const node of siblings) {
    if (node === panel) continue;
    if (typeof node.setAttribute !== "function") continue;
    if (on) node.setAttribute("inert", "");
    else node.removeAttribute("inert");
  }
}

export function openPalette() {
  const panel = paletteEl();
  const input = $("palette-input");
  if (!panel || !input) return;
  paletteRestoreFocus = document.activeElement;
  paletteEntries = collectPaletteEntries();
  paletteIndex = 0;
  input.value = "";
  panel.classList.remove("hidden");
  setBackgroundInert(true);
  renderPaletteList();
  input.focus();
}

export function closePalette() {
  const panel = paletteEl();
  if (!panel || panel.classList.contains("hidden")) return;
  panel.classList.add("hidden");
  // inert 必须先摘:还留着的话下面这次 focus() 会落在惰性子树上而静默失败。
  setBackgroundInert(false);
  // 焦点回到打开面板前的位置:不还回去的话 Tab 序列会从文档开头重新开始。
  if (paletteRestoreFocus && typeof paletteRestoreFocus.focus === "function") paletteRestoreFocus.focus();
  paletteRestoreFocus = null;
}

export function movePaletteSelection(delta) {
  const rows = $("palette-list")?.querySelectorAll(".palette-row") ?? [];
  if (!rows.length) return;
  paletteIndex = (paletteIndex + delta + rows.length) % rows.length;
  renderPaletteList();
}

defer(() => {
  $("palette-input")?.addEventListener("input", () => {
    paletteIndex = 0;
    renderPaletteList();
  });
});
defer(() => {
  $("palette-input")?.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      movePaletteSelection(1);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      movePaletteSelection(-1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (paletteEntries.active) runPaletteEntry(paletteEntries.active);
    } else if (event.key === "Escape") {
      event.preventDefault();
      closePalette();
    }
  });
});
// 点遮罩(不是面板本体)关闭。
defer(() => {
  paletteEl()?.addEventListener("mousedown", (event) => {
    if (event.target === paletteEl()) closePalette();
  });
});

defer(() => {
  window.addEventListener("keydown", (event) => {
    // Esc 必须挂在**窗口**上,不能只挂输入框:焦点一旦离开输入框(Tab 一下、
    // 或点了一下列表),输入框级的 Escape 就再也收不到,面板看起来关不掉了。
    if (event.key === "Escape" && paletteIsOpen()) {
      event.preventDefault();
      closePalette();
      return;
    }
    const modifier = event.ctrlKey || event.metaKey;
    if (!modifier || event.altKey) return;
    if (event.key.toLowerCase() !== "p") return;
    // WebView 里 Ctrl+P 默认是打印,必须拦下。
    event.preventDefault();
    if (paletteIsOpen()) closePalette();
    else openPalette();
  });
});

// R-264 B10：命令面板进入 ESM；未迁移的 classic 提供方通过 globalThis 兼容桥提供 API。
