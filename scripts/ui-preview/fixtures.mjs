// kanzei UI 预览夹具:按 Tauri 命令名返回贴近真机的数据。浏览器(mock-ipc.js)与 Node(shoot.mjs)共用。
//
// 形状对照:crates/kanzei-app/src(ProcessInfo、settings_get、models_list)与 scripts/ui-runtime-smoke.mjs 的桩。
// 数据刻意保留真机里「难看」的形态(长 Windows 路径、JSON 串、ANSI 码、GBK 乱码行、超长进展字段),
// 预览的用途就是让这些问题在截图里原样暴露,不要把夹具修漂亮。
//
// createFixtures({ scene, theme, params }) → {
//   commands: { [cmd]: value | (args, ctx) => value },   ctx.emit(event, payload) 可回放事件
//   defaultFor(cmd, args),                               未模拟命令的兜底返回
//   ids, events,                                         场景脚本用的身份与事件序列
//   latencyMs,
// }

// ── 分区:记忆图谱 ──
import { MEMORY_GRAPH_FIXTURE, memoryEntriesFor, memoryEntryFor } from "./memory-graph-fixture.mjs";
// ── 分区:架构图 ──
import { archSnapshot } from "./arch-fixture.mjs";

export const PROJECT = "C:/Users/kanzei/Documents/kanzei code";
export const PROJECT_B = "C:/Users/kanzei/Documents/持续学习";
export const PROJECT_C = "C:/Users/kanzei/Documents/kanzei-rel-0926";
export const GLOBAL_CONFIG = "C:/Users/kanzei/.kanzei/kanzei.toml";
export const PROJECT_CONFIG = `${PROJECT}/.kanzei/kanzei.toml`;

export const IDS = {
  mainProcess: "d|kanzei-code",
  mainSession: "ses_01J8Q2M4K7ZP3W9D1X6N5B2C0A",
  lineProcess: "p|thread-r366-b2",
  lineSession: "ses_01J8Q9V1R2T3Y4U5I6O7P8A9S0",
  idleProcess: "p|thread-d759",
  idleSession: "ses_01J8QA0B1C2D3E4F5G6H7J8K9L",
};

const GLOBAL_PRIMARY = "codex:gpt-6-luna";
const PROJECT_PRIMARY = "codex:gpt-5.6-luna";
const FAST_MODEL = "ollama:qwen3.5:4b";

// ---------------------------------------------------------------------------------------------
// tracker
const docEntry = (id, title, status, extra = {}) => ({
  id, title, status, priority: "P1", closed: false, fields: [], nextStatuses: status === "todo" ? ["doing"] : status === "doing" ? ["done", "todo"] : status === "open" ? ["fixing"] : status === "fixing" ? ["fixed", "open"] : [],
  severity: null, complexity: null, batches: { done: 0, total: 1 },
  blocked: false, block_reasons: [], claimed_by: null, dependencies: [], dependents: [],
  execution_model: null, work_units: [],
  ...extra,
});

const R364_DISCOVERY = JSON.stringify({
  Intent: "降低每步工具 schema 注入成本",
  Explicit: "A 档全收;常驻约 20 个,其余按需加载",
  Assumptions: "低频工具靠名称目录与 denial_hint 仍可被发现",
  Ambiguities: "原生 defer_loading 的字段名与载体待探针核对",
  领域对象: "常驻层、延迟目录、tool_search、已加载集",
  最小成功闭环: "首个请求只含常驻层,select 加载后下一步可调用,压缩后仍在",
  延后决策: "已加载集跨重启持久化;子代理是否分层",
});

const R364_PROGRESS = [
  "B1 进行中:逐工具 schema 字符账单已跑通(crates/kanzei-tools/src/registry.rs:88-164 新增 schema_bill(),按 description+parameters 两段计字符),dev 档 38 个工具合计 61,204 字符,其中 research_* 7 个占 18.9%、work/req/defect 三个 tracker 工具占 14.2%;常驻候选按近 30 天调用频次 × 单次必要性筛出 21 个(CLI 20 + 桌面 browser),名单与淘汰理由写入 docs/design/cc_codex_alignment_impl_maps.md §1.3。",
  "下一步:把账单接进 verify 的预算门禁(拆成常驻面 ≤ 24k 与延迟目录 ≤ 3k 两个数),再核对 codex 原生 defer_loading 字段名(探针脚本 scripts/probe-defer-loading.mjs 已写,待真机跑)。",
  "风险:task 子代理的工具面目前复用主代理注册表,分层后子代理会看不到延迟工具——边界里写了「子代理不分层」,实现时要在 SubagentBase::tools() 显式跳过分层。",
].join("||");

const R364_ACCEPT = "①dev 档主代理首个请求的 tools 只含常驻层(CLI 20、桌面 21),延迟工具以「名称 — 一句话」进 system,账单有 tools/catalog;②tool_search 支持 select 精确加载与关键词检索,加载后下一步起可调用,压缩与溢出恢复后仍可用;③直接调用未加载的延迟工具时自动加载并执行;④预算门禁拆为常驻面与延迟目录两个数,超限时报出具体工具名;⑤denial_hint 指向 tool_search,提示词同版改写,弱模型 5 轮回放里至少 4 轮能自行加载所需工具。";

const R364_CONTENT = "规格见 docs/design/cc_codex_alignment_20260925.md §5.1/§5.2;实施地图(行号、批次、陷阱、裁决)见 docs/design/cc_codex_alignment_impl_maps.md §1。B1 逐工具 schema 字符账单并定稿常驻名单;B2 tool_search 与通用追加路径、预算门禁拆成常驻面与延迟目录两个数、denial_hint 指向 tool_search 与提示词改写(与 B4 同版发布);B3 Anthropic 原生 defer_loading 探针与接线;B4 CLI/桌面同口径回归与弱模型回放。";

const smokeWorkUnit = {
  unit_id: "R-364/W1", requirement_id: "R-364", objective: "逐工具 schema 字符账单并定稿常驻名单",
  status: "active", claimed_by: null, scope: ["crates/kanzei-tools/src/registry.rs", "docs/design/cc_codex_alignment_impl_maps.md"],
  dependencies: [], acceptance: ["账单按工具列出 description 与 parameters 字符数", "常驻名单 21 个且每个有保留理由"],
  verification: ["cargo test -p kanzei-tools schema_bill", "node scripts/verify-policy-smoke.mjs"],
  base_revision: "2009581f", blocked_reason: null,
  last_checkpoint: {
    summary: "账单函数与 dev 档统计完成,常驻名单初稿 21 个",
    next_action: "账单接入 verify 预算门禁",
    decisions: ["browser 只在桌面常驻", "research_* 全部延迟"],
    retrieval_refs: ["M-041", "M-058"],
  },
  evidence: [{ criterion: "账单按工具列出 description 与 parameters 字符数", evidence_refs: ["crates/kanzei-tools/src/registry.rs:88"] }],
  created_at: Date.parse("2026-09-26T09:12:00+08:00"), updated_at: Date.parse("2026-09-26T13:48:00+08:00"),
};

function buildRequirements() {
  return [
    docEntry("R-364", "工具延迟加载:常驻层约 20 个工具,其余经 tool_search 按需加载", "doing", {
      complexity: "大", batches: { done: 0, total: 4 },
      fields: [
        ["内容", R364_CONTENT],
        ["发现记录", R364_DISCOVERY],
        ["复杂度", "大"],
        ["批次", "0/4"],
        ["来源", "2026-09-25 用户审阅 CC/Codex 三方对照,A 档回答「同意」并要求「直接登记就行」;R-312 B1 实测工具 schema 占每步系统注入 48.6%"],
        ["标签", "核心"],
        ["边界", "不做 MCP/skills/hooks;不改 Part::ToolResult 与 LlmRequest 结构;research/readonly/子代理不分层;已加载集不跨重启持久化"],
        ["验收", R364_ACCEPT],
        ["进展", R364_PROGRESS],
        ["refs", "D-662 R-312 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md"],
        ["优先级", "P1"],
        ["observed_head", "2009581f3c7d1e2b9a8f6e5d4c3b2a1908f7e6d5"],
        ["recorded_at", "1790386080000"],
      ],
      dependents: ["R-369"], execution_model: "work_units_v1", work_units: [smokeWorkUnit],
    }),
    docEntry("R-366", "回退:每条用户消息一个检查点,可选对话+代码/只回退对话/只回退代码", "doing", {
      complexity: "大", batches: { done: 1, total: 4 }, claimed_by: "kanzei/thread-r366-b2",
      fields: [
        ["内容", "规格见 docs/design/cc_codex_alignment_20260925.md §5.9;实施地图见 docs/design/cc_codex_alignment_impl_maps.md §3。B1 检查点存储;B2 conversation.rewind 事件与历史重建;B3 代码还原;B4 前端入口。"],
        ["发现记录", JSON.stringify({ Intent: "像 CC 一样可回退到任一用户消息", Explicit: "默认对话+代码,可选只对话或只代码,外部改动默认跳过", Ambiguities: "无阻塞项" })],
        ["标签", "核心"],
        ["验收", "①edit/write/insert 首次触碰文件时保存前像;②回退对话后桌面、下一轮 prior 与 kz run 都看不到被回退段;③回退代码恢复前像,外部改动过的文件默认跳过并列出。"],
        ["进展", "B1 已提交 2d3eaef9:文件检查点按裁决落地(代码树根与相对路径口径、单次开库、捕获失败留哨兵)。B2 在分支线 kanzei/thread-r366-b2 推进 conversation.rewind 事件。"],
      ],
    }),
    docEntry("R-365", "网页搜索与抓取升级:模型自带搜索、webfetch 按问题提取与翻页查找、websearch 批量与过滤", "todo", {
      complexity: "大", batches: { done: 0, total: 5 },
      fields: [["标签", "核心"], ["验收", "①B1 的探针结论与脱敏样例写入设计文档;②探针通过的订阅通道上主对话可用模型自带搜索;③webfetch 支持 url|ref、行号翻页、页内查找。"]],
    }),
    docEntry("R-367", "先读后写:edit/write/insert 对未读或读后被改的文件返回纠错码", "todo", {
      complexity: "中", batches: { done: 0, total: 3 }, priority: "P1",
      fields: [["标签", "后端"], ["验收", "①带账本时对已存在文件未 read 就写返回 READ_BEFORE_WRITE 且文件不变;②部分读取也算读过。"]],
    }),
    docEntry("R-368", "文档引用标记、引用历史与引用图:统一抽取、写入校验、git 推导历史、前端侧栏与邻域图", "todo", {
      complexity: "大", batches: { done: 0, total: 5 }, priority: "P2",
      fields: [["标签", "核心"]],
    }),
    docEntry("R-369", "工具面预算门禁接入 verify:常驻面与延迟目录分开计数", "todo", {
      complexity: "小", priority: "P2", dependencies: ["R-364"], blocked: true,
      block_reasons: ["未完成依赖: R-364"],
      fields: [["标签", "流程"]],
    }),
    docEntry("R-361", "记忆收益闭环与任务信息呈现改进", "doing", {
      complexity: "大", batches: { done: 3, total: 3 }, blocked: true,
      block_reasons: ["用户：提供一个可重复的真实 provider 运行入口（已配置模型/凭据或运行中的 provider）、固定任务与允许执行窗口；解除条件:用户"],
      fields: [["标签", "前端"], ["阻塞", "用户：提供一个可重复的真实 provider 运行入口；解除条件:用户"]],
    }),
    docEntry("R-370", "设置页模型配置区分全局默认与项目覆盖", "todo", { complexity: "中", priority: "P1", fields: [["标签", "前端"]] }),
    docEntry("R-352", "运行画像按任务关闭为主粒度", "done", { closed: true, priority: "P2", fields: [["标签", "前端"]], nextStatuses: [] }),
  ];
}

function buildDefects() {
  return [
    docEntry("D-764", "新对话按钮需点多次才得到干净对话,旧会话内容残留", "open", {
      severity: "high", complexity: "中",
      fields: [["复现", "对话中点侧栏「新对话」→ 主区仍显示上一段对话的末尾消息与工具块;连点 2-3 次才清空"], ["标签", "前端"]],
    }),
    docEntry("D-765", "状态栏显示的运行模型与顶栏所选模型不一致", "open", {
      severity: "medium",
      fields: [["复现", "全局 primary=codex:gpt-6-luna,项目 kanzei.toml 覆盖为 codex:gpt-5.6-luna;顶栏显示 agent 默认,状态栏显示 gpt-5.6-luna"], ["标签", "前端"]],
    }),
    docEntry("D-759", "手机提问卡片按 id 对账与显式取消", "fixed", { closed: true, severity: "medium", nextStatuses: [], fields: [["标签", "前端"]] }),
    docEntry("D-766", "工具行显示整段 Windows 路径与乱码输出", "open", {
      severity: "low",
      fields: [["复现", "待澄清: bash 输出 GBK 乱码行是否应在渲染层转码,还是只在活动面板保留原文?"], ["标签", "前端"]],
    }),
  ];
}

function buildDocsSnapshot() {
  return {
    requirements: buildRequirements(),
    defects: buildDefects(),
    ideas: [docEntry("I-031", "子代理呈现参考 Claude:按角色折叠、可展开完整轨迹", "inbox")],
    sources: [],
    findings: [],
    research_topics: [],
    incident_metrics: {
      schema_version: 2, total_occurrences: 0, total_events: 0, promotion_events: 0, by_class: {},
      overall: { escaped: 0, escaped_rate: 0, repair_duration_ms_total: 0, repair_duration_ms_average: null, repair_duration_samples: 0 },
      historical_replay: { sample_count: 0, consistent_count: 0, consistent: true, formal_defect_samples: 0, execution_incidents_excluded: 0, samples: [] },
    },
    root: PROJECT,
    warnings: [],
    work_units: [smokeWorkUnit],
    archived: { req: 351, defect: 742, idea: 12, source: 0, finding: 0 },
    conventions: { exists: true, has_proposal: false, headings: ["开发规则", "测试要求", "提交规范"] },
  };
}

// ---------------------------------------------------------------------------------------------
// 对话
const READ_OUTPUT = [
  "     1\t# CC/Codex 对齐实施地图",
  "     2\t",
  "     3\t## §1 R-364 工具延迟加载",
  "     4\t",
  "     5\t### 1.1 现状",
  "     6\t- 注册入口:`crates/kanzei-tools/src/lib.rs:41` `register_tools()`,dev 档 38 个工具全部进每步请求",
  "     7\t- 账单口径:description + JSON schema 序列化后字符数(不含 system 其它段)",
  "     8\t",
  "     9\t### 1.2 批次",
  "    10\t| 批次 | 内容 | 陷阱 |",
  "    11\t|---|---|---|",
  "    12\t| B1 | 账单 + 常驻名单 | research_* 在研究档是常驻 |",
  "    13\t| B2 | tool_search + 门禁 | 压缩后已加载集要重放 |",
  // 真实格式(read.rs):截断标记带总行数。
  "... (truncated at line 14 of 842; use offset to continue)",
].join("\n");

// 真实格式(grep.rs):路径相对 cwd,匹配行 `path:行: 文本`,上下文行 `path-行- 文本`。
const GREP_OUTPUT = [
  "crates/kanzei-tools/src/read.rs:141:     fn name(&self) -> &'static str { \"read\" }",
  "crates/kanzei-tools/src/grep.rs:58:     fn name(&self) -> &'static str { \"grep\" }",
  "crates/kanzei-tools/src/symbols.rs:36:     fn name(&self) -> &'static str { \"symbols\" }",
  "crates/kanzei-tools/src/bash.rs-211-     #[allow(clippy::too_many_lines)]",
  "crates/kanzei-tools/src/bash.rs:212:     fn name(&self) -> &'static str { \"bash\" }",
  "crates/kanzei-tools/src/research_plan.rs:77:     fn name(&self) -> &'static str { \"research_plan\" }",
  "... (stopped at limit 6; narrow the pattern or raise limit)",
].join("\n");

// 真实格式(symbols.rs):`== 相对路径` 表头 + `  {pub|  } {kind} {name}:{line}`。
const SYMBOLS_OUTPUT = [
  "== crates/kanzei-tools/src/registry.rs",
  "  pub fn register_tools:41",
  "     fn schema_bill:88",
  "  pub struct ToolBill:166",
  "     impl ToolBill:181",
  "     fn resident_set:203",
  "     const RESIDENT_CLI:219",
].join("\n");

const BILL_JSON = JSON.stringify({
  profile: "dev",
  total_chars: 61204,
  tools: [
    { name: "research_workflow", description: 1840, parameters: 2911, share: 0.0776 },
    { name: "work", description: 1622, parameters: 2408, share: 0.0658 },
    { name: "req", description: 1507, parameters: 2012, share: 0.0575 },
    { name: "bash", description: 1311, parameters: 402, share: 0.028 },
  ],
  resident: 21,
  deferred: 17,
}, null, 0);

const BASH_OK = [
  "\u001b[1m\u001b[32m   Compiling\u001b[0m kanzei-tools v0.9.26 (C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\kanzei-tools)",
  "\u001b[1m\u001b[32m    Finished\u001b[0m `test` profile [unoptimized + debuginfo] target(s) in 38.41s",
  "\u001b[1m\u001b[32m     Running\u001b[0m unittests src\\lib.rs (target\\debug\\deps\\kanzei_tools-7f3c2a91e0b4d5c6.exe)",
  "",
  "running 3 tests",
  "test registry::tests::schema_bill_counts_description_and_parameters ... ok",
  "test registry::tests::resident_set_is_stable ... ok",
  "test registry::tests::deferred_catalog_lists_name_and_summary ... ok",
  "",
  "test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 412 filtered out; finished in 0.04s",
].join("\n");

const BASH_FAIL = [
  "exit code: 101",
  "\u001b[1m\u001b[31merror[E0425]\u001b[0m: cannot find value `RESIDENT_DESKTOP` in this scope",
  "   --> crates\\kanzei-tools\\src\\registry.rs:231:18",
  "    |",
  "231 |     let extra = RESIDENT_DESKTOP.iter();",
  "    |                 ^^^^^^^^^^^^^^^^ help: a constant with a similar name exists: `RESIDENT_CLI`",
  "鏈壘鍒版寚瀹氱殑璺緞銆� (os error 3)",
  "error: could not compile `kanzei-tools` (lib test) due to 1 previous error",
].join("\n");

const TASK_RESULT = [
  "## 常驻层审计结论",
  "",
  "- 21 个常驻工具中 **19 个**近 30 天调用 ≥ 5 次;`frontend_locate` 与 `incident` 低于阈值,建议移入延迟目录。",
  "- `browser` 仅桌面端注册,CLI 常驻层应为 20。",
  "",
  "```json",
  "{\"keep\":19,\"defer\":[\"frontend_locate\",\"incident\"],\"desktop_only\":[\"browser\"]}",
  "```",
].join("\n");

// 真实格式(tracker/actions.rs update_close):`updated: {id} [{status}] {title}` + `变更: 键: 旧 → 新; …`。
const REQ_UPDATE_RESULT = "updated: R-364 [doing] 工具延迟加载:常驻层约 20 个工具,其余经 tool_search 按需加载\n变更: 进展: B1 待开工 → B1 进行中:逐工具 schema 字符账单已跑通…";

// 真实格式(test_record.rs render_snapshot)。
const TEST_RECORD_RESULT = "recorded T-1786922727068. active: 0, archived: 1 (path: C:\\Users\\kanzei\\Documents\\kanzei code\\.kanzei\\project\\tests.md, archive: C:\\Users\\kanzei\\Documents\\kanzei code\\.kanzei\\project\\tests-archive.md)";

// 真实格式(edit.rs + local_validation.rs):替换回执 + 局部结构校验摘要与明细。
const editResult = (file) => [
  `replaced 1 occurrence(s) in C:\\Users\\kanzei\\Documents\\kanzei code\\${file.replace(/\//g, "\\")}`,
  "局部结构校验通过: 1 个低成本检查",
  "局部校验明细:",
  `- rustfmt-check [passed] command: rustfmt --check --edition 2021 ${file}`,
].join("\n");

function mainConversation() {
  return [
    { role: "user", parts: [{ type: "text", text: "继续 R-364:先把 B1 的逐工具 schema 字符账单跑出来,定稿常驻名单。完成后更新进展。" }] },
    {
      role: "assistant",
      parts: [
        { type: "reasoning", text: "**确认口径**\n先读实施地图 §1 看账单口径,再从注册表拿全部工具名。常驻名单要按调用频次和必要性两条筛。\n研究档的 research_* 在 dev 档可以全部延迟。" },
        { type: "text", text: "我先读实施地图和现有工具注册表,确认账单口径后再动代码。" },
        { type: "tool_call", id: "h-read-1", name: "read", input: { path: "docs/design/cc_codex_alignment_impl_maps.md", offset: 1, limit: 13 } },
        { type: "tool_result", call_id: "h-read-1", is_error: false, content: READ_OUTPUT },
        { type: "tool_call", id: "h-grep-1", name: "grep", input: { pattern: "fn name\\(&self\\) -> &'static str", path: "crates/kanzei-tools/src", limit: 6, context: 1 } },
        { type: "tool_result", call_id: "h-grep-1", is_error: false, content: GREP_OUTPUT },
        { type: "tool_call", id: "h-symbols-1", name: "symbols", input: { path: "crates/kanzei-tools/src/registry.rs" } },
        { type: "tool_result", call_id: "h-symbols-1", is_error: false, content: SYMBOLS_OUTPUT },
        { type: "tool_call", id: "h-edit-1", name: "edit", input: { path: "crates/kanzei-tools/src/registry.rs", old_string: "pub fn register_tools(", new_string: "pub(crate) fn schema_bill(tools: &[Box<dyn Tool>]) -> ToolBill {\n    // …\n}\n\npub fn register_tools(" } },
        { type: "tool_result", call_id: "h-edit-1", is_error: false, content: editResult("crates/kanzei-tools/src/registry.rs") },
        { type: "tool_call", id: "h-bash-fail", name: "bash", input: { command: "cargo test -p kanzei-tools registry::", timeout_ms: 600000 } },
        { type: "tool_result", call_id: "h-bash-fail", is_error: true, content: BASH_FAIL },
        { type: "tool_call", id: "h-edit-2", name: "edit", input: { path: "crates/kanzei-tools/src/registry.rs", old_string: "RESIDENT_DESKTOP.iter()", new_string: "RESIDENT_CLI.iter().chain([\"browser\"].iter())" } },
        { type: "tool_result", call_id: "h-edit-2", is_error: false, content: editResult("crates/kanzei-tools/src/registry.rs") },
        { type: "tool_call", id: "h-bash-ok", name: "bash", input: { command: "cargo test -p kanzei-tools registry::" } },
        { type: "tool_result", call_id: "h-bash-ok", is_error: false, content: `exit code: 0\n${BASH_OK}` },
        { type: "tool_call", id: "h-bash-bill", name: "bash", input: { command: "cargo run -q -p kanzei --bin kz -- tools bill --profile dev --json" } },
        { type: "tool_result", call_id: "h-bash-bill", is_error: false, content: `exit code: 0\n${BILL_JSON}` },
        { type: "tool_call", id: "architecture_scout", name: "task", input: { prompt: "审计常驻层 21 个工具近 30 天的调用频次,给出保留/延迟建议并附证据", description: "审计常驻层调用频次", phase: "scouting", role: "architecture_scout", model: "fast" } },
        { type: "tool_result", call_id: "architecture_scout", is_error: false, content: TASK_RESULT },
        { type: "tool_call", id: "h-test-1", name: "test_record", input: { title: "cargo test -p kanzei-tools registry::", status: "passed", command: "cargo test -p kanzei-tools registry::", summary: "3 passed; 0 failed", refs: ["R-364"] } },
        { type: "tool_result", call_id: "h-test-1", is_error: false, content: TEST_RECORD_RESULT },
        { type: "tool_call", id: "h-req-1", name: "req", input: { action: "update", id: "R-364", fields: { 进展: "B1 进行中:逐工具 schema 字符账单已跑通…" } } },
        { type: "tool_result", call_id: "h-req-1", is_error: false, content: REQ_UPDATE_RESULT },
        {
          type: "text",
          text: [
            "## B1 账单结果",
            "",
            "dev 档 38 个工具合计 **61,204** 字符。占比最高的四个:",
            "",
            "| 工具 | description | parameters | 占比 |",
            "|---|---:|---:|---:|",
            "| research_workflow | 1,840 | 2,911 | 7.8% |",
            "| work | 1,622 | 2,408 | 6.6% |",
            "| req | 1,507 | 2,012 | 5.8% |",
            "| bash | 1,311 | 402 | 2.8% |",
            "",
            "常驻名单定为 **21 个**(CLI 20 + 桌面 `browser`),子代理审计建议把 `frontend_locate`、`incident` 移入延迟目录。",
            "",
            "下一步:把账单接进 `verify.ps1` 的预算门禁,拆成常驻面与延迟目录两个数。",
          ].join("\n"),
        },
      ],
    },
    { role: "user", parts: [{ type: "text", text: "好,继续把门禁接上。" }] },
    {
      role: "assistant",
      parts: [
        { type: "reasoning", text: "门禁在 scripts/verify-policy.mjs 里定义预算;要新增两个数而不是改老的 schema_budget。" },
        { type: "tool_call", id: "h-read-2", name: "read", input: { path: "scripts/verify-policy.mjs" } },
        { type: "tool_result", call_id: "h-read-2", is_error: true, content: "path not found: C:\\Users\\kanzei\\Documents\\kanzei code\\scripts\\verify-policy.mjs\n(did you mean scripts/verify-policy-smoke.mjs?)" },
        { type: "tool_call", id: "h-glob-1", name: "glob", input: { pattern: "scripts/verify*" } },
        { type: "tool_result", call_id: "h-glob-1", is_error: false, content: "scripts/verify-policy-smoke.mjs\nscripts/verify-policy.mjs\nscripts/verify.ps1" },
      ],
    },
  ];
}

function mainTraces() {
  const now = Date.now();
  const tool = (id, name, summary, ok, durationMs, extra = {}) => ([
    { kind: "tool.started", id, name, summary, at: now - 600000 },
    { kind: "tool.completed", id, name, ok, durationMs, at: now - 600000 + durationMs, ...extra },
  ]);
  // run.trace 里落库的子代理进度(无 kind 字段;入参是截到 4K 的字符串),历史回放据此补齐卡片的过程与计数。
  const taskTrace = (id, trace, text = "") => ({ id, text, trace });
  return [{
    events: [
      { kind: "turn.started" },
      // 真实轨迹(run/events tool.completed)只存 runner::preview:首行 120 字 + " (+N lines)";
      // 失败另带 error(= preview 前 400 字)与 outcome/code。
      ...tool("h-read-1", "read", "docs/design/cc_codex_alignment_impl_maps.md", true, 12, { preview: "     1\t# CC/Codex 对齐实施地图 (+13 lines)" }),
      ...tool("h-grep-1", "grep", "fn name\\(&self\\)", true, 88, { preview: "crates/kanzei-tools/src/read.rs:141:     fn name(&self) -> &'static str { \"read\" } (+6 lines)" }),
      ...tool("h-bash-fail", "bash", "cargo test -p kanzei-tools registry::", false, 41200, { outcome: "failed", preview: "exit code: 101 (+42 lines)", error: "exit code: 101 (+42 lines)" }),
      ...tool("h-bash-ok", "bash", "cargo test -p kanzei-tools registry::", true, 39800, { preview: "exit code: 0 (+10 lines)" }),
      ...tool("h-bash-bill", "bash", "cargo run -q -p kanzei --bin kz -- tools bill --profile dev --json", true, 5300, { preview: "exit code: 0 (+1 lines)" }),
      ...tool("architecture_scout", "task", "审计常驻层 21 个工具近 30 天的调用频次", true, 73400, { preview: "## 常驻层审计结论 (+7 lines)" }),
      taskTrace("architecture_scout", { child_id: "architecture_scout", phase: "meta", agent: "explore", model: FAST_MODEL, summary: "fast" }, `explore · ${FAST_MODEL}`),
      taskTrace("architecture_scout", { child_id: "as-1", phase: "start", name: "grep", summary: "fn name", input: JSON.stringify({ pattern: "fn name\\(&self\\)", path: "crates/kanzei-tools/src" }) }),
      taskTrace("architecture_scout", { child_id: "as-1", phase: "end", name: "grep", ok: true, preview: "crates/kanzei-tools/src/read.rs:141: fn name(&self) (+38 lines)" }),
      taskTrace("architecture_scout", { child_id: "as-2", phase: "start", name: "read", summary: ".kanzei/metrics/tool_calls.jsonl", input: JSON.stringify({ path: ".kanzei/metrics/tool_calls.jsonl", limit: 400 }) }),
      taskTrace("architecture_scout", { child_id: "as-2", phase: "end", name: "read", ok: true, preview: "{\"tool\":\"read\",\"calls\":412} (+399 lines)" }),
      taskTrace("architecture_scout", { phase: "text", text: "`frontend_locate` 30 天只调了 2 次,`incident` 3 次,都低于 5 次阈值。" }),
      taskTrace("architecture_scout", { child_id: "as-3", phase: "start", name: "grep", summary: "register_desktop", input: JSON.stringify({ pattern: "register_desktop", path: "crates" }) }),
      taskTrace("architecture_scout", { child_id: "as-3", phase: "end", name: "grep", ok: true, preview: "crates/kanzei-app/src/tools.rs:22: register_desktop(&mut registry) (+1 lines)" }),
      taskTrace("architecture_scout", { phase: "usage", name: "", usage: { input: 38200, output: 1450, cache_read: 21000 } }),
      ...tool("h-read-2", "read", "scripts/verify-policy.mjs", false, 3, { outcome: "failed", code: "READ_PATH_NOT_FOUND", preview: "path not found: C:/Users/kanzei/Documents/kanzei code/scripts/verify-policy.mjs (+1 lines)", error: "path not found: C:/Users/kanzei/Documents/kanzei code/scripts/verify-policy.mjs (+1 lines)" }),
    ],
  }];
}

function lineConversation() {
  return [
    { role: "user", parts: [{ type: "text", text: "R-366 B2:conversation.rewind 事件与历史重建" }] },
    { role: "assistant", parts: [{ type: "text", text: "在分支线上推进 B2,先补事件定义再改历史重建。" }] },
  ];
}

// ---------------------------------------------------------------------------------------------
// 事件序列(场景脚本回放)
export function liveEvents(ids = IDS) {
  const sessionId = ids.mainSession;
  return {
    meta: { sessionId, model: PROJECT_PRIMARY, agent: "dev", profile: "dev", reasoning: "high", codexFastMode: true, contextLimit: 400000 },
    turn: { sessionId, step: 3, maxSteps: 0 },
    status: { sessionId, stage: "实现", detail: "R-364 B1 · 账单接入预算门禁" },
    reasoning: { sessionId, text: "**接预算门禁**\n先在 verify-policy.mjs 里加 resident_chars 与 catalog_chars 两个预算,再让 schema_bill 输出这两个数。" },
    text: { sessionId, text: "门禁脚本找到了,现在把账单拆成常驻面与延迟目录两个数接进去。" },
    step: { sessionId, input: 148200, output: 2310, cacheRead: 131000, cacheWrite: 0 },
    // 运行中的终端工具
    bashStart: { sessionId, id: "live-bash-1", name: "bash", summary: "cargo test -p kanzei-tools", input: { command: "cargo test -p kanzei-tools tool_search -- --nocapture" } },
    bashProgress: { sessionId, id: "live-bash-1", chunk: "\u001b[1m\u001b[32m   Compiling\u001b[0m kanzei-harness v0.9.26 (C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\kanzei-harness)\n\u001b[1m\u001b[32m   Compiling\u001b[0m kanzei-tools v0.9.26 (C:\\Users\\kanzei\\Documents\\kanzei code\\crates\\kanzei-tools)\n" },
    // 编排派发的勘察子代理(运行中,带子工具轨迹)
    scoutStart: {
      sessionId, id: "review_gate", name: "task", summary: "复核预算门禁改动",
      input: { prompt: "复核 verify-policy.mjs 的预算门禁拆分:常驻面 ≤ 24k、延迟目录 ≤ 3k,超限时是否报出具体工具名", description: "复核预算门禁拆分", phase: "review", role: "review_gate", model: "fast" },
    },
    scoutProgress: [
      // UI-0926 #8:后端先报 meta(实际人格与模型 id),再有任何子工具进度。
      { sessionId, id: "review_gate", text: `explore · ${FAST_MODEL}`, trace: { child_id: "review_gate", phase: "meta", agent: "explore", model: FAST_MODEL, summary: "fast" } },
      { sessionId, id: "review_gate", text: "读取 scripts/verify-policy.mjs", trace: { phase: "start", child_id: "rg-1", name: "read", summary: "scripts/verify-policy.mjs", input: { path: "scripts/verify-policy.mjs" } } },
      { sessionId, id: "review_gate", text: "读取完成", trace: { phase: "end", child_id: "rg-1", name: "read", ok: true, preview: "     1\t// R-354 验证门禁的档位策略:按改动面挑命令集。 (+411 lines)" } },
      { sessionId, id: "review_gate", text: "检索 schema_budget 调用方", trace: { phase: "start", child_id: "rg-2", name: "grep", summary: "schema_budget", input: { pattern: "schema_budget", path: "scripts" } } },
      { sessionId, id: "review_gate", text: "检索完成", trace: { phase: "end", child_id: "rg-2", name: "grep", ok: true, preview: "scripts/verify-policy.mjs:88:   schema_budget: 24000, (+2 lines)" } },
      { sessionId, id: "review_gate", text: "", trace: { phase: "text", text: "旧的 `schema_budget` 仍被 verify.ps1 第 214 行引用,拆分后要同步改名,否则门禁会读到 undefined 而放行。" } },
      { sessionId, id: "review_gate", text: "", trace: { phase: "usage", name: "", usage: { input: 18400, output: 620, cache_read: 12000 } } },
      { sessionId, id: "review_gate", text: "运行 node scripts/verify-policy-smoke.mjs", trace: { phase: "start", child_id: "rg-3", name: "bash", summary: "node scripts/verify-policy-smoke.mjs", input: { command: "node scripts/verify-policy-smoke.mjs" } } },
    ],
    // 模型自派的 task(已完成)
    selfTaskStart: {
      sessionId, id: "call_task_7Hq2", name: "task", summary: "核对 denial_hint 文案",
      input: { prompt: "列出所有 denial_hint 文案,标出哪些还没指向 tool_search", description: "核对 denial_hint 文案", model: "fast" },
    },
    selfTaskProgress: [
      { sessionId, id: "call_task_7Hq2", text: `explore · ${FAST_MODEL}`, trace: { child_id: "call_task_7Hq2", phase: "meta", agent: "explore", model: FAST_MODEL, summary: "fast" } },
      { sessionId, id: "call_task_7Hq2", text: "检索 denial_hint", trace: { phase: "start", child_id: "t7-1", name: "grep", summary: "denial_hint", input: { pattern: "denial_hint", path: "crates" } } },
      { sessionId, id: "call_task_7Hq2", text: "检索完成", trace: { phase: "end", child_id: "t7-1", name: "grep", ok: true, preview: "crates/kanzei-core/src/runner/drive/permissions.rs:41:         snapshot.denial_hint(action, &resource), (+13 lines)" } },
      { sessionId, id: "call_task_7Hq2", text: "读取 permissions.rs", trace: { phase: "start", child_id: "t7-2", name: "read", summary: "permissions.rs", input: { path: "crates/kanzei-core/src/runner/drive/permissions.rs", offset: 30, limit: 40 } } },
      { sessionId, id: "call_task_7Hq2", text: "读取完成", trace: { phase: "end", child_id: "t7-2", name: "read", ok: true, preview: "    30\tpub(crate) fn denial_hint(action: &str, resource: &str) -> String { (+39 lines)" } },
      { sessionId, id: "call_task_7Hq2", text: "", trace: { phase: "text", text: "14 处 denial_hint 中 **9 处**仍写「该工具在当前档位不可用」,没有提示 `tool_search`。" } },
      { sessionId, id: "call_task_7Hq2", text: "", trace: { phase: "usage", name: "", usage: { input: 9800, output: 740, cache_read: 4100 } } },
    ],
    // UI-0926 #8 并行场景:模型同一轮并行派发 3 个 task(tool-start 连续到达,合成一组)。
    parallelStarts: [
      { sessionId, id: "call_par_a", name: "task", summary: "找出 token 校验调用点", input: { prompt: "找出所有调用 verify_token 的位置,列出文件与行号", description: "找出 token 校验调用点" } },
      { sessionId, id: "call_par_b", name: "task", summary: "设计 token 刷新方案", input: { prompt: "基于现有会话层设计 token 刷新方案,给出改动面", description: "设计 token 刷新方案", agent: "plan", model: "primary" } },
      { sessionId, id: "call_par_c", name: "task", summary: "定位相关测试", input: { prompt: "定位覆盖 token 校验的测试文件", description: "定位相关测试" } },
    ],
    parallelProgress: [
      { sessionId, id: "call_par_a", text: `explore · ${FAST_MODEL}`, trace: { child_id: "call_par_a", phase: "meta", agent: "explore", model: FAST_MODEL, summary: "fast" } },
      { sessionId, id: "call_par_b", text: `plan · ${PROJECT_PRIMARY}`, trace: { child_id: "call_par_b", phase: "meta", agent: "plan", model: PROJECT_PRIMARY, summary: "primary" } },
      { sessionId, id: "call_par_c", text: `explore · ${FAST_MODEL}`, trace: { child_id: "call_par_c", phase: "meta", agent: "explore", model: FAST_MODEL, summary: "fast" } },
      { sessionId, id: "call_par_a", text: "", trace: { phase: "start", child_id: "pa-1", name: "grep", summary: "verify_token", input: { pattern: "verify_token", path: "crates" } } },
      { sessionId, id: "call_par_a", text: "", trace: { phase: "end", child_id: "pa-1", name: "grep", ok: true, preview: "crates/kanzei-app/src/auth/session.rs:88: verify_token(&claims) (+6 lines)" } },
      { sessionId, id: "call_par_a", text: "", trace: { phase: "start", child_id: "pa-2", name: "read", summary: "session.rs", input: { path: "crates/kanzei-app/src/auth/session.rs" } } },
      { sessionId, id: "call_par_b", text: "", trace: { phase: "start", child_id: "pb-1", name: "read", summary: "middleware.rs", input: { path: "crates/kanzei-app/src/auth/middleware.rs" } } },
      { sessionId, id: "call_par_c", text: "", trace: { phase: "start", child_id: "pc-1", name: "glob", summary: "tests", input: { pattern: "crates/**/tests/*token*.rs" } } },
      { sessionId, id: "call_par_c", text: "", trace: { phase: "end", child_id: "pc-1", name: "glob", ok: true, preview: "crates/kanzei-app/tests/token_refresh.rs (+1 lines)" } },
      { sessionId, id: "call_par_c", text: "", trace: { phase: "usage", name: "", usage: { input: 1600, output: 420, cache_read: 0 } } },
      { sessionId, id: "call_par_a", text: "", trace: { phase: "usage", name: "", usage: { input: 11200, output: 380, cache_read: 6100 } } },
    ],
    parallelEnd: {
      sessionId, id: "call_par_c", name: "task", ok: true, outcome: "success",
      preview: "2 个测试文件覆盖 token 校验 (+2 lines)",
      content: "2 个测试文件覆盖 token 校验\n\n- crates/kanzei-app/tests/token_refresh.rs\n- crates/kanzei-app/src/auth/session_tests.rs",
      contentBytes: 110, contentTruncated: false, durationMs: 9400, display: null,
    },
    selfTaskEnd: {
      sessionId, id: "call_task_7Hq2", name: "task", ok: true, outcome: "success",
      preview: "14 处 denial_hint,9 处未指向 tool_search(清单见详情) (+3 lines)",
      content: "14 处 denial_hint,9 处未指向 tool_search(清单见详情)\n\n- permissions.rs:41\n- serial_tools.rs:131",
      contentBytes: 120, contentTruncated: false, durationMs: 48200,
      display: null,
    },
  };
}

function asks(ids = IDS) {
  return {
    // 真实形态:bash 的资源是 {command, workdir} JSON(bash.rs resources_with_ctx),
    // 「记住为」经 generalize_resource 原样返回。
    permission: {
      id: "ask-01J8QB3", kind: "permission", sessionId: ids.mainSession,
      action: "bash", resource: JSON.stringify({ command: "git push origin release/2026-09-26-ui --force-with-lease", workdir: PROJECT }),
      remember: JSON.stringify({ command: "git push origin release/2026-09-26-ui --force-with-lease", workdir: PROJECT }),
    },
    question: {
      id: "ask-01J8QB4", kind: "question", sessionId: ids.mainSession,
      question: "常驻名单里 browser 只在桌面端注册。CLI 常驻层按 20 还是 21 计入预算?",
      options: [
        { label: "按 20 计(CLI 口径)", note: "桌面多出的 browser 单独豁免,门禁数与 CLI 一致" },
        { label: "按 21 计(桌面口径)", note: "CLI 预算留 1 个空位,两端共用一个上限" },
        { label: "两端分别设上限", note: "门禁拆成 cli/desktop 两个数,配置更多但最准确" },
      ],
      default: "",
    },
  };
}

// ---------------------------------------------------------------------------------------------
// ── 分区:文件编辑 ── UI2-0926 #6:文件页的内存磁盘。形状对照 crates/kanzei-app/src/files_edit.rs(file_preview /
// file_stat / file_write)与 scripts/ipc-contract.json;指纹只求稳定可区分(真机是 FNV-1a)。托管文档只读,CRLF 文件保持 CRLF。
const PREVIEW_REGISTRY_RS = [
  "//! 工具注册表:按档位收集工具,生成常驻 schema 账单(R-364 B1)。",
  "",
  "use std::collections::BTreeMap;",
  "use std::sync::Arc;",
  "",
  "use kanzei_harness::{Tool, ToolCtx};",
  "",
  "/// 常驻工具名单:每个都要有保留理由,其余按需加载(tool_search)。",
  "pub const RESIDENT: &[&str] = &[",
  "    \"read\", \"edit\", \"write\", \"bash\", \"grep\", \"glob\", \"work\", \"req\",",
  "];",
  "",
  "pub struct Registry {",
  "    tools: BTreeMap<&'static str, Arc<dyn Tool>>,",
  "}",
  "",
  "impl Registry {",
  "    pub fn new() -> Self {",
  "        Self { tools: BTreeMap::new() }",
  "    }",
  "",
  "    /// 注册一个工具;同名后者覆盖前者(档位组件按顺序叠加)。",
  "    pub fn add(&mut self, tool: Arc<dyn Tool>) {",
  "        self.tools.insert(tool.name(), tool);",
  "    }",
  "",
  "    /// schema 字符账单:description + parameters 的字符数,按工具列出。",
  "    pub fn schema_bill(&self) -> Vec<(&'static str, usize, usize)> {",
  "        self.tools",
  "            .values()",
  "            .map(|tool| {",
  "                let schema = tool.input_schema().to_string();",
  "                (tool.name(), tool.description().chars().count(), schema.chars().count())",
  "            })",
  "            .collect()",
  "    }",
  "",
  "    pub fn resident(&self, ctx: &ToolCtx) -> impl Iterator<Item = &Arc<dyn Tool>> {",
  "        let _ = ctx;",
  "        self.tools.values().filter(|tool| RESIDENT.contains(&tool.name()))",
  "    }",
  "}",
  "",
].join("\r\n");
const PREVIEW_REQUIREMENTS_MD = [
  "# Requirements",
  "",
  "## R-364 工具延迟加载:常驻层约 20 个工具,其余经 tool_search 按需加载 [doing]",
  "- 优先级: P1",
  "- 复杂度: 大",
  "- 批次: 0/4",
  "- 内容: 规格见 docs/design/cc_codex_alignment_20260925.md §5.2。",
  "",
  "## R-366 回退:每条用户消息一个检查点 [doing]",
  "- 优先级: P1",
  "- 批次: 1/4",
  "",
].join("\n");
const PREVIEW_FILES_DOC_MD = "# 文件页编辑\n\n- 身份: live_design\n- 一句话: 文件浏览带编辑、可拖拽伸缩。\n";
function previewFileDisk() {
  let clock = 1790389800000;
  const disk = new Map();
  const put = (path, text, { bom = false, note = null } = {}) => disk.set(path, { text, bom, note, mtime: (clock += 1000) });
  put("crates/kanzei-tools/src/registry.rs", PREVIEW_REGISTRY_RS, { note: "工具注册与 schema 账单" });
  put("crates/kanzei-tools/src/edit.rs", "//! edit 工具:精确字符串替换。\n\npub struct EditTool;\n", { note: "精确替换编辑" });
  put("crates/kanzei-app/src/files_edit.rs", "//! 文件页编辑:路径解析、只读策略、BOM/换行、file_stat/file_write。\n", { note: "文件页写通道" });
  put("crates/kanzei-app/ui/17-files-editor.js", "// 文件页编辑:保存、冲突、轮询、草稿。\nexport const FILES_WATCH_MS = 2000;\n", { note: "文件页编辑器" });
  put("docs/design/files_editor.md", PREVIEW_FILES_DOC_MD, { bom: true });
  put(".kanzei/project/requirements.md", PREVIEW_REQUIREMENTS_MD, { note: "需求清单(托管)" });
  put(".kanzei/kanzei.toml", "[models]\nprimary = \"codex:gpt-5.6-luna\"\n");
  put("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n");
  return { disk, tick: () => (clock += 1000) };
}
function previewFingerprint(file) {
  let h = 0xcbf29ce4;
  for (const ch of `${file.bom ? "\uFEFF" : ""}${file.text}`) h = Math.imul(h ^ ch.codePointAt(0), 0x01000193) >>> 0;
  return `fnv-${h.toString(16).padStart(16, "0")}`;
}
function previewReadonly(path) {
  if (/(^|\/)\.git(\/|$)/i.test(path)) return "git";
  if (/^\.kanzei\/(project|memory)(\/|$)/i.test(path)) return "managed";
  return null;
}
function previewFilesSnapshot(disk) {
  const files = [...disk.entries()].map(([path, file]) => {
    const lines = file.text.split(/\r\n|\n/).length - (file.text.endsWith("\n") ? 1 : 0);
    const md = /\.md$/i.test(path);
    return { path, size: file.text.length + (file.bom ? 3 : 0), lines: md ? null : lines, chars: md ? file.text.length : null, oversized: false, note: file.note };
  });
  files.push({ path: "crates/kanzei-app/ui/09-sessions.js", size: 48120, lines: 1140, oversized: true, note: "线路与会话" });
  const dirs = { "": { files: 0, size: 0, lines: 0 } };
  for (const file of files) {
    const parts = file.path.split("/");
    for (let i = 0; i < parts.length; i += 1) {
      const key = parts.slice(0, i).join("/");
      dirs[key] ??= { files: 0, size: 0, lines: 0 };
      dirs[key].files += 1;
      dirs[key].size += file.size;
      dirs[key].lines += file.lines ?? 0;
    }
  }
  return { files, dirs, dirNotes: { crates: "Rust workspace", "crates/kanzei-tools": "内置工具" }, annotated: 5, annotatable: 9, unannotated: 4, reused: 0 };
}
// ── 分区:文件编辑(完) ──

// ── 分区:网页预览前端 ── UI2-0926 #8:按 IPC 契约(docs/design/preview_pane.md)模拟 preview_* 与 tool_image / delivered_image。
// 浏览器预览里没有原生子 webview:preview_open 只回放一条 kz:preview-state(真机由后端导航后发),截图 / 缩略图用 canvas
// 现画一张假页面。?state=error 时打开的页面报「连接被拒」。
const PREVIEW_STATIC = "http://127.0.0.1:52341/t/0f3a9c7d/r/0/";
function previewUrlFor(target) {
  const text = String(target ?? "");
  if (/^[a-z][\w+.-]*:/i.test(text) && !/^[A-Za-z]:[\\/]/.test(text)) return text;
  return `${PREVIEW_STATIC}${text.replace(/\\/g, "/").replace(/^\.?\//, "")}`;
}
/// 一张假网页截图(base64 PNG,不带 data: 前缀)。只在浏览器里调用(Node 侧 mockedCommandNames 不会执行它)。
export async function previewPagePng({ title = "Acme Dashboard", chart = false, width = 900, height = 600 } = {}) {
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const g = canvas.getContext("2d");
  g.fillStyle = "#f6f7f9";
  g.fillRect(0, 0, width, height);
  g.fillStyle = "#1f2a44";
  g.fillRect(0, 0, width, 64);
  g.fillStyle = "#ffffff";
  g.font = "600 24px 'Segoe UI', sans-serif";
  g.fillText(title, 32, 41);
  g.font = "15px 'Segoe UI', sans-serif";
  g.fillStyle = "#c9d3ea";
  g.fillText("概览   订单   用户   设置", width - 300, 40);
  const cards = [["今日订单", "1,284"], ["转化率", "3.9%"], ["退款", "12"]];
  cards.forEach(([label, value], index) => {
    const x = 32 + index * ((width - 96) / 3 + 16);
    const w = (width - 96) / 3;
    g.fillStyle = "#ffffff";
    g.fillRect(x, 92, w, 104);
    g.strokeStyle = "#e3e6ec";
    g.strokeRect(x + 0.5, 92.5, w - 1, 103);
    g.fillStyle = "#6b7280";
    g.font = "15px 'Segoe UI', sans-serif";
    g.fillText(label, x + 18, 124);
    g.fillStyle = "#111827";
    g.font = "600 32px 'Segoe UI', sans-serif";
    g.fillText(value, x + 18, 172);
  });
  g.fillStyle = "#ffffff";
  g.fillRect(32, 220, width - 64, height - 252);
  g.strokeStyle = "#e3e6ec";
  g.strokeRect(32.5, 220.5, width - 65, height - 253);
  g.strokeStyle = chart ? "#e0662c" : "#3b82f6";
  g.lineWidth = 3;
  g.beginPath();
  const points = [0.62, 0.48, 0.55, 0.36, 0.42, 0.28, 0.33, 0.2, 0.26];
  points.forEach((v, i) => {
    const x = 64 + i * ((width - 128) / (points.length - 1));
    const y = 240 + v * (height - 300);
    if (i) g.lineTo(x, y);
    else g.moveTo(x, y);
  });
  g.stroke();
  return canvas.toDataURL("image/png").split(",")[1];
}
function previewCommands(params) {
  const pane = { url: "", visible: false, processId: null, bounds: null };
  const errorMode = params.state === "error";
  const stateOf = () => ({
    url: pane.url, title: errorMode ? "" : "Acme Dashboard", loading: false, canBack: !errorMode, canForward: false,
    visible: pane.visible, boundProcessId: pane.processId, device: "fill", scheme: "auto",
    ...(errorMode ? { error: { kind: "connection_refused", text: "net::ERR_CONNECTION_REFUSED" } } : {}),
  });
  return {
    preview_open: (args, ctx) => {
      pane.url = previewUrlFor(args?.target);
      pane.visible = true;
      pane.processId = args?.processId ?? null;
      pane.bounds = args?.bounds ?? null;
      setTimeout(() => ctx.emit("kz:preview-state", stateOf()), 20);
      return null;
    },
    preview_set_bounds: (args) => {
      pane.bounds = args;
      return null;
    },
    // 与 pane.rs 同一语义(preview_pane.md §4 / §6):显示时按上报的 processId 覆盖绑定(null 即解绑),隐藏时保留绑定。
    preview_set_visible: (args) => {
      pane.visible = Boolean(args?.visible);
      if (pane.visible) pane.processId = args?.processId ?? null;
      return null;
    },
    preview_nav: () => null,
    // 关闭 = 释放面板(后端发 visible:false、url:"" 的关闭态,绑定一并清掉)。
    preview_close: () => {
      pane.url = "";
      pane.visible = false;
      pane.processId = null;
      return null;
    },
    preview_capture: async () => ({ png: await previewPagePng(), width: 900, height: 600 }),
    preview_console: () => ({ entries: [] }),
    preview_console_clear: () => null,
    preview_device: () => null,
    preview_pick: () => null,
    preview_snippet: () => ({ url: `${PREVIEW_STATIC.replace("/r/0/", "/s/")}1.html` }),
    preview_dev_urls: () => ({
      urls: [
        { url: "http://localhost:5173/", command: "npm run dev -- --host", pid: 18244 },
        { url: "http://localhost:6006/", command: "npm run storybook", pid: 18302 },
      ],
    }),
    preview_clear_site_data: () => null,
    preview_open_devtools: () => null,
    preview_open_external: () => null,
    tool_image: async (args) => ({ png: await previewPagePng({ title: /login/.test(String(args?.rel ?? "")) ? "Acme · 登录" : "Acme Dashboard" }) }),
    delivered_image: async () => ({ png: await previewPagePng({ title: "dashboard.png", chart: true }) }),
  };
}
// ── 分区:网页预览前端(完) ──

export function createFixtures({ scene = "chat", theme = "dark", params = {} } = {}) {
  const now = Date.now();
  const running = scene === "chat" || scene === "agents";
  const dialog = params.dialog || "ask";
  const language = params.lang === "en" ? "en" : "zh";
  const state = {
    uiPrefs: {
      theme, work_priority: {}, auto_max: null, continue_prompt: null, process_auto_state: {},
      workspace_state: {}, memory_view: null,
      // ── 分区:架构图 ── 场景参数 tab=<标签 key>、full=1 落到 ui_layout.arch(架构页读它选标签与依赖范围)。
      ...(params.tab || params.full ? { ui_layout: { arch: { ...(params.tab ? { diagram: params.tab } : {}), ...(params.full === "1" ? { deps_full: true } : {}) } } } : {}),
    },
    docs: buildDocsSnapshot(),
    fileDisk: previewFileDisk(), // ── 分区:文件编辑 ──
    processes: [
      {
        id: IDS.mainProcess, origin_project: PROJECT, project_dir: PROJECT, worktree_path: null, branch: "release/2026-09-26-ui",
        session_id: IDS.mainSession, model: null, profile: "dev", research_topic: null, reasoning: null,
        manual_models: [], phase_pipeline: false, subagents_enabled: true, tracker_writes: false,
        authority: "primary", stage: running ? "实现" : "空闲", running, label: "主会话",
      },
      {
        id: IDS.lineProcess, origin_project: PROJECT, project_dir: PROJECT, worktree_path: "C:/Users/kanzei/Documents/kanzei code/.kanzei/worktrees/thread-r366-b2", branch: "kanzei/thread-r366-b2",
        session_id: IDS.lineSession, model: "codex:gpt-6-sol", profile: "dev", research_topic: null, reasoning: "high",
        manual_models: [], phase_pipeline: true, subagents_enabled: true, tracker_writes: false,
        authority: "parallel", stage: "复核", running: true, label: "R-366 B2 回退事件",
      },
      {
        id: IDS.idleProcess, origin_project: PROJECT, project_dir: PROJECT, worktree_path: "C:/Users/kanzei/Documents/kanzei code/.kanzei/worktrees/thread-d759", branch: "kanzei/thread-d759",
        session_id: IDS.idleSession, model: null, profile: "dev", research_topic: null, reasoning: null,
        manual_models: [], phase_pipeline: false, subagents_enabled: true, tracker_writes: false,
        authority: "parallel", stage: "空闲", running: false, label: "D-759 手机提问卡片",
      },
    ],
    conversations: new Map([
      [IDS.mainProcess, [
        { sequence: 14, sequences: [14], title: "R-364 B1 账单与常驻名单", preview: "继续 R-364:先把 B1 的逐工具 schema 字符账单跑出来…", message_count: 46, updated_at: "2026-09-26 14:18" },
        { sequence: 13, sequences: [13], title: "build-2009581f 发版", preview: "发版前先跑 verify 全量…", message_count: 18, updated_at: "2026-09-26 11:02" },
        { sequence: 12, sequences: [11, 12], title: "D-759 D-760 D-761 前端修复", preview: "手机提问卡片按 id 对账…", message_count: 31, updated_at: "2026-09-25 22:40" },
        { sequence: 10, sequences: [10], title: "CC/Codex 三方对照登记", preview: "把 A 档全部登记成需求…", message_count: 12, updated_at: "2026-09-25 17:15" },
      ]],
      [IDS.lineProcess, [
        { sequence: 3, sequences: [3], title: "R-366 B2 conversation.rewind", preview: "在分支线上推进 B2…", message_count: 22, updated_at: "2026-09-26 14:05" },
      ]],
      [IDS.idleProcess, []],
    ]),
    cleared: new Set(),
    settings: {
      primary: GLOBAL_PRIMARY, fast: FAST_MODEL, compact: null, language, proxy: "env", profileDefault: "dev",
      reasoning: "high", codexFastMode: false,
    },
  };
  // ── 分区:星座背景 ── ?backdrop=kanzei|big-dipper|orion|cassiopeia|off 预置对话背景偏好(ui_prefs_get.backdrop,截图用);
  // 不带参数 = 后端无记录,走默认(kanzei 标志星座)。
  if (params.backdrop) state.uiPrefs.backdrop = params.backdrop === "off" ? { enabled: false } : { enabled: true, preset: params.backdrop };
  const ids = IDS;
  const events = liveEvents(ids);
  const askFixtures = asks(ids);

  const processById = (id) => state.processes.find((item) => item.id === id);
  const limits = {
    maxTokens: null, subagentMaxTokens: null, subagentTimeoutSecs: null, contextBudgetRatio: null,
    recentVerbatimRatio: null, maxTasksPerTurn: null, maxParallelTools: null, transportRetries: null,
    rateLimitRetries: null, streamRestarts: null,
  };
  const limitDefaults = {
    maxTokens: 32000, subagentMaxTokens: 16000, subagentTimeoutSecs: 900, contextBudgetRatio: 0.7,
    recentVerbatimRatio: 0.35, maxTasksPerTurn: 16, maxParallelTools: 8, transportRetries: 2,
    rateLimitRetries: 3, streamRestarts: 2,
  };

  const settingsGet = ({ projectDir } = {}) => {
    const withProject = Boolean(projectDir);
    return {
      path: GLOBAL_CONFIG,
      primary: state.settings.primary,
      fast: state.settings.fast,
      compact: state.settings.compact,
      language: state.settings.language,
      proxy: state.settings.proxy,
      profileDefault: state.settings.profileDefault,
      reasoning: state.settings.reasoning,
      codexFastMode: state.settings.codexFastMode,
      phaseRosterCapacity: 5,
      providers: [
        { name: "codex", protocol: "openai-responses", baseUrl: "https://chatgpt.com/backend-api/codex", apiKeyEnv: null, apiKey: null, keyPresent: null, auth: "codex", legacyClaudeSubscription: false, contextLimit: 400000, builtin: true, source: "builtin" },
        { name: "deepseek", protocol: "deepseek-responses", baseUrl: "https://api.deepseek.com", apiKeyEnv: "DEEPSEEK_API_KEY", apiKey: null, keyPresent: true, auth: null, legacyClaudeSubscription: false, contextLimit: 128000, builtin: false, source: "global" },
        { name: "ollama", protocol: "openai", baseUrl: "http://127.0.0.1:11434/v1", apiKeyEnv: null, apiKey: null, keyPresent: null, auth: null, legacyClaudeSubscription: false, contextLimit: 32768, builtin: false, source: "global" },
      ],
      limits,
      limitDefaults,
      cadence: { full_test: "entry_close", full_test_batches: null, targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
      cadenceDefaults: { full_test: "entry_close", full_test_batches: null, targeted_test: "every_commit", commit: "per_batch", push: "per_entry" },
      profiles: {},
      permissions: [],
      // UI-0926 #3:effective 只剩本页其余会被项目覆盖的标量;模型的项目覆盖见 projectModelOverrides。
      effective: withProject ? { proxy: "env", profileDefault: "dev", limits: { ...limits, maxTasksPerTurn: 8 } } : null,
      projectConfig: withProject ? PROJECT_CONFIG : null,
      projectModelOverrides: [
        { project: PROJECT, name: "kanzei code", configPath: PROJECT_CONFIG, keys: ["primary", "fast", "compact", "reasoning", "codexFastMode"], current: projectDir === PROJECT },
      ],
    };
  };

  // UI-0926 #3 项目模型配置弹窗:用户现场——项目把五个键全固定成 gpt-5.6-luna/high/Fast 开
  // (与 settings_get.projectModelOverrides 列的五个键一致)。「全局」一列取自上面 settings_get 用的
  // 同一份 state.settings,设置页与弹窗两处说法一致;全局没写的角色键继承时跟随(项目的)primary。
  const previewProjectModels = (projectDir = PROJECT) => {
    const global = state.settings;
    const role = (globalValue) => ({
      project: PROJECT_PRIMARY, global: globalValue ?? null,
      inherited: globalValue || PROJECT_PRIMARY, inheritedSource: globalValue ? "global" : "project", inheritedFollowsPrimary: !globalValue,
      effective: PROJECT_PRIMARY, source: "project", followsPrimary: false,
    });
    return {
      projectDir,
      configPath: PROJECT_CONFIG,
      exists: true,
      fields: {
        primary: { project: PROJECT_PRIMARY, global: global.primary, inherited: global.primary, inheritedSource: "global", inheritedFollowsPrimary: false, effective: PROJECT_PRIMARY, source: "project", followsPrimary: false },
        fast: role(global.fast),
        compact: role(global.compact),
        reasoning: { project: "high", global: global.reasoning, inherited: global.reasoning, inheritedSource: "global", inheritedFollowsPrimary: false, effective: "high", source: "project", followsPrimary: false },
        codexFastMode: { project: true, global: global.codexFastMode, inherited: global.codexFastMode, inheritedSource: "global", inheritedFollowsPrimary: false, effective: true, source: "project", followsPrimary: false },
      },
    };
  };

  // run_prompt:回放一轮假回复,方便在浏览器里点「发送」看流式渲染。
  const fakeRun = (args, ctx) => {
    const process = processById(args?.processId) ?? state.processes[0];
    const sessionId = process.session_id;
    process.running = true;
    const text = String(args?.prompt ?? args?.text ?? "").slice(0, 80);
    const steps = [
      ["kz:meta", { sessionId, model: PROJECT_PRIMARY, agent: "dev", profile: "dev", reasoning: "high", codexFastMode: true, contextLimit: 400000 }],
      ["kz:turn", { sessionId, step: 1, maxSteps: 0 }],
      ["kz:status", { sessionId, stage: "思考", detail: "预览模拟回复" }],
      ["kz:reasoning", { sessionId, text: "**预览模式**\n这是 scripts/ui-preview 的模拟回复,不连接模型。" }],
      ["kz:tool-start", { sessionId, id: `sim-${now}`, name: "read", summary: "README.md", input: { path: "README.md" } }],
      // UI-0926 #6:tool-end 带与历史同源的 content、contentBytes/contentTruncated 与 durationMs。
      ["kz:tool-end", {
        sessionId, id: `sim-${now}`, name: "read", ok: true, outcome: "success", preview: "     1\t# kanzei (+2 lines)",
        content: "     1\t# kanzei\n     2\t\n     3\t文件优先的日常开发工具。\n", contentBytes: 72, contentTruncated: false, durationMs: 14,
      }],
      ["kz:text", { sessionId, text: `收到:「${text}」。\n\n这是**预览模式**的模拟回复——` }],
      ["kz:text", { sessionId, text: "真实运行请在桌面端里发送。" }],
      ["kz:step", { sessionId, input: 1200, output: 80, cacheRead: 0, cacheWrite: 0 }],
      // 形状对照 crates/kanzei-app/src/run/persistence.rs 的 kz:done 与 commands/run.rs 的 kz:idle。
      ["kz:done", { sessionId, steps: 2, halted: false, history: 4, elapsedMs: 1400, input: 1200, output: 80, cacheRead: 0, cacheWrite: 0, tools: { read: 1 }, autoAction: { type: "NoContinue" } }],
      ["kz:idle", { sessionId, reason: "completed" }],
    ];
    let delay = 60;
    for (const [event, payload] of steps) {
      setTimeout(() => {
        if (event === "kz:idle") process.running = false;
        ctx.emit(event, payload);
      }, delay);
      delay += 140;
    }
    return null;
  };

  const commands = {
    // ---- 启动与全局
    app_info: { version: "0.9.26", build: "0.9.26 2009581f 2026-09-26" },
    update_check: { status: "latest", newer: false, current: "0.9.26", latest: "0.9.26" },
    ui_prefs_get: () => state.uiPrefs,
    ui_prefs_set: (args) => { state.uiPrefs = { ...state.uiPrefs, ...args }; return null; },
    settings_get: settingsGet,
    settings_save: (args) => {
      const patch = args?.settings ?? args ?? {};
      for (const key of ["primary", "fast", "language", "proxy", "profileDefault", "reasoning", "codexFastMode"]) {
        if (Object.hasOwn(patch, key)) state.settings[key] = patch[key];
      }
      return null;
    },
    settings_open: null,
    projects_get: () => ({
      current: PROJECT,
      projects: [PROJECT, PROJECT_C, PROJECT_B],
      names: { [PROJECT]: "kanzei code", [PROJECT_C]: "kanzei-rel-0926", [PROJECT_B]: "持续学习" },
    }),
    projects_select: ({ path }) => ({
      current: path || PROJECT,
      projects: [PROJECT, PROJECT_C, PROJECT_B],
      names: { [PROJECT]: "kanzei code", [PROJECT_C]: "kanzei-rel-0926", [PROJECT_B]: "持续学习" },
    }),
    projects_rename: ({ path, name }) => ({ current: path, projects: [PROJECT, PROJECT_C, PROJECT_B], names: { [PROJECT]: "kanzei code", [path]: name } }),
    projects_isolation_report: { autoRepaired: [], shared: [] },
    project_root_info: ({ projectDir }) => ({ selected: projectDir || PROJECT, resolved: projectDir || PROJECT, shared: false }),
    fast_model_status: { managed: true, model: "qwen3.5:4b", installed: true, serviceUp: true, modelPresent: true, ready: true },
    // UI-0926 #3:下一轮视图(形状对照 crates/kanzei-app/src/model_config.rs 的 TurnView / FieldView)。
    model_effective: ({ processId, agent } = {}) => {
      const item = processById(processId);
      const line = item?.model || null;
      const resolved = line && !["primary", "fast", "compact"].includes(line) ? line : PROJECT_PRIMARY;
      const codex = resolved.startsWith("codex:");
      const defaultModel = { ref: "primary", resolved: PROJECT_PRIMARY, source: "project", role: "primary", followsPrimary: false, error: null };
      return {
        agent: agent ?? "dev-pair",
        agentError: null,
        model: line ? { ref: line, resolved, source: "line", role: null, followsPrimary: false, error: null } : defaultModel,
        defaultModel,
        reasoning: item?.reasoning ? { value: item.reasoning, source: "line" } : { value: "high", source: "project" },
        defaultReasoning: { value: "high", source: "project" },
        codexFastMode: { enabled: true, applies: codex, active: codex, source: "project" },
        contextLimit: 400000,
      };
    },
    project_models_get: ({ projectDir } = {}) => previewProjectModels(projectDir),
    project_models_save: ({ projectDir } = {}) => previewProjectModels(projectDir),
    project_config_open: null,
    models_list: () => [
      { id: "primary", label: `primary → ${PROJECT_PRIMARY}` },
      { id: PROJECT_PRIMARY, label: PROJECT_PRIMARY },
      { id: "fast", label: `fast → ${FAST_MODEL}` },
      { id: FAST_MODEL, label: FAST_MODEL },
      ...["gpt-6-astra", "gpt-6-sol", "gpt-6-luna", "gpt-5.6-sol", "gpt-5.6-terra"].map((model) => ({ id: `codex:${model}`, label: `codex:${model}` })),
      { id: "deepseek:deepseek-chat", label: "deepseek:deepseek-chat" },
    ],
    git_status: ({ worktreePath }) => worktreePath
      ? { branch: "kanzei/thread-r366-b2", changes: 2, last: "2d3eaef9 D-762 R-366 B1 文件检查点按裁决落地", files: [
        { path: "crates/kanzei-app/src/run/rewind.rs", additions: 142, deletions: 8 },
        { path: "crates/kanzei-core/src/conversation.rs", additions: 37, deletions: 12 },
      ], additions: 179, deletions: 20 }
      : { branch: "release/2026-09-26-ui", changes: 3, last: "2009581f D-763 规范与注释如实描述各门禁 clippy 口径", files: [
        { path: "crates/kanzei-tools/src/registry.rs", additions: 79, deletions: 1 },
        { path: "docs/design/cc_codex_alignment_impl_maps.md", additions: 24, deletions: 3 },
        { path: "scripts/probe-defer-loading.mjs", untracked: true },
      ], additions: 103, deletions: 4 },
    list_pending_inputs: [],
    cancel_input: false,
    voice_settings_get: { port: 8765, language: "zh" },
    // ---- 线路
    process_list: ({ projectDir } = {}) => state.processes.filter((item) => !projectDir || item.origin_project === projectDir),
    process_create: (args) => {
      const n = state.processes.length + 1;
      const item = {
        id: `p|preview-${n}`, origin_project: args?.projectDir || PROJECT, project_dir: args?.projectDir || PROJECT,
        worktree_path: null, branch: null, session_id: `ses_preview_${n}`, model: null, profile: args?.profile || "dev",
        research_topic: args?.researchTopic || null, reasoning: null, manual_models: [], phase_pipeline: Boolean(args?.phasePipeline),
        subagents_enabled: true, tracker_writes: false, authority: "parallel", stage: "空闲", running: false, label: `预览线路 ${n}`,
      };
      state.processes.push(item);
      state.conversations.set(item.id, []);
      return item;
    },
    process_update: (args) => {
      const item = processById(args?.processId);
      if (item) {
        if (Object.hasOwn(args, "model")) item.model = args.model || null;
        if (Object.hasOwn(args, "reasoning")) item.reasoning = args.reasoning || null;
        if (Object.hasOwn(args, "profile")) item.profile = args.profile;
        if (Object.hasOwn(args, "phasePipeline")) item.phase_pipeline = args.phasePipeline;
        if (Object.hasOwn(args, "subagentsEnabled")) item.subagents_enabled = args.subagentsEnabled;
      }
      return null;
    },
    process_close: ({ processId }) => {
      state.processes = state.processes.filter((item) => item.id !== processId);
      return `已关闭线路 ${processId}`;
    },
    collaboration_snapshot: () => state.processes.map((item) => ({
      process_id: item.id, label: item.label, branch: item.branch, worktree_path: item.worktree_path,
      claim: item.id === IDS.lineProcess ? "R-366" : item.id === IDS.mainProcess ? "R-364" : "未取得条目",
      phase: item.stage, current_tool: item.running ? (item.id === IDS.mainProcess ? "bash" : "edit") : null, running: item.running,
      steps: item.running ? 23 : 0, input_tokens: 148200, output_tokens: 9100,
      changed_files: item.id === IDS.mainProcess
        ? ["crates/kanzei-tools/src/registry.rs", "docs/design/cc_codex_alignment_impl_maps.md"]
        : item.id === IDS.lineProcess ? ["crates/kanzei-app/src/run/rewind.rs", "crates/kanzei-tools/src/registry.rs"] : [],
    })),
    worktree_list: [
      { path: "C:/Users/kanzei/Documents/kanzei code/.kanzei/worktrees/thread-r366-b2", branch: "kanzei/thread-r366-b2", clean: false, files: ["crates/kanzei-app/src/run/rewind.rs", "crates/kanzei-core/src/conversation.rs"], bound_process: IDS.lineProcess },
      { path: "C:/Users/kanzei/Documents/kanzei code/.kanzei/worktrees/thread-d759", branch: "kanzei/thread-d759", clean: true, files: [], bound_process: IDS.idleProcess },
      { path: "C:/Users/kanzei/Documents/kanzei code/.kanzei/worktrees/thread-r340-old", branch: "kanzei/thread-r340", clean: false, files: ["crates/kanzei-app/ui/13-memory.js"], bound_process: null },
      // 用户现场是 12 棵、7 棵有改动:侧栏只列 6 棵(有改动的排前),其余「查看全部」。多给几棵孤儿树把上限撑出来。
      ...["r301", "r318", "d702", "r355", "d731", "r362"].map((tag, index) => ({
        path: `C:/Users/kanzei/Documents/kanzei code/.kanzei/worktrees/thread-${tag}`, branch: `kanzei/thread-${tag}`,
        clean: index % 2 === 0, files: index % 2 === 0 ? [] : ["crates/kanzei-core/src/lib.rs", "docs/design/memory_control_plane.md"].slice(0, index % 3 + 1), bound_process: null,
      })),
    ],
    worktree_harvest_candidates: [],
    pending_asks_get: () => {
      if (scene !== "overlays") return [];
      if (dialog === "ask") return [askFixtures.permission, askFixtures.question];
      if (dialog === "question") return [askFixtures.question];
      return [];
    },
    answer_ask: null,
    auto_state_update: null,
    auto_state_reset: null,
    run_prompt: fakeRun,
    stop_run: (args, ctx) => {
      const process = processById(args?.processId) ?? state.processes[0];
      process.running = false;
      setTimeout(() => ctx.emit("kz:stopped", { sessionId: process.session_id }), 50);
      return null;
    },
    stop_task: null,
    // ---- 对话
    // sequence=null 读当前段;点了「新对话」(conversation_clear)的线路当前段为空,旧段仍可按 sequence 打开。
    conversation_get: ({ processId, sequence }) => (state.cleared.has(processId) && sequence == null) ? []
      : processId === IDS.mainProcess ? mainConversation()
        : processId === IDS.lineProcess ? lineConversation() : [],
    conversation_trace_get: ({ processId, sequence }) => (state.cleared.has(processId) && sequence == null) ? []
      : processId === IDS.mainProcess ? mainTraces() : [],
    conversation_list: ({ processId }) => state.conversations.get(processId) ?? [],
    // 与后端 ConversationDeleteOutcome 同形;清单里序号最大的一段是当前段,删了它当前段即为空。
    conversation_delete: ({ processId, sequences }) => {
      const list = state.conversations.get(processId) ?? [];
      const drop = new Set((sequences ?? []).flat());
      const dropped = list.filter((item) => item.sequences.some((seq) => drop.has(seq)));
      const latest = Math.max(0, ...list.flatMap((item) => item.sequences));
      const clearedCurrent = dropped.some((item) => item.sequences.includes(latest));
      if (clearedCurrent) state.cleared.add(processId);
      state.conversations.set(processId, list.filter((item) => !dropped.includes(item)));
      return { deleted: dropped.length, redacted_inputs: 0, segments: dropped.length, cleared_current: clearedCurrent };
    },
    conversation_clear: ({ processId }) => {
      if (!processId) return null;
      state.cleared.add(processId);
      const list = state.conversations.get(processId) ?? [];
      const next = Math.max(0, ...list.flatMap((item) => item.sequences)) + 1;
      state.conversations.set(processId, [
        { sequence: next, sequences: [next], title: "新对话", preview: "", message_count: 0, updated_at: "2026-09-26 14:20" },
        ...list,
      ]);
      return null;
    },
    conversation_cleanup: { artifact_cleanup_errors: [], backup_cleanup_errors: [], actual_freed_bytes: 18432 },
    summarize_chat: { summary: "预览模式不生成总结", path: "" },
    // ---- 文档
    docs_snapshot: () => state.docs,
    docs_update: (args) => `updated: ${args?.id ?? "?"} (预览模式,未写盘)`,
    docs_archive_entries: ({ kind }) => kind === "req" ? [docEntry("R-352", "运行画像按任务关闭为主粒度", "done", { closed: true })] : [docEntry("D-759", "手机提问卡片按 id 对账与显式取消", "fixed", { closed: true })],
    docs_read: () => ({ path: `${PROJECT}/.kanzei/project/requirements.md`, name: "requirements.md", content: "# Requirements\n\n## R-364 工具延迟加载 [doing]\n- 内容: …" }),
    docs_read_custom: ({ path }) => ({ path, name: String(path).split(/[\\/]/).pop(), content: "# 设计文档\n\n预览模式下的占位内容。" }),
    docs_open: null,
    test_runs_init_refs: { backfilled: 0 },
    test_runs_snapshot: {
      active: [
        { id: "T-413", title: "cargo test -p kanzei-tools tool_search", status: "running", fields: [{ key: "命令", value: "cargo test -p kanzei-tools tool_search" }], refs: ["R-364"] },
      ],
      archived: [
        { id: "T-412", title: "cargo test -p kanzei-tools registry::", status: "passed", fields: [{ key: "命令", value: "cargo test -p kanzei-tools registry::" }, { key: "摘要", value: "3 passed; 0 failed" }], refs: ["R-364"] },
        { id: "T-411", title: "scripts/verify.ps1 全量", status: "failed", fields: [{ key: "摘要", value: "13 步中第 9 步 ui-i18n-smoke 失败" }], refs: ["D-763"] },
      ],
    },
    defect_review: { empty: false, defectCount: 3, report: "# 缺陷自动审查报告\n\n- D-764: `crates/kanzei-app/ui/09-sessions.js:279` 有可复核证据" },
    quick_req: "added: R-371",
    idea_split: "I-031 → R-371",
    conventions_init: null,
    // ---- 设置
    agent_directory_get: {
      profile: "dev",
      agents: [
        { name: "dev", source: "builtin", path: null, profile: "dev", mode: "primary", model: "primary", steps: 0, status: "available", systemPreview: "你是 kanzei 的开发主代理……", error: null },
        { name: "explore", source: "builtin", path: null, profile: "dev", mode: "subagent", model: "fast", steps: 12, status: "available", systemPreview: "只读勘察子代理", error: null },
        { name: "review_gate", source: "project", path: `${PROJECT}/.kanzei/agents/review_gate.md`, profile: "dev", mode: "subagent", model: "fast", steps: 8, status: "available", systemPreview: "复核门禁改动", error: null },
      ],
    },
    agent_directory_open: null,
    // 真实形态(settings.rs permission_rules_get):{path, rules:[{index, action, resource, effect}]},只列 allow。
    permission_rules_get: {
      path: PROJECT_CONFIG,
      rules: [
        { index: 0, action: "bash", resource: JSON.stringify({ command: "cargo test --workspace", workdir: PROJECT }), effect: "allow" },
        { index: 1, action: "edit", resource: "crates/kanzei-tools/src/registry.rs", effect: "allow" },
      ],
    },
    permission_rule_delete: null,
    provider_test: { ok: true, latencyMs: 412, message: "连接正常" },
    mobile_device_list: [],
    mobile_service_start: { address: "http://127.0.0.1:8790", token: "PREVIEW", lan: false, devices: [] },
    mobile_service_stop: null,
    // ---- 其它视图
    // 形状对照 crates/kanzei-app/src/projects.rs workspace_snapshot:status 是会话状态(running/idle/failed),
    // conversation 是 conversation_list 的首段 {title, message_count},recent_activity 是 run.trace 载荷
    // ({events:[{name,text}]}),lines 是 process_list 的线级现场。三个项目与 projects_get 同序。
    workspace_snapshot: {
      current: PROJECT,
      projects: [
        {
          path: PROJECT, name: "kanzei code", current: true, status: "running", updated_at: 1790389800000, pending_count: 1,
          conversation: { sequence: 812, created_at: 1790386200000, title: "R-364 B1 账单与常驻名单", message_count: 46 },
          recent_activity: [{ events: [{ name: "bash", text: "cargo test -p kanzei-tools" }, { name: "edit", text: "crates/kanzei-tools/src/registry.rs" }, { name: "read", text: "docs/design/cc_codex_alignment_impl_maps.md" }] }],
          lines: [
            { id: IDS.mainProcess, label: "主会话", running: true, stage: "实现", branch: "release/2026-09-26-ui", worktree_path: null, profile: "dev" },
            { id: IDS.lineProcess, label: "R-366 B2 回退事件", running: true, stage: "复核", branch: "kanzei/thread-r366-b2", worktree_path: `${PROJECT}/.kanzei/worktrees/thread-r366-b2`, profile: "dev" },
            { id: IDS.idleProcess, label: "D-759 手机提问卡片", running: false, stage: "空闲", branch: "kanzei/thread-d759", worktree_path: `${PROJECT}/.kanzei/worktrees/thread-d759`, profile: "dev" },
          ],
          running_lines: 2,
        },
        {
          path: PROJECT_C, name: "kanzei-rel-0926", current: false, status: "idle", updated_at: 1790371800000, pending_count: 0,
          conversation: { sequence: 64, created_at: 1790368200000, title: "合并 release/2026-09-26-ui 并跑 verify", message_count: 12 },
          recent_activity: [{ events: [{ name: "bash", text: "pwsh scripts/verify.ps1 -Full" }] }],
          lines: [{ id: "d|kanzei-rel-0926", label: "主会话", running: false, stage: "空闲", branch: "main", worktree_path: null, profile: "dev" }],
          running_lines: 0,
        },
        {
          path: PROJECT_B, name: "持续学习", current: false, status: "failed", updated_at: 1790299800000, pending_count: 0,
          conversation: null, recent_activity: [], lines: [], running_lines: 0,
        },
      ],
    },
    // ── 分区:文件编辑 ── 内存磁盘(previewFileDisk):场景 files 经 state.fileDisk 模拟「代理改了磁盘」。
    files_snapshot: () => previewFilesSnapshot(state.fileDisk.disk),
    file_preview: ({ path } = {}) => {
      const file = state.fileDisk.disk.get(path);
      if (!file) throw `无法打开 ${path}: 文件不存在`;
      const crlf = (file.text.match(/\r\n/g) ?? []).length;
      const total = (file.text.match(/\r\n|\r|\n/g) ?? []).length;
      return {
        content: file.text, binary: false, truncated: false, size: file.text.length + (file.bom ? 3 : 0), hash: previewFingerprint(file),
        bom: file.bom, eol: total && crlf * 2 > total ? "crlf" : "lf", mixedEol: crlf > 0 && crlf < total, encoding: "utf-8",
        mtimeMs: file.mtime, readonly: previewReadonly(path),
      };
    },
    file_stat: ({ path } = {}) => {
      const file = state.fileDisk.disk.get(path);
      return file ? { exists: true, size: file.text.length + (file.bom ? 3 : 0), mtimeMs: file.mtime } : { exists: false, size: 0, mtimeMs: null };
    },
    file_write: ({ path, content, expectedHash, bom, evidence } = {}) => {
      const code = previewReadonly(path);
      if (code) throw `READONLY:${code}`;
      const disk = state.fileDisk.disk;
      const file = disk.get(path);
      if (file) {
        const current = previewFingerprint(file);
        if (expectedHash !== current) return { status: "conflict", hash: current, size: file.text.length, mtimeMs: file.mtime, exists: true, evidence: null };
      } else if (expectedHash != null) {
        return { status: "conflict", hash: null, size: 0, mtimeMs: null, exists: false, evidence: null };
      }
      const next = { text: String(content ?? ""), bom: Boolean(bom), note: file?.note ?? null, mtime: state.fileDisk.tick() };
      disk.set(path, next);
      return { status: "saved", hash: previewFingerprint(next), size: next.text.length, mtimeMs: next.mtime, exists: true, evidence: evidence && file ? `.kanzei/quarantine/files-overwrite-${next.mtime}/${path}` : null };
    },
    // ── 分区:文件编辑(完) ──
    architecture_snapshot: {
      index_path: `${PROJECT}/.kanzei/project/architecture/README.md`,
      index: "# 架构索引\n\n- [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md):记忆控制平面基线。\n",
      design_docs: [{ name: "memory_control_plane.md", title: "记忆控制平面", bytes: 18200 }, { name: "cc_codex_alignment_20260925.md", title: "CC/Codex 对齐", bytes: 40210 }],
      graph: [["kanzei-app", "kanzei-core"], ["kanzei-app", "kanzei-tools"], ["kanzei-tools", "kanzei-harness"], ["kanzei-core", "kanzei-harness"], ["kanzei-tools", "kanzei-llm"]],
    },
    research_library_list: { entries: [], diagnostics: [] },
    run_metrics: { rounds: [] },
    run_metrics_by_category: { categories: [] },
    run_metrics_by_task: { completed_tasks: [], in_progress_tasks: [], trend: { closed_task_count: 0, completed_task_count: 0 } },
    memory_overview: { scopes: [{ scope: "project", root: PROJECT, total: 0, hitsTotal: 0, categories: {}, integrity: [], inboxPending: 0 }] },
    // ── 分区:记忆图谱 ── 真实记忆子集(memory-graph-fixture.mjs);memory / memory-graph 场景读它。
    memory_entries: (args) => memoryEntriesFor(args?.scope ?? "project"),
    memory_graph: () => MEMORY_GRAPH_FIXTURE,
    // ── 分区:架构图 ──
    architecture_snapshot: () => archSnapshot(params),
    memory_entry_get: (args) => {
      const entry = memoryEntryFor(args?.scope, args?.id);
      if (!entry) throw `记忆 ${args?.id} 不存在(活动与归档里都没有)`;
      return entry;
    },
    memory_recalls: { rounds: [], rounds_total: 0 },
    memory_note_candidates: [],
    memory_value_flags: { zero_read: [], frequent: [], stale_archived: 0 },
    memory_control_plane: { backlog: 0, oldest_waiting: null, batch: null, promotion_gaps: 0, recall: { recalled: 0, injected: 0, read: 0, read_observed: 0 }, effects: [], experience_facts: [] },
    memory_context_bill: { turns: [] },
    memory_chat_history: [],
    // ── 分区:网页预览前端 ──
    ...previewCommands(params),
  };

  const EMPTY_ARRAY = /(_list|_entries|_history|_candidates|_templates|_recalls|_page)$/;
  const defaultFor = (cmd) => (EMPTY_ARRAY.test(cmd) ? [] : null);

  return { commands, defaultFor, ids, events, asks: askFixtures, latencyMs: 4, project: PROJECT, state };
}

/// Node 侧:列出全部模拟命令名(shoot.mjs 报告用)。
export function mockedCommandNames() {
  return Object.keys(createFixtures().commands).sort();
}
