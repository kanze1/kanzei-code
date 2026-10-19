const $ = id => document.getElementById(id);
const frame = $("product");
const model = { design: "a", scenario: "empty", baseline: false, narrow: false, theme: "light", selected: null };
const examples = [
  { id: "R-418", title: "优化对话页工作展示栏", note: "进行中 · 当前对话负责" },
  { id: "R-421", title: "完善窄屏下的输入区布局", note: "可选 · 待开始" },
  { id: "D-207", title: "修复切换对话后状态残留", note: "可选 · 待修复" }
];
const descriptions = {
  a: "保留一眼可见的工作上下文，收为 36px 单行；没有工作时合并空状态。",
  b: "工作收进现有标题栏，点击展开详情；对话区不再增加固定展示行。",
  c: "工作放到输入框上沿，发送前就近确认；点击展开本轮和下轮工作。"
};
const icons = {
  chevron: '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="m6 8 4 4 4-4"/></svg>',
  arrow: '<svg viewBox="0 0 20 20" aria-hidden="true"><path d="M4 10h12m-4-4 4 4-4 4"/></svg>',
  work: '<svg viewBox="0 0 20 20" aria-hidden="true"><rect x="3" y="6" width="14" height="11" rx="2"/><path d="M7 6V4a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2M3 10h14M8 10v2h4v-2"/></svg>'
};
let productDocument, originalRail, oldHeight, popover, picker;
let anchorForPopover;

function element(tag, className, text, owner = productDocument) {
  const node = owner.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}
function icon(name) {
  const template = productDocument.createElement("template");
  template.innerHTML = icons[name];
  return template.content.firstElementChild;
}
function button(className, text, action) {
  const node = element("button", className, text);
  node.type = "button"; node.addEventListener("click", action);
  return node;
}
function data() {
  if (model.scenario === "empty") return { current: null, next: null, pending: false, selected: false };
  const current = model.scenario === "long" ? { ...examples[0], title: "优化对话中当前工作与下一候选的展示层级，并保证长标题、窄屏和运行中切换时的信息完整" } : examples[0];
  if (model.selected && model.selected.fromEmpty) return { current: model.selected.entry, next: examples.find(entry => entry.id !== model.selected.entry.id), pending: false, selected: true };
  return { current, next: model.selected?.entry || examples[1], pending: model.scenario === "switch", selected: false };
}
function fullTitle(entry) { return `${entry.id} · ${entry.title}`; }
function closePanel() {
  if (popover.matches(":popover-open")) popover.hidePopover();
}
function trigger(className, text) {
  const node = button(className, text, () => showPanel(node));
  node.setAttribute("aria-haspopup", "dialog"); node.setAttribute("aria-expanded", "false"); node.setAttribute("aria-controls", "wf-details");
  return node;
}
function appendEntry(node, entry, label) {
  node.title = fullTitle(entry);
  node.setAttribute("aria-label", `${label}：${fullTitle(entry)}`);
  if (label) node.append(element("span", "wf-label", label));
  node.append(element("span", "wf-id", entry.id), element("span", "wf-title", entry.title));
}

function updateOriginal(value) {
  for (const [position, label, entry] of [
    ["current", value.selected ? "当前选取" : "正在进行", value.current],
    ["next", value.pending ? "已选工作 · 下一轮生效" : "下一个候选", value.next]
  ]) {
    const slot = originalRail.querySelector(`[data-position="${position}"]`);
    const heading = element("small", "", label);
    slot.dataset.hasWork = String(Boolean(entry));
    slot.replaceChildren(heading);
    if (entry) {
      const link = trigger("work-focus-entry", "");
      link.title = fullTitle(entry); link.setAttribute("aria-label", fullTitle(entry));
      link.append(element("span", "work-focus-id", entry.id), element("span", "work-focus-title", entry.title));
      slot.append(link);
    } else slot.append(element("span", "work-focus-empty", "暂无"));
  }
}

function render() {
  if (!productDocument) return;
  closePanel();
  for (const node of productDocument.querySelectorAll("[data-wf-injected]")) node.remove();
  const value = data();
  updateOriginal(value);
  const design = model.design;
  delete productDocument.body.dataset.wfDesign;
  oldHeight = originalRail.getBoundingClientRect().height;
  if (model.baseline) delete productDocument.body.dataset.wfDesign;
  else productDocument.body.dataset.wfDesign = design;
  document.body.dataset.baseline = String(model.baseline);
  $("baseline").setAttribute("aria-pressed", String(model.baseline));
  $("baseline").textContent = model.baseline ? "返回方案" : "对比原设计";
  $("description").textContent = model.baseline ? "当前源码的原有两列展示，使用相同示例状态，方便直接对照占位。" : descriptions[design];
  $("workspace-preview").setAttribute("aria-labelledby", `tab-${design}`);
  for (const tab of document.querySelectorAll("[data-design]")) {
    const active = tab.dataset.design === design;
    tab.setAttribute("aria-selected", String(active)); tab.tabIndex = active ? 0 : -1;
  }
  if (!model.baseline && design === "a") {
    const rail = element("aside", "wf-row"); rail.dataset.wfInjected = "rail"; rail.setAttribute("aria-label", "当前对话工作");
    const current = trigger(`wf-summary${value.current ? "" : " wf-empty"}`, "");
    current.append(element("span", "wf-dot"));
    if (value.current) {
      appendEntry(current, value.current, value.selected ? "已选" : "进行中");
      if (value.pending) current.append(element("span", "wf-mobile-pending", "下轮已选"));
    }
    else { current.append(element("span", "wf-title", "尚未选择工作")); current.setAttribute("aria-label", "查看工作详情，尚未选择工作"); }
    const chevron = icon("chevron"); chevron.classList.add("wf-chevron"); current.append(chevron); rail.append(current);
    if (value.next) {
      const next = trigger(`wf-next${value.pending ? " wf-pending" : ""}`, ""); next.append(icon("arrow"));
      appendEntry(next, value.next, value.pending ? "下轮已选" : "下一项"); rail.append(next);
    }
    const actions = element("div", "wf-actions");
    actions.append(button("wf-choose", value.current ? "更换" : "选择工作", () => openPicker()), button("wf-all", "全部工作 ↗", () => openPicker(true)));
    rail.append(actions); productDocument.getElementById("view-chat").append(rail);
  } else if (!model.baseline && design === "b") {
    const node = trigger("wf-anchor wf-header-anchor", ""); node.dataset.wfInjected = "header";
    node.append(icon("work"));
    const copy = element("span", "wf-anchor-title");
    copy.append(element("span", "wf-header-prefix", "工作 ·"), element("strong", "", value.current ? value.current.id : "未选")); node.append(copy);
    if (value.pending) node.append(element("span", "wf-dot")); node.append(icon("chevron"));
    node.title = value.current ? `${fullTitle(value.current)}${value.pending ? `；下轮：${fullTitle(value.next)}` : ""}` : "查看当前和下一项工作";
    productDocument.getElementById("workspace-header").append(node);
  } else if (!model.baseline && design === "c") {
    const context = element("div", "wf-composer-context"); context.dataset.wfInjected = "composer";
    const node = value.current ? trigger("wf-anchor", "") : button("wf-anchor wf-empty", "", () => openPicker()); node.append(icon("work"));
    if (value.current) appendEntry(node, value.current, value.selected ? "已选" : "本轮");
    else node.append(element("span", "wf-title", "选择工作"));
    node.append(icon("chevron")); context.append(node);
    if (value.next) context.append(button("wf-composer-next", value.pending ? `下轮 · ${value.next.id}` : `下一项 · ${value.next.id}`, event => showPanel(event.currentTarget)));
    productDocument.getElementById("composer").prepend(context);
  }
  const proposed = model.baseline ? oldHeight : design === "a" ? 36 : design === "b" ? 0 : Math.round(productDocument.querySelector(".wf-composer-context").getBoundingClientRect().height + 7);
  $("old-height").textContent = `原设计 ${Math.round(oldHeight)}px`;
  $("new-height").textContent = model.baseline ? `${Math.round(oldHeight)}px` : design === "b" ? "独立占位 0px" : design === "c" ? `输入区 +${proposed}px` : `${proposed}px`;
  $("space-saved").textContent = !model.baseline && proposed < oldHeight ? `省 ${Math.round((oldHeight - proposed) / oldHeight * 100)}%` : "";
  $("interaction-tip").textContent = model.design === "b" && !model.baseline ? "试一下：点击产品标题栏右侧的“工作”，查看当前、下一项与更换入口。" : model.design === "c" && !model.baseline ? "试一下：点击输入框上沿的工作入口；运行中更换，会标为下轮生效。" : "试一下：选择工作、点击工作标题，或切换到“已选下轮”。";
  document.documentElement.dataset.ready = "true";
}

function detail(label, entry, description, pending = false) {
  const section = element("section", `wf-detail${pending ? " wf-detail-pending" : ""}`);
  section.append(element("small", "", label));
  const title = element("strong", "", entry ? "" : "尚未选择工作");
  if (entry) title.append(element("span", "", entry.id), productDocument.createTextNode(entry.title));
  section.append(title, element("p", "", description)); return section;
}
function showPanel(anchor) {
  if (popover.matches(":popover-open")) { closePanel(); return; }
  const value = data(); anchorForPopover = anchor;
  const head = element("header", "wf-panel-head"); head.append(element("strong", "", "对话工作"), button("", "×", closePanel));
  head.lastElementChild.setAttribute("aria-label", "关闭工作详情");
  const body = element("div", "wf-panel-body");
  body.append(detail(value.selected ? "当前选取" : "本轮工作", value.current, value.current ? value.selected ? "在下一次发送时使用所选工作。" : "当前轮次继续推进这项工作。" : "可以直接对话，也可以选择一项工作。"));
  body.append(detail(value.pending ? "下轮已选 · 下一轮生效" : "下一候选", value.next, value.pending ? "本轮工作保持不变，下次发送或自动续跑时切换。" : value.next ? "队列中的下一项，可以手动更换。" : "当前没有下一候选。", value.pending));
  const foot = element("footer", "wf-panel-foot"); foot.append(button("", value.current ? "更换工作" : "选择工作", () => openPicker()), button("", "全部工作 ↗", () => openPicker(true)));
  popover.replaceChildren(head, body, foot); popover.showPopover();
  const rect = anchor.getBoundingClientRect(); const box = popover.getBoundingClientRect();
  const view = productDocument.defaultView;
  const left = Math.max(12, Math.min(rect.left, view.innerWidth - box.width - 12));
  const fitsBelow = rect.bottom + box.height + 8 < view.innerHeight - 12;
  const top = fitsBelow ? rect.bottom + 8 : Math.max(12, rect.top - box.height - 8);
  popover.style.left = `${left}px`; popover.style.top = `${top}px`; anchor.setAttribute("aria-expanded", "true");
}

function openPicker(all = false) {
  closePanel(); picker.replaceChildren();
  const head = element("header"); head.append(element("h2", "", all ? "全部工作" : "选择工作"), button("", "×", () => picker.close()));
  head.lastElementChild.setAttribute("aria-label", "关闭工作选择");
  const copy = element("p", "", "示例工作 · 选择只改变原型。运行中更换在下一轮生效。");
  const input = element("input"); input.type = "search"; input.placeholder = "搜索编号或标题"; input.setAttribute("aria-label", input.placeholder);
  const list = element("div", "wf-choice-list");
  const draw = () => {
    list.replaceChildren();
    const value = data();
    for (const entry of examples.filter(item => fullTitle(item).toLowerCase().includes(input.value.toLowerCase()))) {
      const row = button("", "", () => {
        const fromEmpty = model.scenario === "empty" || model.selected?.fromEmpty;
        model.selected = { entry, fromEmpty };
        model.scenario = fromEmpty || entry.id === value.current?.id ? "active" : "switch";
        if (!fromEmpty && entry.id === value.current?.id) model.selected = null;
        $("scenario").value = model.scenario;
        picker.close(); render();
        $("lab-status").textContent = model.scenario === "active" ? `已选择 ${fullTitle(entry)}` : `已选择 ${fullTitle(entry)}，下一轮生效`;
      });
      row.append(element("span", "wf-id", entry.id));
      const text = element("span", "wf-choice-copy"); text.append(element("span", "", entry.title), element("small", "", entry.note)); row.append(text);
      if (entry.id === (value.pending ? value.next?.id : value.current?.id)) row.append(element("span", "wf-check", value.pending ? "下轮已选" : "当前"));
      list.append(row);
    }
    if (!list.childElementCount) list.append(element("p", "wf-choice-empty", "没有匹配的工作"));
  };
  input.addEventListener("input", draw); draw(); picker.append(head, copy, input, list); picker.showModal(); input.focus();
}

function setDesign(design) { model.design = design; model.baseline = false; render(); }
function reflectTheme() {
  document.documentElement.dataset.theme = model.theme;
  $("theme").setAttribute("aria-pressed", String(model.theme === "dark"));
  $("theme").textContent = model.theme === "dark" ? "浅色" : "深色";
}
function syncProductTheme() {
  if (productDocument && productDocument.documentElement.dataset.theme !== model.theme) productDocument.getElementById("theme-toggle").click();
}
for (const tab of document.querySelectorAll("[data-design]")) {
  tab.addEventListener("click", () => setDesign(tab.dataset.design));
  tab.addEventListener("keydown", event => {
    if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    const names = ["a", "b", "c"], index = names.indexOf(model.design);
    const next = event.key === "Home" ? "a" : event.key === "End" ? "c" : names[(index + (event.key === "ArrowRight" ? 1 : 2)) % names.length];
    setDesign(next); $(`tab-${next}`).focus();
  });
}
$("scenario").addEventListener("change", event => { model.scenario = event.target.value; model.selected = null; render(); });
$("baseline").addEventListener("click", () => { model.baseline = !model.baseline; render(); });
$("narrow").addEventListener("click", () => {
  model.narrow = !model.narrow; document.body.dataset.narrow = String(model.narrow);
  $("narrow").setAttribute("aria-pressed", String(model.narrow)); $("narrow").textContent = model.narrow ? "恢复宽屏" : "窄屏预览";
  if (productDocument) closePanel();
});
$("theme").addEventListener("click", () => {
  model.theme = model.theme === "light" ? "dark" : "light";
  reflectTheme(); syncProductTheme();
});

frame.addEventListener("load", async () => {
  await frame.contentWindow.__kzPreview.readyPromise;
  productDocument = frame.contentDocument;
  // Keep the original comparison after the product adopts the composer design.
  originalRail = productDocument.getElementById("project-chat-work").cloneNode(true);
  originalRail.id = "work-focus-prototype-baseline";
  originalRail.className = "work-focus-original";
  originalRail.removeAttribute("role");
  originalRail.querySelector(".work-focus-heading")?.remove();
  productDocument.getElementById("view-chat").append(originalRail);
  oldHeight = originalRail.getBoundingClientRect().height;
  const style = element("link"); style.rel = "stylesheet"; style.href = "/__work-focus/proposal.css";
  const loaded = new Promise((resolve, reject) => { style.addEventListener("load", resolve, { once: true }); style.addEventListener("error", reject, { once: true }); });
  productDocument.head.append(style); await loaded;
  syncProductTheme();
  productDocument.addEventListener("kz:theme", event => { model.theme = event.detail.theme; reflectTheme(); });
  popover = element("div", "wf-panel"); popover.id = "wf-details"; popover.setAttribute("popover", "auto"); popover.setAttribute("role", "dialog"); popover.setAttribute("aria-label", "工作详情");
  popover.addEventListener("toggle", () => { if (anchorForPopover) anchorForPopover.setAttribute("aria-expanded", String(popover.matches(":popover-open"))); });
  picker = element("dialog", "wf-dialog"); picker.setAttribute("aria-label", "选择示例工作");
  productDocument.body.append(popover, picker);
  productDocument.getElementById("tasks-close").click();
  originalRail.addEventListener("click", event => {
    if (!event.target.closest(".work-focus-choose,.work-focus-all")) return;
    event.preventDefault(); event.stopImmediatePropagation(); openPicker(Boolean(event.target.closest(".work-focus-all")));
  }, true);
  frame.contentWindow.addEventListener("resize", render);
  render(); $("loading").hidden = true;
});
