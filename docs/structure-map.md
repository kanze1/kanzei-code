# Kanzei 代码结构地图

> 依据 2026-10-03 主工作目录实际扫描。Rust workspace（8 crates）+ Tauri 桌面端 + 无构建链原生 ESM 前端 + 脚本工具链。

---

## 0. 顶层目录

```
kanzei-code/
├─ crates/          858 文件 · Rust workspace + Tauri 桌面端 + 前端
├─ docs/            164 文件 · 设计/架构/报告/原型
├─ extras/           60 文件 · visual-authoring（可选素材制作）
├─ scripts/         133 文件 · 验证/发布/UI 冒烟/构建脚本
├─ tests/fixtures/        测试夹具
├─ Cargo.toml / Cargo.lock
├─ package.json / eslint.config.js
└─ README.md / LICENSE.md
```

`scripts/` 近 80 个 `ui-*-smoke.mjs` 是前端回归主力；`verify.ps1` / `verify-policy.mjs` / `package.ps1` / `release.ps1` 构成发布门禁链。

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
                    kanzei-tools         126 rs：全部内置工具 + profile 组件
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
| `kanzei` | core, llm, tools, harness | 入口 | `kz` CLI |
| `kanzei-app` | core, harness, llm, tools | 入口 | Tauri 桌面端 `kzapp` |

两处刻意破环的复制有注释说明：`kanzei-memory/src/scheduling.rs`（tools/tracker 逐字副本）、`kanzei-tools/src/lib.rs`（再导出 memory/docstore/embed/replay_eval，保证调用点零改动）。入口 crate 不直接依赖 `kanzei-memory`，记忆能力经 tools 透传。

---

## 2. 各 crate 内部结构

### 2.1 `kanzei-base/src`（4 rs · 扁平）

| 文件 | 职责 |
|---|---|
| `lib.rs` | crate 根 + `content_hash` / `file_content_hash`（FNV-1a 指纹） |
| `atomic_file.rs` | 原子写 + 跨进程文件锁 `FileLock` |
| `path_form.rs` | 路径形态规范化与判定 |
| `write_log.rs` | 写日志（路径 + 写后指纹 + 身份），围栏收口对账凭据 |

### 2.2 `kanzei-core/src`（64 rs）

```
lib.rs                 crate 根 + re-export
assemble.rs            路由 / 上下文装配
history.rs             消息历史过滤
phase.rs               PhaseOrchestrator / ScoutTask 阶段机
orchestration.rs       编排
research.rs            研究文档 / topic markdown 解析
research_runner.rs     研究进程回调行解析
replay.rs              事件回放
notification.rs        AgentNotification
experience_events.rs   经验事件
│
├─ store/  (24)        项目级 SQLite 会话事件存储：只持久化事实
│    mod.rs schema.rs typed.rs(+projection.rs) session.rs(+artifact_refs.rs)
│    events.rs task.rs work.rs inbox.rs processes.rs episodes.rs decisions.rs
│    eval.rs deliveries.rs notifications.rs memory_observations.rs
│    research_runs.rs file_checkpoints.rs rewind.rs path_migration.rs
│    telemetry.rs workspace.rs mobile_devices.rs testutil.rs
│
└─ runner/ (18 + drive/ 11)   agent 主循环：harness 快照驱动，每次调用过硬门禁
     mod.rs drive.rs(71KB) subagent.rs(+spec.rs) tool_exec.rs context.rs
     compaction.rs recall.rs metrics.rs event.rs line_runtime.rs
     redundancy.rs item_context.rs(+tests.rs) tool_images.rs
     tool_failure_telemetry.rs schema_check.rs delegation.rs
     └─ drive/: assembly.rs batch.rs serial_tools.rs parallel_tools.rs
                permissions.rs question.rs context_budget.rs tool_catalog.rs
                halt.rs history.rs task_results.rs
```

关键：`store/` 输入先进 `inbox`，提升后才成可见消息；`runner/drive/batch.rs` 承载长运行收口（每 32 步或 15 分钟进入最多 8 步）。

### 2.3 `kanzei-harness/src`（32 rs）

```
核心契约   harness.rs(Harness/HarnessSnapshot/HarnessDraft) registry.rs
           component: context.rs markdown.rs tool.rs defs.rs
门禁       permission.rs(44KB, Ruleset/Rule/Effect) permission_persist.rs
           managed_fence.rs auto_run.rs(HarnessIntensity) repair.rs
编排       orchestration.rs(读写租约/屏障/阶段) handoff.rs pending_question.rs
工具面     tool_pipeline.rs tool_search.rs
其他       config.rs(106KB KanzeiConfig) areas.rs conventions.rs refs.rs
           home.rs project_root.rs read_ledger.rs progress.rs async_mailbox.rs
└─ config/ (7)  cadence.rs embeddings.rs limits.rs models.rs
                permissions.rs web.rs
```

### 2.4 `kanzei-llm/src`（16 rs）

```
lib.rs  client.rs(LlmClient/Endpoint/Route)  request.rs  event.rs(LlmEvent)
error.rs  sse.rs(SSE 解帧)  proxy.rs
├─ auth/     (3)  mod.rs store.rs codex.rs
└─ protocol/ (5)  mod.rs anthropic.rs openai.rs
                  openai_responses.rs deepseek_responses.rs
```

### 2.5 `kanzei-memory/src`（28 rs）

```
lib.rs  docstore.rs  embed.rs(向量通道)  replay_eval.rs(六臂回放评估)
        scheduling.rs(取活调度链副本)
├─ memory/ (15 + retrieval/ 2 + store/archive.rs)
│    markdown 是内容真源；index.db 是可重建检索索引
│    mod.rs(128KB) store.rs(150KB) index.rs manager.rs tools.rs
│    admission.rs inbox.rs ledger.rs lifecycle.rs migration.rs
│    telemetry.rs preference.rs relevance.rs
│    └─ retrieval/: mod.rs recall.rs search.rs
└─ docstore/ (6)  archive.rs model.rs parse.rs render.rs
                  repository.rs validation.rs
```

分级 = scope(Global/Project) × category(preference/habit/fact/sop)；episode 走 `state.db`。检索为 fingerprint/lexical + 可选 dense/hybrid，向量存 SQLite 表由 Rust 余弦扫描 + RRF 融合（非 sqlite-vec）。

### 2.6 `kanzei-tools/src`（126 rs · 最大能力聚合体）

**一级文件（60）**

| 族 | 文件 |
|---|---|
| 文件读写 | `read.rs` `write.rs` `edit.rs` `files.rs` `glob.rs` `grep.rs` `symbols.rs` |
| 执行 | `bash.rs` `shell.rs` `process.rs` `run.rs` `background.rs` `managed.rs` |
| 研究（12） | `research_loop` `research_plan` `research_runner` `research_write` `research_verify` `research_workflow` `research_index` `research_control` `research_environment` `prior_art` `latex_tool` `plot_tool` |
| 记录追踪 | `tracker.rs`(210KB, 最大) `test_record.rs` `incident.rs` `close_telemetry.rs` `quarantine.rs` `work.rs` `git_batches.rs` |
| 架构约定 | `architecture.rs` `arch_diagram.rs` `arch_diagram_lint.rs` `conventions.rs` `memory_consolidation.rs` `project_state.rs` `cross_tree.rs` |
| 联网 | `webfetch.rs` `websearch.rs` `web_refs.rs` |
| 其他 | `base.rs` `browser_tool.rs` `preview_server.rs` `dev_urls.rs` `palette.rs` `question.rs` `subagent.rs` `worktree.rs` `git.rs` `frontend.rs` `local_validation.rs` |

**子目录（15 个模块）**

| 目录 | 职责 |
|---|---|
| `background/` | 后台进程注册/生命周期/监控/持久化 |
| `git/` | git 命令/索引/计划/收尾/worktree |
| `profiles/` | 档位组件 dev / general / readonly / research + policy |
| `refgraph/` | 记忆知识图谱与引用抽取纯函数（R-368） |
| `research_workflow/` | compute / lifecycle / paper / rounds / tool |
| `schedules/` | 定时任务 hosts / when / executor |
| `team/` | 团队代理 mod / store / tools / workspace |
| `tracker/` + `tracker/actions/` | 条目字段/校验/调度 + action_helpers/maintenance/normalize |
| `verification/` | 冻结源码快照与独立 worker |
| `work/` | Work Unit：context/log/output/reconcile/resume/tool |
| `browser_tool/` `latex_tool/` `plot_tool/` `prior_art/` `conventions/` `project_state/` `test_record/` | 各自后端适配与实现分片 |

### 2.7 `kanzei/src`（38 rs · `kz` CLI）

```
main.rs
└─ cli/ (18)
     mod.rs(46KB 命令分发)  run.rs(+run/events.rs finalize.rs permissions.rs)
     metrics.rs config.rs lock.rs work.rs worktree.rs
     tracker.rs memory.rs eval.rs artifacts.rs compact.rs
     quarantine.rs schedule.rs shadow.rs
```

### 2.8 `kanzei-app/src`（104 rs · Tauri）

**一级模块（约 62）**

| 域 | 文件 |
|---|---|
| 入口契约 | `main.rs`(163 command 注册) `state.rs`(44KB) `ipc_contract.rs` `typed_events.rs` `runtime_events.rs` `runtime_service.rs` `runtime_continuation.rs` |
| 会话运行 | `conversation.rs` `conversation_actions.rs` `auto_run.rs` `subagents.rs` `collaboration.rs` `general_chat.rs` `side_question.rs` `durable_questions.rs` `manual_compact.rs` |
| 文件 | `files_view.rs` `files_edit.rs`(50KB) `files_draft.rs` `attachments.rs` |
| 记忆文档 | `memory.rs` `memory_chat.rs` `docs.rs`(44KB) `research_library.rs` `research_topics.rs` `research_latex.rs` `research_auto.rs` |
| 配置 | `settings.rs`(68KB) `model_config.rs`(61KB) `prefs.rs` `fast_model.rs` |
| 项目工作区 | `projects.rs` `workspace.rs` `schedules.rs` |
| 移动端外部 | `mobile.rs`(69KB) `mobile_notify.rs` `desktop_bridge.rs` `update.rs` |
| 其他 | `voice.rs` `voice_service.rs` `screenshot.rs` `softwire.rs` `decisions.rs` `deliveries.rs` `agent_directory.rs` `agent_team.rs` `async_mailbox.rs` `experience_events.rs` `harness_ext.rs` `orchestration_trace.rs` `projection_gate.rs` `terminal_monitor.rs` `verification_monitor.rs` |

**子目录**：`commands/`(+`run/`) · `run/` · `preview/`(子 webview + WebView2 CDP) · `processes/`(gate/lifecycle/naming/registry/workspace) · `mobile/`(approvals/pwa/sse) · `docs/` · 各一级模块的实现分片

---

## 3. 前端 `crates/kanzei-app/ui/`

**形态**：无打包器、无框架、无 node_modules。`index.html` 直接引原生 ESM，第三方库以 `.mjs` 放 `vendor/`。约 110 个业务 `.js` 平铺在根，靠 **`NN-` 数字前缀**表达加载顺序 + 功能域。

```
index.html(168KB 主页面, 内嵌模板)  runtime.html  gallery.html  oc-studio.html
style.css(333KB) + app/surface/workbench/decision-console/softwire/
                   project-conversations/async-workspace/resource-types/oc-studio .css
assets/    kanzei.svg · voice-capture-worklet.js · oc/
vendor/    pixi 7.4.3 · mermaid(+约 100 个按需分片: katex/elk/dagre/cose-bilkent…)
```

| 前缀 | 域 | 代表文件 |
|---|---|---|
| `00` | 品牌 / 窗口框架 / 表面基线 | `00-brand.js` `00-frame.js` `00-surface.js` |
| `01` | 核心工具 + 全局状态总线 | `01-core.js` |
| `02` | 国际化（196KB，最大） | `02-i18n.js` |
| `03` | 外壳布局：shell/工作区/会话舞台 | `03-shell.js` `03-layout.js` `03-workspaces.js` `03-session-stage.js` `03-general-scope.js` `03-research-library.js` |
| `04` | 渲染原语：markdown/mermaid/结构化 | `04-markdown.js` `04-diagram.js` `04-structured.js` `04-resource-types.js` |
| `05` | 会话渲染：聊天/工具摘要/子代理 | `05-chat-render.js` `05-tool-summary.js` `05-subagents.js` |
| `06` | 侧栏与活动面板 | `06-activity.js` `06-agent-panel.js` `06-deliveries.js` |
| `07` | 事件流渲染 | `07-events.js` |
| `08` | 输入区与模型选择 | `08-compose.js` `08-model-picker.js` `08-auto.js` |
| `09` | 会话管理 | `09-sessions.js` |
| `10–15` | 文档中心 + 决策台 + 记忆页 + 约定 | `10-docs-core.js` `12-decision-console.js` `12-workbench.js` `13-memory.js` `15-conventions.js` |
| `16` | 设置页、移动端入口 | `16-settings.js` `16-mobile.js` |
| `17` | 文件浏览与编辑器 | `17-files.js` `17-files-editor.js` |
| `18` | 启动引导 / 可选 UI 开关 | `18-startup.js` `18-optional-ui.js` |
| `19` | 研究工作台 | `19-research.js` `19-arch.js` `19-research-auto.js` |
| `20–21` | 开发线视图、命令面板 | `20-lines.js` `21-palette.js` |
| `22` | 视觉层：星图/星座 + OC 伴侣角色 | `22-constellation.js` `22-oc-renderer.js` `22-visual-runtime.js` |
| `23` | 语音：采集/控制器/文案 | `23-voice.js` `23-voice-controller.js` |
| `24` | 网页预览、记忆图谱、日程 | `24-preview.js` `24-memory-graph.js` `24-schedules.js` |
| `25` | Softwire 操作台 | `25-softwire.js` `25-softwire-model.js` `25-softwire-run.js` |
| `26–29` | 项目会话/需求研究/代理团队/异步工作区/通用聊天 | `26-project-conversations.js` `27-agent-team.js` `28-async-workspace.js` `29-general-chat.js` |
| 无前缀 | 独立调试展示页 | `gallery.js` `oc-studio.js` |

---

## 4. `docs/` 与 `scripts/`

```
docs/
├─ 目录.md              文档总入口
├─ 使用手册.md
├─ current-state.md     ★ 当前实现口径（先读这个）
├─ cleanup-2026-10-02.md
├─ architecture/        01_runtime_loop.md · 02_harness_registries.md
├─ design/              92 文件 · 81 md（direction_taste / research_mode /
│                       research_library / general_chat / softwire_* …）
├─ prototypes/          3 组示例交互（css/js/html）
├─ reference/           deepseek_harness_reference_20260814.md
├─ reports/             41 文件 · 注明日期的审计/验收
│    └─ 2026-10-02-code-map/   完整代码地图（01–11 + inventory/validation.json
│                               + web/index.html 审查入口 + scan.py/validate.py）
└─ assets/
```

```
scripts/
├─ 78 × ui-*-smoke.mjs      前端功能回归冒烟（主要回归手段）
├─ oc-*.cjs / oc-*.mjs      OC 视觉与布局质检
├─ verify.ps1 / verify-policy.mjs   验证门禁
├─ package.ps1 / release.ps1 / install-setup.ps1   发布链
├─ gen-esm-*.mjs            ESM 图生成与迁移
├─ ui-esm-graph.json · ipc-contract.json · key-paths.json
├─ fixtures/ · ui-preview/ · voice/ · oc-h3/
└─ reader-*.py / pixelize.py
```

---

## 5. 结构信号（值得注意）

1. **运行时能力按层切分而非单一归属**：`async_mailbox.rs`、`auto_run.rs`、`experience_events.rs`、`orchestration.rs`、`schedules/`、`research_runner.rs` 在 core / harness / tools / app 中重复出现同名文件。
2. **`kanzei-tools` 是巨型聚合体**：126 rs，`tracker.rs` 单文件 210KB；`kanzei-memory/src/memory/store.rs` 150KB、`memory/mod.rs` 128KB，`kanzei-core/src/runner/drive.rs` 71KB —— 大文件集中在工具与记忆层。
3. **前端靠命名约定分层**：没有构建工具，加载顺序与职责完全由 `NN-` 前缀承载，改文件名即改加载顺序。
4. **真源分裂点**：Markdown 是记忆/需求/决策真源，`state.db` 是会话事件与运行状态真源（不可从 Markdown 重建），`index.db` 是可重建检索索引。
5. **`.kanzei/project/`** 存需求/缺陷/想法/决策；**`.kanzei/research/<topic>/`** 存来源/发现/报告 —— 均以主根为唯一事实源，worktree 线内不复制。

---

## 6. 延伸阅读

- `docs/current-state.md` —— 当前实现口径表（各主题 ↔ 源码入口）
- `docs/reports/2026-10-02-code-map/` —— 全量代码地图（功能/运行/存储/工具/接口/符号索引 + 网页审查入口）
- `docs/architecture/01_runtime_loop.md`、`02_harness_registries.md`
- `.kanzei/project/architecture/README.md` —— 架构索引与历史身份
