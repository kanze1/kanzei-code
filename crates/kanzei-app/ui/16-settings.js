import { defer } from "./01-core.js";
import { $, confirmDialog, inputDialog, invoke } from "./01-core.js";
import { LANGUAGE_PREFERENCES, normalizeLanguagePreference, setLanguagePreference, t } from "./02-i18n.js";
import {
  currentProject,
  playRunNotice,
  readSoundSettings,
  saveSoundSettings,
  sessionStates,
  toast,
  toastError,
} from "./03-shell.js";
import { flushLayout, onLayoutChange, setLayoutPref } from "./03-layout.js";
import { isGeneralChat } from "./03-general-scope.js";
import { fastStatusText } from "./06-activity.js";
import { state } from "./08-compose.js";
import { MANUAL_MODEL_SENTINEL } from "./08-models.js";
import { openProjectModelsDialog } from "./08-project-models.js";
// UI-0926 #10:权限规则表的资源列按结构渲染(bash 规则是 {command, workdir} JSON)。
import { permissionResourceText } from "./04-structured-parse.js";
import { renderPermissionResource } from "./04-structured.js";

// ---------- 设置 ----------
export let settingsProviders = [];

// UX-119:界面语言切换即时生效,持久化也必须即时——原先要再点「保存」才写进 kanzei.toml,
// 切完语言直接关窗就丢。现在走 ui_prefs 通道(app.json 的 ui_layout.prefs.language,与主题等同源,D-404),
// 不再属于「保存」表单。旧版写在 kanzei.toml 的值由 18-startup.js 启动时一次性迁过来。
export function persistLanguagePreference(preference) {
  if (!LANGUAGE_PREFERENCES.has(preference)) return;
  setLayoutPref("prefs", "language", preference);
  flushLayout();
}

export async function testProvider(provider) {
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

// provider 对象 → 它那一行的名称/地址输入框(保存前校验失败时聚焦用,不靠选择器去找)。
const providerInputs = new WeakMap();
export function renderProviders() {
  const tbody = document.querySelector("#providers-table tbody");
  tbody.innerHTML = "";
  settingsProviders.forEach((p, index) => {
    const tr = document.createElement("tr");

    const tdName = document.createElement("td");
    const nameInput = document.createElement("input");
    nameInput.value = p.name;
    nameInput.setAttribute("aria-label", t("名称"));
    nameInput.addEventListener("input", () => {
      p.name = nameInput.value;
      nameInput.removeAttribute("aria-invalid");
    });
    tdName.appendChild(nameInput);
    if (p.source) {
      const source = document.createElement("span");
      source.className = "provider-source dim";
      source.textContent = `(${providerSourceLabel(p.source)})`;
      tdName.appendChild(source);
    }

    const tdProtocol = document.createElement("td");
    const protocolSelect = document.createElement("select");
    for (const proto of ["anthropic", "openai", "openai-responses", "deepseek-responses"]) {
      const opt = document.createElement("option");
      opt.value = proto;
      opt.textContent = proto;
      if (p.protocol === proto) opt.selected = true;
      protocolSelect.appendChild(opt);
    }
    protocolSelect.setAttribute("aria-label", t("协议"));
    protocolSelect.addEventListener("change", () => (p.protocol = protocolSelect.value));
    tdProtocol.appendChild(protocolSelect);

    const tdUrl = document.createElement("td");
    const urlInput = document.createElement("input");
    urlInput.value = p.baseUrl;
    urlInput.setAttribute("aria-label", "Base URL");
    urlInput.addEventListener("input", () => {
      p.baseUrl = urlInput.value;
      urlInput.removeAttribute("aria-invalid");
    });
    tdUrl.appendChild(urlInput);
    providerInputs.set(p, { name: nameInput, url: urlInput });

    const tdKey = document.createElement("td");
    if (p.auth) {
      // Codex 订阅登录态只展示,不可编辑成 API key。
      const badge = document.createElement("span");
      badge.className = "key-state key-ok";
      badge.textContent = `${t("订阅登录态")}(${p.auth})`;
      tdKey.appendChild(badge);
    } else {
      const envInput = document.createElement("input");
      envInput.value = p.apiKeyEnv ?? "";
      envInput.placeholder = t("环境变量名(可选)");
      envInput.title = t("读取该环境变量作为 key");
      envInput.setAttribute("aria-label", t("环境变量名(可选)"));
      envInput.addEventListener("input", () => (p.apiKeyEnv = envInput.value));
      const keyInput = document.createElement("input");
      keyInput.type = "password";
      keyInput.value = p.apiKey ?? "";
      keyInput.placeholder = t("或直接粘贴 key");
      keyInput.title = t("直填优先于环境变量;明文存 kanzei.toml");
      keyInput.setAttribute("aria-label", t("或直接粘贴 key"));
      keyInput.addEventListener("input", () => (p.apiKey = keyInput.value));
      tdKey.append(envInput, keyInput);
      if (p.legacyClaudeSubscription) {
        const warning = document.createElement("span");
        warning.className = "key-state key-missing";
        warning.textContent = p.keyPresent
          ? t("旧版 Claude 订阅已停用；保存后会使用这里配置的 API Key。")
          : t("Claude 订阅登录已停用，请改用 Anthropic API Key。");
        tdKey.appendChild(warning);
      }
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
    ctxInput.setAttribute("aria-label", t("上下文(token)"));
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

function permissionRulesProjectCurrent(projectDir) {
  return !!projectDir && projectDir === currentProject && !isGeneralChat(projectDir);
}

export async function deletePermissionRule(rule, projectDir) {
  if (!permissionRulesProjectCurrent(projectDir)) return;
  try {
    await invoke("permission_rule_delete", {
      projectDir,
      index: rule.index,
      expectedRule: { action: rule.action, resource: rule.resource, effect: rule.effect },
    });
    if (!permissionRulesProjectCurrent(projectDir)) return;
    toast(t("已删除权限规则"));
    await loadPermissionRules();
  } catch (err) {
    if (!permissionRulesProjectCurrent(projectDir)) return;
    toastError(`${t("删除失败")}: ${err}`, { retry: () => deletePermissionRule(rule, projectDir) });
  }
}

// 最近一次渲染的权限规则:切语言时据此重画(行内的 title/aria-label 是渲染点写的 t())。
let lastPermissionRules = null;
let permissionRulesLoadToken = 0;
export function renderPermissionRules(data, projectDir = currentProject) {
  if (projectDir !== currentProject) return;
  lastPermissionRules = { data, projectDir };
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
    resource.appendChild(renderPermissionResource(rule.action, rule.resource));
    const controls = document.createElement("td");
    const remove = document.createElement("button");
    const ruleText = permissionResourceText(rule.action, rule.resource);
    remove.className = "icon-btn";
    remove.title = t("删除规则");
    remove.setAttribute("aria-label", `${t("删除权限规则")} ${ruleText}`);
    remove.textContent = "×";
    remove.addEventListener("click", async () => {
      if (!permissionRulesProjectCurrent(projectDir)) return;
      if (!(await confirmDialog({ title: t("删除权限规则"), message: `${ruleText}？`, okText: t("删除"), danger: true }))) return;
      if (!permissionRulesProjectCurrent(projectDir)) return;
      await deletePermissionRule(rule, projectDir);
    });
    controls.appendChild(remove);
    row.append(action, resource, controls);
    tbody.appendChild(row);
  }
}

export async function loadPermissionRules() {
  const token = ++permissionRulesLoadToken;
  const projectDir = currentProject;
  const isCurrent = () => token === permissionRulesLoadToken && projectDir === currentProject;
  if (lastPermissionRules?.projectDir !== projectDir) renderPermissionRules({ rules: [] }, projectDir);
  if (!permissionRulesProjectCurrent(projectDir)) {
    renderPermissionRules({ rules: [] }, projectDir);
    return;
  }
  try {
    const data = await invoke("permission_rules_get", { projectDir });
    if (!isCurrent()) return;
    renderPermissionRules(data, projectDir);
  } catch (err) {
    if (!isCurrent()) return;
    renderPermissionRules({ rules: [] }, projectDir);
    toastError(`${t("读取权限规则失败")}: ${err}`, { retry: () => {
      if (permissionRulesProjectCurrent(projectDir)) return loadPermissionRules();
    } });
  }
}
// D-157:设置页是一张表单,填了不点保存不生效。此前没有任何提示,于是界面显示
// deepseek、运行却用 anthropic,而报错只说"provider anthropic 需要环境变量",
// 完全看不出"你以为改了的那个根本没生效"。这里做脏状态可见。
export const SETTINGS_FORM_IDS = [
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
export const SETTINGS_TOGGLE_IDS = ["set-codex-fast-mode"];
export let settingsSnapshot = "";
export function settingsFingerprint() {
  // provider 表格是动态行,单独序列化;它和标量字段一起构成"这张表单当前的样子"。
  const scalars = SETTINGS_FORM_IDS.map((id) => `${id}=${$(id)?.value ?? ""}`).join("|");
  const toggles = SETTINGS_TOGGLE_IDS.map((id) => `${id}=${$(id)?.checked ? 1 : 0}`).join("|");
  const providers = JSON.stringify(
    settingsProviders.map((p) => [p.name, p.protocol, p.baseUrl, p.apiKeyEnv, p.apiKey, p.contextLimit]),
  );
  return `${scalars}|${toggles}||${providers}`;
}
export function syncSettingsDirty() {
  const badge = $("settings-dirty");
  if (!badge) return;
  badge.classList.toggle("hidden", settingsFingerprint() === settingsSnapshot);
}
export function markSettingsSaved() {
  settingsSnapshot = settingsFingerprint();
  syncSettingsDirty();
}

// 生效值与全局值不一致 = 项目级 kanzei.toml 覆盖了。必须明说,否则用户会
// 一直在改一个不生效的值(D-168)。
// UI-0926 #3:模型五键不在这里报——设置页只编辑全局默认,各项目的模型覆盖由
// renderProjectModelOverrides 一行中性说明列出,点进「项目模型配置」逐键看/改。
// 这里只剩本页其余会被项目文件覆盖的标量:代理、默认模式、运行上限。
export function renderEffectiveNotice(s) {
  const box = $("settings-effective");
  if (!box) return;
  // UI-0926 #10:一行「；」拼接的长句改成 标题 + 三列表(字段 | 本页 | 实际生效)。
  const diffs = []; // [字段, 本页, 实际生效]
  const effective = s.effective;
  const unset = () => `(${t("未设")})`;
  // 只比 effective 里**确实带了的键**:后端没报的键(旧版本 / 新加的字段还没接线)
  // 一律跳过,否则 undefined 会被当成"实际生效是未设",提示条天天误报,
  // 用户很快就学会无视它,真被覆盖时反而看不见。
  const has = (key) => effective && Object.prototype.hasOwnProperty.call(effective, key);
  // 值也说人话:env/off、dev/research 是配置里的枚举,界面上写成下拉里看到的那几个词。
  const valueText = (key, value) => {
    if (value === null || value === undefined) return unset();
    if (key === "proxy") return { env: t("跟随环境变量"), off: t("直连") }[value] ?? String(value);
    if (key === "profileDefault") return { dev: t("开发"), research: t("研究") }[value] ?? String(value);
    return String(value);
  };
  for (const [key, label] of [["proxy", t("网络代理")], ["profileDefault", t("默认空间")]]) {
    if (!has(key)) continue;
    const global = s[key];
    const eff = effective[key];
    if ((eff ?? null) !== (global ?? null)) {
      diffs.push([label, valueText(key, global), valueText(key, eff)]);
    }
  }
  // 运行上限十项合成一行:项目级只要覆盖了任意一个键就弹十行会把这张表废掉。
  if (has("limits")) {
    const overridden = LIMIT_FIELDS
      .map(([, key]) => key)
      .filter((key) => (s.limits?.[key] ?? null) !== (effective.limits?.[key] ?? null));
    if (overridden.length) {
      const side = (limits) => overridden.map((key) => `${t(LIMIT_LABELS[key] ?? key)} ${limits?.[key] ?? unset()}`).join("、");
      diffs.push([t("运行上限"), side(s.limits), side(effective.limits)]);
    }
  }
  box.replaceChildren();
  box.classList.toggle("hidden", diffs.length === 0);
  if (!diffs.length) return;
  const title = document.createElement("div");
  title.textContent = `${t("当前项目的配置覆盖了这些全局值")}${s.projectConfig ? `(${s.projectConfig})` : ""}`;
  const table = document.createElement("table");
  table.className = "sv-table";
  const head = document.createElement("tr");
  for (const key of ["字段", "本页", "实际生效"]) {
    const th = document.createElement("th");
    th.textContent = t(key);
    th.dataset.i18nKey = key;
    head.appendChild(th);
  }
  const thead = document.createElement("thead");
  thead.appendChild(head);
  const tbody = document.createElement("tbody");
  for (const cells of diffs) {
    const row = document.createElement("tr");
    for (const value of cells) {
      const td = document.createElement("td");
      td.textContent = value;
      row.appendChild(td);
    }
    tbody.appendChild(row);
  }
  table.append(thead, tbody);
  box.append(title, table);
}

// UI-0926 #3:哪些项目有自己的模型配置、不用这里的默认值。中性说明(不是警告):项目覆盖
// 是正常用法,需要的是「看得见、点得进去」。每个项目一个链接,打开它的「项目模型配置」。
function projectOverrideKeyLabel(key) {
  return {
    primary: t("主模型"), fast: t("快速模型"), compact: t("压缩模型"),
    reasoning: t("思考强度"), codexFastMode: "Codex Fast mode",
  }[key] ?? key;
}
// 项目模型配置弹窗保存后,这一行要跟着变(可能刚把最后一个键恢复成继承)。只刷只读区,不碰表单。
defer(() => {
  document.addEventListener("kz-model-config-changed", async (event) => {
    if (event?.detail?.scope !== "project" || !settingsHydrated) return;
    try {
      renderProjectModelOverrides(await invoke("settings_get", { projectDir: currentProject }));
    } catch {
      // 读失败不打扰:下次进设置页 loadSettings 会重读并走它自己的错误出口。
    }
  });
});
export function renderProjectModelOverrides(s) {
  const box = $("settings-project-overrides");
  if (!box) return;
  const list = Array.isArray(s?.projectModelOverrides) ? s.projectModelOverrides : [];
  box.replaceChildren();
  box.classList.toggle("hidden", list.length === 0);
  if (!list.length) return;
  const lead = document.createElement("span");
  lead.textContent = t("这些项目有自己的模型配置,不使用这里的默认值:");
  box.appendChild(lead);
  list.forEach((entry, index) => {
    if (index > 0) box.appendChild(document.createTextNode("、"));
    const link = document.createElement("button");
    link.type = "button";
    link.className = "link-btn";
    link.dataset.project = entry.project;
    const keys = (entry.keys ?? []).map(projectOverrideKeyLabel).join("、");
    link.textContent = `${entry.name}(${keys})`;
    link.title = entry.configPath ?? entry.project;
    if (entry.current) link.dataset.current = "true";
    link.addEventListener("click", () => void openProjectModelsDialog(entry.project, { name: entry.name }));
    box.appendChild(link);
  });
}

// R-305 B1:兼容显式高级勘察复核(phase_pipeline)的角色上限。
// 未保存输入也要即时可见,避免用户调小上限后不知道会少派几个只读子代理;
// 够用时(恒为「不会截断」)不出声——那句话没有信息量(B2)。
export let settingsEffectiveSnapshot = null;
export function renderRosterCapNotice(s) {
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
  box.textContent = omitted > 0 ? `${t("高级勘察复核配置会少派只读子代理")}: ${omitted}` : "";
}

defer(() => {
  $("set-max-tasks")?.addEventListener("input", () => renderRosterCapNotice(settingsEffectiveSnapshot));
});

export function providerSourceLabel(source) {
  return {
    project: t("本项目配置"),
    global: t("全局配置"),
    builtin: t("内建"),
  }[source] || source;
}
export function agentSourceLabel(source) {
  return { builtin: t("内建"), global: t("全局"), project: t("项目") }[source] || source;
}
export function agentStatusLabel(status) {
  return {
    available: t("可用"),
    configurationError: t("配置错误"),
    hidden: t("当前档位隐藏"),
  }[status] || status;
}
// 子代理定义里的档位/模式/模型角色是后端的原始枚举(dev/research、primary/subagent、primary/fast/compact):
// 中文界面显示与模型配置区同一套词;不在表里的值(自定义模型 id 等)原样显示。
export function agentEnumLabel(kind, value) {
  const words = {
    profile: { dev: t("开发"), research: t("研究"), all: t("全部") },
    mode: { primary: t("主对话"), subagent: t("子代理") },
    model: { primary: t("主模型"), fast: t("快速模型"), compact: t("压缩模型") },
  }[kind] || {};
  return words[value] ?? value;
}
export function appendAgentField(card, label, value) {
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
// 最近一次渲染的子代理定义快照:切语言时据此重画,不必再问后端。
let agentDirectorySnapshot = null;
export function renderAgentDirectory(snapshot) {
  agentDirectorySnapshot = snapshot;
  const host = $("agent-directory");
  if (!host) return;
  host.replaceChildren();
  const agents = Array.isArray(snapshot?.agents) ? snapshot.agents : [];
  if (agents.length === 0) {
    const empty = document.createElement("p");
    empty.className = "dim";
    empty.textContent = t("没有子代理定义");
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
    appendAgentField(card, t("档位"), agentEnumLabel("profile", agent.profile));
    appendAgentField(card, t("模式"), agentEnumLabel("mode", agent.mode));
    appendAgentField(card, t("模型"), agentEnumLabel("model", agent.model));
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
export async function loadAgentDirectory() {
  const status = $("agent-directory-status");
  try {
    const snapshot = await invoke("agent_directory_get", {
      projectDir: currentProject || null,
      profile: $("set-profile")?.value || null,
    });
    renderAgentDirectory(snapshot);
    if (status) status.textContent = `${snapshot.agents?.length || 0} ${t("个")}`;
  } catch (err) {
    if (status) status.textContent = t("子代理定义读取失败");
    const host = $("agent-directory");
    if (host) {
      host.replaceChildren();
      const error = document.createElement("p");
      error.className = "form-hint-warn";
      error.textContent = `${t("子代理定义读取失败")}: ${err}`;
      host.append(error);
    }
  }
}
defer(() => {
  $("agent-directory-refresh")?.addEventListener("click", () => void loadAgentDirectory());
});
defer(() => {
  $("set-profile")?.addEventListener("change", () => void loadAgentDirectory());
});


// 模型角色改成真下拉:自由文本框要手打 `provider:model`,拼错一个字母要到运行时
// 才炸,而那时人早已离开设置页。这里从各 provider 探测到的清单里选,手填只作兜底。
export let knownModelIds = [];
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
export function applyModelOptions(desired, ids) {
  const roles = [[$("set-primary"), "primary"], [$("set-fast"), "fast"], [$("set-compact"), "compact"]].filter(([el]) => el);
  if (!roles.length) return;
  const current = desired ? null : roles.map(([el]) => el.value);
  knownModelIds = ids;
  roles.forEach(([select, role], index) => {
    const keep = desired ? (desired[role] ?? "") : current[index];
    select.innerHTML = "";
    const none = document.createElement("option");
    none.value = "";
    none.textContent = role === "fast" || role === "compact"
      ? t("(未设 · 跟随主模型)")
      : t("(未设 · 用内置默认)");
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
export async function probeModelsAndMergeOptions(token) {
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
export function ensureSelectOption(select, value) {
  if (!select || !value) return;
  if ([...select.options].some((o) => o.value === value)) return;
  const opt = document.createElement("option");
  opt.value = value;
  opt.textContent = value;
  select.appendChild(opt);
}

// 手填分支:两个角色下拉共用。选中哨兵值时弹输入,校验格式后插回列表。
export function wireManualModelRole(id) {
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
defer(() => {
  wireManualModelRole("set-primary");
});
defer(() => {
  wireManualModelRole("set-fast");
});
defer(() => {
  wireManualModelRole("set-compact");
});
defer(() => {
  $("models-refresh")?.addEventListener("click", async () => {
    const ok = await probeModelsAndMergeOptions(settingsLoadToken);
    if (ok) toast(`${t("已重新探测")}:${knownModelIds.length}`);
  });
});

// R-136:fast 子代理模型的就绪状态与一键安装。此前要用户手工装 Ollama、
// 手工 pull、手工配置——三步里断任何一步,记忆整理/快速记录这些子代理杂活
// 就全部静默失效,而界面上毫无线索。
export async function refreshFastStatus() {
  const status = $("fast-status");
  const btn = $("fast-setup");
  if (!status || !btn) return;
  let s;
  try {
    s = await invoke("fast_model_status");
  } catch (error) {
    status.textContent = `${t("快速模型状态获取失败")}:${error}`;
    status.classList.remove("hidden");
    status.classList.add("warn-text");
    btn.classList.add("hidden");
    return;
  }
  // UI-0926 #3:fast 指向外部 provider(不由本机托管)时这一行对用户没有信息量,整行收起。
  if (!s.managed) {
    status.textContent = "";
    status.classList.add("hidden");
    btn.classList.add("hidden");
    return;
  }
  status.classList.remove("hidden");
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
defer(() => {
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
});

// [limits] 表单:输入框 id ↔ 后端 camelCase 键。加参数时只改这一张表,
// 读取与保存两侧都走它,不会再出现"读了没存"或"存了没读"的半边接线。
export const LIMIT_FIELDS = [
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

// 键 → 界面名(与 index.html 里各输入框的 label 同一套词;项目覆盖提示表也用它,别把 maxTasksPerTurn 这类键名露给用户)。
export const LIMIT_LABELS = {
  maxTokens: "主对话输出上限",
  subagentMaxTokens: "子代理输出上限",
  subagentTimeoutSecs: "子代理时长上限",
  maxTasksPerTurn: "单轮子代理数上限",
  contextBudgetRatio: "压缩触发线",
  recentVerbatimRatio: "压缩保留近期",
  maxParallelTools: "并行工具数上限",
  streamRestarts: "流中断重试次数",
  transportRetries: "传输重试次数",
  rateLimitRetries: "限流重试次数",
};

// 节奏(R-157):id ↔ settings_get 返回的 cadence snake_case 键。
// 留空 = None,保存时该键从 [cadence] 移除,回落 §1.4 默认。
export const CADENCE_FIELDS = [
  ["set-cadence-full-test", "full_test"],
  ["set-cadence-targeted-test", "targeted_test"],
  ["set-cadence-commit", "commit"],
  ["set-cadence-push", "push"],
];

export function collectCadence() {
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
export function collectLimits() {
  const out = {};
  for (const [id, key] of LIMIT_FIELDS) {
    const raw = $(id).value.trim();
    const value = raw === "" ? null : Number(raw);
    out[key] = value === null || Number.isNaN(value) ? null : value;
  }
  return out;
}

// 每次加载自增;await 回来对不上就说明用户已经切走/重载过,回填一律丢弃。
export let settingsLoadToken = 0;
// 首次成功回填后置 true。没有它的话,空表单的指纹和「干净基线」永远对不上,
// 脏值守卫会在第一次加载时就把自己挡掉。
export let settingsHydrated = false;
// 表单有未保存改动时**拒绝**用磁盘值覆盖,并把「为什么没刷新」说出来——
// 静悄悄地把用户填了一半的东西回滚成磁盘值,正是「突然刷新」最恼人的那一下。
export function showSettingsStale() {
  const el = $("settings-stale");
  if (el) el.classList.remove("hidden");
}
export function hideSettingsStale() {
  const el = $("settings-stale");
  if (el) el.classList.add("hidden");
}
defer(() => {
  $("settings-discard")?.addEventListener("click", () => {
    settingsHydrated = false;
    void loadSettings({ force: true });
  });
});
export async function loadSettings({ force = false } = {}) {
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
  renderProjectModelOverrides(s);
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
  void loadOpenTools();
}
export function hydrateSettingsForm(s) {
  // UX-119:语言的真源是 app.json 的 ui_layout.prefs.language(启动时已同步进 localStorage 缓存),
  // 不再从 kanzei.toml 回填——否则旧文件里的陈值会在进设置页时把下拉改回去。
  const storedLanguage = normalizeLanguagePreference(localStorage.getItem("kz-language"));
  // rerender:false —— persist:false 时语言压根没变,整串重渲(applyLanguage +
  // 侧栏 + 工作区 + refreshWorktrees + refreshConversationList + 多画一遍 provider 表)
  // 是纯白干,还让整个界面在进设置页时抖一下。
  setLanguagePreference(storedLanguage, { persist: false, rerender: false });
  // UI-0926 #3:不再有「保存到」作用域——本页只编辑全局默认,项目级模型覆盖走「项目模型配置」。
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
    if (fallback === undefined) defaultPlaceholders.delete(id);
    else defaultPlaceholders.set(id, fallback);
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
  if (defaultBatches === undefined || defaultBatches === null) defaultPlaceholders.delete("set-cadence-full-test-batches");
  else defaultPlaceholders.set("set-cadence-full-test-batches", defaultBatches);
  batchesEl.placeholder = defaultBatches === undefined || defaultBatches === null ? "" : `${t("默认")} ${defaultBatches}`;
  // R-170:cadence 表单回填即止,不再联动继续文案(规则剥离,文案仅承载用户意图)。
  settingsProviders = s.providers;
  renderProviders();
  // 刚从磁盘读回来 = 干净态,以此为基准比对后续改动。
  markSettingsSaved();
  // R-187:提示音是本地偏好(不进 kanzei.toml),设置页控件回填 + change 即存。
  loadSoundSettingsControls();
}

// 输入框占位「默认 N」(元素 id → 默认值):切语言时只重写占位文字,不动输入框里的值。
const defaultPlaceholders = new Map();
function refreshDefaultPlaceholders() {
  for (const [id, value] of defaultPlaceholders) {
    const el = $(id);
    if (el) el.placeholder = `${t("默认")} ${value}`;
  }
}
export function loadSoundSettingsControls() {
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

export function bindSoundSettingsControls() {
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
defer(() => {
  bindSoundSettingsControls();
  // 后端 ui_layout 稍后到达(启动时先按缓存落地):提示音控件按到达的值回填一次。
  onLayoutChange((section) => { if (section === "*" || section === "prefs") loadSoundSettingsControls(); });
});
defer(() => {
  for (const id of SETTINGS_FORM_IDS) {
    $(id)?.addEventListener("input", syncSettingsDirty);
    $(id)?.addEventListener("change", syncSettingsDirty);
  };
});
// checkbox 只有 change 有意义(input 事件对它不触发脏状态之外的语义)。
defer(() => {
  for (const id of SETTINGS_TOGGLE_IDS) {
    $(id)?.addEventListener("change", syncSettingsDirty);
  };
});
// provider 表格是动态重建的,逐行绑会随重绘丢失;在容器上做事件委托一次覆盖全表。
// 委托要在捕获阶段之后跑——行内的 input 监听器先把值写回 settingsProviders,
// 我们才比对得到新指纹。
defer(() => {
  for (const event of ["input", "change"]) {
    $("providers-table")?.addEventListener(event, () => setTimeout(syncSettingsDirty, 0));
  };
});

// R-184 P6(D-247):选「指定地址」却留空时,后端按空串回落 env——这是静默降级,
// 界面必须把「将回落环境变量」说出来,不许用户以为地址已生效。
export function updateProxyHint() {
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
defer(() => {
  $("set-proxy-mode").addEventListener("change", () => {
    $("set-proxy-url").classList.toggle("hidden", $("set-proxy-mode").value !== "custom");
    // 留空时输入框保持可见,否则提示「回落」但地址框都找不到,更迷惑。
    if ($("set-proxy-mode").value === "custom") $("set-proxy-url").classList.remove("hidden");
    updateProxyHint();
  });
});
defer(() => {
  $("set-proxy-url").addEventListener("input", updateProxyHint);
});

// UX-135:服务起来后显示手机能直接输入的访问地址与配对码,并给「复制地址 / 复制配对链接」。
// 此前只有一行「0.0.0.0:随机端口 · token xxx」,手机没法照着输,端口还每次变。
// 配对链接 = 访问地址 + `#pair=配对码`:手机打开即自动填好配对码(片段不会发给服务器)。
defer(() => {
  $("provider-add").addEventListener("click", () => {
    settingsProviders.push({ name: "", protocol: "openai", baseUrl: "http://", apiKeyEnv: "" });
    renderProviders();
    syncSettingsDirty();
  });
});

defer(() => {
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
});

// UX-118:表格里没填名称的行,后端会静默丢掉——保存照样提示「已保存」,重开设置才发现它没了。
// 保存前先拦下并指到那一格;完全没动过的空行(点了「+ 添加」又没填)直接丢掉,不拿它打断保存。
function isBlankProviderRow(p) {
  const url = (p.baseUrl ?? "").trim();
  return !(p.name ?? "").trim() && (url === "" || /^https?:\/\/$/i.test(url))
    && !(p.apiKeyEnv ?? "").trim() && !(p.apiKey ?? "").trim() && !p.auth;
}
export function validateProviders() {
  if (settingsProviders.some(isBlankProviderRow)) {
    settingsProviders = settingsProviders.filter((p) => !isBlankProviderRow(p));
    renderProviders();
    syncSettingsDirty();
  }
  const seen = new Set();
  for (const p of settingsProviders) {
    const name = (p.name ?? "").trim();
    let problem = null;
    let field = "name";
    if (!name) problem = t("Provider 还没有名称:请填写,或点 × 删除这一行");
    else if (seen.has(name)) problem = `${t("Provider 名称重复")}: ${name}`;
    else if (!/^https?:\/\/\S+/i.test((p.baseUrl ?? "").trim())) {
      problem = `${name}: ${t("Base URL 要以 http:// 或 https:// 开头")}`;
      field = "url";
    }
    seen.add(name);
    if (!problem) continue;
    const group = $("sg-providers");
    if (group) group.open = true;
    const input = providerInputs.get(p)?.[field];
    if (input) {
      input.setAttribute("aria-invalid", "true");
      input.focus?.();
      input.scrollIntoView?.({ block: "center" });
    }
    toast(problem, { kind: "warn" });
    return false;
  }
  return true;
}

defer(() => {
  $("settings-save").addEventListener("click", async () => {
    if (!validateProviders()) return;
    const mode = $("set-proxy-mode").value;
    const proxy = mode === "custom" ? $("set-proxy-url").value.trim() : mode;
    try {
      // UI-0926 #3:本页只写全局 ~/.kanzei/kanzei.toml(不再带 scope/projectDir)。
      await invoke("settings_save", {
        payload: {
          // 界面语言不在这里:它切换即存(persistLanguagePreference,UX-119),kanzei.toml 里的旧值保存时移除。
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
      // 全局默认变了:输入框上方的「下一轮将使用」重新解析(provider 可能也改了,目录一并重拉)。
      document.dispatchEvent(new CustomEvent("kz-model-config-changed", { detail: { scope: "global" } }));
    } catch (err) {
      toastError(`${t("保存失败")}: ${err}`, { retry: () => $("settings-save").click() });
    }
  });
});

defer(() => {
  $("settings-open").addEventListener("click", () => invoke("settings_open").catch((e) => toastError(String(e), { retry: () => $("settings-open").click() })));
});

defer(() => {
  $("export-pick-dir").addEventListener("click", async () => {
    try {
      const path = await invoke("export_pick_dir");
      if (path) $("export-output-dir").value = path;
    } catch (error) {
      toastError(`${t("选择导出目录失败")}:${error}`);
    }
  });
});
defer(() => {
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
});

// ---------- 版本与更新(GitHub Releases 为源) ----------
export let updateUrl = null;
// D-287:「没有可装的东西」有三种成因,以前一律渲染成「已是最新(<latest>)」——
// 于是「当前版本 a7a122a」下面紧挨着「已是最新(build-c99304f)」,两个 hash 打架,
// 看着就像更新检查坏了。只有 status=latest 这一态有资格说「已是最新」;本地领先
// 与无法比较各自说自己的话(D-004:不做的理由要说出来),别人的 hash 一律标成
// 「最新发布」,不冒充「当前」。
export function updateResultText(r) {
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
defer(() => {
  $("update-check").addEventListener("click", async () => {
    $("update-result").textContent = t("检查中…");
    $("update-install").classList.add("hidden");
    // #7:进行中的按钮统一由 button[aria-busy="true"] 转圈(读屏也能读到「忙」)。
    $("update-check").setAttribute("aria-busy", "true");
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
    } finally {
      $("update-check").removeAttribute("aria-busy");
    }
  });
});
// 还在跑的线(含停止中、鞭挞轮间等待):更新会先停掉全部运行与后台终端再退出,有活干时必须让用户点头。
export function runningLineCount() {
  let count = 0;
  for (const lineState of sessionStates.values()) {
    if (lineState.running || ["starting", "running", "stopping", "auto_pending"].includes(lineState.phase)) count += 1;
  }
  return count;
}
defer(() => {
  $("update-install").addEventListener("click", async () => {
    if (!updateUrl) return;
    const busy = runningLineCount();
    if (busy > 0) {
      const ok = await confirmDialog({
        title: t("现在更新并退出?"),
        message: `${t("仍在运行的对话")}: ${busy}。${t("更新会先停止它们和后台终端,再退出应用;装完后需要你手动启动。")}`,
        okText: t("停止并更新"),
        danger: true,
      });
      if (!ok) return;
    }
    $("update-result").textContent = t("下载中…(应用将退出,安装完成后请手动启动)");
    $("update-install").disabled = true;
    $("update-install").setAttribute("aria-busy", "true");
    try {
      $("update-result").textContent = await invoke("update_install", { url: updateUrl });
    } catch (err) {
      $("update-result").textContent = String(err);
    } finally {
      $("update-install").disabled = false;
      $("update-install").removeAttribute("aria-busy");
    }
  });
});

// ---------- 切语言:设置页里靠 JS 渲染的几块跟着重画(UX-160) ----------
// 覆盖提示、项目模型覆盖、子代理数提示、fast 状态、打开方式清单、子代理定义都是渲染点里调 t() 写出来的,
// 不挂 data-i18n——不重画就会留着上一种语言(切到 English 后这几块仍是中文)。
defer(() => {
  document.addEventListener("kz:language", () => {
    const loaded = settingsEffectiveSnapshot;
    if (loaded) {
      renderEffectiveNotice(loaded);
      renderProjectModelOverrides(loaded);
      renderRosterCapNotice(loaded);
    }
    renderOpenTools(openTools);
    if (agentDirectorySnapshot) {
      renderAgentDirectory(agentDirectorySnapshot);
      const agentCount = $("agent-directory-status");
      if (agentCount) agentCount.textContent = `${agentDirectorySnapshot.agents?.length || 0} ${t("个")}`;
    }
    // 三个模型下拉里的「(未设 · …)」「＋ 手填模型…」是 JS 写的 option:按当前值重建一遍,值不动。
    if (Array.isArray(knownModelIds) && $("set-primary")?.options.length) applyModelOptions(null, knownModelIds);
    if (lastPermissionRules) renderPermissionRules(lastPermissionRules.data, lastPermissionRules.projectDir);
    refreshDefaultPlaceholders();
    void refreshFastStatus();
  });
});

// ---------- 分区目录(UX-112) ----------
// 设置页十几个可折叠分组,一屏看不全。目录按各分组的 summary 现场生成(别处新增分组自动进目录),
// 点一下展开该组并滚到它;文案挂 data-i18n-key,切语言时由 applyDataI18nKeys 重译。
export function buildSettingsToc() {
  const nav = $("settings-toc");
  if (!nav) return;
  nav.replaceChildren();
  for (const group of document.querySelectorAll(".settings-group")) {
    if (group.parentNode?.id !== "settings-scroll") continue;
    const summary = group.querySelector("summary");
    const key = summary?.dataset?.i18nKey || summary?.textContent;
    if (!key) continue;
    const link = document.createElement("button");
    link.type = "button";
    link.dataset.i18nKey = key;
    link.textContent = t(key);
    link.addEventListener("click", () => {
      group.open = true;
      group.scrollIntoView?.({ block: "start" });
      summary.focus?.({ preventScroll: true });
    });
    nav.appendChild(link);
  }
}
defer(() => {
  buildSettingsToc();
});
// 别处「去设置里的某一组」:先 navigate_view("settings"),再发 kz:open-settings-section { id },
// 这里等页面显出来后展开该组、滚到它(例:右键菜单「在设置里添加打开方式…」落到「打开方式」,而不是设置页顶部)。
defer(() => {
  document.addEventListener("kz:open-settings-section", (event) => {
    const group = $(event.detail?.id);
    if (!group?.classList?.contains("settings-group")) return;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      group.open = true;
      group.scrollIntoView?.({ block: "start" });
      group.querySelector("summary")?.focus?.({ preventScroll: true });
    }));
  });
});

// ---------- 恢复默认(高级运行参数 / 验证与提交节奏) ----------
// 把整组输入留空 = 回到内置默认;和别的改动一样要点「保存」才落盘,所以只清表单、照常标脏。
function clearSettingsFields(ids) {
  for (const id of ids) {
    const el = $(id);
    if (el) el.value = "";
  }
  renderRosterCapNotice(settingsEffectiveSnapshot);
  syncSettingsDirty();
}
defer(() => {
  $("limits-reset")?.addEventListener("click", () => clearSettingsFields(LIMIT_FIELDS.map(([id]) => id)));
  $("cadence-reset")?.addEventListener("click", () =>
    clearSettingsFields([...CADENCE_FIELDS.map(([id]) => id), "set-cadence-full-test-batches"]));
});

// ---------- 设置 → 打开方式(open_tools_list / open_tools_save) ----------
// 右键项目/对话的「用 {工具} 打开」读的就是这张清单:内置项(VS Code、Windows 终端…)由后端探测是否已安装,
// 用户自配项存 app.json 的 open_tools。与「保存」表单无关:添加/删除即时写盘,失败当场报。
export let openTools = [];
const OPEN_TOOL_DEFAULT_ARGS = "{path}";
/// 参数按空白切开,双引号包住的整段算一个(路径里有空格时用)。
export function parseToolArgs(text) {
  const out = [];
  const re = /"([^"]*)"|(\S+)/g;
  let match;
  while ((match = re.exec(String(text ?? ""))) !== null) out.push(match[1] ?? match[2]);
  return out;
}
export function formatToolArgs(args) {
  return (Array.isArray(args) ? args : []).map((arg) => (/\s/.test(arg) ? `"${arg}"` : arg)).join(" ");
}
function setOpenToolsStatus(text, { warn = false } = {}) {
  const el = $("open-tools-status");
  if (!el) return;
  el.textContent = text;
  el.classList.toggle("warn-text", warn);
}
export function renderOpenTools(list) {
  openTools = Array.isArray(list) ? list : [];
  const host = $("open-tools-list");
  if (!host) return;
  host.replaceChildren();
  if (!openTools.length) {
    const empty = document.createElement("li");
    empty.className = "dim";
    empty.textContent = t("还没有可用的打开方式");
    host.appendChild(empty);
    return;
  }
  for (const tool of openTools) {
    const row = document.createElement("li");
    row.dataset.toolId = tool.id;
    const name = document.createElement("span");
    name.className = "open-tool-name";
    name.textContent = tool.builtin ? t(tool.label) : tool.label;
    const meta = document.createElement("span");
    meta.className = "open-tool-meta";
    meta.textContent = tool.builtin ? t("内置") : [tool.command, formatToolArgs(tool.args)].filter(Boolean).join(" ");
    const state = document.createElement("span");
    state.className = "open-tool-state";
    state.dataset.available = String(Boolean(tool.available));
    state.textContent = tool.available ? t("可用") : t("未检测到");
    row.append(name, meta, state);
    if (!tool.builtin) {
      const remove = document.createElement("button");
      remove.type = "button";
      remove.className = "icon-btn";
      remove.textContent = "×";
      remove.title = t("删除");
      remove.setAttribute("aria-label", `${t("删除")} ${tool.label}`);
      remove.addEventListener("click", () => void removeOpenTool(tool.id));
      row.appendChild(remove);
    }
    host.appendChild(row);
  }
}
export async function loadOpenTools() {
  try {
    renderOpenTools(await invoke("open_tools_list"));
  } catch (err) {
    setOpenToolsStatus(`${t("读取打开方式失败")}: ${err}`, { warn: true });
  }
}
/// 自配项的写盘载荷。列表里的自配项必须带回 command/args(后端 open_tools_list 返回它们),
/// 缺了就别写——拿空命令覆盖掉用户配好的工具比保存失败糟得多。
export function customOpenToolsPayload(list = openTools) {
  const custom = list.filter((tool) => !tool.builtin);
  if (custom.some((tool) => typeof tool.command !== "string")) return null;
  return custom.map((tool) => ({
    id: tool.id,
    label: tool.label,
    command: tool.command,
    args: Array.isArray(tool.args) ? tool.args : parseToolArgs(OPEN_TOOL_DEFAULT_ARGS),
  }));
}
async function saveOpenTools(tools, doneText) {
  try {
    await invoke("open_tools_save", { tools });
    await loadOpenTools();
    setOpenToolsStatus(doneText);
    // 右键菜单等读清单的地方据此刷新缓存。
    document.dispatchEvent(new CustomEvent("kz:open-tools-changed"));
    return true;
  } catch (err) {
    setOpenToolsStatus(`${t("保存失败")}: ${err}`, { warn: true });
    return false;
  }
}
export async function removeOpenTool(id) {
  const kept = customOpenToolsPayload(openTools.filter((tool) => tool.id !== id));
  if (!kept) {
    setOpenToolsStatus(t("读不到已有自配工具的命令,先点「重新检测」再试"), { warn: true });
    return false;
  }
  return saveOpenTools(kept, t("已删除"));
}
export async function addOpenTool() {
  const label = $("open-tool-label").value.trim();
  const command = $("open-tool-command").value.trim();
  const args = parseToolArgs($("open-tool-args").value);
  // 参数里没写 {path} 就补在最后(与后端 validate_tools 同口径):工具总得拿到要打开的文件夹。
  if (!args.some((arg) => arg.includes("{path}"))) args.push(OPEN_TOOL_DEFAULT_ARGS);
  const problem = !label ? t("请填写名称") : !command ? t("请填写命令") : null;
  if (problem) {
    setOpenToolsStatus(problem, { warn: true });
    return false;
  }
  if (openTools.some((tool) => tool.label === label)) {
    setOpenToolsStatus(`${t("已有同名的打开方式")}: ${label}`, { warn: true });
    return false;
  }
  const kept = customOpenToolsPayload();
  if (!kept) {
    setOpenToolsStatus(t("读不到已有自配工具的命令,先点「重新检测」再试"), { warn: true });
    return false;
  }
  // id 留空:后端按 custom-N 分配不重名的(标识只允许字母数字 - _,名称里的中文放不进去)。
  const ok = await saveOpenTools([...kept, { id: "", label, command, args }], t("已添加"));
  if (ok) {
    $("open-tool-label").value = "";
    $("open-tool-command").value = "";
    $("open-tool-args").value = OPEN_TOOL_DEFAULT_ARGS;
  }
  return ok;
}
defer(() => {
  $("open-tool-add")?.addEventListener("click", () => void addOpenTool());
  $("open-tools-refresh")?.addEventListener("click", async () => {
    setOpenToolsStatus(`${t("检测中")}…`);
    await loadOpenTools();
    setOpenToolsStatus("");
  });
});

// ---------- 侧边栏分区折叠:标题文字收/展,记忆到 localStorage ----------
defer(() => {
  document.querySelectorAll(".sidebar-section").forEach((section) => {
    const title = section.querySelector(".section-title > span:first-child");
    if (!title) return;
    const collapseKey = section.dataset.collapseKey || title.textContent.replace(/[\d\s]/g, "").slice(0, 8);
    const key = `kz-collapse-${collapseKey}`;
    const legacyKey = `kz-collapse-${title.textContent.replace(/[\d\s]/g, "").slice(0, 8)}`;
    const saved = localStorage.getItem(key) ?? (legacyKey === key ? null : localStorage.getItem(legacyKey));
    // data-collapse-default="collapsed":没有存过偏好时默认收起(机制保留;侧栏「项目」列表删掉后,
    // 目前没有分区用它——项目切换只剩侧栏头部的项目卡菜单,UI2-0926 #1)。
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
});
