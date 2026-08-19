// ---------- 设置 ----------
let settingsProviders = [];
let languagePreferenceLoaded = null;
let languagePreferenceDirty = false;

function markLanguagePreferenceDirty() {
  languagePreferenceDirty = true;
  syncSettingsDirty();
}

async function testProvider(provider) {
  try {
    const mode = $("set-proxy-mode")?.value;
    const proxy = mode === "custom" ? $("set-proxy-url")?.value.trim() : mode;
    return await invoke("provider_test", {
      protocol: provider.protocol,
      baseUrl: provider.baseUrl,
      apiKeyEnv: provider.apiKeyEnv || null,
      apiKey: provider.apiKey || null,
      auth: provider.auth || null,
      proxy: proxy || null,
    });
  } catch (err) {
    return `${t("测试失败")}:${err}`;
  }
}

function renderProviders() {
  const tbody = document.querySelector("#providers-table tbody");
  tbody.innerHTML = "";
  settingsProviders.forEach((p, index) => {
    const tr = document.createElement("tr");

    const tdName = document.createElement("td");
    const nameInput = document.createElement("input");
    nameInput.value = p.name;
    nameInput.addEventListener("input", () => (p.name = nameInput.value));
    tdName.appendChild(nameInput);

    const tdProtocol = document.createElement("td");
    const protocolSelect = document.createElement("select");
    for (const proto of ["anthropic", "openai", "openai-responses", "deepseek-responses"]) {
      const opt = document.createElement("option");
      opt.value = proto;
      opt.textContent = proto;
      if (p.protocol === proto) opt.selected = true;
      protocolSelect.appendChild(opt);
    }
    protocolSelect.addEventListener("change", () => (p.protocol = protocolSelect.value));
    tdProtocol.appendChild(protocolSelect);

    const tdUrl = document.createElement("td");
    const urlInput = document.createElement("input");
    urlInput.value = p.baseUrl;
    urlInput.addEventListener("input", () => (p.baseUrl = urlInput.value));
    tdUrl.appendChild(urlInput);

    const tdKey = document.createElement("td");
    if (p.auth) {
      // 特殊认证(codex 订阅登录态):只展示,不可编辑成 key。
      const badge = document.createElement("span");
      badge.className = "key-state key-ok";
      badge.textContent = `${t("订阅登录态")}(${p.auth})`;
      tdKey.appendChild(badge);
    } else {
      const envInput = document.createElement("input");
      envInput.value = p.apiKeyEnv ?? "";
      envInput.placeholder = t("环境变量名(可选)");
      envInput.title = t("读取该环境变量作为 key");
      envInput.addEventListener("input", () => (p.apiKeyEnv = envInput.value));
      const keyInput = document.createElement("input");
      keyInput.type = "password";
      keyInput.value = p.apiKey ?? "";
      keyInput.placeholder = t("或直接粘贴 key");
      keyInput.title = t("直填优先于环境变量;明文存 kanzei.toml");
      keyInput.addEventListener("input", () => (p.apiKey = keyInput.value));
      tdKey.append(envInput, keyInput);
      if (p.keyPresent !== null && p.keyPresent !== undefined) {
        const state = document.createElement("span");
        state.className = `key-state ${p.keyPresent ? "key-ok" : "key-missing"}`;
        state.textContent = p.keyPresent ? t("已设") : t("缺失");
        tdKey.appendChild(state);
      }
    }
    // 当场探测:401/超时都给可操作提示,不用跑一轮对话才发现 key 坏了。
    {
      const testBtn = document.createElement("button");
      testBtn.className = "ghost mini";
      testBtn.textContent = t("测试");
      testBtn.setAttribute("aria-label", `${t("测试")} ${p.name || "provider"} ${t("连接")}`);
      const result = document.createElement("div");
      result.className = "key-test-result";
      testBtn.addEventListener("click", async () => {
        testBtn.disabled = true;
        result.textContent = `${t("测试中")}…`;
        try {
          result.textContent = await testProvider(p);
        } finally {
          testBtn.disabled = false;
        }
      });
      tdKey.append(testBtn, result);
    }

    // D-015:context_limit 必须在表单可见可编辑,保存不许丢字段。
    const tdCtx = document.createElement("td");
    const ctxInput = document.createElement("input");
    ctxInput.type = "number";
    ctxInput.value = p.contextLimit ?? "";
    ctxInput.placeholder = `(${t("不限")})`;
    ctxInput.addEventListener("input", () => {
      const n = parseInt(ctxInput.value, 10);
      p.contextLimit = Number.isFinite(n) && n > 0 ? n : null;
    });
    tdCtx.appendChild(ctxInput);

    const tdRemove = document.createElement("td");
    if (p.builtin) {
      // R-184 P6(D-246):内置 provider 由配置兜底无条件回填,删除会「删了重开又回来」,
      // 换成可见的「内置」标记,不给用户错误预期。
      const builtin = document.createElement("span");
      builtin.className = "provider-builtin";
      builtin.textContent = t("内置");
      builtin.title = t("内置 provider 由 kanzei 默认提供,不可删除;可改配置或编辑连接信息");
      tdRemove.appendChild(builtin);
    } else {
      const removeBtn = document.createElement("button");
      removeBtn.className = "icon-btn";
      removeBtn.textContent = "×";
      removeBtn.setAttribute("aria-label", `${t("移除 provider")} ${p.name || index + 1}`);
      removeBtn.addEventListener("click", () => {
        settingsProviders.splice(index, 1);
        renderProviders();
        // 删行是 click,不是 input/change,表格上的事件委托抓不到它:不显式同步就会
        // 出现"删了 provider 却没有未保存提示",切走视图一重载又原样回来。
        syncSettingsDirty();
      });
      tdRemove.appendChild(removeBtn);
    }

    tr.append(tdName, tdProtocol, tdUrl, tdKey, tdCtx, tdRemove);
    tbody.appendChild(tr);
  });
}

async function deletePermissionRule(rule) {
  try {
    await invoke("permission_rule_delete", { projectDir: currentProject, index: rule.index });
    toast(t("已删除权限规则"));
    await loadPermissionRules();
  } catch (err) {
    toastError(`${t("删除失败")}: ${err}`, { retry: () => deletePermissionRule(rule) });
  }
}

function renderPermissionRules(data) {
  const tbody = $("permission-rules-table").querySelector("tbody");
  tbody.replaceChildren();
  const rules = data?.rules ?? [];
  $("permission-rules-empty").classList.toggle("hidden", rules.length > 0);
  $("permission-rules-path").textContent = data?.path ? `${t("配置")}: ${data.path}` : "";
  for (const rule of rules) {
    const row = document.createElement("tr");
    const action = document.createElement("td");
    action.textContent = rule.action;
    const resource = document.createElement("td");
    resource.textContent = rule.resource;
    const controls = document.createElement("td");
    const remove = document.createElement("button");
    remove.className = "icon-btn";
    remove.title = t("删除规则");
    remove.setAttribute("aria-label", `${t("删除权限规则")} ${rule.action} ${rule.resource}`);
    remove.textContent = "×";
    remove.addEventListener("click", async () => {
      if (!(await confirmDialog({ title: t("删除权限规则"), message: `${rule.action} / ${rule.resource}？`, okText: t("删除"), danger: true }))) return;
      await deletePermissionRule(rule);
    });
    controls.appendChild(remove);
    row.append(action, resource, controls);
    tbody.appendChild(row);
  }
}

async function loadPermissionRules() {
  if (!currentProject) {
    renderPermissionRules({ rules: [] });
    return;
  }
  try {
    renderPermissionRules(await invoke("permission_rules_get", { projectDir: currentProject }));
  } catch (err) {
    renderPermissionRules({ rules: [] });
    toastError(`${t("读取权限规则失败")}: ${err}`, { retry: loadPermissionRules });
  }
}
// D-157:设置页是一张表单,填了不点保存不生效。此前没有任何提示,于是界面显示
// deepseek、运行却用 anthropic,而报错只说"provider anthropic 需要环境变量",
// 完全看不出"你以为改了的那个根本没生效"。这里做脏状态可见。
const SETTINGS_FORM_IDS = [
  "language-select",
  "set-primary", "set-fast", "set-compact", "set-profile", "set-reasoning",
  "set-proxy-mode", "set-proxy-url",
  // 运行上限也算表单的一部分:漏登记就会出现"改了数字却没有未保存提示",
  // 而这正是 D-157 那条"界面显示 A、运行用 B"的复现路径。
  "set-max-tokens", "set-subagent-max-tokens", "set-subagent-timeout", "set-max-tasks",
  "set-context-ratio", "set-verbatim-ratio", "set-max-parallel", "set-stream-restarts",
  "set-transport-retries", "set-rate-retries",
  // 节奏(R-157):与运行上限同规——漏登记就会出现"改了却没未保存提示"。
  "set-cadence-full-test", "set-cadence-full-test-batches",
  "set-cadence-targeted-test", "set-cadence-commit", "set-cadence-push",
];
// 开关类控件不能混进 SETTINGS_FORM_IDS:checkbox 的 .value 恒为 "on"(勾不勾都一样),
// 拿它做指纹永远比不出差异。脏状态必须读 .checked。漏登记的后果不是"少个角标"——
// 03-shell.js:107 每次进设置页都重跑 loadSettings,把表单整张覆盖回磁盘值:走开一趟
// 再回来,勾过的开关就悄悄弹回去了,而角标从头到尾没亮过。用户看到的就是
// "这个开关点了没用"(D-157 那条"界面显示 A、运行用 B"的开关版)。
const SETTINGS_TOGGLE_IDS = ["set-codex-fast-mode"];
let settingsSnapshot = "";
function settingsFingerprint() {
  // provider 表格是动态行,单独序列化;它和标量字段一起构成"这张表单当前的样子"。
  const scalars = SETTINGS_FORM_IDS.map((id) => `${id}=${$(id)?.value ?? ""}`).join("|");
  const toggles = SETTINGS_TOGGLE_IDS.map((id) => `${id}=${$(id)?.checked ? 1 : 0}`).join("|");
  const providers = JSON.stringify(
    settingsProviders.map((p) => [p.name, p.protocol, p.baseUrl, p.apiKeyEnv, p.apiKey, p.contextLimit]),
  );
  return `${scalars}|${toggles}||${providers}`;
}
function syncSettingsDirty() {
  const badge = $("settings-dirty");
  if (!badge) return;
  badge.classList.toggle("hidden", settingsFingerprint() === settingsSnapshot);
}
function markSettingsSaved() {
  settingsSnapshot = settingsFingerprint();
  syncSettingsDirty();
}

// 生效值与全局值不一致 = 项目级 kanzei.toml 覆盖了。必须明说,否则用户会
// 一直在改一个不生效的值(D-168)。
function renderEffectiveNotice(s) {
  const box = $("settings-effective");
  if (!box) return;
  const diffs = [];
  const effective = s.effective;
  // 只比 effective 里**确实带了的键**:后端没报的键(旧版本 / 新加的字段还没接线)
  // 一律跳过,否则 undefined 会被当成"实际生效是未设",提示条天天误报,
  // 用户很快就学会无视它,真被覆盖时反而看不见。
  const has = (key) => effective && Object.prototype.hasOwnProperty.call(effective, key);
  // 模型角色之外的标量也会被项目级 kanzei.toml 覆盖(D-168 当年只堵了模型角色这一个口):
  // 用户改全局值、页面显示「已保存」、运行永远用项目值,又是一次"保存没生效"。
  for (const [key, label] of [
    ["primary", "primary"], ["fast", "fast"], ["reasoning", t("思考强度")],
    ["proxy", t("代理")], ["profileDefault", t("默认模式")], ["codexFastMode", "Codex Fast mode"],
  ]) {
    if (!has(key)) continue;
    const global = key === "reasoning" ? (s.reasoning === "off" ? null : s.reasoning) : s[key];
    const eff = effective[key];
    if ((eff ?? null) !== (global ?? null)) {
      diffs.push(`${label}:${t("本页")} ${global ?? `(${t("未设")})`} → ${t("实际生效")} ${eff ?? `(${t("未设")})`}`);
    }
  }
  // 运行上限十项合成一条:项目级只要覆盖了任意一个键就弹十条提示会把这条提示废掉。
  if (has("limits")) {
    const overridden = LIMIT_FIELDS
      .map(([, key]) => key)
      .filter((key) => (s.limits?.[key] ?? null) !== (effective.limits?.[key] ?? null));
    if (overridden.length) {
      diffs.push(`${t("运行上限")}:${overridden.join("、")}`);
    }
  }
  box.classList.toggle("hidden", diffs.length === 0);
  if (diffs.length) {
    box.textContent =
      `${t("以下项被项目级配置覆盖,本页的改动不会生效")}:${diffs.join("；")}` +
      (s.projectConfig ? `(${s.projectConfig})` : "");
  }
}

// R-305 B1:把 phase_pipeline 的 roster_cap 从日志事实投影到策略面板。
// 未保存输入也要即时可见，避免用户调小上限后不知道阶段角色会被省略。
let settingsEffectiveSnapshot = null;
function renderRosterCapNotice(s) {
  const box = $("set-max-tasks-hint");
  if (!box) return;
  const capacity = Number(s?.phaseRosterCapacity ?? 5);
  const raw = $("set-max-tasks")?.value.trim() ?? "";
  const fallback = s?.limitDefaults?.maxTasksPerTurn ?? 16;
  const limit = raw === "" ? Number(fallback) : Number(raw);
  if (!Number.isFinite(limit) || !Number.isFinite(capacity)) {
    box.textContent = "";
    return;
  }
  const omitted = Math.max(0, capacity - Math.max(0, Math.floor(limit)));
  box.textContent = omitted > 0
    ? `${t("阶段流水线角色上限")}: ${Math.floor(limit)}/${capacity} · ${t("阶段流水线会省略角色")}: ${omitted}`
    : `${t("阶段流水线角色上限")}: ${Math.floor(limit)}/${capacity} · ${t("阶段流水线不会截断角色")}`;
}

$("set-max-tasks")?.addEventListener("input", () => renderRosterCapNotice(settingsEffectiveSnapshot));

function agentSourceLabel(source) {
  return { builtin: t("内建"), global: t("全局"), project: t("项目") }[source] || source;
}
function agentStatusLabel(status) {
  return {
    available: t("可用"),
    configurationError: t("配置错误"),
    hidden: t("当前档位隐藏"),
  }[status] || status;
}
function appendAgentField(card, label, value) {
  const row = document.createElement("div");
  row.className = "agent-directory-field";
  const name = document.createElement("span");
  name.className = "dim";
  name.textContent = `${label}: `;
  const content = document.createElement("span");
  content.textContent = value ?? "";
  row.append(name, content);
  card.append(row);
}
function renderAgentDirectory(snapshot) {
  const host = $("agent-directory");
  if (!host) return;
  host.replaceChildren();
  const agents = Array.isArray(snapshot?.agents) ? snapshot.agents : [];
  if (agents.length === 0) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("没有Agent定义");
    host.append(empty);
    return;
  }
  for (const agent of agents) {
    const card = document.createElement("article");
    card.className = "agent-directory-card";
    const heading = document.createElement("h4");
    heading.textContent = agent.name || "agent";
    const status = document.createElement("span");
    status.className = `agent-directory-status ${agent.status || ""}`;
    status.textContent = agentStatusLabel(agent.status);
    heading.append(" ", status);
    card.append(heading);
    appendAgentField(card, t("来源"), agentSourceLabel(agent.source));
    appendAgentField(card, t("档位"), agent.profile);
    appendAgentField(card, t("模式"), agent.mode);
    appendAgentField(card, t("模型"), agent.model);
    appendAgentField(card, t("轮数"), String(agent.steps ?? ""));
    if (agent.path) {
      appendAgentField(card, t("原文路径"), agent.path);
      const open = document.createElement("button");
      open.className = "ghost";
      open.type = "button";
      open.textContent = t("打开原文");
      open.addEventListener("click", () => invoke("agent_directory_open", {
        projectDir: currentProject || null,
        path: agent.path,
      }).catch((err) => toastError(String(err), { retry: () => open.click() })));
      card.append(open);
    }
    if (agent.error) appendAgentField(card, t("配置错误"), agent.error);
    if (agent.systemPreview) appendAgentField(card, t("系统提示词预览"), agent.systemPreview);
    host.append(card);
  }
}
async function loadAgentDirectory() {
  const status = $("agent-directory-status");
  try {
    const snapshot = await invoke("agent_directory_get", {
      projectDir: currentProject || null,
      profile: $("set-profile")?.value || null,
    });
    renderAgentDirectory(snapshot);
    if (status) status.textContent = `${snapshot.agents?.length || 0} Agent`;
  } catch (err) {
    if (status) status.textContent = t("Agent目录读取失败");
    const host = $("agent-directory");
    if (host) {
      host.replaceChildren();
      const error = document.createElement("p");
      error.className = "form-hint-warn";
      error.textContent = `${t("Agent目录读取失败")}: ${err}`;
      host.append(error);
    }
  }
}
$("agent-directory-refresh")?.addEventListener("click", () => void loadAgentDirectory());
$("set-profile")?.addEventListener("change", () => void loadAgentDirectory());


// 模型角色改成真下拉:自由文本框要手打 `provider:model`,拼错一个字母要到运行时
// 才炸,而那时人早已离开设置页。这里从各 provider 探测到的清单里选,手填只作兜底。
let knownModelIds = [];
/// desired = { primary, fast }:调用方把"该保留哪个值"显式传进来(loadSettings 用已存值)。
/// 不传则以下拉当前值为基准(「重新探测模型」「一键就绪子代理」——那时选项已经建好,
/// 读 DOM 才是对的)。**绝不能**让 loadSettings 靠"先 select.value = 已存值、再来这里读
/// DOM"当基准:首次进设置页时两个 select 在 index.html 里是零个 option 的空壳,按 HTML
/// 规范给 select 赋一个没有匹配 option 的值只会把 selectedIndex 打到 -1、value 读回空串,
/// 那两行赋值等于没写。基准一空,下面的手填兜底 option 就不会建,已存的模型被静默清成
/// 「未设」,用户改别的字段点一次保存就把 [models] primary/fast 从 kanzei.toml 里删掉,
/// 运行回落内置默认——正是 08-compose.js:747 记下的同一个坑,顶栏躲过了,这里没有。
// 只重建 option、**永不主动改 value**:desired 只在首次回填那一次给,之后一律
// 以下拉当前值为准(那可能正是用户刚选的)。原来这个函数把「网络探测」和「写表单」
// 焊在一起,await 期间用户填的东西会被 resolve 后的全量重建整个抹掉。
function applyModelOptions(desired, ids) {
  const roles = [[$("set-primary"), "primary"], [$("set-fast"), "fast"], [$("set-compact"), "compact"]].filter(([el]) => el);
  if (!roles.length) return;
  const current = desired ? null : roles.map(([el]) => el.value);
  knownModelIds = ids;
  roles.forEach(([select, role], index) => {
    const keep = desired ? (desired[role] ?? "") : current[index];
    select.innerHTML = "";
    const none = document.createElement("option");
    none.value = "";
    none.textContent = t("(未设 · 用内置默认)");
    select.appendChild(none);
    for (const id of knownModelIds) {
      const opt = document.createElement("option");
      opt.value = id;
      opt.textContent = id;
      select.appendChild(opt);
    }
    // 已保存的值可能来自探测不到的端点(没实现 /models、key 未配):必须保留,
    // 否则一进设置页就被下拉悄悄改成别的值,保存一次就把配置改坏了。
    if (keep && !knownModelIds.includes(keep)) {
      const opt = document.createElement("option");
      opt.value = keep;
      opt.textContent = `${keep}(${t("手填")})`;
      select.appendChild(opt);
    }
    const manual = document.createElement("option");
    manual.value = MANUAL_MODEL_SENTINEL;
    manual.textContent = t("＋ 手填模型…");
    select.appendChild(manual);
    // 结构性不变量:keep 非空时上面必然已存在 value === keep 的 option(要么在
    // knownModelIds 里,要么刚补的手填兜底),所以这句赋值必然落得下去;keep 为空则
    // 选中「未设」。任何时候都不会出现"赋了个无效值 → 静默变空串"。
    select.value = keep ?? "";
  });
}
// 探测彻底移出加载的关键路径:它只补下拉选项,不碰任何 value,而且带令牌——
// 用户在这几秒里切走或重载过设置页,迟到的结果直接丢弃。
// (models_list 串行探测每个 provider 六秒超时,配几个远端就能拖十几秒,
// 这段时间里页面是可交互的,原来 resolve 之后一次全量重建就把输入吃掉了。)
async function probeModelsAndMergeOptions(token) {
  let ids;
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    // 角色不能再指向角色(primary → primary 会绕成死循环)。
    ids = models.map((m) => m.id).filter((id) => id !== "primary" && id !== "fast" && id !== "compact");
  } catch (error) {
    toastError(`${t("模型列表获取失败")}:${error}`);
    return false;
  }
  if (token !== settingsLoadToken) return false;
  applyModelOptions(null, ids);
  syncSettingsDirty();
  return true;
}

/// 下拉里没有这个值就补一个兜底 option。选项表写死在 index.html 里,而配置文件的合法
/// 取值集合比它大(例如 [profile] default 还认 readonly),硬塞一个不存在的值只会让
/// select 落到空串,保存一次就把用户配置改成默认档——与模型角色同一个坑。
function ensureSelectOption(select, value) {
  if (!select || !value) return;
  if ([...select.options].some((o) => o.value === value)) return;
  const opt = document.createElement("option");
  opt.value = value;
  opt.textContent = value;
  select.appendChild(opt);
}

// 手填分支:两个角色下拉共用。选中哨兵值时弹输入,校验格式后插回列表。
function wireManualModelRole(id) {
  const select = $(id);
  if (!select) return;
  let last = select.value;
  select.addEventListener("change", async () => {
    if (select.value !== MANUAL_MODEL_SENTINEL) {
      last = select.value;
      return;
    }
    const input = ((await inputDialog({
      title: t("填 provider:model,例如 deepseek:deepseek-chat"),
    })) || "").trim();
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = last;
      return;
    }
    const opt = document.createElement("option");
    opt.value = input;
    opt.textContent = `${input}(${t("手填")})`;
    select.insertBefore(opt, select.lastElementChild);
    select.value = input;
    last = input;
    syncSettingsDirty();
  });
}
wireManualModelRole("set-primary");
wireManualModelRole("set-fast");
wireManualModelRole("set-compact");
$("models-refresh")?.addEventListener("click", async () => {
  const ok = await probeModelsAndMergeOptions(settingsLoadToken);
  if (ok) toast(`${t("已重新探测")}:${knownModelIds.length}`);
});

// R-136:fast 子代理模型的就绪状态与一键安装。此前要用户手工装 Ollama、
// 手工 pull、手工配置——三步里断任何一步,记忆整理/快速记录这些子代理杂活
// 就全部静默失效,而界面上毫无线索。
async function refreshFastStatus() {
  const status = $("fast-status");
  const btn = $("fast-setup");
  if (!status || !btn) return;
  let s;
  try {
    s = await invoke("fast_model_status");
  } catch (error) {
    status.textContent = `${t("快速模型状态获取失败")}:${error}`;
    status.classList.add("warn-text");
    btn.classList.add("hidden");
    return;
  }
  if (!s.managed) {
    status.textContent = fastStatusText(s).text;
    btn.classList.add("hidden");
    return;
  }
  if (s.ready) {
    status.textContent = fastStatusText(s).text;
    status.classList.remove("warn-text");
    btn.classList.add("hidden");
    return;
  }
  const st = fastStatusText(s);
  status.textContent = st.text;
  status.classList.add("warn-text");
  btn.classList.remove("hidden");
}
$("fast-setup")?.addEventListener("click", async () => {
  const btn = $("fast-setup");
  btn.disabled = true;
  try {
    const done = await invoke("fast_model_setup");
    toast(done);
    await probeModelsAndMergeOptions(settingsLoadToken);
  } catch (err) {
    toastError(`${t("子代理安装失败")}:${err}`);
  } finally {
    btn.disabled = false;
    refreshFastStatus();
  }
});

// [limits] 表单:输入框 id ↔ 后端 camelCase 键。加参数时只改这一张表,
// 读取与保存两侧都走它,不会再出现"读了没存"或"存了没读"的半边接线。
const LIMIT_FIELDS = [
  ["set-max-tokens", "maxTokens"],
  ["set-subagent-max-tokens", "subagentMaxTokens"],
  ["set-subagent-timeout", "subagentTimeoutSecs"],
  ["set-max-tasks", "maxTasksPerTurn"],
  ["set-context-ratio", "contextBudgetRatio"],
  ["set-verbatim-ratio", "recentVerbatimRatio"],
  ["set-max-parallel", "maxParallelTools"],
  ["set-stream-restarts", "streamRestarts"],
  ["set-transport-retries", "transportRetries"],
  ["set-rate-retries", "rateLimitRetries"],
];

// 节奏(R-157):id ↔ settings_get 返回的 cadence snake_case 键。
// 留空 = None,保存时该键从 [cadence] 移除,回落 §1.4 默认。
const CADENCE_FIELDS = [
  ["set-cadence-full-test", "full_test"],
  ["set-cadence-targeted-test", "targeted_test"],
  ["set-cadence-commit", "commit"],
  ["set-cadence-push", "push"],
];

function collectCadence() {
  const out = {};
  for (const [id, key] of CADENCE_FIELDS) {
    const value = $(id).value.trim();
    out[key] = value === "" ? null : value;
  }
  const batchesRaw = $("set-cadence-full-test-batches").value.trim();
  const batches = batchesRaw === "" ? null : Number(batchesRaw);
  out.full_test_batches = batches === null || Number.isNaN(batches) ? null : batches;
  return out;
}

/// 空 → null(后端据此删掉该键,回落内置默认);非法输入也当空,不把 NaN 写进配置。
function collectLimits() {
  const out = {};
  for (const [id, key] of LIMIT_FIELDS) {
    const raw = $(id).value.trim();
    const value = raw === "" ? null : Number(raw);
    out[key] = value === null || Number.isNaN(value) ? null : value;
  }
  return out;
}

// 每次加载自增;await 回来对不上就说明用户已经切走/重载过,回填一律丢弃。
let settingsLoadToken = 0;
// 首次成功回填后置 true。没有它的话,空表单的指纹和「干净基线」永远对不上,
// 脏值守卫会在第一次加载时就把自己挡掉。
let settingsHydrated = false;
// 表单有未保存改动时**拒绝**用磁盘值覆盖,并把「为什么没刷新」说出来——
// 静悄悄地把用户填了一半的东西回滚成磁盘值,正是「突然刷新」最恼人的那一下。
function showSettingsStale() {
  const el = $("settings-stale");
  if (el) el.classList.remove("hidden");
}
function hideSettingsStale() {
  const el = $("settings-stale");
  if (el) el.classList.add("hidden");
}
$("settings-discard")?.addEventListener("click", () => {
  settingsHydrated = false;
  void loadSettings({ force: true });
});
async function loadSettings({ force = false } = {}) {
  const token = ++settingsLoadToken;
  let s;
  try {
    s = await invoke("settings_get", { projectDir: currentProject });
  } catch (err) {
    // 配置损坏时不能留一张空白表单让用户无从下手(保存会把默认值写回,反而丢配置)。
    $("settings-path").textContent = t("配置读取失败");
    toastError(`${t("设置读取失败")}:${err}`, { retry: loadSettings });
    return;
  }
  if (token !== settingsLoadToken) return;
  // 只读区永远允许更新:它一行 input 都不碰,刷新它不会吃掉任何输入。
  $("settings-path").textContent = s.path;
  settingsEffectiveSnapshot = s;
  renderRosterCapNotice(s);
  renderEffectiveNotice(s);
  loadPermissionRules();
  refreshFastStatus();
  if (!force && settingsHydrated && settingsFingerprint() !== settingsSnapshot) {
    showSettingsStale();
    return;
  }
  hideSettingsStale();
  hydrateSettingsForm(s);
  settingsHydrated = true;
  // 探测不再挡在回填前面(原来是 await,几秒后 resolve 再整表重建)。
  void probeModelsAndMergeOptions(token);
  void loadAgentDirectory();
}
function hydrateSettingsForm(s) {
  const storedLanguage = LANGUAGE_PREFERENCES.has(s.language)
    ? s.language
    : normalizeLanguagePreference(localStorage.getItem("kz-language"));
  languagePreferenceLoaded = LANGUAGE_PREFERENCES.has(s.language) ? s.language : null;
  languagePreferenceDirty = false;
  // rerender:false —— persist:false 时语言压根没变,整串重渲(applyLanguage +
  // 侧栏 + 工作区 + refreshWorktrees + refreshConversationList + 多画一遍 provider 表)
  // 是纯白干,还让整个界面在进设置页时抖一下。
  setLanguagePreference(storedLanguage, { persist: false, rerender: false });
  // R-178 批4 D7 作用域选择器:settings_get 返回 projectConfig 时才允许选「本项目」。
  // 无项目上下文(未选中项目)时 project 选项禁用,避免把"全局"意图落进一个偶然的
  // 工作目录。
  const projectConfig = s.projectConfig;
  const scopeSelect = $("set-save-scope");
  const projectOption = scopeSelect.querySelector('option[value="project"]');
  projectOption.disabled = !projectConfig;
  if (!projectConfig && scopeSelect.value === "project") scopeSelect.value = "global";
  $("settings-scope-hint").textContent = projectConfig
    ? t("D7 只覆盖模型角色;Provider 与密钥始终写全局")
    : t("未选中项目,仅可保存到全局");
  $("settings-scope-note").textContent = projectConfig
    ? t("本项目将写入") + " " + projectConfig
    : "";
  // 已存值必须**显式传给** fillKnownModels 当基准。此前是"先 select.value = 已存值,
  // 建完选项再塞一次",两次都是空操作:首次进设置页时下拉里一个 option 都没有,给
  // select 赋没有匹配项的值按规范只会把它打到空串。基准一空,探测不到的已存模型就被
  // 静默清成「未设」,而 markSettingsSaved() 还把这个已经被清空的状态当成干净基线
  // (角标不亮,零告警),用户改任意别的字段点保存,后端就把 [models] 的键删了。
  // 同步回填,零 IPC:用上一次探测到的 knownModelIds 建选项,探测在后台单独跑。
  applyModelOptions({ primary: s.primary ?? "", fast: s.fast ?? "", compact: s.compact ?? "" }, knownModelIds);
  // 配置里可能是 readonly 这种下拉没有的合法档位:没有兜底 option 就会变空串。
  ensureSelectOption($("set-profile"), s.profileDefault);
  $("set-profile").value = s.profileDefault;
  $("set-reasoning").value = s.reasoning || "off";
  $("set-codex-fast-mode").checked = s.codexFastMode === true;
  // 运行上限:值为空即"用内置默认",占位符显示该默认值——不写死在 HTML 里,
  // 免得改了 Rust 默认值而界面还在展示旧数字。
  for (const [id, key] of LIMIT_FIELDS) {
    const el = $(id);
    const value = s.limits?.[key];
    el.value = value === null || value === undefined ? "" : String(value);
    const fallback = s.limitDefaults?.[key];
    el.placeholder = fallback === undefined ? "" : `${t("默认")} ${fallback}`;
  }
  renderRosterCapNotice(s);
  const proxy = s.proxy;
  if (proxy === "env" || proxy === "off") {
    $("set-proxy-mode").value = proxy;
    $("set-proxy-url").classList.add("hidden");
  } else {
    $("set-proxy-mode").value = "custom";
    $("set-proxy-url").value = proxy;
    $("set-proxy-url").classList.remove("hidden");
  }
  updateProxyHint();
  // 节奏:已存值回填下拉;空 = 用默认。间隔输入框占位显示默认 N。
  const cd = s.cadence ?? {};
  const cdDefaults = s.cadenceDefaults ?? {};
  for (const [id, key] of CADENCE_FIELDS) {
    const value = cd[key];
    $(id).value = value === null || value === undefined || value === "" ? "" : String(value);
  }
  const batchesEl = $("set-cadence-full-test-batches");
  const batches = cd.full_test_batches;
  batchesEl.value = batches === null || batches === undefined ? "" : String(batches);
  const defaultBatches = cdDefaults.full_test_batches;
  batchesEl.placeholder = defaultBatches === undefined || defaultBatches === null ? "" : `${t("默认")} ${defaultBatches}`;
  // R-170:cadence 表单回填即止,不再联动继续文案(规则剥离,文案仅承载用户意图)。
  settingsProviders = s.providers;
  renderProviders();
  // 刚从磁盘读回来 = 干净态,以此为基准比对后续改动。
  markSettingsSaved();
  // R-187:提示音是本地偏好(不进 kanzei.toml),设置页控件回填 + change 即存。
  loadSoundSettingsControls();
  // R-251:使用手册显隐同样是本地偏好,回填 + change 即存,不进 kanzei.toml。
  loadManualShowControl();
}

function loadSoundSettingsControls() {
  const s = readSoundSettings();
  const set = (id, value) => {
    const el = $(id);
    if (el) el.value = value;
  };
  const setChecked = (id, checked) => {
    const el = $(id);
    if (el) el.checked = checked;
  };
  setChecked("set-sound-enabled", s.enabled);
  set("set-sound-volume", String(Math.round(s.volume * 100)));
  setChecked("set-sound-completed", s.completed);
  setChecked("set-sound-failed", s.failed);
  setChecked("set-sound-stopped", s.stopped);
}

// R-251:使用手册显隐是本地显示偏好,回填控件 + change 即存(与 R-187 sound 同模式)。
function loadManualShowControl() {
  const el = $("set-show-manual");
  if (el) el.checked = readManualShowPref();
}
$("set-show-manual")?.addEventListener("change", () => {
  const el = $("set-show-manual");
  if (!el) return;
  saveManualShowPref(el.checked);
  if (typeof refreshManual === "function") refreshManual();
});

function bindSoundSettingsControls() {
  const collect = () => ({
    enabled: $("set-sound-enabled")?.checked ?? true,
    volume: (Number($("set-sound-volume")?.value ?? 12)) / 100,
    completed: $("set-sound-completed")?.checked ?? true,
    failed: $("set-sound-failed")?.checked ?? true,
    stopped: $("set-sound-stopped")?.checked ?? true,
  });
  for (const id of ["set-sound-enabled", "set-sound-volume", "set-sound-completed", "set-sound-failed", "set-sound-stopped"]) {
    $(id)?.addEventListener("change", () => saveSoundSettings(collect()));
    $(id)?.addEventListener("input", () => saveSoundSettings(collect()));
  }
  // 试听:用当前音量播一次「完成」音,让用户调完能立即听到效果。
  $("sound-preview")?.addEventListener("click", () => {
    const s = collect();
    saveSoundSettings(s);
    playRunNotice("completed");
  });
}
bindSoundSettingsControls();
for (const id of SETTINGS_FORM_IDS) {
  $(id)?.addEventListener("input", syncSettingsDirty);
  $(id)?.addEventListener("change", syncSettingsDirty);
}
// checkbox 只有 change 有意义(input 事件对它不触发脏状态之外的语义)。
for (const id of SETTINGS_TOGGLE_IDS) {
  $(id)?.addEventListener("change", syncSettingsDirty);
}
// provider 表格是动态重建的,逐行绑会随重绘丢失;在容器上做事件委托一次覆盖全表。
// 委托要在捕获阶段之后跑——行内的 input 监听器先把值写回 settingsProviders,
// 我们才比对得到新指纹。
for (const event of ["input", "change"]) {
  $("providers-table")?.addEventListener(event, () => setTimeout(syncSettingsDirty, 0));
}

// R-184 P6(D-247):选「指定地址」却留空时,后端按空串回落 env——这是静默降级,
// 界面必须把「将回落环境变量」说出来,不许用户以为地址已生效。
function updateProxyHint() {
  const hint = $("set-proxy-hint");
  if (!$("set-proxy-url") || !hint) return;
  const mode = $("set-proxy-mode").value;
  const emptyCustom = mode === "custom" && !$("set-proxy-url").value.trim();
  hint.classList.toggle("hidden", !emptyCustom);
  if (emptyCustom) {
    hint.textContent = t("地址留空将回落「跟随环境变量」");
    $("set-proxy-url").classList.remove("hidden");
  }
}
$("set-proxy-mode").addEventListener("change", () => {
  $("set-proxy-url").classList.toggle("hidden", $("set-proxy-mode").value !== "custom");
  // 留空时输入框保持可见,否则提示「回落」但地址框都找不到,更迷惑。
  if ($("set-proxy-mode").value === "custom") $("set-proxy-url").classList.remove("hidden");
  updateProxyHint();
});
$("set-proxy-url").addEventListener("input", updateProxyHint);

$("mobile-service-stop").addEventListener("click", async () => {
  try {
    await invoke("mobile_service_stop");
    $("mobile-service-status").textContent = t("移动端本机桥接已停止");
    $("mobile-service-start").classList.remove("hidden");
    $("mobile-service-stop").classList.add("hidden");
    $("mobile-pair-regenerate").disabled = true;
    $("mobile-device-list").textContent = t("启动服务后显示");
  } catch (error) {
    toastError(`${t("停止移动端桥接失败")}:${error}`, { retry: () => $("mobile-service-stop").click() });
  }
});

// D-386:设备列表 + 逐台撤销 + 配对码再生。revoke/list/regenerate 命令 R-270 已注册,
// 此前 UI 零调用(多设备实际不可能、撤销=空集、配对码不可再生)。
async function refreshMobileDevices() {
  const container = $("mobile-device-list");
  if (!container) return;
  try {
    const devices = await invoke("mobile_device_list");
    if (!devices || devices.length === 0) {
      container.innerHTML = `<span class="dim">${t("暂无已配对设备")}</span>`;
      return;
    }
    container.innerHTML = "";
    for (const device of devices) {
      const row = document.createElement("span");
      row.className = "mobile-device-row";
      row.textContent = `${device.device_id} · ${device.name} · `;
      const revoke = document.createElement("button");
      revoke.className = "ghost danger-text";
      revoke.textContent = t("撤销");
      revoke.addEventListener("click", async () => {
        try {
          await invoke("mobile_device_revoke", { deviceId: device.device_id });
          toast(`${t("已撤销设备")}: ${device.device_id}`);
          refreshMobileDevices();
        } catch (error) {
          toastError(`${t("撤销设备失败")}:${error}`);
        }
      });
      row.appendChild(revoke);
      container.appendChild(row);
      container.appendChild(document.createElement("br"));
    }
  } catch (error) {
    container.textContent = `${t("读取设备列表失败")}:${error}`;
  }
}

$("mobile-service-start").addEventListener("click", async () => {
  // 启动成功后启用配对码再生 + 加载设备列表(上面的 start 逻辑由事件顺序保证先执行)。
  const lan = !!$("mobile-service-lan")?.checked;
  try {
    const info = await invoke("mobile_service_start", { projectDir: currentProject, port: null, lan });
    const lanLabel = lan ? "LAN" : t("回环");
    $("mobile-service-status").textContent = `${lanLabel} · ${info.address} · token ${info.token}`;
    $("mobile-service-start").classList.add("hidden");
    $("mobile-service-stop").classList.remove("hidden");
    $("mobile-pair-regenerate").disabled = false;
    refreshMobileDevices();
    toast(t("移动端本机桥接已启动"));
  } catch (error) {
    toastError(`${t("启动移动端桥接失败")}:${error}`, { retry: () => $("mobile-service-start").click() });
  }
});
$("mobile-pair-regenerate").addEventListener("click", async () => {
  try {
    const newCode = await invoke("mobile_pair_code_regenerate");
    $("mobile-service-status").textContent = `${$("mobile-service-status").textContent.split(" · token ")[0]} · token ${newCode}`;
    toast(`${t("新配对码")}: ${newCode}`);
  } catch (error) {
    toastError(`${t("重新生成配对码失败")}:${error}`);
  }
});
async function agentContainerAction(action) {
  const agentId = $("agent-container-id").value.trim();
  if (!agentId) return toast(t("先填写 agent id"));
  try {
    const command = action === "create" ? "agent_container_create" : action === "upgrade" ? "agent_container_upgrade" : "agent_container_rollback";
    const args = { agentId };
    if (action === "upgrade") args.version = "2";
    const manifest = await invoke(command, args);
    const actionLabel = action === "rollback" ? t("回滚") : action === "create" ? t("创建") : t("升级");
    toast(`${t("代理容器")} ${manifest.agent_id} v${manifest.version} ${actionLabel}`);
  } catch (error) {
    toastError(String(error), { retry: () => agentContainerAction(action) });
  }
}
$("agent-container-create").addEventListener("click", () => agentContainerAction("create"));
$("agent-container-upgrade").addEventListener("click", () => agentContainerAction("upgrade"));
$("agent-container-rollback").addEventListener("click", () => agentContainerAction("rollback"));

$("provider-add").addEventListener("click", () => {
  settingsProviders.push({ name: "", protocol: "openai", baseUrl: "http://", apiKeyEnv: "" });
  renderProviders();
  syncSettingsDirty();
});

$("providers-test").addEventListener("click", async () => {
  const button = $("providers-test");
  const result = $("providers-test-result");
  if (!settingsProviders.length) {
    result.textContent = t("没有可测试的 provider");
    return;
  }
  button.disabled = true;
  result.textContent = `${t("测试中")}(0/${settingsProviders.length})…`;
  try {
    let passed = 0;
    for (const [index, provider] of settingsProviders.entries()) {
      const status = await testProvider(provider);
      if (status.startsWith("✓")) passed += 1;
      result.textContent = `${t("测试中")}(${index + 1}/${settingsProviders.length})…`;
    }
    result.textContent = `${t("连通性检查完成")}: ${passed}/${settingsProviders.length} ${t("可用")}`;
  } finally {
    button.disabled = false;
  }
});

$("settings-save").addEventListener("click", async () => {
  const mode = $("set-proxy-mode").value;
  const proxy = mode === "custom" ? $("set-proxy-url").value.trim() : mode;
  const scope = $("set-save-scope").value;
  try {
    await invoke("settings_save", {
      // R-178 批4 D7:scope=project 只把模型角色写进主根 .kanzei/kanzei.toml;
      // 其余字段(proxy/limits/cadence/providers)始终走全局,后端按 scope 拦截。
      scope,
      projectDir: scope === "project" ? currentProject : null,
      payload: {
        // 未显式改过且后端返回 null 时继续传 null,不要因为表单默认中文就把默认键写入配置。
        language: languagePreferenceDirty ? $("language-select").value : languagePreferenceLoaded,
        primary: $("set-primary").value,
        fast: $("set-fast").value,
        compact: $("set-compact") ? $("set-compact").value : "",
        proxy,
        profileDefault: $("set-profile").value,
        reasoning: $("set-reasoning").value,
        codexFastMode: $("set-codex-fast-mode").checked,
        limits: collectLimits(),
        cadence: collectCadence(),
        // 约定:清单非空 = 清单即权威,后端会删掉配置里不在清单中的 [providers.X]
        // (否则表格里点了「×」保存后重开又回来)。所以这里**必须发整张表**,
        // 任何时候都不许只发"改动过的那几行"。
        providers: settingsProviders.map((p) => ({
          name: p.name,
          protocol: p.protocol,
          baseUrl: p.baseUrl,
          apiKeyEnv: p.apiKeyEnv || null,
          apiKey: p.apiKey || null,
          auth: p.auth || null,
          contextLimit: p.contextLimit ?? null,
        })),
      },
    });
    toast(t("已保存"));
    // force:刚存完就是干净态,但指纹要等 markSettingsSaved 才更新,不 force 会被
    // 脏值守卫挡住,用户看到一个莫名其妙的「磁盘上的配置已更新」。
    loadSettings({ force: true });
  } catch (err) {
    toastError(`${t("保存失败")}: ${err}`, { retry: () => $("settings-save").click() });
  }
});

$("settings-open").addEventListener("click", () => invoke("settings_open").catch((e) => toastError(String(e), { retry: () => $("settings-open").click() })));

$("export-pick-dir").addEventListener("click", async () => {
  try {
    const path = await invoke("export_pick_dir");
    if (path) $("export-output-dir").value = path;
  } catch (error) {
    toastError(`${t("选择导出目录失败")}:${error}`);
  }
});
$("export-project").addEventListener("click", async () => {
  if (!currentProject) return toast(t("先在左侧「项目」里添加并选择一个目录"));
  const outputDir = $("export-output-dir").value.trim();
  if (!outputDir) return toast(t("选择导出目录"));
  const button = $("export-project");
  button.disabled = true;
  $("export-result").textContent = `${t("导出工作资料")}…`;
  try {
    const result = await invoke("export_project_data", {
      options: {
        projectDir: currentProject,
        outputDir,
        includeMemory: $("export-memory").checked,
        includeRequirements: $("export-requirements").checked,
        includeDefects: $("export-defects").checked,
        includeConfig: $("export-config").checked,
      },
    });
    $("export-result").textContent = `${t("导出完成")}: ${result.path} (${result.files.length} ${t("条")})`;
    toast(t("导出完成"));
  } catch (error) {
    $("export-result").textContent = String(error);
    toastError(`${t("导出失败")}:${error}`);
  } finally {
    button.disabled = false;
  }
});

// ---------- 版本与更新(GitHub Releases 为源) ----------
let updateUrl = null;
// D-287:「没有可装的东西」有三种成因,以前一律渲染成「已是最新(<latest>)」——
// 于是「当前版本 a7a122a」下面紧挨着「已是最新(build-c99304f)」,两个 hash 打架,
// 看着就像更新检查坏了。只有 status=latest 这一态有资格说「已是最新」;本地领先
// 与无法比较各自说自己的话(D-004:不做的理由要说出来),别人的 hash 一律标成
// 「最新发布」,不冒充「当前」。
function updateResultText(r) {
  if (r.status === "none") return r.message;
  const latest = `${t("最新发布")}:${r.latest}`;
  switch (r.status) {
    case "update":
      return `${t("发现新版本")}:${r.latest}`;
    case "ahead":
      return `${t("本地构建晚于最新发布,无需更新")}(${latest})`;
    case "dev":
      return `${t("本地是开发构建,无法与发布版比较;要装发布版得手动运行安装器")}(${latest})`;
    case "unknown":
      return `${t("拿不到可比的构建时间,无法判断新旧")}(${latest})`;
    default:
      return `${t("已是最新")}(${r.latest || r.current})`;
  }
}
$("update-check").addEventListener("click", async () => {
  $("update-result").textContent = t("检查中…");
  $("update-install").classList.add("hidden");
  updateUrl = null;
  try {
    const r = await invoke("update_check");
    if (r.current) $("update-current").textContent = r.current;
    $("update-result").textContent = updateResultText(r);
    if (r.newer && r.url) {
      updateUrl = r.url;
      $("update-install").classList.remove("hidden");
    }
  } catch (err) {
    $("update-result").textContent = `${t("检查失败")}:${err}`;
  }
});
$("update-install").addEventListener("click", async () => {
  if (!updateUrl) return;
  $("update-result").textContent = t("下载中…(应用将退出,安装完成后请手动启动)");
  $("update-install").disabled = true;
  try {
    $("update-result").textContent = await invoke("update_install", { url: updateUrl });
  } catch (err) {
    $("update-result").textContent = String(err);
  } finally {
    $("update-install").disabled = false;
  }
});

// ---------- 侧边栏分区折叠:标题文字收/展,记忆到 localStorage ----------
document.querySelectorAll(".sidebar-section").forEach((section) => {
  const title = section.querySelector(".section-title > span:first-child");
  if (!title) return;
  const collapseKey = section.dataset.collapseKey || title.textContent.replace(/[\d\s]/g, "").slice(0, 8);
  const key = `kz-collapse-${collapseKey}`;
  const legacyKey = `kz-collapse-${title.textContent.replace(/[\d\s]/g, "").slice(0, 8)}`;
  const saved = localStorage.getItem(key) ?? (legacyKey === key ? null : localStorage.getItem(legacyKey));
  // data-collapse-default="collapsed":没有存过偏好时默认收起。项目列表用它——
  // 当前项目已经由侧栏工作区头常驻显示,整份列表只在切项目时才需要。
  const collapsedByDefault = section.dataset.collapseDefault === "collapsed";
  if (saved === "1" || (saved === null && collapsedByDefault)) {
    section.classList.add("collapsed");
    if (legacyKey !== key) localStorage.setItem(key, "1");
  }
  title.setAttribute("role", "button");
  title.setAttribute("tabindex", "0");
  const syncExpanded = () => title.setAttribute("aria-expanded", String(!section.classList.contains("collapsed")));
  const toggle = () => {
    const collapsed = section.classList.toggle("collapsed");
    localStorage.setItem(key, collapsed ? "1" : "0");
    syncExpanded();
  };
  syncExpanded();
  title.addEventListener("click", toggle);
  title.addEventListener("keydown", (event) => {
    if (event.key !== "Enter" && event.key !== " ") return;
    event.preventDefault();
    toggle();
  });
});
// 工作区头的箭头方向要跟着上面刚恢复的折叠态走(09-sessions.js 定义)。
if (typeof syncProjectSwitchExpanded === "function") syncProjectSwitchExpanded();
