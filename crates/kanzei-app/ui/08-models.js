// ---------- 模型直选 ----------
const SHOW_ALL_MODELS_SENTINEL = "__show_all_models__";
let showAllModels = false;
async function loadModels({ showAll = false } = {}) {
  showAllModels = showAll;
  const select = $("model-select");
  // R-178 批3:首次进入项目时把 localStorage 旧键上迁后端(幂等,成功后不再执行)。
  void migrateLegacyModelPrefs();
  // 顶栏回显的**唯一**来源是活动线自己存的模型。原来是 `activeModel || 旧全局键`:
  // 一条没设过模型的线(model=null,即 agent 默认)会回落到 localStorage 的项目级/全局
  // 旧键,于是每条这样的线都显示同一个模型——用户看到的就是「切线路模型不变」。更糟的是
  // 发送读的是这个下拉(见 sendText),而鞭挞续跑读的是 item.model:同一条线上手动发和自动
  // 轮能用两个不同的模型。旧键只在**还不知道活动线是谁**(进程列表未到/迁移前)时才作数。
  const activeItem = processItems.find((item) => item.id === activeProcessId);
  const saved = activeItem ? activeItem.model || "" : legacyModelPrefValue();
  const selectedIds = new Set([saved, ...manualModels()].filter(Boolean));
  select.innerHTML = "";
  const def = document.createElement("option");
  def.value = "";
  def.textContent = t("模型:agent 默认");
  select.appendChild(def);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    const ids = new Set(models.map((m) => m.id));
    // 顶栏默认只展示已选/已记住的模型和两个角色入口；完整探测清单仍可按需展开。
    const visibleModels = showAll
      ? models
      : models.filter((m) => ["primary", "fast"].includes(m.id) || selectedIds.has(m.id));
    for (const m of visibleModels) {
      const opt = document.createElement("option");
      opt.value = m.id;
      opt.textContent = m.label;
      if (m.id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    // 当前线路/项目记住的直指模型即使未被 /models 返回，也必须保留为可见选项。
    // 这是 DeepSeek 等端点探测失败时仍能继续使用的关键兜底。
    for (const id of selectedIds) {
      if (ids.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(${t("已记住")})`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    if (!showAll && models.some((m) => !visibleModels.includes(m))) {
      const all = document.createElement("option");
      all.value = SHOW_ALL_MODELS_SENTINEL;
      all.textContent = t("显示全部探测模型…");
      select.appendChild(all);
    }
    // D-167:探测不到不等于用不了——端点可能没实现 /models,key 也可能还没配好。
    // 手填过的模型要留在列表里,否则下次重开又得再填一遍。
    for (const id of manualModels()) {
      if (ids.has(id) || selectedIds.has(id)) continue;
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = `${id}(${t("手填")})`;
      if (id === saved) opt.selected = true;
      select.appendChild(opt);
    }
    const custom = document.createElement("option");
    custom.value = MANUAL_MODEL_SENTINEL;
    custom.textContent = t("＋ 手填模型…");
    select.appendChild(custom);
    log(`${t("模型列表已刷新")}(${models.length} 个可选)`);
  } catch (err) {
    reportPersistentError(`${t("模型列表获取失败")}:${err}`);
  }
  // 显式落值,不靠上面那些 opt.selected:选项是整棵重建的(innerHTML=""),探测失败时
  // try 块中途退出,回显就停在上一条线的值。这一行保证「列表画成什么样,值都是本线的」。
  select.value = saved;
}

// 顶栏模型下拉 ← 活动线。原来只有 switchProcess 尾巴上那一句在做这件事,于是冷启动、
// 切项目、以及 renderProcesses 兜底选中活动线(线路被回收后)这三条路径全都不回显——
// 下拉停在上一条线的值,而用户以为那就是当前线的模型。
function syncModelSelectToActiveLine() {
  const select = $("model-select");
  if (!select) return;
  const item = processItems.find((candidate) => candidate.id === activeProcessId);
  if (!item) return;
  const value = item.model || "";
  if (select.value === value) return;
  // 该线的直指模型可能不在当前选项里(探测清单变了/刚换项目):补一个再选,
  // 否则赋值会被浏览器静默丢弃、回落成空(D-167 同源)。
  if (value && ![...select.options].some((option) => option.value === value)) {
    select.appendChild(new Option(`${value}(${t("已记住")})`, value));
  }
  select.value = value;
}

// 手填模型:provider:model 直指。有些 OpenAI 兼容端点不提供 /models,
// 或者 key 尚未配好导致探测为空,这条通道保证配了 provider 就一定能用。
const MANUAL_MODEL_SENTINEL = "__manual__";
// R-178 批3:localStorage 旧键一次性上迁后端(②层),前端不再以 localStorage 为真源。
// 旧键形态:`kz-model`(更早的全局键)、`kz-model:<project>`(R-115 项目级)、
// `kz-manual-models:<project>`(手填候选)。保留旧键 fallback 一个版本——迁移执行前
// 旧值仍可读(legacyModelPrefValue/legacyManualModels),迁移成功后旧键即清除。
// 首次进入项目时把 localStorage 旧键上迁到默认进程(②层持久选择)并清除旧键。
// 幂等:旧键不存在时直接返回;迁移失败保留旧键,下次 loadModels 再试。不设一次性
// 标志——「旧键清除后自然不再迁移」就是幂等,也让失败可重试(保留旧键 fallback
// 一个版本:迁移函数保留到下一大版本再删)。
function legacyModelPrefValue() {
  return localStorage.getItem(prefKey("model")) ?? localStorage.getItem("kz-model") ?? "";
}
function legacyManualModels() {
  const list = readJson(prefKey("manual-models"), []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
async function migrateLegacyModelPrefs() {
  if (!currentProject) return;
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
function manualModels() {
  const legacy = legacyManualModels();
  const list = legacy.length > 0 ? legacy : (processItems.find((item) => item.id.startsWith("d|"))?.manual_models ?? []);
  return Array.isArray(list) ? list.filter((x) => typeof x === "string") : [];
}
function addManualModel(id) {
  const list = manualModels();
  if (!list.includes(id)) list.push(id);
  const defaultProcess = processItems.find((item) => item.id.startsWith("d|"));
  if (defaultProcess) {
    return queueProcessUpdate(defaultProcess.id, { manualModels: list })
      .then(() => refreshProcesses())
      .catch((error) => reportPersistentError(`${t("手填模型保存失败")}:${error}`));
  }
  // 默认进程未就绪(极端时序),退回 localStorage 暂存,由迁移函数下次接手。
  writeJson(prefKey("manual-models"), list);
  return Promise.resolve();
}
// R-115:模型与思考强度按项目记——不同项目常配不同模型,共用一个全局键会互相打架。
// 思考强度此前只写不读(kz-reasoning 全仓零处 getItem),等于每次重启都回默认档。
function prefKey(name) {
  return `kz-${name}:${currentProject || "default"}`;
}
function restoreProjectPrefs() {
  const reasoning = localStorage.getItem(prefKey("reasoning"));
  const select = $("reasoning-select");
  // 选项不存在时不要硬塞:赋一个无效值会让 select 落到空串,反而清掉配置默认档。
  if (reasoning !== null && [...select.options].some((o) => o.value === reasoning)) {
    select.value = reasoning;
  }
  const delivery = localStorage.getItem("kz-delivery");
  const deliverySelect = $("delivery-select");
  if (delivery && [...deliverySelect.options].some((o) => o.value === delivery)) {
    deliverySelect.value = delivery;
  }
  restoreDocFilters();
}

// 思考强度:空值=用配置默认档,其余为本进程覆盖。
$("reasoning-select").addEventListener("change", () => {
  const value = $("reasoning-select").value;
  localStorage.setItem(prefKey("reasoning"), value);
  if (activeProcessId) {
    updateLocalProcessItem(activeProcessId, { reasoning: value });
    queueProcessUpdate(activeProcessId, { reasoning: value })
      .catch((error) => reportPersistentError(`${t("进程思考强度保存失败")}:${error}`));
  }
});

$("model-select").addEventListener("change", async () => {
  const select = $("model-select");
  if (select.value === SHOW_ALL_MODELS_SENTINEL) {
    const selected = processItems.find((item) => item.id === activeProcessId)?.model ||
      legacyModelPrefValue();
    loadModels({ showAll: true }).then(() => {
      select.value = selected;
    });
    return;
  }
  if (select.value === MANUAL_MODEL_SENTINEL) {
    const input = ((await inputDialog({
      title: t("填 provider:model,例如 deepseek:deepseek-chat"),
    })) || "").trim();
    // provider 名必须对得上配置里的键,否则后端 resolve_model 会直接失败。
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = processItems.find((item) => item.id === activeProcessId)?.model || "";
      return;
    }
    addManualModel(input).then(() => loadModels()).then(() => {
      $("model-select").value = input;
    });
    if (activeProcessId) {
      updateLocalProcessItem(activeProcessId, { model: input });
      queueProcessUpdate(activeProcessId, { model: input })
        .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
    }
    return;
  }
  if (activeProcessId) {
    // 空串=清除本进程的模型覆盖(回落 agent 默认);传 null 会被后端当作"不修改"。
    updateLocalProcessItem(activeProcessId, { model: select.value || null });
    queueProcessUpdate(activeProcessId, { model: select.value })
      .catch((error) => reportPersistentError(`${t("进程模型保存失败")}:${error}`));
  }
});
