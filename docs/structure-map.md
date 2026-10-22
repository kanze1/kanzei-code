# Kanzei 代码结构地图

> 依据 2026-10-22 主工作目录实际扫描。Rust workspace（9 crates）+ Tauri 桌面端 + 无构建链原生 ESM 前端 + 脚本工具链。
> 口径：`find crates/<crate>/src -name '*.rs'` 计模块数，`ls ui/*.js` 计前端模块数，KB 为文件实际字节。原 8 个 crate 的 `src/` 合计 469 个 `.rs`；另有 32 个 `src/` 外的 `tests/`·`examples/`·`build.rs`（`kanzei/tests` 22 · `kanzei-core` 4 · `kanzei-tools/examples` 3 · `kanzei-app` 2 · `kanzei-llm/tests` 1），不计入下方各节。

---

## 0. 顶层目录

```
kanzei-code/
├─ crates/         1022 文件 · Rust workspace + Tauri 桌面端 + 前端
├─ docs/            494 文件 · 设计/架构/报告/原型/评审
├─ scripts/         236 文件 · 验证/发布/UI 冒烟/构建脚本
├─ tests/fixtures/    2 文件 · 附件夹具（budget.xlsx / duration.xlsx）
├─ extras/            0 文件 · 空目录残留（visual-authoring 已不在仓库）
├─ Cargo.toml / Cargo.lock
├─ package.json / eslint.config.js
└─ README.md / LICENSE.md
```

`scripts/` 的 118 个 `ui-*-smoke.mjs` 是前端回归主力；`verify.ps1` / `verify-policy.mjs` / `package.ps1` / `release.ps1` 构成发布门禁链。

受版本管理的顶层只有 `crates/ docs/ scripts/ tests/` 加根级配置；`.kanzei/` 内被跟踪的 329 个文件是 `memory/`(294，Markdown 真源) · `research/`(24) · `project/`(9) · `kanzei.toml` · `project-knowledge.json`，其余（`state.db`、`memory/index.db`、`artifacts/`、`quarantine/`、各类 `.lock`、`summaries/`）与 `dist/ output/ target/ node_modules/` 全部 gitignore，不入本图口径。`logs/`、`extras/` 是空目录。

---

## 1. Crate 依赖图（单向无环）

```
                    kanzei-base          零依赖底座：原子写/文件锁/路径/写日志/指纹
                   ╱     │      ╲
          kanzei-llm     │      kanzei-harness
          (协议/流式)     │      (五注册表 + 硬门禁)
                   ╲     │      ╱
                    kanzei-core          运行引擎：session runner + SQLite 事件溯源
                         │
                    kanzei-memory        记忆控制面 + docstore + embed + 回放评估
                         │
                    kanzei-tools         157 rs：全部内置工具 + profile 组件
                         │
                    kanzei-runtime       运行装配、定时执行与子代理配置
                    ╱          ╲
             kanzei (kz CLI)   kanzei-app (Tauri 桌面端)
```

| Crate | 依赖（workspace 内） | 分组 | 职责 |
|---|---|---|---|
| `kanzei-base` | — | 基础 | 原子写、跨进程 `FileLock`、路径形态、写日志、内容指纹 |
| `kanzei-llm` | base | 运行时 | Protocol × Endpoint × Auth × Framing 四轴归一为 `LlmEvent` 流 |
| `kanzei-harness` | base | 运行时 | agents/tools/skills/context/permissions 五注册表 + 拦截器硬门禁 |
| `kanzei-core` | base, llm, harness | 运行时 | session runner、调度、事件存储、压缩、记忆与执行协调 |
| `kanzei-memory` | base, harness, llm, core | 能力 | 分层记忆、docstore、embed、检索、回放评估 |
| `kanzei-tools` | base, harness, llm, memory, core | 能力 | read/write/edit/bash/git/tracker/team/研究/定时/验证 |
| `kanzei-runtime` | base, core, harness, llm, memory, tools | 装配 | CLI/桌面共用运行装配与定时执行 |
| `kanzei` | core, llm, tools, harness, memory, runtime | 入口 | `kz` CLI |
| `kanzei-app` | core, harness, llm, tools, memory, runtime | 入口 | Tauri 桌面端 `kzapp` |

运行装配已移入 `kanzei-runtime`，能力 crate 不依赖装配层。入口直接声明 `kanzei-memory` 依赖；tools 不再透传 memory/docstore/embed/replay_eval。memory 的调度副本已删除，工作选择由入口负责，memory 只接收检索词。core 的 `store` 实现模块为私有，外部只使用公开领域服务。边界由编译器和 `scripts/architecture-boundaries.mjs` 共同校验。

---

## 2. 各 crate 内部结构

### 2.1 `kanzei-base/src`（11 rs · 扁平 + 两个实现分片目录）

| 文件 | 职责 |
|---|---|
| `lib.rs` | crate 根 + `content_hash` / `file_content_hash`（FNV-1a 指纹） |
| `atomic_file.rs` (+`atomic_file/`) | 原子写 + 跨进程文件锁 `FileLock`；分片 `lock.rs`、`lock/directory.rs`、`lock_tests.rs`、`tests.rs` |
| `path_form.rs` | 路径形态规范化与判定 |
| `write_log.rs` (+`write_log/`) | 写日志（路径 + 写后指纹 + 身份），围栏收口对账凭据；分片 `codec.rs`、`tests.rs` |
| `test_support.rs` | 各 crate 共用的测试夹具 |

### 2.2 `kanzei-core/src`（74 rs）

```
一级 (9)   lib.rs assemble.rs(路由/上下文装配) history.rs(消息历史过滤)
           orchestration.rs research.rs research_callback.rs replay.rs
           notification.rs experience_events.rs
│
├─ store/  (31)        项目级 SQLite 会话事件存储：只持久化事实
│    mod.rs schema.rs typed.rs(+typed/projection.rs, typed/writer_owner.rs)
│    session.rs(+session/artifact_refs.rs) events.rs task.rs work.rs inbox.rs
│    processes.rs episodes.rs decisions.rs eval.rs deliveries.rs
│    notifications.rs memory_observations.rs memory_retention.rs
│    research_runs.rs file_checkpoints.rs rewind.rs path_migration.rs
│    telemetry.rs workspace.rs mobile_devices.rs testutil.rs
│    artifact_liveness.rs context_manifest.rs segment_metadata.rs
│    session_execution.rs
│
└─ runner/ (17 + drive/ 15)   agent 主循环：harness 快照驱动，每次调用过硬门禁
     mod.rs drive.rs(95KB) subagent.rs(+subagent/spec.rs) tool_exec.rs
     context.rs compaction.rs recall.rs metrics.rs event.rs observations.rs
     recovery.rs redundancy.rs item_context.rs(+item_context/tests.rs)
     tool_images.rs tool_failure_telemetry.rs schema_check.rs delegation.rs
     └─ drive/: assembly.rs batch.rs serial_tools.rs parallel_tools.rs(+tests.rs)
                permissions.rs question.rs context_budget.rs tool_catalog.rs
                halt.rs history.rs task_results.rs execution_tests.rs
                └─ execution_tests/: input.rs mutation.rs
```

关键：`store/` 输入先进 `inbox`，提升后才成可见消息；`runner/drive/batch.rs` 承载长运行收口（每 32 步或 15 分钟进入最多 8 步）。新增的 `runner/recovery.rs`、`runner/observations.rs` 与 `store/memory_retention.rs`、`store/session_execution.rs` 把恢复证据与记忆保留纳入了同一套事件真源。

### 2.3 `kanzei-harness/src`（39 rs）

```
核心契约   harness.rs(Harness/HarnessSnapshot/HarnessDraft) registry.rs
           execution.rs  component: context.rs markdown.rs tool.rs defs.rs
           skills.rs subagent_policy.rs
门禁       permission.rs(55KB, Ruleset/Rule/Effect；分片 permission/{filesystem,
           scoped_file,sensitivity}.rs) permission_persist.rs managed_fence.rs
           auto_run.rs(HarnessIntensity) repair.rs project_trust.rs
编排       orchestration.rs(读写租约/屏障/阶段) handoff.rs pending_question.rs
工具面     tool_pipeline.rs tool_search.rs
其他       config.rs(113KB KanzeiConfig) areas.rs conventions.rs refs.rs
           home.rs project_root.rs read_ledger.rs progress.rs async_mailbox.rs
└─ config/ (6)  cadence.rs embeddings.rs limits.rs models.rs permissions.rs web.rs
```

> 原 `core/src/phase.rs`（PhaseOrchestrator / ScoutTask 固定勘察复核流水线）与 `runner/line_runtime.rs` 已整体删除，不是改名——见 `6da95ac0 改为三档子代理协作并移除固定勘察复核流水线`。

### 2.4 `kanzei-llm/src`（17 rs）

```
lib.rs  client.rs(LlmClient/Endpoint/Route)  request.rs  event.rs(LlmEvent)
error.rs  sse.rs(SSE 解帧)  proxy.rs
├─ auth/     (4)  mod.rs store.rs codex.rs(+codex/models.rs)
└─ protocol/ (6)  mod.rs anthropic.rs openai.rs
                  openai_responses.rs deepseek_responses.rs tool_schema.rs
```

### 2.5 `kanzei-memory/src`（35 rs）

```
lib.rs  docstore.rs  embed.rs(向量通道)  replay_eval.rs(六臂回放评估)
        scheduling.rs(取活调度链副本)
├─ memory/ (18 + retrieval/ 3 + store/ 2)
│    markdown 是内容真源；index.db 是可重建检索索引
│    mod.rs(132KB) store.rs(151KB) index.rs manager.rs tools.rs
│    admission.rs inbox.rs ledger.rs lifecycle.rs migration.rs retention.rs
│    telemetry.rs relevance.rs privacy.rs recall_search.rs sources.rs usage.rs
│    └─ retrieval/: mod.rs recall.rs search.rs
│    └─ store/    : archive.rs creation.rs
└─ docstore/ (7)  archive.rs model.rs parse.rs render.rs
                  repository.rs requirement.rs validation.rs
```

分级 = scope(Global/Project) × category(preference/habit/fact/sop)；episode 走 `state.db`。检索为 fingerprint/lexical + 可选 dense/hybrid，向量存 SQLite 表由 Rust 余弦扫描 + RRF 融合（非 sqlite-vec）。

### 2.6 `kanzei-tools/src`（157 rs · 最大能力聚合体）

**一级文件（64）**

| 族 | 文件 |
|---|---|
| 文件读写 | `read.rs` `write.rs` `edit.rs` `files.rs` `file_access.rs` `glob.rs` `grep.rs` `symbols.rs` `search_walk.rs` |
| 执行 | `bash.rs` `shell.rs` `process.rs` `background.rs` `managed.rs` `sandbox.rs` |
| 研究（12） | `research_loop` `research_plan` `research_runner` `research_write` `research_verify` `research_workflow` `research_index` `research_control` `research_environment` `prior_art` `latex_tool` `plot_tool` |
| 记录追踪 | `tracker.rs`(238KB, 最大) `test_record.rs` `incident.rs` `close_telemetry.rs` `quarantine.rs` `work.rs` `git_batches.rs` |
| 架构约定 | `architecture.rs` `arch_diagram.rs` `arch_diagram_lint.rs` `conventions.rs` `memory_consolidation.rs` `memory_dream.rs` `project_state.rs` `project_knowledge.rs` `cross_tree.rs` |
| 联网 | `webfetch.rs` `websearch.rs` `web_refs.rs` `web_content.rs` `public_http.rs` |
| 其他 | `base.rs` `browser_tool.rs` `preview_server.rs` `dev_urls.rs` `palette.rs` `question.rs` `subagent.rs` `worktree.rs` `git.rs` `frontend.rs` `local_validation.rs` `profiles.rs` `lib.rs` |

**子目录（27 个模块，共 93 rs）**

| 目录 | rs | 职责 |
|---|---|---|
| `tracker/` (+`actions/`) | 10 | 条目字段/校验/取活调度 + `requirement_contract.rs` 需求契约 |
| `git/` | 7 | commands/index/observe/plan/finalize/tool/worktree |
| `profiles/` | 7 | 档位组件 dev / general / readonly / research / **isolated** + policy |
| `research_workflow/` | 7 | compute / lifecycle / paper / rounds / tool |
| `work/` | 7 | Work Unit：context/execution/log/output/reconcile/resume/tool |
| `refgraph/` | 6 | 记忆知识图谱与引用抽取（含 `memory_graph/`，R-368） |
| `team/` | 6 | 团队代理 mod/store/tools/workspace/**capacity** |
| `verification/` | 5 | 冻结源码快照与独立 worker |
| `background/` | 4 | 后台进程注册/生命周期/监控/持久化 |
| `browser_tool/` `websearch/` | 4 | 各自后端适配与实现分片 |
| `schedules/` | 4 | 定时任务 hosts / when / executor |
| `memory_dream/` | 3 | 记忆整理（dream）执行分片 |
| `bash/` `managed/` `prior_art/` `project_state/` `test_record/` | 2 | 各自实现分片 |
| `conventions/` `latex_tool/` `plot_tool/` `project_knowledge/` `read/` `research_control/` `web_refs/` `webfetch/` | 1 | 单文件后端分片 |

### 2.7 `kanzei/src`（20 rs · `kz` CLI）

```
main.rs
└─ cli/ (19)
     mod.rs(49KB 命令分发)  run.rs(+run/events.rs finalize.rs permissions.rs)
     metrics.rs config.rs lock.rs work.rs worktree.rs trust.rs
     tracker.rs memory.rs eval.rs artifacts.rs compact.rs
     quarantine.rs schedule.rs shadow.rs
```

### 2.8 `kanzei-app/src`（116 rs · Tauri）

**一级模块（68）**

| 域 | 文件 |
|---|---|
| 入口契约 | `main.rs`(257 行，注册点为 `.invoke_handler(runtime_service::route(native_dispatch::handler()))`，不再逐条枚举 command) `state.rs`(46KB) `ipc_contract.rs` `typed_events.rs` `runtime_events.rs` `runtime_service.rs` `runtime_continuation.rs` `native_dispatch.rs` `native_desktop.rs` `native_control.rs` `app_events.rs` |
| 会话运行 | `conversation.rs` `conversation_actions.rs` `auto_run.rs` `subagents.rs` `subagents_capture_tests.rs` `collaboration.rs` `general_chat.rs` `durable_questions.rs` `manual_compact.rs` `composer_drafts.rs` |
| 文件 | `files_view.rs` `files_edit.rs`(50KB) `files_draft.rs` `attachments.rs` |
| 记忆文档 | `memory.rs` `memory_chat.rs` `docs.rs`(46KB) `research_library.rs` `research_topics.rs` `research_latex.rs` `research_auto.rs` `requirement_capture.rs` |
| 配置 | `settings.rs`(82KB) `model_config.rs`(65KB) `prefs.rs` `fast_model.rs` `skills.rs` |
| 项目工作区 | `projects.rs` `workspace.rs` `schedules.rs` `context_view.rs` `worktree_tests.rs` |
| 移动端外部 | `mobile.rs`(95KB) `mobile_notify.rs` `desktop_bridge.rs` `update.rs` |
| 其他 | `voice.rs` `voice_service.rs` `screenshot.rs` `softwire.rs` `decisions.rs` `deliveries.rs` `agent_directory.rs` `agent_team.rs` `async_mailbox.rs` `experience_events.rs` `harness_ext.rs` `orchestration_trace.rs` `projection_gate.rs` `terminal_monitor.rs` `verification_monitor.rs` `browser_selection.rs` |

**子目录（13 个，共 49 rs）**：`run/`(12：mod/assembly/baseline/coordinator/execution/facts/input/persistence/work_start + `assembly/components.rs` `coordinator/steering_tests.rs` `events/mod.rs`) · `commands/`(10：mod/models/os_open/summarize/run + `run/{handoff,input_mode,metrics,tool_process}` + `run/handoff/tests.rs`) · `preview/`(8：子 webview + WebView2 CDP) · `processes/`(7：mod/gate/lifecycle/naming/registry/workspace + 会话测试) · `mobile/`(4：approvals/http/pwa/sse) · `runtime_service/`(2：compatibility/shutdown) · `docs/` `native_control/` `research_auto/` `research_library/` `update/` `workspace/` 各 1（实现或测试分片）。

> `phase_pipeline/` 是空目录残留（0 个 `.rs`），可删。`side_question.rs` 亦已不存在，插话逻辑并入 `run/coordinator`。

全 app `#[tauri::command]` 共 149 个，分布在 47 个文件。

---

## 3. 前端 `crates/kanzei-app/ui/`

**形态**：无打包器、无框架、无 node_modules。`index.html` 直接引原生 ESM，第三方库整目录放 `vendor/`。139 个 `.js` 模块 平铺在根，靠 **`NN-` 数字前缀**表达加载顺序 + 功能域（当前用到 `00`–`32`，共 135 个带前缀 + 4 个不带前缀）。

```
index.html(170KB 主页面, 内嵌模板)  gallery.html  native-control.html
style.css(338KB) + 另 12 个 css：app / surface / workbench / decision-console /
             softwire / context / familiar-workspace / project-conversations /
             project-knowledge / resource-types / skills
assets/    kanzei.svg · kanzei-symbol.svg · voice-capture-worklet.js
vendor/    268 文件 = mermaid(109) · monaco(91) · katex(65) · force-graph(3)
无前缀      bootstrap.js · gallery.js · native-control-panel.js · native-control-view.js
```

| 前缀 | 数 | 域 | 代表文件 |
|---|---|---|---|
| `00` | 23 | 基座：品牌/窗口框架/表面 + IPC/DOM/视图/会话态/草稿 | `00-brand.js` `00-frame.js` `00-surface.js` `00-ipc.js` `00-dom.js` `00-views.js` `00-session-state.js` `00-outbox.js` |
| `01` | 4 | 核心工具 + 全局状态总线 | `01-core.js` `01-runtime-events.js` `01-chat-panes.js` `01-project-resources.js` |
| `02` | 2 | 国际化（229KB，前端最大文件） | `02-i18n.js` `02-language-ui.js` |
| `03` | 9 | 外壳布局：shell/工作区/会话舞台/研究库/作用域 | `03-shell.js` `03-layout.js` `03-workspaces.js` `03-session-stage.js` `03-session-runtime.js` `03-general-scope.js` `03-memory-scope.js` |
| `04` | 9 | 渲染原语：markdown/图/数学/结构化/代码块 | `04-markdown.js` `04-diagram.js` `04-structured.js` `04-structured-parse.js` `04-math.js` `04-code-blocks.js` `04-quote-blocks.js` `04-resource-types.js` |
| `05` | 4 | 会话渲染：聊天/工具摘要/子代理/动作 | `05-chat-render.js` `05-tool-summary.js` `05-subagents.js` `05-conversation-actions.js` |
| `06` | 4 | 侧栏与活动面板 | `06-activity.js` `06-agent-panel.js` `06-deliveries.js` `06-side-policy.js` |
| `07` | 3 | 事件流与上下文视图 | `07-events.js` `07-context-model.js` `07-context-view.js` |
| `08` | 7 | 输入区与模型选择 | `08-compose.js` `08-compose-runtime.js` `08-model-picker.js` `08-models.js` `08-project-models.js` `08-auto.js` `08-native-desktop.js` |
| `09` | 1 | 会话管理 | `09-sessions.js` |
| `10`–`11` | 2 | 文档中心核心与列表 | `10-docs-core.js` `11-docs-list.js` |
| `12` | 8 | 文档页 + 决策台 + 工作台 + 会话树 | `12-docs-pages.js` `12-decision-console.js` `12-workbench.js` `12-session-tree.js` `12-session-menus.js` `12-conversation-model.js` `12-open-tools.js` |
| `13` | 4 | 记忆页 | `13-memory.js` `13-memory-chat.js` `13-memory-dream.js` `13-memory-jobs.js` |
| `14` | 2 | 文档动作与快捷入口 | `14-docs-actions.js` `14-docs-quick.js` |
| `15` | 2 | 约定与其他视图 | `15-conventions.js` `15-views-misc.js` |
| `16` | 2 | 设置页、移动端入口 | `16-settings.js` `16-mobile.js` |
| `17` | 2 | 文件浏览与编辑器 | `17-files.js` `17-files-editor.js` |
| `18` | 2 | 启动引导 / 可选 UI 开关 | `18-startup.js` `18-optional-ui.js` |
| `19` | 5 | 研究工作台与架构视图 | `19-research.js` `19-arch.js` `19-research-auto.js` `19-research-navigation.js` `19-structured-navigation.js` |
| `20`–`21` | 2 | 开发线视图、命令面板 | `20-lines.js` `21-palette.js` |
| `22` | 8 | 视觉层：星图/星座/品牌背景/神经流 | `22-constellation.js` `22-constellation-core.js` `22-brand-backdrop.js` `22-neural-flow.js` `22-visual-runtime.js` |
| `23` | 5 | 语音：采集/控制器/文案 | `23-voice.js` `23-voice-core.js` `23-voice-controller.js` `23-voice-audio.js` `23-voice-copy.js` |
| `24` | 6 | 网页预览、记忆图谱、项目知识、日程 | `24-preview.js` `24-memory-graph.js` `24-memory-graph-model.js` `24-graph-view.js` `24-project-knowledge.js` `24-schedules.js` |
| `25` | 6 | Softwire 操作台 + 会话提问 | `25-softwire.js` `25-softwire-model.js` `25-softwire-run.js` `25-softwire-view.js` `25-softwire-composer.js` `25-chat-questions.js` |
| `26` | 3 | 项目会话 / 需求契约 / 需求研究 | `26-project-conversations.js` `26-requirement-contract.js` `26-requirement-research.js` |
| `27`–`29` | 3 | 代理团队 / 运行时连接 / 通用聊天 | `27-agent-team.js` `28-runtime-connection.js` `29-general-chat.js` |
| `30` | 3 | 管理层与工作区模型/UI | `30-management.js` `30-workspace-model.js` `30-workspace-ui.js` |
| `31` | 3 | 会话控制 / 导出 / 工作选择 | `31-conversation-controls.js` `31-conversation-export.js` `31-work-selection.js` |
| `32` | 1 | 技能页 | `32-skills.js` |

> 相比 2026-10-03：`runtime.html` / `oc-studio.html` / `oc-studio.css` / `assets/oc/` 与 pixi 依赖已整体移除，`native-control.html` + `native-control.css` + `08-native-desktop.js` 取代了 OC 伴侣角色那条线。

---

## 4. `docs/` 与 `scripts/`

```
docs/  (494 文件)
├─ 目录.md              文档总入口
├─ 使用手册.md
├─ current-state.md     ★ 当前实现口径（先读这个）
├─ structure-map.md     本图
├─ cleanup-2026-10-02.md · cleanup-2026-10-03.md
├─ architecture/  (7)   01_runtime_loop.md · 02_harness_registries.md …
├─ design/       (83)   全部 .md：direction_taste / research_mode /
│                       research_library / general_chat / softwire_* /
│                       familiar_workspace / project_workspace /
│                       workspace_information_architecture / global-skills …
├─ reviews/   (279)     ★ 新增域：按日期的 UI 信息架构 / 产品设计审计 /
│                       file-audit / frontend-design 评审批次
├─ reports/    (64)     48 md + 附件，注明日期的审计/验收
│    └─ 2026-10-02-code-map/   完整代码地图（01–11 + inventory/validation.json
│                               + web/index.html 审查入口 + scan.py/validate.py）
├─ prototypes/ (45)     示例交互（css/js/html）
├─ reference/   (1)     deepseek_harness_reference_20260814.md
└─ assets/      (9)
```

```
scripts/  (236 文件，根级 190)
├─ 118 × ui-*-smoke.mjs   前端功能回归冒烟（主要回归手段，全部 .mjs 共 166）
├─ 10 × .ps1              verify / package / release / dev / install-setup / 围栏
├─  9 × .py               file-audit-inventory / resource-native-* / runtime-headless-smoke …
├─ verify.ps1 · verify-policy.mjs · verification-checks.json   验证门禁
├─ package.ps1 · release.ps1 · install-setup.ps1               发布链
├─ gen-esm-*.mjs            ESM 图生成与迁移
├─ check-readme-crates.mjs（README 项目结构表 ↔ Cargo members）
├─ check-design-freshness.mjs（设计文档身份 ↔ tracker 终态）
├─ ui-esm-graph.json · ipc-contract.json · ipc-commands.json · key-paths.json
├─ voice/(22) · ui-preview/(11) · fixtures/(9) · tests/(1)
└─ oc-*.cjs / oc-*.mjs 已随 OC 视觉线整体删除
```

---

## 5. 结构信号（值得注意）

1. **运行时能力按层切分而非单一归属**：`async_mailbox.rs`、`auto_run.rs`、`experience_events.rs`、`orchestration.rs`、`schedules/` 在 core / harness / tools / runtime / app 中对应不同职责；研究回调协议单列为 core 的 `research_callback.rs`。
2. **`kanzei-tools` 是巨型聚合体**：157 rs，`tracker.rs` 单文件 238KB；`kanzei-core/src/store/typed.rs` 146KB、`kanzei-memory/src/memory/store.rs` 151KB、`memory/mod.rs` 132KB、`runner/drive.rs` 95KB、`kanzei-app/src/mobile.rs` 95KB、前端 `02-i18n.js` 229KB —— 大文件集中在工具、记忆与桌面入口层。
3. **前端靠命名约定分层**：没有构建工具，加载顺序与职责完全由 `NN-` 前缀承载，改文件名即改加载顺序；前缀已排到 `32`。
4. **真源分裂点**：Markdown 是记忆/需求/决策真源，`state.db` 是会话事件与运行状态真源（不可从 Markdown 重建），`index.db` 是可重建检索索引。
5. **`.kanzei/project/`** 存需求/缺陷/想法/决策；**`.kanzei/research/<topic>/`** 存来源/发现/报告 —— 均以主根为唯一事实源，worktree 线内不复制。
6. **`kz quarantine` 有归类盲区**：`classify()` 按最后一个 `-` 切类型并要求命中 `KNOWN_KINDS`（`quarantine.rs:15-21`），而 `background/lifecycle.rs` 用 `bg-{process.id}` 生成 tag，落盘成 `bg-bg1-<ts>` → 类型判为 `bg-bg1`，`--type bg` 永远匹配不到，这类目录无法被自动回收，只能人工删。
7. **空目录残留**：`extras/`（曾是 visual-authoring）、`logs/` 与 `crates/kanzei-app/src/phase_pipeline/`（0 个 `.rs`）都已是空壳，git 内无文件。
8. **本图数字无门禁**：`check-readme-crates.mjs` 只对齐 Cargo members ↔ README 项目结构表，`check-design-freshness.mjs` 只管设计文档身份，都不校验本图的文件数与 KB，因此人工扫描口径会随重构漂移（本图 2026-10-03 版即整体过时一次）。

---

## 6. 延伸阅读

- `docs/current-state.md` —— 当前实现口径表（各主题 ↔ 源码入口）
- `docs/reports/2026-10-02-code-map/` —— 全量代码地图（功能/运行/存储/工具/接口/符号索引 + 网页审查入口）
- `docs/architecture/01_runtime_loop.md`、`02_harness_registries.md`
- `.kanzei/project/architecture/README.md` —— 架构索引与历史身份

运行装配源码：`crates/kanzei-runtime/src/lib.rs`；定时执行：`src/schedules.rs`；对应装配、隔离 profile 与真实命令测试位于该 crate 的 `tests/`。文件数量是观测时快照，当前依赖以 Cargo 清单为准。

前端 `01-core.js` 是测试和原生探针使用的兼容入口，生产模块直接导入状态所有者；未使用的 `22-visual-feedback.js` 已删除。React 依赖用于两个浏览器原生表单测试的受控输入夹具，不参与生产 UI。
