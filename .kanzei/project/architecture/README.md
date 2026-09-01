# 设计文档身份索引

本索引只记录入口和结构化时效元数据；设计正文仍是各主题的内容真源。身份由治理工具消费，不由模型凭自然语言推断。

字段约定：

- `live_design`：仍约束当前实现；必须有 `last_verified_commit`。
- `validated_design`：主体已交付，剩余边界必须映射到 tracker；必须有 `last_verified_commit`。
- `historical_snapshot`：保留某个时点的事实或审计结论；使用 `as_of_commit`，不因当前 tracker 变化失败。
- `superseded`：正文不再是当前方案；必须给出有效 `superseded_by`，默认上下文不得注入其正文。

## live_design

- [identity: live_design; last_verified_commit: 6e816b98] [`agent_visualization_tools.md`](../../../docs/design/agent_visualization_tools.md)：Agent 绘图工具统一设计草案(R-335；架构图与 research 科学图表 API、验证、产物和迁移边界，最终引擎组合待用户评审)。
- [identity: live_design; last_verified_commit: d374cb9f] [`app_icon.md`](../../../docs/design/app_icon.md)：图标设计规范与资产清单(R-061 done,规范仍有效)。
- [identity: live_design; last_verified_commit: d374cb9f] [`bootstrap_quality_audit.md`](../../../docs/design/bootstrap_quality_audit.md)：自举质量波次审计 SOP，规定只读审计、证据替身、最后一公里接线与注释承诺检查。
- [identity: live_design; last_verified_commit: d374cb9f] [`deepseek_harness_upgrade.md`](../../../docs/design/deepseek_harness_upgrade.md)：Typed Session Events、Surface Projection、Tool Pipeline/Spill 与 LineRuntime 的升级草案(R-241～R-246,A-012 待转 accepted)。
- [identity: live_design; last_verified_commit: d374cb9f] [`design_freshness_audit_20260820.md`](../../../docs/design/design_freshness_audit_20260820.md)：设计文档时效审计与 R-318 治理基线；审计结论和四类身份契约仍约束本轮治理。
- [identity: live_design; last_verified_commit: d374cb9f] [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线——可替代区复刻优先、创新只投护城河；取活与验收判据。
- [identity: live_design; last_verified_commit: d374cb9f] [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md)：Memory 控制平面——证据账本/编译器/召回控制器/反事实评估四模块(R-161~R-167,D-229~D-231)。
- [identity: live_design; last_verified_commit: d374cb9f] [`memory_system.md`](../../../docs/design/memory_system.md)：Memory 系统设计基线(R-103~R-107,现行实施依据)。
- [identity: live_design; last_verified_commit: d374cb9f] [`phase2_system_upgrade.md`](../../../docs/design/phase2_system_upgrade.md)：自举二期 research/memory/运行体验/动画/voice 的依赖、波次、Go/No-Go 与联合验收总纲(R-283)。
- [identity: live_design; last_verified_commit: d374cb9f] [`research_mode.md`](../../../docs/design/research_mode.md)：研究模式设计基线草案(2026-08-12 八维度审计维度 8 产出；八个定调点待用户确认,R-221 承接)。
- [identity: live_design; last_verified_commit: 1ebbb218] [`research_experiment_runner.md`](../../../docs/design/research_experiment_runner.md)：Research 实验运行与路线图的字段与 Markdown 格式冻结(Experiment/Run/Result/Environment、@@kanzei 回调协议、本机+SSH 执行、路线图投影;A-014~A-018,R-343~R-348 承接)。
- [identity: live_design; last_verified_commit: d374cb9f] [`session_state_and_line_runtime.md`](../../../docs/design/session_state_and_line_runtime.md)：会话状态与线路运行态设计(状态持久化、恢复与并发线路隔离)。
- [identity: live_design; last_verified_commit: d374cb9f] [`subagent_management.md`](../../../docs/design/subagent_management.md)：子代理管理四层方案(R-058 done,策略层未实施)。
- [identity: live_design; last_verified_commit: d374cb9f] [`weakness_register_20260820.md`](../../../docs/design/weakness_register_20260820.md)：弱点登记与 Agent 减负方向(2026-08-20 两轮外部评估对照；R-310~R-313、D-575/D-577/D-578,§六 为需求发现实测复核,减负方案待 R-312 勘察后评审)。
- [identity: live_design; last_verified_commit: d374cb9f] [`readme.md`](../../../docs/design/readme.md)：docs/design 的记录规范与文档模板；本文件定义设计正文最小结构和方案变更规则。
- [identity: live_design; last_verified_commit: d374cb9f] [`harness_m1.md`](../../../docs/design/harness_m1.md)：Harness 六注册表 + 拦截器链 + dev/research 双 profile 架构基线，并接入 R-317 执行层权威。
- [identity: live_design; last_verified_commit: 6505dfb8] [`model_autonomy_and_harness_intensity.md`](../../../docs/design/model_autonomy_and_harness_intensity.md)：模型自治与门禁强度——结伴/自主两档门禁、模型停机权与编排抽象层(R-322/R-323,D-661/D-662;2026-08-21 外部七点评估的逐点定调)。
- [identity: live_design; last_verified_commit: c3222943] [`work_unit_foundation.md`](../../../docs/design/work_unit_foundation.md)：Work Unit 底座——Outcome/执行状态/历史三分离的事件存储、投影与迁移契约(R-317)。
- [identity: live_design; last_verified_commit: c9f42992] [`context_supply_bill_20260821.md`](../../../docs/design/context_supply_bill_20260821.md)：R-312 B1 真实 session 上下文注入账单；记录 7 个有非空账单的 session、块级字符占比、粗 token 估算及进展/对账/停车字段的当前测量缺口。
- [identity: live_design; last_verified_commit: dbafb50f] [`run_metrics_task_granularity.md`](../../../docs/design/run_metrics_task_granularity.md)：R-337 运行画像按执行任务关闭粒度的审计与设计草案；B1 已完成现状证据，B2 任务级方案待评审。

## validated_design

- [identity: validated_design; last_verified_commit: 5c9e1df] [`architecture_browser.md`](../../../docs/design/architecture_browser.md)：可视化架构浏览与记忆设置——技术栈选型评估(R-122 done,方案 A:既有 classic script + 目录树复用)。
- [identity: validated_design; last_verified_commit: c0ea88d] [`ci_release_evidence_chain.md`](../../../docs/design/ci_release_evidence_chain.md)：CI 与发布证据链——本地门禁 + commit 锚定(R-152/R-146/R-156/R-298 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`continue_prompt_dissection.md`](../../../docs/design/continue_prompt_dissection.md)：继续文案拆解与鞭挞引擎化——实施前拆解与 R-128/R-157/R-169/R-170 交付映射。
- [identity: validated_design; last_verified_commit: e791536] [`deep_parallel_dev.md`](../../../docs/design/deep_parallel_dev.md)：任务级并行基线——一线一 worktree、显式主根、diff/合并/恢复与模型隔离(R-177/R-178/R-179/R-182 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`interaction_modes.md`](../../../docs/design/interaction_modes.md)：双人格与对话为主布局设计(R-036 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`m2_sqlite_store.md`](../../../docs/design/m2_sqlite_store.md)：SQLite 会话存储 Schema v1(R-003 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`memory_decision_sufficiency.md`](../../../docs/design/memory_decision_sufficiency.md)：Memory 判据层升级——决策充分性(R-145/R-150 done,含边界拍板与实证修正记录)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`monolith_decomposition.md`](../../../docs/design/monolith_decomposition.md)：巨石拆解方案——app/main.rs、ui/main.js、core/runner.rs、core/store.rs 分文件拆解(R-153~R-156,A-008;后续 R-253~R-258 批次落地与装配对照表)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`monolith_decomposition_round2.md`](../../../docs/design/monolith_decomposition_round2.md)：第二轮巨石拆解计划(R-253~R-258 的符号级批次地基,与 monolith_decomposition.md 配合)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`parallel_lines_ui.md`](../../../docs/design/parallel_lines_ui.md)：多线协作可见性——协作上下文、并列线路状态、文件冲突预警及收活流程(R-184/R-185/R-222 done，P3 三级卡住判据仍是边界)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`r059_mobile_agent_communication.md`](../../../docs/design/r059_mobile_agent_communication.md)：主代理/子代理消息与通知演进设计(R-059 dropped、R-270/R-271 done，R-288 真机 E3 仍在范围)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`r108_ai_design_decision_records.md`](../../../docs/design/r108_ai_design_decision_records.md)：设计记录规范的真实示例(R-108 done)。
- [identity: validated_design; last_verified_commit: 8ed3256f] [`r310_repo_map_design.md`](../../../docs/design/r310_repo_map_design.md)：代码地图形态与 token 成本对比——symbols 实时按需查询胜出，拒绝全量注入与持久索引(R-310 B3 done,实现在 kanzei-tools/src/symbols.rs)。
- [identity: validated_design; last_verified_commit: e08eb0a] [`research_workspace.md`](../../../docs/design/research_workspace.md)：研究工作台与来源/发现工件交互设计(R-276/R-277 done,D-413 fixed)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`reliability_usability_self_hosting_quality.md`](../../../docs/design/reliability_usability_self_hosting_quality.md)：可靠性、可用性与自举质量不变量、验证证据和 R-317 执行模型权威。
- [identity: validated_design; last_verified_commit: d374cb9f] [`ui_esm_migration.md`](../../../docs/design/ui_esm_migration.md)：前端 ESM 迁移评估(结论:保持有序 classic script,A-008)。

## historical_snapshot

- [identity: historical_snapshot; as_of_commit: 3e510b1] [`audit_20260812_eight_dimensions.md`](../../../docs/design/audit_20260812_eight_dimensions.md)：2026-08-12 八维度审计记录(巨石/记忆/验证/协作等维度的方法论与发现源)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`context_compaction.md`](../../../docs/design/context_compaction.md)：上下文压缩设计(历史基线；当前压缩实现见 runner compaction)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`frontend_phase3.md`](../../../docs/design/frontend_phase3.md)：前端能力差距与需求整理记录(R-031~R-051 系列)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`metrics_baseline.md`](../../../docs/design/metrics_baseline.md)：巨石度量基线快照(R-258 批2,`kz metrics` Top-30 榜单与阈值读数,供拆解条目前后对照)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`parallel_read_serial_write_orchestration.md`](../../../docs/design/parallel_read_serial_write_orchestration.md)：R-171 阶段编排历史基线；项目级单 writer 与实现阶段全串行已于 2026-08-11 被 R-182 取代，仍有效部分见文首修订说明。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`research_mode_prior_art.md`](../../../docs/design/research_mode_prior_art.md)：research 模式先行调研与同类系统对照资料，作为 R-221/R-277 的设计输入。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tier1_handoff_20260811.md`](../../../docs/design/tier1_handoff_20260811.md)：第一梯队交付移交记录(2026-08-11)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tier1_implementation_plan.md`](../../../docs/design/tier1_implementation_plan.md)：第一梯队实施计划(R-001~R-020 等首批条目)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tool_edit_recovery.md`](../../../docs/design/tool_edit_recovery.md)：edit 工具恢复机制设计(匹配失败诊断与恢复策略,M-021/M-022 的记忆来源)。

## superseded

- [identity: superseded; as_of_commit: 3e510b1; superseded_by: deep_parallel_dev.md] [`r030_process_decoupling.md`](../../../docs/design/r030_process_decoupling.md)：多进程解耦设计(R-030 done)；其 worktree/深并行残余已由 `deep_parallel_dev.md` 接替，正文保留历史决策和兼容边界。