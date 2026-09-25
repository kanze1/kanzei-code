import { defer } from "./01-core.js";
import { $, invoke, readJson, writeJson } from "./01-core.js";
import { t } from "./02-i18n.js";
import {
  activeProcessId,
  activeSessionId,
  currentProject,
  log,
  processItems,
  renderTokens,
  reportPersistentError,
  sessionMetaCache,
  setCtxLimit,
  toast,
  toastError,
} from "./03-shell.js";
import { selectedAgent } from "./08-auto.js";
import { queueProcessUpdate, updateLocalProcessItem } from "./08-compose-runtime.js";
import { refreshProcesses } from "./09-sessions.js";
import { restoreDocFilters } from "./10-docs-core.js";

// ---------- 模型选择:数据层(UI-0926 #3) ----------
// 输入框上方芯片、菜单高亮、tooltip、线路页「跟随默认 · X」的**唯一真源**是后端
// model_effective:它与运行路径复用同一组解析函数,回答「下一轮真正会用哪个模型 /
// 哪一档思考 / Fast mode 开没开,各来自哪一层」。这里只负责按时去问、丢弃迟到的回答、
// 把结果广播给渲染层(08-model-picker.js)。不再从 localStorage 或下拉 DOM 猜。
//
// 三个编辑面各写一层:芯片只写本线(process_update);「项目模型配置」弹窗只写项目文件;
// 设置页只写全局。「设为本项目默认」= 把本线值提升到项目层并清掉本线。

// models_list 探测结果(按项目缓存:同项目内切线不再重复探测,每个 provider 最多 6 秒)。
export let modelCatalog = [];
export let modelCatalogProject = null;
// 当前活动线的 model_effective 结果;null = 还没问到或解析失败(见 effectiveError)。
export let effectiveModel = null;
export let effectiveError = null;
export let effectivePending = false;
// 当前项目下「跟随默认」解析出的模型(线路页空选项显示它)。
export let projectDefaultModel = null;
let effectiveGeneration = 0;
let effectiveProject = null;

function announce(type, detail = null) {
  document.dispatchEvent(new CustomEvent(type, { detail }));
}

// 问一次后端。generation 令牌 + 在途时的项目/线路快照:切线、切项目之后迟到的回答直接丢弃,
// 不会把上一条线的模型画到这一条线上。
export async function refreshEffectiveModel() {
  const generation = ++effectiveGeneration;
  const forProject = currentProject;
  const forProcess = activeProcessId;
  if (!forProject) {
    // 没有项目就没有「下一轮」:清掉旧视图并广播,芯片收成中性占位——不广播的话
    // syncModelSelectToActiveLine 刚立起的在途标记没人撤,芯片会一直停在「…」。
    effectiveModel = null;
    effectiveError = null;
    effectivePending = false;
    effectiveProject = null;
    projectDefaultModel = null;
    announce("kz-effective-model", null);
    return null;
  }
  if (effectiveProject !== forProject) projectDefaultModel = null;
  const { profile, agent } = selectedAgent();
  const stale = () => generation !== effectiveGeneration || currentProject !== forProject || activeProcessId !== forProcess;
  try {
    const view = await invoke("model_effective", { projectDir: forProject, processId: forProcess, profile, agent });
    if (stale()) return null;
    effectiveModel = view;
    effectiveError = null;
    effectivePending = false;
    effectiveProject = forProject;
    projectDefaultModel = view?.defaultModel?.resolved ?? null;
    // 这条线本次还没跑过(没有 kz:meta 可回放,applySessionMeta 已把上限清空):上下文占比
    // 按下一轮会用的模型的上限算,不沿用上一条线的。跑过的线以 kz:meta 为准,这里不碰。
    if (!sessionMetaCache.has(activeSessionId)) {
      setCtxLimit(view?.contextLimit ?? null);
      renderTokens();
    }
    announce("kz-effective-model", view);
    return view;
  } catch (error) {
    if (stale()) return null;
    effectiveModel = null;
    effectiveError = String(error);
    effectivePending = false;
    announce("kz-effective-model", null);
    log(`${t("模型解析失败")}:${error}`, "warn");
    return null;
  }
}

// 芯片 ← 活动线。名字保留(切线/兜底选线/冷启动都调它,parallel-lines-regression 按名字守)。
// 先广播 pending:回答到来之前芯片不能继续把上一条线的值当作「本线临时」显示。
export function syncModelSelectToActiveLine() {
  effectivePending = true;
  announce("kz-effective-model-pending");
  return refreshEffectiveModel();
}

// 只拉目录(失败走持久错误出口,探测不到不等于用不了),再问一次下一轮视图。
export async function loadModels() {
  // R-178 批3:首次进入项目时把 localStorage 旧键上迁后端(幂等,成功后不再执行)。
  void migrateLegacyModelPrefs();
  const forProject = currentProject;
  try {
    const models = await invoke("models_list", { projectDir: forProject });
    if (currentProject === forProject) {
      modelCatalog = Array.isArray(models) ? models : [];
      modelCatalogProject = forProject;
      announce("kz-model-catalog", modelCatalog);
    }
    log(`${t("模型列表已刷新")}(${modelCatalog.length} 个可选)`);
  } catch (err) {
    reportPersistentError(`${t("模型列表获取失败")}:${err}`);
  }
  await refreshEffectiveModel();
}

function optimistic(patch) {
  announce("kz-effective-model-optimistic", patch);
}

// 本线临时覆盖:空串 = 清除(跟随默认)。**必须 await 写入**再问后端——否则后端读到的还是
// 旧的本线值,芯片会闪回上一个模型(冒烟变异 pickerAwaitsUpdate 守这一行)。
export async function setLineModel(value) {
  const processId = activeProcessId;
  if (!processId) return;
  updateLocalProcessItem(processId, { model: value || null });
  optimistic({ model: value || "" });
  try {
    await queueProcessUpdate(processId, { model: value });
  } catch (error) {
    reportPersistentError(`${t("进程模型保存失败")}:${error}`);
  }
  if (activeProcessId === processId) await refreshEffectiveModel();
}

export async function setLineReasoning(value) {
  const processId = activeProcessId;
  if (!processId) return;
  updateLocalProcessItem(processId, { reasoning: value || null });
  optimistic({ reasoning: value || "" });
  try {
    await queueProcessUpdate(processId, { reasoning: value });
  } catch (error) {
    reportPersistentError(`${t("进程思考强度保存失败")}:${error}`);
  }
  if (activeProcessId === processId) await refreshEffectiveModel();
}

// 「设为本项目默认」写哪个键:默认那一项按哪个角色解析就写哪个角色。agent 直指模型时
// 项目层写什么都不影响这条线,不提供这个动作。
export function promotableRole(view = effectiveModel) {
  const role = view?.defaultModel?.role;
  return ["primary", "fast", "compact"].includes(role) ? role : null;
}

export async function promoteLineModelToProject() {
  const view = effectiveModel;
  const role = promotableRole(view);
  const resolved = view?.model?.resolved;
  if (!currentProject || !role || !resolved || view?.model?.source !== "line") return false;
  try {
    await invoke("project_models_save", { projectDir: currentProject, set: { [role]: resolved }, unset: [] });
  } catch (error) {
    toastError(`${t("保存失败")}: ${error}`);
    return false;
  }
  toast(t("已保存到本项目"));
  await setLineModel("");
  announce("kz-model-config-changed", { scope: "project", projectDir: currentProject, silent: true });
  return true;
}

export async function promoteLineReasoningToProject() {
  const view = effectiveModel;
  const value = view?.reasoning?.value;
  if (!currentProject || !value || view?.reasoning?.source !== "line") return false;
  try {
    await invoke("project_models_save", { projectDir: currentProject, set: { reasoning: value }, unset: [] });
  } catch (error) {
    toastError(`${t("保存失败")}: ${error}`);
    return false;
  }
  toast(t("已保存到本项目"));
  await setLineReasoning("");
  announce("kz-model-config-changed", { scope: "project", projectDir: currentProject, silent: true });
  return true;
}

// 手填模型:provider:model 直指。有些 OpenAI 兼容端点不提供 /models,
// 或者 key 尚未配好导致探测为空,这条通道保证配了 provider 就一定能用。
export const MANUAL_MODEL_SENTINEL = "__manual__";
export const MANUAL_MODEL_PATTERN = /^[\w.-]+:.+$/;
// R-178 批3:localStorage 旧键一次性上迁后端(②层),前端不再以 localStorage 为真源。
// 旧键形态:`kz-model`(更早的全局键)、`kz-model:<project>`(R-115 项目级)、
// `kz-manual-models:<project>`(手填候选)。迁移成功后旧键即清除;失败保留旧键,下次重试。
// UI-0926 #3:`kz-reasoning:<project>` 曾是思考强度的显示真源(从不跟线同步),一并清掉。
export function legacyModelPrefValue() {
  return localStorage.getItem(prefKey("model")) ?? localStorage.getItem("kz-model") ?? "";
}
export function legacyManualModels() {
  const list = readJson(prefKey("manual-models"), []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
export async function migrateLegacyModelPrefs() {
  if (!currentProject) return;
  localStorage.removeItem(prefKey("reasoning"));
  const legacyModel = legacyModelPrefValue();
  const legacyManual = legacyManualModels();
  if (!legacyModel && legacyManual.length === 0) return;
  const defaultProcess = processItems.find((item) => item.id.startsWith("d|"));
  if (!defaultProcess) return; // 默认进程尚未就绪,待 process_list 后由 loadModels 再触发
  const patch = {};
  if (legacyModel) patch.model = legacyModel;
  if (legacyManual.length > 0) patch.manualModels = legacyManual;
  try {
    await invoke("process_update", { processId: defaultProcess.id, ...patch });
    localStorage.removeItem(prefKey("model"));
    localStorage.removeItem("kz-model");
    localStorage.removeItem(prefKey("manual-models"));
    log(`${t("已迁移旧模型偏好到后端")}:${JSON.stringify(patch)}`);
  } catch (error) {
    reportPersistentError(`${t("旧模型偏好迁移失败")}:${error}`);
  }
}
export function manualModels() {
  const legacy = legacyManualModels();
  const list = legacy.length > 0 ? legacy : (processItems.find((item) => item.id.startsWith("d|"))?.manual_models ?? []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
export function addManualModel(id) {
  const list = manualModels();
  if (!list.includes(id)) list.push(id);
  const defaultProcess = processItems.find((item) => item.id.startsWith("d|"));
  if (defaultProcess) {
    updateLocalProcessItem(defaultProcess.id, { manual_models: list });
    return queueProcessUpdate(defaultProcess.id, { manualModels: list })
      .then(() => refreshProcesses())
      .catch((error) => reportPersistentError(`${t("手填模型保存失败")}:${error}`));
  }
  // 默认进程未就绪(极端时序),退回 localStorage 暂存,由迁移函数下次接手。
  writeJson(prefKey("manual-models"), list);
  return Promise.resolve();
}
// R-115:按项目记的界面偏好(筛选、交付方式)。模型与思考强度已经是按线存在后端的状态。
export function prefKey(name) {
  return `kz-${name}:${currentProject || "default"}`;
}
export function restoreProjectPrefs() {
  const delivery = localStorage.getItem("kz-delivery");
  const deliverySelect = $("delivery-select");
  if (delivery && [...deliverySelect.options].some((o) => o.value === delivery)) {
    deliverySelect.value = delivery;
  }
  restoreDocFilters();
}

// ---------- 刷新触发点(只由事件触发,不轮询) ----------
// 模式芯片换人格 = 换 agent,agent 定义里的模型可能不同。
defer(() => {
  $("profile-select")?.addEventListener("change", () => void syncModelSelectToActiveLine());
});
// 项目模型配置弹窗 / 设置页保存之后:项目层只影响解析,重问一次;全局保存可能改了 provider,目录也重拉。
defer(() => {
  document.addEventListener("kz-model-config-changed", (event) => {
    if (event?.detail?.scope === "global") void loadModels();
    else void refreshEffectiveModel();
  });
});
// 配置文件可能在外部编辑器里改过:窗口回到前台时重问一次(2 秒节流)。
let lastFocusRefresh = 0;
defer(() => {
  window.addEventListener("focus", () => {
    const now = Date.now();
    if (now - lastFocusRefresh < 2000 || !currentProject) return;
    lastFocusRefresh = now;
    void refreshEffectiveModel();
  });
});
