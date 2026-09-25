// 弹层原语唯一入口(零 import)。设计见 docs/design/ui_surface_stack.md。
//
// 模态对话框、锚定菜单/浮层、停靠卡片、toast、tooltip 全部经这里开关:
// - 一个栈:谁在最上面只有一个答案;
// - 一个 Esc 入口:document 捕获阶段,只关栈顶,关完即停止传播——别处不再挂 document/window 级 Esc;
// - 一套焦点规则:模态记住并归还焦点;卡片在用户正在输入时不抢焦点;
// - 一种外观:元素只带 .k-surface 等类名,底色/边框/圆角/阴影全在 surface.css。
// 打开时摘掉 .hidden、关闭时加回(镜像):旧代码与冒烟读 classList.contains("hidden") 仍然成立;
// 除本模块外,任何代码都不得直接切换弹层的 .hidden(ui-surface-rules J1)。
//
// 零 import 是硬约束:样例页(gallery.html)与假 DOM 冒烟都要能单独加载它,也免得卷进 ESM 循环依赖。
// 翻译函数由外部注入(01-core.js 启动时调用 setSurfaceTranslator)。

let t = (key) => key;
export function setSurfaceTranslator(fn) {
  if (typeof fn === "function") t = fn;
}

const hasDocument = typeof document !== "undefined" && document !== null;
const now = () => (typeof performance !== "undefined" && performance.now ? performance.now() : Date.now());

// 栈元素:{ el, type: "modal"|"menu"|"popover"|"card", escape, onClose, anchor, lightDismiss, ... }
const stack = [];
let anchorSeed = 0;
let menuSeed = 0;

const byId = (id) => (hasDocument ? document.getElementById(id) : null);

function isInside(node, container) {
  for (let n = node; n; n = n.parentNode) if (n === container) return true;
  return false;
}
function activeElement() {
  return hasDocument ? document.activeElement ?? null : null;
}
function isEditable(node) {
  if (!node) return false;
  const tag = String(node.tagName || "").toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || node.isContentEditable === true;
}
// 文字输入类元素:在它里面按 Esc 有局部含义(取消输入、收起查找框)。勾选框/按钮/下拉不算。
function isTextEntry(node) {
  if (!node) return false;
  const tag = String(node.tagName || "").toLowerCase();
  if (tag === "textarea" || node.isContentEditable === true) return true;
  return tag === "input" && !/^(checkbox|radio|button|submit|reset|range|color|file|image)$/i.test(String(node.type || ""));
}
// 程序化聚焦(模态初始焦点、卡片抢焦点、菜单首项、关闭后归还焦点)期间为真:
// tooltip 的 focusin 看到它就不弹提示——否则键盘打开弹窗时,初始焦点按钮上会立刻冒出提示盖住弹窗角。
let quietFocus = false;
function quietly(fn) {
  const prior = quietFocus;
  quietFocus = true;
  try { return fn(); } finally { quietFocus = prior; }
}
function focusEl(node, { quiet = true } = {}) {
  if (node && typeof node.focus === "function") {
    const run = () => {
      try { node.focus(); } catch { /* 已离开文档的节点 */ }
    };
    if (quiet) quietly(run);
    else run();
  }
}
function resolveTarget(ref, scope) {
  if (!ref) return null;
  if (typeof ref !== "string") return ref;
  if (ref.startsWith("#") && /^#[\w-]+$/.test(ref)) return byId(ref.slice(1));
  return scope?.querySelector?.(ref) ?? null;
}
function firstFocusable(el) {
  return el?.querySelector?.("[autofocus]")
    ?? el?.querySelector?.("button:not([disabled]), [role^='menuitem']:not([aria-disabled='true']), input, select, textarea")
    ?? null;
}
function mirrorHidden(el, hidden) {
  el?.classList?.toggle("hidden", hidden);
}
function handleFor(el) {
  return stack.find((h) => h.el === el) ?? null;
}
function supportsClosedBy() {
  return typeof HTMLDialogElement !== "undefined" && "closedBy" in HTMLDialogElement.prototype;
}
function popoverShowing(el) {
  try {
    if (typeof el?.matches === "function") return el.matches(":popover-open");
  } catch { /* 旧运行时不认 :popover-open */ }
  return Boolean(el?._popoverOpen);
}
// JS 现造的弹层统一挂在 #kz-surface-root(首次使用时追加到 body 末尾)。
// 现造节点另存引用:不依赖 getElementById 能查到后加的节点(冒烟假 DOM 只认 index.html 里的 id)。
let rootNode = null;
function surfaceRoot() {
  if (rootNode?.parentNode) return rootNode;
  rootNode = byId("kz-surface-root");
  if (!rootNode && hasDocument && document.body) {
    rootNode = document.createElement("div");
    rootNode.id = "kz-surface-root";
    document.body.appendChild(rootNode);
  }
  return rootNode;
}

export function isModalOpen() {
  return stack.some((h) => h.type === "modal");
}
function topModal() {
  for (let i = stack.length - 1; i >= 0; i -= 1) if (stack[i].type === "modal") return stack[i];
  return null;
}
export function stackDepth() {
  return stack.length;
}
export function isSurfaceOpen(el) {
  return Boolean(el && handleFor(el));
}

// ---------- 关闭:所有路径都汇到 finish ----------
function finish(handle, value, { nativeClosed = false } = {}) {
  if (!handle || handle.closed) return;
  handle.closed = true;
  const index = stack.indexOf(handle);
  // 叠在它上面、锚在它里面的浮层(菜单里开的子菜单、卡片里开的菜单)随它一起关。
  if (index >= 0) {
    for (const above of stack.slice(index + 1).reverse()) {
      if (isInside(above.anchor, handle.el) || isInside(above.el, handle.el)) finish(above);
    }
  }
  const at = stack.indexOf(handle);
  if (at >= 0) stack.splice(at, 1);
  const el = handle.el;
  const focusWasInside = isInside(activeElement(), el);
  if (!nativeClosed) {
    // 浏览器关 dialog/popover 时会自己把焦点还给之前的元素:同属程序化聚焦,不弹提示。
    quietly(() => {
      try {
        if (handle.native === "modal" && el.open && typeof el.close === "function") el.close();
        else if (handle.native === "popover" && typeof el.hidePopover === "function" && popoverShowing(el)) el.hidePopover();
      } catch { /* 元素已被移除或状态已变:镜像仍要落地 */ }
    });
  }
  mirrorHidden(el, true);
  syncExpanded(handle, false);
  if (handle.generated) el.remove();
  try { handle.onClose?.(value); } finally {
    handle.resolve?.(value);
  }
  // 焦点:模态与抢过焦点的卡片把焦点还回去;菜单/浮层只在焦点原本在里面时还给锚点。
  if (handle.returnFocus && handle.returnFocus.isConnected !== false && (focusWasInside || handle.type === "modal")) {
    focusEl(handle.returnFocus);
  } else if (focusWasInside && handle.anchor) {
    focusEl(handle.anchor);
  }
  if (handle.type === "modal") activateQueued(el);
}

export function closeSurface(handleOrEl, value) {
  if (!handleOrEl) return;
  // 传句柄:只关这一个(已关过的旧句柄是空操作,不会误关同一元素后来的新句柄)。
  if (handleOrEl.tagName === undefined && handleOrEl.el) {
    finish(handleOrEl, value);
    return;
  }
  const el = handleOrEl;
  const handle = handleFor(el);
  if (handle) {
    finish(handle, value);
    return;
  }
  // 不在栈里:保证它确实是关着的(旧状态或外部误开)。
  try {
    if (el.open && typeof el.close === "function") el.close();
    if (typeof el.hidePopover === "function" && popoverShowing(el)) el.hidePopover();
  } catch { /* 忽略 */ }
  mirrorHidden(el, true);
}

function makeHandle(el, type, extra = {}) {
  const handle = { el, type, closed: false, ...extra };
  handle.close = (value) => finish(handle, value);
  return handle;
}

// ---------- Esc:唯一入口 ----------
// 停靠卡片不抢别处的局部 Esc:焦点在卡片外的文字输入框里、或在 Monaco 编辑器里(查找/补全小部件
// 靠自己的 Esc 收起)时,这一下 Esc 归那个元素——权限卡在场不等于用户想拒绝它,
// 想法/缺陷速记表单的 Esc 照常取消输入。与卡片 focus:"auto"「用户在别处打字时不打扰」同一条理由。
function cardYields(handle, target) {
  if (handle.type !== "card" || !target || isInside(target, handle.el)) return false;
  return isTextEntry(target) || Boolean(target.closest?.(".monaco-editor"));
}
// 栈顶第一个可 Esc 的句柄。模态开着时,之后才弹出的停靠卡片在模态背后是惰性的,不参与。
function topEscapable(target) {
  let modalIndex = -1;
  for (let i = stack.length - 1; i >= 0; i -= 1) {
    if (stack[i].type === "modal") { modalIndex = i; break; }
  }
  for (let i = stack.length - 1; i >= 0; i -= 1) {
    const handle = stack[i];
    if (handle.type === "tooltip" || typeof handle.escape !== "function") continue;
    if (modalIndex >= 0 && i > modalIndex && handle.type === "card") continue;
    if (cardYields(handle, target)) continue;
    return handle;
  }
  return null;
}
// 原生下拉(base-select)的列表开着时,Esc 归浏览器关列表,不能顺手把外层菜单也关了。
function nativePickerOpen(event) {
  for (const node of [event?.target, activeElement()]) {
    const select = node?.closest?.("select");
    if (!select) continue;
    try {
      if (select.matches(":open")) return true;
    } catch { /* 不支持 :open 的运行时:没有页面内列表可言 */ }
  }
  return false;
}
function onKeydown(event) {
  if (event.key !== "Escape" || event.isComposing) return;
  if (nativePickerOpen(event)) return;
  const top = topEscapable(event.target ?? activeElement());
  if (!top) return;
  event.preventDefault();
  event.stopImmediatePropagation();
  top.escape();
}

// ---------- 点外关闭(菜单与信息浮层) ----------
// 菜单/浮层用 popover="manual",点外关闭由这里统一做,而不是交给浏览器的 popover="auto":
// 浏览器的轻关闭发生在 pointerdown 分发之前,于是「点触发器收起菜单」会先被轻关闭、
// 再被触发器的 click 重新打开——触发器永远关不掉它。这里把锚点(触发器)排除在外。
function onPointerDown(event) {
  hideTip();
  const target = event.target;
  for (let i = stack.length - 1; i >= 0; i -= 1) {
    const handle = stack[i];
    if (!handle) continue; // finish 会连带关掉嵌套在它上面的弹层,下标可能越过新的栈顶
    if (!handle.lightDismiss) {
      if (handle.type === "modal") break;
      continue;
    }
    if (isInside(target, handle.el) || (handle.anchor && isInside(target, handle.anchor))) break;
    finish(handle);
  }
}

if (hasDocument && typeof document.addEventListener === "function") {
  document.addEventListener("keydown", onKeydown, true);
  document.addEventListener("pointerdown", onPointerDown, true);
}

// ---------- 锚点 ----------
function anchorTo(el, anchor) {
  if (!anchor?.style || !el?.style) return;
  let name = anchor.dataset?.kzAnchor;
  if (!name) {
    anchorSeed += 1;
    name = `--kz-anchor-${anchorSeed}`;
    if (anchor.dataset) anchor.dataset.kzAnchor = name;
  }
  anchor.style.setProperty("anchor-name", name);
  el.style.setProperty("position-anchor", name);
}
function syncExpanded(handle, open) {
  const anchor = handle.anchor;
  if (!anchor?.setAttribute || !anchor.hasAttribute) return;
  if (anchor.hasAttribute("aria-expanded") || anchor.hasAttribute("aria-haspopup")) {
    anchor.setAttribute("aria-expanded", open ? "true" : "false");
  }
}
function ensurePopover(el) {
  if (el.getAttribute?.("popover") !== "manual") el.setAttribute?.("popover", "manual");
}
function showNative(el) {
  try {
    if (typeof el.showPopover === "function" && !popoverShowing(el)) el.showPopover();
  } catch { /* 元素未挂进文档等:镜像状态照常 */ }
}
function wirePopover(el) {
  if (el._kzPopoverWired || typeof el.addEventListener !== "function") return;
  el._kzPopoverWired = true;
  // 浏览器自己关掉它(元素被移出文档、popover 属性被改)时把栈同步回来。
  // toggle 是异步派发的:关掉后同一任务里又打开时,迟到的 closed 事件不能把新句柄关掉。
  el.addEventListener("toggle", (event) => {
    if (event.newState !== "closed" || popoverShowing(el)) return;
    const handle = handleFor(el);
    if (handle && !handle.closed) finish(handle, undefined, { nativeClosed: true });
  });
}
function closeUnrelatedLightDismiss(anchor) {
  for (let i = stack.length - 1; i >= 0; i -= 1) {
    const handle = stack[i];
    if (handle.type === "modal") break;
    if (!handle.lightDismiss) continue;
    if (anchor && isInside(anchor, handle.el)) break;
    finish(handle);
  }
}

// ---------- ① 模态 ----------
function wireDialog(el) {
  if (el._kzDialogWired || typeof el.addEventListener !== "function") return;
  el._kzDialogWired = true;
  // 原生取消(closedby 点外、未被拦下的 Esc):走句柄自己的 escape,结果值由调用方决定。
  el.addEventListener("cancel", (event) => {
    const handle = handleFor(el);
    if (!handle) return;
    event.preventDefault?.();
    handle.escape();
  });
  // close 事件是异步派发的:排队的下一个确认框在同一任务里已经 showModal,
  // 迟到的 close 不能把它当成「被关掉」(否则排队的第二问会立刻以取消收场)。
  el.addEventListener("close", () => {
    if (el.open) return;
    const handle = handleFor(el);
    if (handle && !handle.closed) finish(handle, handle.cancelValue, { nativeClosed: true });
  });
  // closedby="any" 不可用时的退化:点在 dialog 自身、且坐标落在内容框外 = 点在遮罩上。
  el.addEventListener("click", (event) => {
    if (supportsClosedBy() || event.target !== el || typeof el.getBoundingClientRect !== "function") return;
    const r = el.getBoundingClientRect();
    const outside = event.clientX < r.left || event.clientX > r.right || event.clientY < r.top || event.clientY > r.bottom;
    const handle = handleFor(el);
    if (outside && handle) handle.escape();
  });
}
function activate(handle) {
  const el = handle.el;
  // 模态会把背后的一切设为惰性:开着的菜单/浮层先收掉(与浏览器 showModal 的行为一致)。
  closeUnrelatedLightDismiss(null);
  try { handle.prepare?.(); } catch (err) { console.error(err); }
  handle.returnFocus = activeElement();
  stack.push(handle);
  mirrorHidden(el, false);
  wireDialog(el);
  // showModal 自己就会同步聚焦(dialog 聚焦步骤),同样算程序化聚焦。
  quietly(() => {
    try {
      if (typeof el.showModal === "function" && !el.open) el.showModal();
    } catch (err) {
      console.warn(`showModal failed: ${err}`);
    }
  });
  focusEl(resolveTarget(handle.initialFocus, el) ?? firstFocusable(el));
}
function activateQueued(el) {
  const queue = el._kzQueue;
  if (!queue?.length || handleFor(el)) return;
  activate(queue.shift());
}
export function openDialog(el, options = {}) {
  if (!el) return null;
  const existing = handleFor(el);
  if (existing && !options.queue) return existing;
  const handle = makeHandle(el, "modal", {
    native: "modal",
    initialFocus: options.initialFocus,
    prepare: options.prepare,
    onClose: options.onClose,
    cancelValue: options.cancelValue,
  });
  handle.escape = typeof options.onEscape === "function"
    ? () => options.onEscape(handle)
    : () => finish(handle, handle.cancelValue);
  handle.result = new Promise((resolve) => { handle.resolve = resolve; });
  if (existing) {
    // 同一个 dialog 已经开着:排队,不互相覆盖(旧实现并发调用会把前一个的文案冲掉)。
    (el._kzQueue ??= []).push(handle);
    return handle;
  }
  activate(handle);
  return handle;
}

let confirmWired = false;
function wireConfirm() {
  if (confirmWired) return;
  confirmWired = true;
  const overlay = byId("confirm-overlay");
  byId("confirm-ok")?.addEventListener("click", () => closeSurface(overlay, true));
  byId("confirm-safe")?.addEventListener("click", () => closeSurface(overlay, "safe"));
  byId("confirm-cancel")?.addEventListener("click", () => closeSurface(overlay, false));
}
// options: { title, message, list?: string[], okText?, safeText?, danger?: boolean }
// → Promise<true|false|"safe">;确认 true,safeText 按钮 "safe",取消/Esc/点外 false。
export function confirmDialog(options = {}) {
  const overlay = byId("confirm-overlay");
  if (!overlay) return Promise.resolve(false);
  wireConfirm();
  const handle = openDialog(overlay, {
    queue: true,
    cancelValue: false,
    initialFocus: "#confirm-ok",
    prepare: () => {
      byId("confirm-title").textContent = options.title ?? t("确认");
      byId("confirm-message").textContent = options.message ?? "";
      const listEl = byId("confirm-list");
      listEl.textContent = "";
      if (options.list && options.list.length) {
        for (const item of options.list) {
          const li = document.createElement("li");
          li.textContent = item;
          listEl.appendChild(li);
        }
        listEl.classList.remove("hidden");
      } else {
        listEl.classList.add("hidden");
      }
      const ok = byId("confirm-ok");
      ok.textContent = options.okText ?? t("确认");
      ok.classList.toggle("danger", !!options.danger);
      const safe = byId("confirm-safe");
      safe.textContent = options.safeText ?? t("删除并安全整理");
      safe.classList.toggle("hidden", !options.safeText);
      safe.classList.toggle("danger", !!options.danger);
    },
  });
  return handle.result;
}

let inputWired = false;
function wireInput() {
  if (inputWired) return;
  inputWired = true;
  const overlay = byId("input-overlay");
  const input = byId("input-value");
  byId("input-ok")?.addEventListener("click", () => closeSurface(overlay, input.value));
  byId("input-cancel")?.addEventListener("click", () => closeSurface(overlay, null));
  // Enter 挂在输入框自己身上;输入法组合中的 Enter 是在选词,不是提交。
  input?.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" || event.isComposing || !isSurfaceOpen(overlay)) return;
    event.preventDefault?.();
    closeSurface(overlay, input.value);
  });
}
// options: { title, message?, value?, placeholder?, okText? } → Promise<string|null>
export function inputDialog(options = {}) {
  const overlay = byId("input-overlay");
  if (!overlay) return Promise.resolve(null);
  wireInput();
  const handle = openDialog(overlay, {
    queue: true,
    cancelValue: null,
    initialFocus: "#input-value",
    prepare: () => {
      byId("input-title").textContent = options.title ?? "";
      const message = byId("input-message");
      message.textContent = options.message ?? "";
      message.classList.toggle("hidden", !options.message);
      const input = byId("input-value");
      input.value = options.value ?? "";
      input.placeholder = options.placeholder ?? "";
      input.setAttribute("aria-label", options.title ?? "");
      byId("input-ok").textContent = options.okText ?? t("确认");
    },
  });
  return handle.result;
}

// ---------- ② 锚定弹层:菜单与信息浮层 ----------
// 已有的静态面板(#sop-picker-panel、#context-detail、#file-suggestions、各 data-kz-menu 菜单)。
// anchorEl 为空时取 bindMenus 登记的触发器。已开着就返回原句柄(幂等)。
//
// 模态开着时,浏览器把 dialog 子树之外的一切(包括之后才弹出的顶层 popover)设为惰性:点不到、拿不到焦点。
// 所以弹窗里的菜单/浮层必须是该 <dialog> 的后代——openMenu 自动挂进锚点所在的 dialog;
// 静态弹层要写在 dialog 里面(ui-surface-rules H 组查 index.html)。写错了在这里告警,而不是静默点不动。
function warnIfOutsideModal(el) {
  const modal = topModal();
  if (!modal || isInside(el, modal.el)) return;
  console.warn(
    `openPopover: #${el.id || "?"} 不在当前模态 #${modal.el.id || "?"} 里,模态开着时它是惰性的(点不到、拿不到焦点)。`
    + "静态弹层写进该 <dialog> 内;JS 菜单用 openMenu(自动挂进锚点所在的 dialog)。",
  );
}
export function openPopover(anchorEl, el, options = {}) {
  if (!el) return null;
  const anchor = anchorEl ?? el._kzTrigger ?? null;
  const existing = handleFor(el);
  if (existing) {
    if (anchor && existing.anchor !== anchor) {
      existing.anchor = anchor;
      anchorTo(el, anchor);
    }
    return existing;
  }
  warnIfOutsideModal(el);
  const manual = Boolean(options.manual);
  if (!manual) closeUnrelatedLightDismiss(anchor);
  const handle = makeHandle(el, options.type ?? "popover", {
    native: "popover",
    anchor,
    lightDismiss: !manual,
    onClose: options.onClose,
    generated: Boolean(options.generated),
  });
  handle.escape = typeof options.onEscape === "function" ? () => options.onEscape(handle) : () => finish(handle);
  ensurePopover(el);
  if (options.placement && el.dataset) el.dataset.placement = options.placement;
  if (anchor) anchorTo(el, anchor);
  stack.push(handle);
  mirrorHidden(el, false);
  wirePopover(el);
  showNative(el);
  syncExpanded(handle, true);
  return handle;
}

function menuItems(menu) {
  return [...(menu?.querySelectorAll?.(
    "[role^='menuitem']:not([aria-disabled='true']), button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled])",
  ) ?? [])];
}
function moveMenuFocus(menu, event) {
  if (event.key !== "ArrowDown" && event.key !== "ArrowUp" && event.key !== "Home" && event.key !== "End") return;
  const tag = String(event.target?.tagName || "").toLowerCase();
  if (tag === "textarea" || tag === "select" || (tag === "input" && !/^(checkbox|radio|button)$/i.test(event.target.type || ""))) return;
  const items = menuItems(menu);
  if (!items.length) return;
  event.preventDefault?.();
  const current = items.indexOf(event.target);
  let next = 0;
  if (event.key === "End") next = items.length - 1;
  else if (event.key === "ArrowDown") next = current < 0 ? 0 : (current + 1) % items.length;
  else if (event.key === "ArrowUp") next = current <= 0 ? items.length - 1 : current - 1;
  focusEl(items[next], { quiet: false }); // 用户在用方向键走菜单:这是键盘导航,提示照常
}

// items:{ label, desc?, kbd?, checked?, disabled?, danger?, onSelect } | "separator" | { heading }
// 点击一项:先关菜单,再调 onSelect。再次对同一锚点调用 = 收起(切换语义)。
export function openMenu(anchorEl, items, { placement = "bottom-start", label, onClose } = {}) {
  const open = stack.find((h) => h.generated && h.anchor === anchorEl && h.type === "menu");
  if (open) {
    finish(open);
    return open;
  }
  const menu = document.createElement("div");
  menuSeed += 1;
  menu.id = `kz-menu-${menuSeed}`;
  menu.className = "k-surface k-menu";
  menu.setAttribute("role", "menu");
  if (label) menu.setAttribute("aria-label", label);
  menu.setAttribute("popover", "manual");
  let handle = null;
  for (const item of items || []) {
    if (item === "separator") {
      const sep = document.createElement("div");
      sep.className = "k-menu-sep";
      sep.setAttribute("role", "separator");
      menu.appendChild(sep);
      continue;
    }
    if (item && item.heading) {
      const heading = document.createElement("div");
      heading.className = "k-menu-heading";
      heading.textContent = item.heading;
      menu.appendChild(heading);
      continue;
    }
    if (!item) continue;
    const button = document.createElement("button");
    button.type = "button";
    button.className = "k-menu-item";
    const checkable = typeof item.checked === "boolean";
    button.setAttribute("role", checkable ? "menuitemcheckbox" : "menuitem");
    if (checkable) button.setAttribute("aria-checked", item.checked ? "true" : "false");
    if (item.disabled) button.setAttribute("aria-disabled", "true");
    if (item.danger) button.dataset.tone = "danger";
    const main = document.createElement("span");
    main.className = "k-menu-item-main";
    const text = document.createElement("span");
    text.className = "k-menu-item-label";
    text.textContent = item.label ?? "";
    main.appendChild(text);
    if (item.desc) {
      const desc = document.createElement("span");
      desc.className = "k-menu-item-desc";
      desc.textContent = item.desc;
      main.appendChild(desc);
    }
    button.appendChild(main);
    if (item.kbd) {
      const kbd = document.createElement("kbd");
      kbd.className = "k-menu-item-kbd";
      kbd.textContent = item.kbd;
      button.appendChild(kbd);
    }
    button.addEventListener("click", () => {
      if (item.disabled) return;
      finish(handle);
      item.onSelect?.();
    });
    menu.appendChild(button);
  }
  menu.addEventListener("keydown", (event) => moveMenuFocus(menu, event));
  // 锚点在开着的 <dialog> 里:菜单挂进这个 dialog(模态之外的节点是惰性的,挂到 body 下就点不动);
  // 显示后它照样进顶层,不受 dialog 的 overflow 裁剪。关 dialog 时它作为嵌套弹层一并关掉。
  (anchorEl?.closest?.("dialog[open]") ?? surfaceRoot())?.appendChild(menu);
  handle = openPopover(anchorEl, menu, { type: "menu", placement, onClose, generated: true });
  focusEl(menuItems(menu)[0]);
  return handle;
}

// 扫描 [data-kz-menu="<弹层 id>"] 触发器并接线:锚点、aria-haspopup/controls/expanded、
// click 切换、ArrowDown 打开并聚焦第一项、菜单内方向键移动。启动时调用一次;重复调用幂等。
export function bindMenus(root = hasDocument ? document : null) {
  for (const trigger of root?.querySelectorAll?.("[data-kz-menu]") ?? []) {
    if (trigger._kzMenuBound) continue;
    const menu = byId(trigger.getAttribute("data-kz-menu") ?? trigger.dataset?.kzMenu);
    if (!menu) continue;
    trigger._kzMenuBound = true;
    menu._kzTrigger = trigger;
    trigger.setAttribute("aria-haspopup", menu.getAttribute("role") === "menu" ? "menu" : "dialog");
    trigger.setAttribute("aria-controls", menu.id);
    trigger.setAttribute("aria-expanded", "false");
    const open = () => openPopover(trigger, menu, { type: "menu", placement: menu.dataset?.placement || "bottom-start" });
    trigger.addEventListener("click", () => {
      if (isSurfaceOpen(menu)) closeSurface(menu);
      else open();
    });
    trigger.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowDown") return;
      event.preventDefault?.();
      open();
      focusEl(menuItems(menu)[0]);
    });
    menu.addEventListener("keydown", (event) => moveMenuFocus(menu, event));
  }
}

// ---------- ③ 停靠卡片(不轻关闭,不参与模态) ----------
// focus:"auto" = 焦点在别处的可编辑元素里(用户正在打字)时不抢,否则聚焦 initialFocus/第一个按钮;
// "none" = 从不抢。onEscape 缺省 = 卡片不响应 Esc。
export function showCard(el, { onEscape, focus = "auto", initialFocus } = {}) {
  if (!el) return null;
  let handle = handleFor(el);
  if (!handle) {
    handle = makeHandle(el, "card", { native: "popover" });
    ensurePopover(el);
    stack.push(handle);
    mirrorHidden(el, false);
    wirePopover(el);
    showNative(el);
  }
  handle.escape = typeof onEscape === "function" ? () => onEscape(handle) : null;
  if (focus === "auto") {
    const active = activeElement();
    if (!(isEditable(active) && !isInside(active, el))) {
      if (!isInside(active, el)) handle.returnFocus = active;
      focusEl(resolveTarget(initialFocus, el) ?? el.querySelector?.("button:not([disabled])"));
    }
  }
  return handle;
}
export function hideCard(el) {
  closeSurface(el);
}

// ---------- ④ 提示 ----------
const TOAST_MAX = 3;
function toastRegion() {
  let region = byId("toast");
  if (!region) {
    region = document.createElement("div");
    region.id = "toast";
    region.className = "k-surface k-toast-region hidden";
    region.setAttribute("popover", "manual");
    surfaceRoot()?.appendChild(region);
  }
  return region;
}
function liveToasts(region) {
  return Array.from(region.children).filter((node) => node.dataset?.kzExpired !== "1");
}
function syncToastRegion(region) {
  const live = liveToasts(region).length > 0;
  mirrorHidden(region, !live);
  try {
    if (!live) {
      if (typeof region.hidePopover === "function" && popoverShowing(region)) region.hidePopover();
      return;
    }
    // 模态开着时重新置顶一次:toast 要浮在遮罩之上,而顶层顺序只由「最后一次 show」决定。
    if (isModalOpen() && popoverShowing(region)) region.hidePopover();
    showNative(region);
  } catch { /* 区域未挂进文档 */ }
}
function expireToast(item) {
  if (item.dataset.kzExpired === "1") return;
  item.dataset.kzExpired = "1";
  item.classList.add("hidden");
  const region = item.parentNode;
  if (region) syncToastRegion(region);
}
// message 由调用方翻译好(03-shell.js 的 toast 负责本地化)。kind: info|ok|warn|err。
// 过期的条目只隐藏、留到下一条 toast 到来时再清掉:读的人(和冒烟)在消失后仍能取到最近一条文案。
export function toast(message, { kind = "info", timeout } = {}) {
  const region = toastRegion();
  for (const old of Array.from(region.children)) if (old.dataset?.kzExpired === "1") old.remove();
  const item = document.createElement("div");
  item.className = "k-surface k-toast";
  item.dataset.kind = kind;
  item.setAttribute("role", kind === "err" ? "alert" : "status");
  item.textContent = String(message ?? "");
  region.appendChild(item);
  const live = liveToasts(region);
  while (live.length > TOAST_MAX) live.shift().remove();
  syncToastRegion(region);
  const timer = setTimeout(() => expireToast(item), timeout ?? (kind === "err" ? 6000 : 2600));
  return () => {
    clearTimeout(timer);
    expireToast(item);
  };
}

// tooltip:接管全局 title。悬停 450ms 显示(键盘聚焦立即显示);显示期间 title 挪进 data-kz-tip
// 以压掉系统提示,隐藏时还回去。系统提示不跟主题、长说明会被画成一大块,这是改它的原因。
const TIP_DELAY_MS = 450;
const tip = { target: null, timer: null, moved: false, describedBy: null, shown: false };
let tooltipsInstalled = false;
let tipNode = null;
function tipElement() {
  let el = tipNode?.parentNode ? tipNode : byId("kz-tip");
  if (!el) {
    el = document.createElement("div");
    el.id = "kz-tip";
    el.className = "k-surface k-tooltip hidden";
    el.setAttribute("role", "tooltip");
    el.setAttribute("popover", "manual");
    // popover="hint" 不会顺手关掉开着的菜单;不支持时退回 manual(效果相同)。
    try {
      el.setAttribute("popover", "hint");
      if (el.popover !== "hint") el.setAttribute("popover", "manual");
    } catch { /* 旧运行时 */ }
    surfaceRoot()?.appendChild(el);
  }
  tipNode = el;
  return el;
}
function hideTip() {
  const target = tip.target;
  clearTimeout(tip.timer);
  tip.timer = null;
  tip.target = null;
  if (tip.shown) {
    tip.shown = false;
    const el = tipNode;
    if (el) {
      try { if (typeof el.hidePopover === "function" && popoverShowing(el)) el.hidePopover(); } catch { /* 忽略 */ }
      mirrorHidden(el, true);
    }
  }
  if (!target) return;
  if (tip.describedBy === null) target.removeAttribute?.("aria-describedby");
  else target.setAttribute?.("aria-describedby", tip.describedBy);
  tip.describedBy = null;
  if (tip.moved) {
    tip.moved = false;
    const text = target.dataset?.kzTip;
    delete target.dataset.kzTip;
    // 显示期间若有人重新写了 title(切语言、实时状态),以新值为准。
    if (text && !target.hasAttribute?.("title")) target.setAttribute("title", text);
  }
}
function showTipNow() {
  const target = tip.target;
  const text = target?.dataset?.kzTip;
  if (!target || !text) return;
  const el = tipElement();
  el.textContent = text;
  anchorTo(el, target);
  mirrorHidden(el, false);
  showNative(el);
  tip.shown = true;
  tip.describedBy = target.getAttribute?.("aria-describedby") ?? null;
  target.setAttribute?.("aria-describedby", "kz-tip");
}
function tipTargetOf(node) {
  let el = null;
  for (let n = node; n && n.getAttribute; n = n.parentNode) {
    if (n.getAttribute("title") || n.dataset?.kzTip) { el = n; break; }
  }
  if (!el || el.id === "kz-tip") return null;
  if (el.closest?.(".monaco-editor")) return null;
  return el;
}
function beginTip(el, immediate) {
  if (tip.target === el) return;
  hideTip();
  const title = el.getAttribute?.("title");
  if (title) {
    el.dataset.kzTip = title;
    el.removeAttribute("title");
    tip.moved = true;
  }
  if (!el.dataset?.kzTip) {
    tip.moved = false;
    return;
  }
  tip.target = el;
  if (immediate) showTipNow();
  else tip.timer = setTimeout(showTipNow, TIP_DELAY_MS);
}
export function installTooltips(root = hasDocument ? document : null) {
  if (tooltipsInstalled || typeof root?.addEventListener !== "function") return;
  tooltipsInstalled = true;
  root.addEventListener("pointerover", (event) => {
    const el = tipTargetOf(event.target);
    if (el) beginTip(el, false);
    else if (tip.target && !isInside(event.target, tip.target)) hideTip();
  }, true);
  root.addEventListener("pointerout", (event) => {
    if (!tip.target) return;
    if (event.relatedTarget && isInside(event.relatedTarget, tip.target)) return;
    if (isInside(event.target, tip.target) || event.target === tip.target) hideTip();
  }, true);
  root.addEventListener("focusin", (event) => {
    if (quietFocus) return; // 程序化聚焦(见 quietly)不弹提示
    const el = tipTargetOf(event.target);
    if (!el) return;
    let keyboard = true;
    try { keyboard = typeof event.target.matches !== "function" || event.target.matches(":focus-visible"); } catch { /* 忽略 */ }
    if (keyboard) beginTip(el, true);
  }, true);
  root.addEventListener("focusout", () => hideTip(), true);
  root.addEventListener("keydown", () => hideTip(), true);
  root.addEventListener("scroll", () => hideTip(), true);
  if (hasDocument && root !== document) document.addEventListener?.("visibilitychange", () => hideTip());
  else root.addEventListener("visibilitychange", () => hideTip());
  if (typeof window !== "undefined" && typeof window.addEventListener === "function") window.addEventListener("blur", () => hideTip());
}
