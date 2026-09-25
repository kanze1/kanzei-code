import { openMenu } from "./00-surface.js";
import { defer } from "./01-core.js";
import { $, inputDialog } from "./01-core.js";
import { t } from "./02-i18n.js";
import { activeProcessId, currentProject, navigate_view, processItems, reasoningLabel, toast } from "./03-shell.js";
import {
  MANUAL_MODEL_PATTERN,
  addManualModel,
  effectiveError,
  effectiveModel,
  effectivePending,
  manualModels,
  modelCatalog,
  modelCatalogProject,
  promotableRole,
  promoteLineModelToProject,
  promoteLineReasoningToProject,
  setLineModel,
  setLineReasoning,
} from "./08-models.js";
import { openProjectModelsDialog } from "./08-project-models.js";

// ---------- 输入框上方的模型 / 思考芯片(UI-0926 #3) ----------
// 芯片文字 = 下一轮**真正**会用的模型与思考档 + 来源标签(临时/项目/全局/内置/Agent),
// 菜单高亮与芯片同出一个对象(08-models.js 的 effectiveModel,来自后端 model_effective)。
// 菜单一律走 00-surface 的 openMenu(弹层唯一写法),不再用原生 select:原生下拉的高亮
// 跟着鼠标走,不代表「当前生效」,暗色主题下还会弹出白底列表。

export const REASONING_LEVELS = ["off", "none", "low", "medium", "high", "xhigh", "max"];
const ROLE_IDS = new Set(["primary", "fast", "compact"]);

export function sourceLabel(source) {
  return {
    line: t("临时"),
    project: t("项目级"),
    global: t("全局"),
    builtin: t("内置"),
    agent: t("Agent"),
  }[source] ?? "";
}

function sourceHint(source) {
  return {
    line: t("仅本线生效,清除后跟随项目/全局默认"),
    project: t("来自本项目的模型配置"),
    global: t("来自全局默认(设置页)"),
    builtin: t("未配置,使用内置默认"),
    agent: t("由当前 Agent 定义指定"),
  }[source] ?? "";
}

export { reasoningLabel };

// 本地乐观值与在途标记:选完菜单到后端回答之间的那一小段。
let pending = false;
let optimisticPatch = null;

function chip({ label, source = null, pendingState = false, error = false, title = "", aria = "", fast = null }) {
  return { label, source, pending: pendingState, error, title, aria: aria || title || label, fast };
}

// 还没打开任何项目:没有「下一轮」可言。芯片给中性占位,不挂在途态(否则会永远停在「…」)。
function idleChip(label) {
  return chip({ label, source: null, title: t("先在左侧「项目」里添加并选择一个目录") });
}

// 模型芯片的文字/来源/提示。纯函数,冒烟直接调。
export function modelChipText(view, { pending: isPending = false, error = null, optimistic = null, noProject = false } = {}) {
  if (noProject) return idleChip(t("模型"));
  if (optimistic && typeof optimistic.model === "string" && optimistic.model) {
    const text = `${t("下一轮将使用")} ${optimistic.model} · ${t("来源")} ${sourceLabel("line")}`;
    return chip({ label: optimistic.model, source: "line", pendingState: true, title: text });
  }
  if (!view) {
    if (error) return chip({ label: t("模型解析失败"), source: null, error: true, title: error });
    return chip({ label: t("模型"), source: "pending", pendingState: true, title: t("正在确认下一轮使用的模型…") });
  }
  const model = view.model ?? {};
  if (isPending || (optimistic && optimistic.model === "")) {
    // 切线/清除覆盖的在途期:保留上一次的文字但不再声称来源,免得把上一条线的临时值当成这一条的。
    return chip({ label: model.resolved ?? t("模型"), source: "pending", pendingState: true, title: t("正在确认下一轮使用的模型…") });
  }
  if (!model.resolved) {
    return chip({ label: t("模型解析失败"), source: model.source, error: true, title: model.error ?? "" });
  }
  const follows = model.followsPrimary ? ` · ${t("跟随 primary")}` : "";
  const title = `${t("下一轮将使用")} ${model.resolved} · ${t("来源")} ${sourceLabel(model.source)}${follows}\n${sourceHint(model.source)}`;
  const fastMode = view.codexFastMode;
  const fast = fastMode?.active
    ? { title: `Codex Fast mode ${t("已开启")} · ${sourceLabel(fastMode.source)}` }
    : null;
  return chip({
    label: model.resolved,
    source: model.source,
    title: fast ? `${title}\n${fast.title}` : title,
    aria: `${t("下一轮将使用")} ${model.resolved},${t("来源")} ${sourceLabel(model.source)}`,
    fast,
  });
}

export function reasoningChipText(view, { pending: isPending = false, optimistic = null, noProject = false } = {}) {
  const prefix = t("思考");
  if (noProject) return idleChip(prefix);
  if (optimistic && typeof optimistic.reasoning === "string" && optimistic.reasoning) {
    const label = `${prefix} ${reasoningLabel(optimistic.reasoning)}`;
    return chip({ label, source: "line", pendingState: true, title: label });
  }
  if (!view?.reasoning) return chip({ label: `${prefix} …`, source: "pending", pendingState: true, title: t("思考强度(仅推理模型有效;越高越慢越贵)") });
  if (isPending || (optimistic && optimistic.reasoning === "")) {
    return chip({ label: `${prefix} ${reasoningLabel(view.reasoning.value)}`, source: "pending", pendingState: true, title: t("正在确认下一轮使用的模型…") });
  }
  const { value, source } = view.reasoning;
  const label = `${prefix} ${reasoningLabel(value)}`;
  const title = `${t("下一轮将使用")} ${label} · ${t("来源")} ${sourceLabel(source)}\n${sourceHint(source)}\n${t("思考强度(仅推理模型有效;越高越慢越贵)")}`;
  return chip({ label, source, title, aria: `${label},${t("来源")} ${sourceLabel(source)}` });
}

function fillChip(button, spec) {
  if (!button) return;
  const label = document.createElement("span");
  label.className = "picker-label";
  label.textContent = spec.label;
  const parts = [label];
  if (spec.source) {
    const tag = document.createElement("span");
    tag.className = "picker-source";
    tag.dataset.source = spec.source;
    tag.textContent = spec.source === "pending" ? "…" : sourceLabel(spec.source);
    parts.push(tag);
  }
  if (spec.fast) {
    const fast = document.createElement("span");
    fast.className = "picker-fast";
    fast.textContent = "⚡";
    fast.setAttribute("aria-hidden", "true");
    parts.push(fast);
  }
  button.replaceChildren(...parts);
  button.dataset.source = spec.source ?? "";
  button.classList.toggle("is-pending", Boolean(spec.pending));
  button.classList.toggle("is-error", Boolean(spec.error));
  button.setAttribute("aria-busy", spec.pending ? "true" : "false");
  button.setAttribute("aria-label", spec.aria);
  button.title = spec.title;
}

export function renderModelPicker() {
  const state = { pending: pending || effectivePending, error: effectiveError, optimistic: optimisticPatch, noProject: !currentProject };
  fillChip($("model-picker"), modelChipText(effectiveModel, state));
  fillChip($("reasoning-picker"), reasoningChipText(effectiveModel, state));
}

function fastModeHeading(view) {
  const fast = view?.codexFastMode;
  if (!fast) return "⚡ Codex Fast mode";
  if (!fast.applies) return `⚡ Codex Fast mode · ${t("不适用(当前模型不是 Codex)")}`;
  return `⚡ Codex Fast mode · ${fast.enabled ? t("开") : t("关")} · ${sourceLabel(fast.source)}`;
}

// 模型菜单的项(纯函数,冒烟直接调)。选择项带 value(""=跟随默认),动作项带 action。
// aria-checked=true 恰好一项:本线有临时覆盖时是那一项,否则是「跟随默认」——与芯片来源同出 view。
export function buildModelPickerItems(view, catalog = [], manual = [], showAll = false, lineModel = "") {
  const model = view?.model;
  const fallback = view?.defaultModel;
  const onLine = model?.source === "line";
  const line = onLine ? String(lineModel || model?.ref || "") : "";
  const items = [];
  const defaultDesc = fallback?.resolved
    ? `${fallback.resolved} · ${sourceLabel(fallback.source)}${fallback.followsPrimary ? ` · ${t("跟随 primary")}` : ""}`
    : `${t("模型解析失败")}${fallback?.error ? ` · ${fallback.error}` : ""}`;
  items.push({ label: t("跟随默认"), desc: defaultDesc, checked: !onLine, value: "" });
  items.push("separator");
  items.push({ heading: t("指定模型(仅本线)") });
  const seen = new Set();
  const addChoice = (id, desc) => {
    if (!id || seen.has(id)) return;
    seen.add(id);
    items.push({ label: id, desc, checked: onLine && line === id, value: id });
  };
  // 旧版按角色存下的本线值(例如 fast):单列一项并选中,方便一键清除。
  if (onLine && ROLE_IDS.has(line)) {
    seen.add(line);
    items.push({ label: `${line} → ${model?.resolved ?? "?"}`, desc: t("旧版按角色保存的本线值"), checked: true, value: line });
  }
  const direct = (catalog || []).map((entry) => entry?.id).filter((id) => id && !ROLE_IDS.has(id));
  if (showAll) for (const id of direct) addChoice(id);
  else addChoice(fallback?.resolved);
  if (onLine && !ROLE_IDS.has(line)) addChoice(line);
  for (const id of manual || []) addChoice(id, t("手填"));
  if (!showAll && direct.some((id) => !seen.has(id))) items.push({ label: t("显示全部探测模型…"), action: "show-all" });
  items.push({ label: t("＋ 手填模型…"), action: "manual" });
  items.push("separator");
  items.push({ heading: fastModeHeading(view) });
  items.push("separator");
  const role = promotableRole(view);
  if (onLine && role && model?.resolved) {
    items.push({ label: t("设为本项目默认"), desc: `${role} = ${model.resolved}`, action: "promote" });
  }
  items.push({ label: t("项目模型配置…"), action: "project" });
  items.push({ label: t("全局默认…"), action: "global" });
  return items;
}

export function buildReasoningPickerItems(view) {
  const current = view?.reasoning;
  const fallback = view?.defaultReasoning;
  const onLine = current?.source === "line";
  const items = [];
  items.push({
    label: t("跟随默认"),
    desc: fallback ? `${reasoningLabel(fallback.value)} · ${sourceLabel(fallback.source)}` : "",
    checked: !onLine,
    value: "",
  });
  items.push("separator");
  items.push({ heading: t("指定档位(仅本线)") });
  for (const level of REASONING_LEVELS) {
    items.push({ label: reasoningLabel(level), checked: onLine && current?.value === level, value: level });
  }
  items.push("separator");
  if (onLine) items.push({ label: t("设为本项目默认"), desc: `reasoning = ${reasoningLabel(current.value)}`, action: "promote" });
  items.push({ label: t("项目模型配置…"), action: "project" });
  return items;
}

function openGlobalDefaults() {
  navigate_view("settings");
  const group = $("settings-models-group");
  if (group) group.open = true;
  group?.scrollIntoView?.({ block: "start" });
}

async function chooseManualModel() {
  const input = ((await inputDialog({
    title: t("填 provider:model,例如 deepseek:deepseek-chat"),
  })) || "").trim();
  // provider 名必须对得上配置里的键,否则后端 resolve_model 会直接失败。
  if (!MANUAL_MODEL_PATTERN.test(input)) {
    if (input) toast(t("格式应为 provider:model"));
    return;
  }
  await addManualModel(input);
  await setLineModel(input);
}

async function choose(kind, spec) {
  if (spec.action === "show-all") {
    openPicker("model", { showAll: true });
    return;
  }
  if (spec.action === "manual") return chooseManualModel();
  if (spec.action === "promote") {
    if (kind === "reasoning") await promoteLineReasoningToProject();
    else await promoteLineModelToProject();
    return;
  }
  if (spec.action === "project") {
    await openProjectModelsDialog(currentProject);
    return;
  }
  if (spec.action === "global") {
    openGlobalDefaults();
    return;
  }
  if (kind === "reasoning") await setLineReasoning(spec.value);
  else await setLineModel(spec.value);
}

let menuHandle = null;
export function closePicker() {
  menuHandle?.close?.();
  menuHandle = null;
}

// 打开(或再点一次收起)某个芯片的菜单。菜单向上弹,左对齐芯片;打开后焦点落在当前生效项上。
export function openPicker(kind, { showAll = false } = {}) {
  const anchor = $(kind === "reasoning" ? "reasoning-picker" : "model-picker");
  if (!anchor) return null;
  const view = effectiveModel;
  const line = processItems.find((item) => item.id === activeProcessId);
  const catalog = modelCatalogProject === currentProject ? modelCatalog : [];
  const specs = kind === "reasoning"
    ? buildReasoningPickerItems(view)
    : buildModelPickerItems(view, catalog, manualModels(), showAll, line?.model ?? "");
  const items = specs.map((spec) => (typeof spec === "string" || spec.heading
    ? spec
    : { ...spec, onSelect: () => void choose(kind, spec) }));
  const handle = openMenu(anchor, items, {
    placement: "top-start",
    label: kind === "reasoning" ? t("思考强度") : t("模型"),
    onClose: () => {
      if (menuHandle === handle) menuHandle = null;
    },
  });
  if (!handle || handle.closed) {
    menuHandle = null;
    return null;
  }
  menuHandle = handle;
  const menu = handle.el;
  menu.classList.add("picker-menu");
  menu.dataset.picker = kind;
  const actionable = specs.filter((spec) => spec && typeof spec === "object" && !spec.heading);
  [...menu.querySelectorAll(".k-menu-item")].forEach((button, index) => {
    // 左侧勾选列:✓ 只跟「当前生效」那一项走。悬停/焦点只有中性底色,与生效项的底色只差几个百分点,
    // 光靠底色分不出「鼠标在哪」和「哪项在生效」——这正是原生下拉被换掉的原因。
    const check = document.createElement("span");
    check.className = "picker-check";
    check.setAttribute("aria-hidden", "true");
    check.textContent = button.getAttribute("aria-checked") === "true" ? "✓" : "";
    button.prepend(check);
    const spec = actionable[index];
    if (!spec) return;
    if (spec.value !== undefined) button.dataset.value = spec.value;
    if (spec.action) button.dataset.action = spec.action;
  });
  // 分组标题与菜单项的文字对齐(让出勾选列的宽度)。标题的内边距归 surface.css,这里只垫一个占位。
  for (const heading of menu.querySelectorAll(".k-menu-heading")) {
    const indent = document.createElement("span");
    indent.className = "picker-indent";
    indent.setAttribute("aria-hidden", "true");
    heading.prepend(indent);
  }
  menu.querySelector('[aria-checked="true"]')?.focus?.();
  return handle;
}

defer(() => {
  for (const [id, kind] of [["model-picker", "model"], ["reasoning-picker", "reasoning"]]) {
    const button = $(id);
    if (!button) continue;
    // 菜单按钮语义(index.html 里也写了;这里再落一次,00-surface 的 aria-expanded 同步认它)。
    button.setAttribute("aria-haspopup", "menu");
    button.setAttribute("aria-expanded", "false");
    button.addEventListener("click", () => openPicker(kind));
    button.addEventListener("keydown", (event) => {
      if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
      event.preventDefault?.();
      if (!menuHandle) openPicker(kind);
    });
  }
  document.addEventListener("kz-effective-model", () => {
    pending = false;
    optimisticPatch = null;
    renderModelPicker();
  });
  document.addEventListener("kz-effective-model-pending", () => {
    pending = true;
    // 切线/换人格:上一条线还在途的乐观值作废——否则 A 线刚选的「X · 临时」会挂到 B 线芯片上,
    // 直到 B 线的 model_effective 回来(变异 optimisticDropsOnSwitch 守这一行)。
    optimisticPatch = null;
    renderModelPicker();
  });
  document.addEventListener("kz-effective-model-optimistic", (event) => {
    optimisticPatch = { ...(optimisticPatch ?? {}), ...(event?.detail ?? {}) };
    renderModelPicker();
  });
  // 切界面语言后芯片文字跟着换(setLanguagePreference 挂在同一个 change 上,先于这里执行)。
  $("language-select")?.addEventListener("change", () => renderModelPicker());
  renderModelPicker();
});
