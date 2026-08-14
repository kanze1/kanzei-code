# 架构与技术档案

本目录与 `requirements.md`、`defects.md` 同属项目管理资产，记录已经验证的架构边界、数据契约、运行流程和发布约束。

## 文档约定

- 一个主题一个 Markdown 文件，文件名使用 `snake_case`。
- 文档只记录当前已实现或已验证的事实；未完成内容标记为 `TODO`。
- 需求、缺陷和目标仍以同级追踪文档为真源，架构文档通过 ID 引用它们。

## 当前索引

### 现行基线（设计真源）

- [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线——可替代区复刻优先、创新只投护城河;取活与验收判据。
- [`memory_system.md`](../../../docs/design/memory_system.md)：Memory 系统设计基线(R-103~R-107,现行实施依据)。
- [`memory_decision_sufficiency.md`](../../../docs/design/memory_decision_sufficiency.md)：Memory 判据层升级——决策充分性(R-149 done/R-150,含边界拍板与实证修正记录)。
- [`reliability_usability_self_hosting_quality.md`](../../../docs/design/reliability_usability_self_hosting_quality.md)：可靠性、可用性与自举质量不变量、验证证据和阶段门禁。
- [`harness_m1.md`](../../../docs/design/harness_m1.md)：Harness 六注册表 + 拦截器链 + dev/research 双 profile 架构基线。
- [`deepseek_harness_upgrade.md`](../../../docs/design/deepseek_harness_upgrade.md)：Typed Session Events、Surface Projection、Tool Pipeline/Spill 与 LineRuntime 的升级草案（R-241～R-246，A-012 待转 accepted）。
- [`r059_mobile_agent_communication.md`](../../../docs/design/r059_mobile_agent_communication.md)：主代理/子代理消息与通知演进设计(进行中)。
- [`monolith_decomposition.md`](../../../docs/design/monolith_decomposition.md)：巨石拆解方案——app/main.rs、ui/main.js、core/runner.rs、core/store.rs 分文件拆解(R-153~R-156,A-008)。
- [`architecture_browser.md`](../../../docs/design/architecture_browser.md)：可视化架构浏览与记忆设置——技术栈选型评估(R-122,方案 A:既有 classic script + 目录树复用)。
- [`deep_parallel_dev.md`](../../../docs/design/deep_parallel_dev.md)：任务级并行基线——一线一 worktree、显式主根、diff/合并/恢复与模型隔离(R-177/R-182 已交付基础闭环)。
- [`parallel_lines_ui.md`](../../../docs/design/parallel_lines_ui.md)：多线协作可见性——协作上下文、并列线路状态、文件冲突预警及后续收活流程(R-184 进行中)。

### 评审中（决策门禁未过）

- [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md)：Memory 控制平面——证据账本/编译器/召回控制器/反事实评估四模块(R-161~R-167,D-229~D-231)。
- [`ci_release_evidence_chain.md`](../../../docs/design/ci_release_evidence_chain.md)：CI 与发布证据链——本地门禁 + commit 锚定(R-152/R-146/R-156)。
- [`continue_prompt_dissection.md`](../../../docs/design/continue_prompt_dissection.md)：继续文案拆解与鞭挞引擎化——保留必要性评估(草案,待用户拍板方案 A/B/C;鞭挞核心部件下沉 harness,R-128 承接阻塞停止分支)。

### 历史记录（对应条目已 done / 已被取代）

- [`r030_process_decoupling.md`](../../../docs/design/r030_process_decoupling.md)：多进程解耦设计(R-030 done,残余 P3 并入 deep_parallel_dev P2)。
- [`interaction_modes.md`](../../../docs/design/interaction_modes.md)：双人格与对话为主布局设计(R-036 done)。
- [`m2_sqlite_store.md`](../../../docs/design/m2_sqlite_store.md)：SQLite 会话存储 Schema v1(R-003 done)。
- [`subagent_management.md`](../../../docs/design/subagent_management.md)：子代理管理四层方案(R-058 done,策略层未实施)。
- [`parallel_read_serial_write_orchestration.md`](../../../docs/design/parallel_read_serial_write_orchestration.md)：R-171 阶段编排历史基线；项目级单 writer 与实现阶段全串行已于 2026-08-11 被 R-182 取代，仍有效部分见文首修订说明。
- [`app_icon.md`](../../../docs/design/app_icon.md)：图标设计规范与资产清单(R-061 done,规范仍有效)。
- [`frontend_phase3.md`](../../../docs/design/frontend_phase3.md)：前端能力差距与需求整理记录(R-031~R-051 系列)。

### 规范与示例

- [`readme.md`](../../../docs/design/readme.md)：docs/design 的记录规范与文档模板。
- [`r108_ai_design_decision_records.md`](../../../docs/design/r108_ai_design_decision_records.md)：设计记录规范的真实示例(R-108 done)。
