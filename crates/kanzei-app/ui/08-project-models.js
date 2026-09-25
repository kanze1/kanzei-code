import { closeSurface, isSurfaceOpen, openDialog } from "./00-surface.js";
import { defer } from "./01-core.js";
import { $, inputDialog, invoke } from "./01-core.js";
import { t } from "./02-i18n.js";
import { currentProject, reasoningLabel, toast, toastError } from "./03-shell.js";
import { MANUAL_MODEL_PATTERN, MANUAL_MODEL_SENTINEL, manualModels, modelCatalog, modelCatalogProject } from "./08-models.js";

// ---------- 项目模型配置弹窗(UI-0926 #3) ----------
// 只写一层:项目 .kanzei/kanzei.toml 的 [models] 五个键。每个字段要么「继承全局 · X」,
// 要么「项目覆盖」,可逐键「恢复继承」;保存只提交变动的键(project_models_save 的 set/unset),
// 没动过的键一个字节都不碰。与设置页(只写全局)、输入框芯片(只写本线)互不越界。
// 不 import 08-model-picker.js:芯片菜单会打开这里,反过来就成了循环依赖。

export const INHERIT = "__inherit__";
const FIELD_KEYS = ["primary", "fast", "compact", "reasoning", "codexFastMode"];
const MODEL_KEYS = new Set(["primary", "fast", "compact"]);
const ROLE_IDS = new Set(["primary", "fast", "compact"]);
const REASONING_LEVELS = ["off", "none", "low", "medium", "high", "xhigh", "max"];

function toDraftValue(key, value) {
  if (value === null || value === undefined || value === "") return INHERIT;
  if (key === "codexFastMode") return value ? "on" : "off";
  return String(value);
}

// 加载值 vs 草稿 → 只含变动键的补丁。变成「继承」的进 unset,其余进 set。纯函数,冒烟直接调。
export function diffProjectModels(loaded, draft) {
  const set = {};
  const unset = [];
  for (const key of FIELD_KEYS) {
    const before = toDraftValue(key, loaded?.[key]);
    const after = draft?.[key] ?? before;
    if (after === before) continue;
    if (after === INHERIT) unset.push(key);
    else set[key] = key === "codexFastMode" ? after === "on" : after;
  }
  return { set, unset };
}

function fieldTitle(key) {
  return {
    primary: ["primary", t("主循环")],
    fast: ["fast", t("子代理/杂活")],
    compact: ["compact", t("压缩纪要")],
    reasoning: [t("思考强度"), ""],
    codexFastMode: ["Codex Fast mode", t("仅对 Codex 生效")],
  }[key];
}

function displayValue(key, value) {
  if (key === "reasoning") return reasoningLabel(value);
  if (key === "codexFastMode") return value ? t("开") : t("关");
  return value ?? "—";
}

function inheritLabel(key, field) {
  const value = displayValue(key, field?.inherited);
  if (field?.inheritedFollowsPrimary) return `${t("跟随 primary")} · ${value}`;
  return field?.inheritedSource === "global" ? `${t("继承全局")} · ${value}` : `${t("继承内置默认")} · ${value}`;
}

function option(value, label) {
  const node = document.createElement("option");
  node.value = value;
  node.textContent = label;
  return node;
}

// 模型行的选项:继承 → 当前项目值 → 探测目录里的直指模型 → 手填过的 → 「＋ 手填模型…」。
function fillModelOptions(select, key, field, catalogIds, keep) {
  const nodes = [option(INHERIT, inheritLabel(key, field))];
  const seen = new Set([INHERIT]);
  const add = (id, label = id) => {
    if (!id || seen.has(id)) return;
    seen.add(id);
    nodes.push(option(id, label));
  };
  add(field?.project ?? "");
  for (const id of catalogIds) add(id);
  for (const id of manualModels()) add(id, `${id}(${t("手填")})`);
  if (keep && keep !== INHERIT) add(keep, `${keep}(${t("手填")})`);
  nodes.push(option(MANUAL_MODEL_SENTINEL, t("＋ 手填模型…")));
  select.replaceChildren(...nodes);
  select.value = keep;
}

function fillFixedOptions(select, key, field, keep) {
  const nodes = [option(INHERIT, inheritLabel(key, field))];
  if (key === "reasoning") {
    for (const level of REASONING_LEVELS) nodes.push(option(level, reasoningLabel(level)));
  } else {
    nodes.push(option("on", t("开")), option("off", t("关")));
  }
  select.replaceChildren(...nodes);
  select.value = keep;
}

let dialogState = null;

function syncRow(row) {
  const select = row.querySelector("select");
  const state = row.querySelector(".pm-state");
  const reset = row.querySelector(".pm-reset");
  const inherit = select.value === INHERIT;
  state.dataset.state = inherit ? "inherit" : "override";
  // 继承时按这一键真正的回落说话:跟随 primary / 内置默认 / 全局。
  const field = row._field;
  state.textContent = !inherit
    ? t("项目覆盖")
    : field?.inheritedFollowsPrimary
      ? t("跟随 primary")
      : field?.inheritedSource === "builtin"
        ? t("继承内置默认")
        : t("继承全局");
  reset.hidden = inherit;
  row.dataset.state = state.dataset.state;
}

function renderRow(key, field, catalogIds) {
  const row = document.createElement("div");
  row.className = "pm-row";
  row.dataset.key = key;
  row._field = field;
  const head = document.createElement("div");
  head.className = "pm-head";
  const [name, desc] = fieldTitle(key);
  const label = document.createElement("label");
  label.className = "pm-label";
  label.htmlFor = `pm-${key}`;
  label.textContent = name;
  if (desc) {
    const hint = document.createElement("span");
    hint.className = "dim";
    hint.textContent = ` ${desc}`;
    label.appendChild(hint);
  }
  const state = document.createElement("span");
  state.className = "pm-state";
  const reset = document.createElement("button");
  reset.type = "button";
  reset.className = "link-btn pm-reset";
  reset.textContent = t("恢复继承");
  head.append(label, state, reset);
  const select = document.createElement("select");
  select.id = `pm-${key}`;
  select.className = "pm-control";
  const keep = toDraftValue(key, field?.project);
  if (MODEL_KEYS.has(key)) fillModelOptions(select, key, field, catalogIds, keep);
  else fillFixedOptions(select, key, field, keep);
  let last = select.value;
  select.addEventListener("change", async () => {
    if (select.value === MANUAL_MODEL_SENTINEL) {
      const input = ((await inputDialog({
        title: t("填 provider:model,例如 deepseek:deepseek-chat"),
      })) || "").trim();
      if (!MANUAL_MODEL_PATTERN.test(input)) {
        if (input) toast(t("格式应为 provider:model"));
        select.value = last;
        syncRow(row);
        return;
      }
      const node = option(input, `${input}(${t("手填")})`);
      select.insertBefore(node, select.lastElementChild ?? null);
      select.value = input;
    }
    last = select.value;
    syncRow(row);
  });
  reset.addEventListener("click", () => {
    select.value = INHERIT;
    last = INHERIT;
    syncRow(row);
    select.focus?.();
  });
  row.append(head, select);
  syncRow(row);
  return row;
}

function draftValues(selects) {
  const draft = {};
  for (const key of FIELD_KEYS) {
    const value = selects?.[key]?.value;
    if (value !== undefined && value !== MANUAL_MODEL_SENTINEL) draft[key] = value;
  }
  return draft;
}

function catalogIdsFrom(models) {
  return (models ?? []).map((entry) => entry?.id).filter((id) => id && !ROLE_IDS.has(id));
}

// 目录探测可能要几秒(每个 provider 最多 6 秒):弹窗先用已知目录打开,探测回来再只补选项、不动草稿。
async function refreshCatalog(projectDir, token) {
  let models;
  try {
    models = await invoke("models_list", { projectDir });
  } catch {
    return;
  }
  if (dialogState?.token !== token) return;
  const ids = catalogIdsFrom(models);
  for (const key of ["primary", "fast", "compact"]) {
    const select = dialogState.selects?.[key];
    const row = select?.closest?.(".pm-row");
    if (!select || !row) continue;
    fillModelOptions(select, key, dialogState.view.fields?.[key], ids, select.value);
    syncRow(row);
  }
}

function projectLabelFor(projectDir, name) {
  if (name) return name;
  if (projectDir === currentProject) {
    const label = $("project-label")?.textContent?.trim();
    if (label) return label;
  }
  return String(projectDir).split(/[\\/]/).filter(Boolean).pop() ?? projectDir;
}

export async function openProjectModelsDialog(projectDir = currentProject, { name = null } = {}) {
  const overlay = $("project-models-overlay");
  if (!projectDir || !overlay) return null;
  // 已经开着(连点了两次链接):不重建,免得把正在编辑的草稿冲掉。
  if (isSurfaceOpen(overlay)) return dialogState?.handle ?? null;
  let view;
  try {
    view = await invoke("project_models_get", { projectDir });
  } catch (error) {
    toastError(`${t("项目模型配置读取失败")}:${error}`);
    return null;
  }
  const cached = modelCatalogProject === projectDir;
  const token = {};
  dialogState = {
    token,
    projectDir,
    view,
    loaded: Object.fromEntries(FIELD_KEYS.map((key) => [key, view?.fields?.[key]?.project ?? null])),
  };
  $("project-models-name").textContent = projectLabelFor(projectDir, name);
  $("project-models-path").textContent = view?.configPath ?? "";
  $("project-models-open").disabled = !view?.exists;
  const ids = cached ? catalogIdsFrom(modelCatalog) : [];
  const rows = FIELD_KEYS.map((key) => renderRow(key, view?.fields?.[key], ids));
  // 控件引用直接留在状态里,不靠 getElementById 回查动态节点。
  dialogState.selects = Object.fromEntries(rows.map((row) => [row.dataset.key, row.querySelector("select")]));
  $("project-models-rows").replaceChildren(...rows);
  const handle = openDialog(overlay, {
    initialFocus: dialogState.selects.primary,
    onClose: () => {
      if (dialogState?.token === token) dialogState = null;
    },
  });
  dialogState.handle = handle;
  if (!cached) void refreshCatalog(projectDir, token);
  return handle;
}

async function saveProjectModels() {
  const state = dialogState;
  if (!state) return;
  const { set, unset } = diffProjectModels(state.loaded, draftValues(state.selects));
  if (!Object.keys(set).length && !unset.length) {
    closeSurface(state.handle ?? $("project-models-overlay"));
    return;
  }
  const button = $("project-models-save");
  button.disabled = true;
  try {
    await invoke("project_models_save", { projectDir: state.projectDir, set, unset });
  } catch (error) {
    toastError(`${t("保存失败")}: ${error}`);
    return;
  } finally {
    button.disabled = false;
  }
  toast(t("已保存到本项目"));
  closeSurface(state.handle ?? $("project-models-overlay"));
  document.dispatchEvent(new CustomEvent("kz-model-config-changed", { detail: { scope: "project", projectDir: state.projectDir } }));
}

defer(() => {
  $("project-models-save")?.addEventListener("click", () => void saveProjectModels());
  $("project-models-cancel")?.addEventListener("click", () => {
    const overlay = $("project-models-overlay");
    if (isSurfaceOpen(overlay)) closeSurface(overlay);
  });
  $("project-models-open")?.addEventListener("click", async () => {
    const projectDir = dialogState?.projectDir;
    if (!projectDir) return;
    try {
      await invoke("project_config_open", { projectDir });
    } catch (error) {
      toastError(String(error));
    }
  });
});
