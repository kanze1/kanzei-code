## 当前索引(按现行基线/评审中/历史/规范分节)

### 现行基线

- [`app_icon.md`](../../../docs/design/app_icon.md)：图标设计规范与资产清单(R-061 done,规范仍有效)。
- [`architecture_browser.md`](../../../docs/design/architecture_browser.md)：可视化架构浏览与记忆设置——技术栈选型评估(R-122,方案 A:既有 classic script + 目录树复用)。
- [`ci_release_evidence_chain.md`](../../../docs/design/ci_release_evidence_chain.md)：CI 与发布证据链——本地门禁 + commit 锚定(R-152/R-146/R-156)。
- [`deep_parallel_dev.md`](../../../docs/design/deep_parallel_dev.md)：任务级并行基线——一线一 worktree、显式主根、diff/合并/恢复与模型隔离(R-177/R-182 已交付基础闭环)。
- [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线——可替代区复刻优先、创新只投护城河;取活与验收判据。
- [`frontend_phase3.md`](../../../docs/design/frontend_phase3.md)：前端能力差距与需求整理记录(R-031~R-051 系列)。
- [`harness_m1.md`](../../../docs/design/harness_m1.md)：Harness 六注册表 + 拦截器链 + dev/research 双 profile 架构基线。
- [`interaction_modes.md`](../../../docs/design/interaction_modes.md)：双人格与对话为主布局设计(R-036 done)。
- [`m2_sqlite_store.md`](../../../docs/design/m2_sqlite_store.md)：SQLite 会话存储 Schema v1(R-003 done)。
- [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md)：Memory 控制平面——证据账本/编译器/召回控制器/反事实评估四模块(R-161~R-167,D-229~D-231)。
- [`memory_decision_sufficiency.md`](../../../docs/design/memory_decision_sufficiency.md)：Memory 判据层升级——决策充分性(R-149 done/R-150,含边界拍板与实证修正记录)。
- [`memory_system.md`](../../../docs/design/memory_system.md)：Memory 系统设计基线(R-103~R-107,现行实施依据)。
- [`metrics_baseline.md`](../../../docs/design/metrics_baseline.md)：巨石度量基线快照(R-258 批2,`kz metrics` Top-30 榜单与阈值读数,供拆解条目前后对照)。
- [`monolith_decomposition.md`](../../../docs/design/monolith_decomposition.md)：巨石拆解方案——app/main.rs、ui/main.js、core/runner.rs、core/store.rs 分文件拆解(R-153~R-156,A-008;后续 R-253~R-258 批次落地与装配对照表)。
- [`parallel_lines_ui.md`](../../../docs/design/parallel_lines_ui.md)：多线协作可见性——协作上下文、并列线路状态、文件冲突预警及后续收活流程(R-184 进行中)。
- [`r030_process_decoupling.md`](../../../docs/design/r030_process_decoupling.md)：多进程解耦设计(R-030 done,残余 P3 并入 deep_parallel_dev P2)。
- [`r059_mobile_agent_communication.md`](../../../docs/design/r059_mobile_agent_communication.md)：主代理/子代理消息与通知演进设计(进行中)。
- [`r108_ai_design_decision_records.md`](../../../docs/design/r108_ai_design_decision_records.md)：设计记录规范的真实示例(R-108 done)。
- [`reliability_usability_self_hosting_quality.md`](../../../docs/design/reliability_usability_self_hosting_quality.md)：可靠性、可用性与自举质量不变量、验证证据和阶段门禁。
- [`subagent_management.md`](../../../docs/design/subagent_management.md)：子代理管理四层方案(R-058 done,策略层未实施)。

### 评审中 / 草案

- [`continue_prompt_dissection.md`](../../../docs/design/continue_prompt_dissection.md)：继续文案拆解与鞭挞引擎化——保留必要性评估(草案,待用户拍板方案 A/B/C;鞭挞核心部件下沉 harness,R-128 承接阻塞停止分支)。
- [`deepseek_harness_upgrade.md`](../../../docs/design/deepseek_harness_upgrade.md)：Typed Session Events、Surface Projection、Tool Pipeline/Spill 与 LineRuntime 的升级草案(R-241～R-246,A-012 待转 accepted)。
- [`phase2_system_upgrade.md`](../../../docs/design/phase2_system_upgrade.md)：自举二期 research/memory/运行体验/动画/voice 的依赖、波次、Go/No-Go 与联合验收总纲(R-283)。
- [`research_mode.md`](../../../docs/design/research_mode.md)：研究模式设计基线草案(2026-08-12 八维度审计维度 8 产出;八个定调点待用户确认,R-221 承接)。
- [`research_mode_prior_art.md`](../../../docs/design/research_mode_prior_art.md)：research 模式先行调研与同类系统对照资料，作为 R-221/R-277 的设计输入。
- [`research_workspace.md`](../../../docs/design/research_workspace.md)：research 工作台与来源/发现工件交互设计，作为 R-276 的设计输入。
- [`session_state_and_line_runtime.md`](../../../docs/design/session_state_and_line_runtime.md)：会话状态与线路运行态设计(状态持久化、恢复与并发线路隔离)。
- [`ui_esm_migration.md`](../../../docs/design/ui_esm_migration.md)：前端 ESM 迁移评估(结论:保持有序 classic script,A-008)。
- [`weakness_register_20260820.md`](../../../docs/design/weakness_register_20260820.md)：弱点登记与 Agent 减负方向(2026-08-20 两轮外部评估对照;R-310~R-313、D-575/D-577/D-578,§六 为需求发现实测复核,减负方案待 R-312 勘察后评审)。

### 历史基线 / 记录

- [`audit_20260812_eight_dimensions.md`](../../../docs/design/audit_20260812_eight_dimensions.md)：2026-08-12 八维度审计记录(巨石/记忆/验证/协作等维度的方法论与发现源)。
- [`context_compaction.md`](../../../docs/design/context_compaction.md)：上下文压缩设计(历史基线;当前压缩实现见 runner compaction)。
- [`monolith_decomposition_round2.md`](../../../docs/design/monolith_decomposition_round2.md)：第二轮巨石拆解计划(R-253~R-258 的符号级批次地基,与 monolith_decomposition.md 配合)。
- [`parallel_read_serial_write_orchestration.md`](../../../docs/design/parallel_read_serial_write_orchestration.md)：R-171 阶段编排历史基线；项目级单 writer 与实现阶段全串行已于 2026-08-11 被 R-182 取代，仍有效部分见文首修订说明。
- [`tier1_handoff_20260811.md`](../../../docs/design/tier1_handoff_20260811.md)：第一梯队交付移交记录(2026-08-11)。
- [`tier1_implementation_plan.md`](../../../docs/design/tier1_implementation_plan.md)：第一梯队实施计划(R-001~R-020 等首批条目)。
- [`tool_edit_recovery.md`](../../../docs/design/tool_edit_recovery.md)：edit 工具恢复机制设计(匹配失败诊断与恢复策略,M-021/M-022 的记忆来源)。

### 规范

- [`bootstrap_quality_audit.md`](../../../docs/design/bootstrap_quality_audit.md)：自举质量波次审计 SOP，规定只读审计、证据替身、最后一公里接线与注释承诺检查。
- [`readme.md`](../../../docs/design/readme.md)：docs/design 的记录规范与文档模板。
