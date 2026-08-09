// ---------- 设置 ----------
let settingsProviders = [];

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
    for (const proto of ["anthropic", "openai", "openai-responses"]) {
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
    const removeBtn = document.createElement("button");
    removeBtn.className = "icon-btn";
    removeBtn.textContent = "×";
    removeBtn.setAttribute("aria-label", `${t("移除 provider")} ${p.name || index + 1}`);
    removeBtn.addEventListener("click", () => {
      settingsProviders.splice(index, 1);
      renderProviders();
    });
    tdRemove.appendChild(removeBtn);

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
    toastError(`删除失败: ${err}`, { retry: () => deletePermissionRule(rule) });
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
      if (!confirm(`${t("删除")} ${rule.action} / ${rule.resource}？`)) return;
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
    toastError(`读取权限规则失败: ${err}`, { retry: loadPermissionRules });
  }
}
// D-157:设置页是一张表单,填了不点保存不生效。此前没有任何提示,于是界面显示
// deepseek、运行却用 anthropic,而报错只说"provider anthropic 需要环境变量",
// 完全看不出"你以为改了的那个根本没生效"。这里做脏状态可见。
const SETTINGS_FORM_IDS = [
  "set-primary", "set-fast", "set-profile", "set-reasoning",
  "set-proxy-mode", "set-proxy-url",
  // 运行上限也算表单的一部分:漏登记就会出现"改了数字却没有未保存提示",
  // 而这正是 D-157 那条"界面显示 A、运行用 B"的复现路径。
  "set-max-tokens", "set-subagent-max-tokens", "set-subagent-timeout", "set-max-tasks",
  "set-context-ratio", "set-verbatim-ratio", "set-max-parallel", "set-stream-restarts",
  "set-transport-retries", "set-rate-retries",
];
let settingsSnapshot = "";
function settingsFingerprint() {
  // provider 表格是动态行,单独序列化;它和标量字段一起构成"这张表单当前的样子"。
  const scalars = SETTINGS_FORM_IDS.map((id) => `${id}=${$(id)?.value ?? ""}`).join("|");
  const providers = JSON.stringify(
    settingsProviders.map((p) => [p.name, p.protocol, p.baseUrl, p.apiKeyEnv, p.apiKey, p.contextLimit]),
  );
  return `${scalars}||${providers}`;
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
  for (const [key, label] of [["primary", "primary"], ["fast", "fast"], ["reasoning", "思考强度"]]) {
    const global = key === "reasoning" ? (s.reasoning === "off" ? null : s.reasoning) : s[key];
    const eff = s.effective?.[key] ?? null;
    if (s.effective && (eff ?? null) !== (global ?? null)) {
      diffs.push(`${label}:本页 ${global ?? "(未设)"} → 实际生效 ${eff ?? "(未设)"}`);
    }
  }
  box.classList.toggle("hidden", diffs.length === 0);
  if (diffs.length) {
    box.textContent =
      `${t("以下项被项目级配置覆盖,本页的改动不会生效")}:${diffs.join("；")}` +
      (s.projectConfig ? `(${s.projectConfig})` : "");
  }
}

// 模型角色改成真下拉:自由文本框要手打 `provider:model`,拼错一个字母要到运行时
// 才炸,而那时人早已离开设置页。这里从各 provider 探测到的清单里选,手填只作兜底。
let knownModelIds = [];
async function fillKnownModels(preserve = true) {
  const roles = [$("set-primary"), $("set-fast")].filter(Boolean);
  if (!roles.length) return;
  const current = roles.map((el) => el.value);
  try {
    const models = await invoke("models_list", { projectDir: currentProject });
    // 角色不能再指向角色(primary → primary 会绕成死循环)。
    knownModelIds = models.map((m) => m.id).filter((id) => id !== "primary" && id !== "fast");
  } catch {
    knownModelIds = [];
  }
  roles.forEach((select, index) => {
    const keep = preserve ? current[index] : select.value;
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
      opt.textContent = `${keep}(手填)`;
      select.appendChild(opt);
    }
    const manual = document.createElement("option");
    manual.value = MANUAL_MODEL_SENTINEL;
    manual.textContent = t("＋ 手填模型…");
    select.appendChild(manual);
    select.value = keep ?? "";
  });
}

// 手填分支:两个角色下拉共用。选中哨兵值时弹输入,校验格式后插回列表。
function wireManualModelRole(id) {
  const select = $(id);
  if (!select) return;
  let last = select.value;
  select.addEventListener("change", () => {
    if (select.value !== MANUAL_MODEL_SENTINEL) {
      last = select.value;
      return;
    }
    const input = (window.prompt(t("填 provider:model,例如 deepseek:deepseek-chat")) || "").trim();
    if (!/^[\w.-]+:.+$/.test(input)) {
      if (input) toast(t("格式应为 provider:model"));
      select.value = last;
      return;
    }
    const opt = document.createElement("option");
    opt.value = input;
    opt.textContent = `${input}(手填)`;
    select.insertBefore(opt, select.lastElementChild);
    select.value = input;
    last = input;
    syncSettingsDirty();
  });
}
wireManualModelRole("set-primary");
wireManualModelRole("set-fast");
$("models-refresh")?.addEventListener("click", async () => {
  await fillKnownModels();
  toast(`${t("已重新探测")}:${knownModelIds.length}`);
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
  } catch {
    return;
  }
  if (!s.managed) {
    status.textContent = t("fast 指向外部 provider,不由本机托管");
    btn.classList.add("hidden");
    return;
  }
  if (s.ready) {
    status.textContent = `✓ ${t("子代理就绪")}(${s.model})`;
    status.classList.remove("warn-text");
    btn.classList.add("hidden");
    return;
  }
  const missing = !s.installed
    ? t("Ollama 未安装")
    : !s.serviceUp
      ? t("Ollama 服务未运行")
      : `${t("模型未拉取")}(${s.model})`;
  status.textContent = `⚠ ${missing} — ${t("子代理杂活(记忆整理/快速记录)暂不可用")}`;
  status.classList.add("warn-text");
  btn.classList.remove("hidden");
}
on("kz:fast-setup", (event) => {
  const text = event.payload?.text;
  if (!text) return;
  $("fast-status").textContent = text;
  log(`${t("子代理安装")}:${text}`);
});
$("fast-setup")?.addEventListener("click", async () => {
  const btn = $("fast-setup");
  btn.disabled = true;
  try {
    const done = await invoke("fast_model_setup");
    toast(done);
    await fillKnownModels();
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

async function loadSettings() {
  let s;
  try {
    s = await invoke("settings_get", { projectDir: currentProject });
  } catch (err) {
    // 配置损坏时不能留一张空白表单让用户无从下手(保存会把默认值写回,反而丢配置)。
    $("settings-path").textContent = t("配置读取失败");
    toastError(`设置读取失败:${err}`, { retry: loadSettings });
    return;
  }
  $("settings-path").textContent = s.path;
  // 先把已保存的值塞进去,再让 fillKnownModels 以它为基准建选项:
  // 顺序反了的话,探测不到的已存值会在建表时丢失,一保存就把配置改坏。
  $("set-primary").value = s.primary ?? "";
  $("set-fast").value = s.fast ?? "";
  await fillKnownModels();
  $("set-primary").value = s.primary ?? "";
  $("set-fast").value = s.fast ?? "";
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
  const proxy = s.proxy;
  if (proxy === "env" || proxy === "off") {
    $("set-proxy-mode").value = proxy;
    $("set-proxy-url").classList.add("hidden");
  } else {
    $("set-proxy-mode").value = "custom";
    $("set-proxy-url").value = proxy;
    $("set-proxy-url").classList.remove("hidden");
  }
  settingsProviders = s.providers;
  renderProviders();
  renderEffectiveNotice(s);
  refreshFastStatus();
  // 刚从磁盘读回来 = 干净态,以此为基准比对后续改动。
  markSettingsSaved();
  loadPermissionRules();
}
for (const id of SETTINGS_FORM_IDS) {
  $(id)?.addEventListener("input", syncSettingsDirty);
  $(id)?.addEventListener("change", syncSettingsDirty);
}
// provider 表格是动态重建的,逐行绑会随重绘丢失;在容器上做事件委托一次覆盖全表。
// 委托要在捕获阶段之后跑——行内的 input 监听器先把值写回 settingsProviders,
// 我们才比对得到新指纹。
for (const event of ["input", "change"]) {
  $("providers-table")?.addEventListener(event, () => setTimeout(syncSettingsDirty, 0));
}

$("set-proxy-mode").addEventListener("change", () => {
  $("set-proxy-url").classList.toggle("hidden", $("set-proxy-mode").value !== "custom");
});

$("mobile-service-start").addEventListener("click", async () => {
  try {
    const info = await invoke("mobile_service_start", { projectDir: currentProject, port: null });
    $("mobile-service-status").textContent = `${info.address} · token ${info.token}`;
    $("mobile-service-start").classList.add("hidden");
    $("mobile-service-stop").classList.remove("hidden");
    toast(t("移动端本机桥接已启动"));
  } catch (error) {
    toastError(`启动移动端桥接失败:${error}`, { retry: () => $("mobile-service-start").click() });
  }
});
$("mobile-service-stop").addEventListener("click", async () => {
  try {
    await invoke("mobile_service_stop");
    $("mobile-service-status").textContent = t("移动端本机桥接已停止");
    $("mobile-service-start").classList.remove("hidden");
    $("mobile-service-stop").classList.add("hidden");
  } catch (error) {
    toastError(`停止移动端桥接失败:${error}`, { retry: () => $("mobile-service-stop").click() });
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
  try {
    await invoke("settings_save", {
      payload: {
        primary: $("set-primary").value,
        fast: $("set-fast").value,
        proxy,
        profileDefault: $("set-profile").value,
        reasoning: $("set-reasoning").value,
        codexFastMode: $("set-codex-fast-mode").checked,
        limits: collectLimits(),
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
    loadSettings();
  } catch (err) {
    toastError(`保存失败: ${err}`, { retry: () => $("settings-save").click() });
  }
});

$("settings-open").addEventListener("click", () => invoke("settings_open").catch((e) => toastError(String(e), { retry: () => $("settings-open").click() })));

$("export-pick-dir").addEventListener("click", async () => {
  try {
    const path = await invoke("export_pick_dir");
    if (path) $("export-output-dir").value = path;
  } catch (error) {
    toastError(`选择导出目录失败:${error}`);
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
    toastError(`导出失败:${error}`);
  } finally {
    button.disabled = false;
  }
});

// ---------- 版本与更新(GitHub Releases 为源) ----------
let updateUrl = null;
$("update-check").addEventListener("click", async () => {
  $("update-result").textContent = t("检查中…");
  $("update-install").classList.add("hidden");
  updateUrl = null;
  try {
    const r = await invoke("update_check");
    if (r.current) $("update-current").textContent = r.current;
    if (r.status === "none") {
      $("update-result").textContent = r.message;
    } else if (r.newer && r.url) {
      updateUrl = r.url;
      $("update-result").textContent = `${t("发现新版本")}:${r.latest}`;
      $("update-install").classList.remove("hidden");
    } else {
      $("update-result").textContent = `${t("已是最新")}(${r.latest || r.current})`;
    }
  } catch (err) {
    $("update-result").textContent = `${t("检查失败")}:${err}`;
  }
});
$("update-install").addEventListener("click", async () => {
  if (!updateUrl) return;
  $("update-result").textContent = t("下载中…(安装器就绪后会自动弹出)");
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
  if (saved === "1") {
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
