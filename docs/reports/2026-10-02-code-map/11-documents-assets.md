# 文档与资源索引

文档状态字段按原文列出，未以文字状态证明实现或验收。资源只列路径和大小，不读取本地 profile 或二进制内容。第三方 vendor 和缓存不在此表。

## 现有文档

| 文档 | 原标题 | 文中状态 |
|---|---|---|
| [docs/architecture/01_runtime_loop.md](<C:/Users/kanzei/Documents/kanzei code/docs/architecture/01_runtime_loop.md:1>) | 运行时主循环 |  |
| [docs/architecture/02_harness_registries.md](<C:/Users/kanzei/Documents/kanzei code/docs/architecture/02_harness_registries.md:1>) | Harness 注册表 |  |
| [docs/assets/icon-concepts/kanzei-folded-k-v2.prompt.md](<C:/Users/kanzei/Documents/kanzei code/docs/assets/icon-concepts/kanzei-folded-k-v2.prompt.md:1>) | kanzei icon — Folded K v2 | Status: design concept |
| [docs/assets/icon-concepts/kanzei-parallel-agents-v1.prompt.md](<C:/Users/kanzei/Documents/kanzei code/docs/assets/icon-concepts/kanzei-parallel-agents-v1.prompt.md:1>) | Kanzei 并行 Agent 图标概念 v1 |  |
| [docs/cleanup-2026-10-02.md](<C:/Users/kanzei/Documents/kanzei code/docs/cleanup-2026-10-02.md:1>) | 2026-10-02 本地整理记录 |  |
| [docs/current-state.md](<C:/Users/kanzei/Documents/kanzei code/docs/current-state.md:1>) | Kanzei 当前实现与文档口径 |  |
| [docs/design/agent_visualization_tools.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/agent_visualization_tools.md:1>) | Agent 绘图工具：架构图与研究科学图表统一设计 | - 状态：草案 |
| [docs/design/app_icon.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/app_icon.md:1>) | kanzei APP 图标设计规范（R-061） | 状态：已落地并验收 |
| [docs/design/architecture_browser.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/architecture_browser.md:1>) | 可视化架构浏览与维护记忆设置——技术栈选型评估报告 | - 状态：已实施；R-122 已完成，D-173 已修复 |
| [docs/design/architecture_diagrams.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/architecture_diagrams.md:1>) | 架构图:好看、agent 能改、一个渲染器 | - 状态: 设计基线(2026-09-26 起草并随 ui2/arch 分支实施,本文描述的是已落地的实现) |
| [docs/design/audit_20260812_eight_dimensions.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/audit_20260812_eight_dimensions.md:1>) | 八维度全面审计与改进计划(2026-08-12) | - 状态: 审计报告 + 改造计划总纲(用户指令发起;19 个子代理完成——8 维度并行分析 + 11 条 critical/即时声称逐条独立反证,其中 5 条被驳回,见 §10) |
| [docs/design/auto_research.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/auto_research.md:1>) | AUTO research：从方向到实验和论文 PDF（R-363） | - 状态：2026-09-24 补齐完整实验返工、历史产物和断点恢复，保留首批全流程工作树实现。 |
| [docs/design/bootstrap_quality_audit.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/bootstrap_quality_audit.md:1>) | 波次质量审计 SOP(手动触发) | - 状态: 生效(2026-08-16 用户批准;触发方式经用户定调为**手动**,不定时、不挂门禁) |
| [docs/design/brand_refresh.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/brand_refresh.md:1>) | kanzei 品牌与界面质感 | 日期：2026-09-26。状态：已接入当前源码；安装包验收以对应发布的验证证据为准。 |
| [docs/design/cc_codex_alignment_20260925.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/cc_codex_alignment_20260925.md:1>) | Claude Code / Codex 能力对照与对齐清单(复刻清单 v1) | - 状态:设计基线(三方对照、用户筛选与接口定义已完成;实施条目 R-364~R-367、R-369~R-377 与 D-748、D-751 已登记,实施地图见 cc_codex_alignment_impl_maps.md) |
| [docs/design/cc_codex_alignment_impl_maps.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/cc_codex_alignment_impl_maps.md:1>) | CC/Codex 对齐条目实施地图 | - 状态:设计基线(实施地图,随条目推进更新) |
| [docs/design/chat_presentation_contract.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/chat_presentation_contract.md:1>) | 主对话区呈现契约:什么算正文,什么算轨迹 | - 状态: 设计基线(2026-09-02 用户就三个方向定调后冻结前两节;§5 为已确认未定项;2026-09-26 增 §4.4 单列与工具组,已随 ui2/chat 分支落地) |
| [docs/design/ci_release_evidence_chain.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/ci_release_evidence_chain.md:1>) | CI 与发布证据链 | - 状态：发布证据门禁已实现；CI 当前为手动触发。 |
| [docs/design/context_compaction.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/context_compaction.md:1>) | 上下文压缩重设计——分层压缩、滚动合并与可配置压缩模型 | - 状态:设计定稿(勘察+调研完成,开发交自举循环,载体 R-236) |
| [docs/design/context_supply_bill_20260821.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/context_supply_bill_20260821.md:1>) | 上下文供给账单（R-312 B1） | - 状态：B1 测量完成；B2 设计待做；B3 评审与实施条目待做 |
| [docs/design/continue_prompt_dissection.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/continue_prompt_dissection.md:1>) | 继续文案拆解与鞭挞引擎化——保留必要性评估 | - 状态：已实施；R-128、R-157、R-169、R-170 均已完成 |
| [docs/design/decision_console_integration.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/decision_console_integration.md:1>) | 决策控制台：现有系统接入说明 | - 状态：决策闭环与冻结快照后台验证已接入源码；用手机阅读器完成真实 CLI / Flutter 实验。未替换安装版。 |
| [docs/design/deep_parallel_dev.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/deep_parallel_dev.md:1>) | 深度并行开发模式与模型选择隔离 — 深度分析 | - 状态: 已实施基线；R-177、R-178、R-179、R-182 已完成，旧单 writer 论证仅作历史决策记录 |
| [docs/design/deepseek_harness_upgrade.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/deepseek_harness_upgrade.md:1>) | DeepSeek Harness 约束驱动的运行时升级 | - 状态：R-241 已进入 shadow 观察；R-242/R-243 的事件投影真源切换、segment reset 与 surface compaction 追加事务已实现并通过定向回归 |
| [docs/design/design_freshness_audit_20260820.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/design_freshness_audit_20260820.md:1>) | 设计文档时效审计与治理基线 | - 状态：审计完成，治理实施交由 R-318 |
| [docs/design/direction_taste.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/direction_taste.md:1>) | 方向基线:可替代区复刻优先,创新只投护城河 | - 状态:设计基线 |
| [docs/design/doc_reference_graph.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/doc_reference_graph.md:1>) | 文档引用标记、引用历史与引用图 | - 状态:草案(实施条目已登记;关系类型词表经用户 2026-09-25 暂定,以后可改) |
| [docs/design/files_editor.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/files_editor.md:1>) | 文件页编辑:可编辑、可拖拽伸缩的文件浏览 | - 状态: 设计基线(2026-09-26 起草并随 ui2/files 分支实施,本文描述的是已落地的实现) |
| [docs/design/frontend_phase3.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/frontend_phase3.md:1>) | 前端三期:能力差距全面分析(2026-08-06) |  |
| [docs/design/harness_m1.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/harness_m1.md:1>) | Harness M1 设计稿 | - 状态: 现行总纲；R-317 执行层已接管 Requirement/Work Unit 的工作权威 |
| [docs/design/interaction_modes.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/interaction_modes.md:1>) | 交互模式沉淀:自主推进 vs 结伴开发(R-036) |  |
| [docs/design/m2_sqlite_store.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/m2_sqlite_store.md:1>) | M2 SQLite 会话存储 |  |
| [docs/design/memory_control_plane.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/memory_control_plane.md:1>) | kanzei Memory 控制平面(Decision-Centric Memory Control Plane) | - 状态: 设计基线 + 已交付实现对账(2026-08-18) |
| [docs/design/memory_decision_sufficiency.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/memory_decision_sufficiency.md:1>) | Memory 决策充分性改造(Control-Sufficient Memory) | - 状态: 已验证基线；R-145、R-150 已完成 |
| [docs/design/memory_feedback_reliability.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/memory_feedback_reliability.md:1>) | 记忆收益与任务信息呈现 | - 状态：第一批及 D-745 修复已发布并通过发布验收，整体实施中 |
| [docs/design/memory_knowledge_graph.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/memory_knowledge_graph.md:1>) | 记忆知识图谱:按架构渲染的记忆关系图 | - 状态: 设计基线(2026-09-26 起草,随 ui2/memgraph 分支实施;本文描述的是已落地的实现) |
| [docs/design/memory_system.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/memory_system.md:1>) | kanzei Memory 系统设计 | 状态: 设计基线(存储形态仍有效) + 已交付实现对账(2026-08-18) |
| [docs/design/metrics_baseline.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/metrics_baseline.md:1>) | 巨石度量基线快照(R-258 批2) |  |
| [docs/design/model_autonomy_and_harness_intensity.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/model_autonomy_and_harness_intensity.md:1>) | 模型自治与门禁强度 | - 状态: 设计基线 |
| [docs/design/monolith_decomposition.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/monolith_decomposition.md:1>) | 巨石文件拆解(app/main.rs · ui/main.js · core/runner.rs · core/store.rs) | - 状态:设计基线(2026-08-09 用户定调「架构 entropy 增速开始追上 feature velocity,拆解优先级最高」) |
| [docs/design/monolith_decomposition_round2.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/monolith_decomposition_round2.md:1>) | 巨石文件拆解 第二轮(app/run.rs · memory/store.rs · app/processes.rs) | - 状态:设计基线(2026-08-15 用户提供第二轮巨石扫描 + 本仓机器复核) |
| [docs/design/oc-h3-deployment.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc-h3-deployment.md:1>) | OC 动画：H3 社区版本与双卡方案 | 核对日期：2026-09-24。状态：已完成 v6 九种状态的视频角色包和三种待机变体，并接入源码与预览。原始视频与生成证据均保留；具体运行处理见 \[制作记录\](oc-production.md)。部署目录、主机名和硬件唯一标识留在本地配置中；公开记录不包含这些值。下文 v5 及更早段落是各阶段的历史记录。 |
| [docs/design/oc-idle-direction.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc-idle-direction.md:1>) | OC 待机与呼吸设计 |  |
| [docs/design/oc-playback.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc-playback.md:1>) | 角色动画播放与性能 |  |
| [docs/design/oc-production.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc-production.md:1>) | OC 角色与动画制作记录 |  |
| [docs/design/oc-references/clean-material-prompt.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc-references/clean-material-prompt.md:1>) | 干净材质修正稿 |  |
| [docs/design/oc-voice-direction.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc-voice-direction.md:1>) | OC 音色方向 |  |
| [docs/design/oc.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/oc.md:1>) | OC 当前设计 |  |
| [docs/design/parallel_lines_ui.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/parallel_lines_ui.md:1>) | 任务级并行的前端：agent 身份贯穿全 UI | - 状态: 部分交付（R-247、R-178、R-179、R-184、R-185、R-222 已完成；P3 的三级卡住判据仍未实现） |
| [docs/design/parallel_read_serial_write_orchestration.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/parallel_read_serial_write_orchestration.md:1>) | 多进程代理编排：阶段流历史基线与任务级并行修订 | - 状态：**历史基线，部分现行** —— 阶段流与只读子代理仍有效；不变量 3~5 的项目级单 writer/全工具串行已由 R-182 取代 |
| [docs/design/phase2_system_upgrade.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/phase2_system_upgrade.md:1>) | 自举二期系统升级总纲 | - 状态：设计基线（2026-08-17，用户确认一期可结项并启动全面升级） |
| [docs/design/preview_pane.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/preview_pane.md:1>) | 网页预览面板与 browser 工具双后端(UI2-0926 #8) |  |
| [docs/design/product_operating_model.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/product_operating_model.md:1>) | Kanzei 产品重整：多项目推进、批量验收与决策记忆 | - 状态：用户已确认方向；决策闭环与后台验证已接入源码，并用手机阅读器验证，未发布。边界见 \[decision_console_integration.md\](decision_console_integration.md)。 |
| [docs/design/project_workspace.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/project_workspace.md:1>) | 工作目录管理:新建项目、项目状态事实与「模型在等你」 | - 状态: 设计基线(2026-09-26 起草并随 ui2/workdir 分支实施,本文描述的是已落地的实现) |
| [docs/design/r030_process_decoupling.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/r030_process_decoupling.md:1>) | R-030 进程与项目解耦(多进程并行)设计 | - 每进程独立渲染状态:currentAssistant/currentReasoning/currentTool/toolChips/runTokens 收进 \`procState: Map<pid, {...}>\`;#messages 内每进程一个 pane,切换页签只切可见性——后台进程的输出持续写入自己的 pane,切回即见全量 |
| [docs/design/r059_mobile_agent_communication.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/r059_mobile_agent_communication.md:1>) | R-059 移动端子代理通信与通知设计 | - 状态：部分交付；R-059 已 dropped，R-270/R-271 已完成，R-288 保留为 Android 真机 E3 验收 |
| [docs/design/r108_ai_design_decision_records.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/r108_ai_design_decision_records.md:1>) | R-108 AI 设计讨论与技术决策记录 | - 状态：设计基线 |
| [docs/design/r310_repo_map_design.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/r310_repo_map_design.md:1>) | Repo Map Design for R-310 Batch 3 | **Status:** Accepted for R-310 B3; implementation landed in \`crates/kanzei-tools/src/symbols.rs\`. |
| [docs/design/readme.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/readme.md:1>) | 设计与 AI 讨论记录规范 | - 状态：草案 \| 设计基线 \| 已验证 \| 已废弃 |
| [docs/design/reliability_usability_self_hosting_quality.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/reliability_usability_self_hosting_quality.md:1>) | Kanzei 可靠性、可用性与自举质量打磨设计 | - 状态：原则基线仍有效；执行模型由 R-317 的 Outcome/Work Unit/事件投影取代旧阶段计划 |
| [docs/design/research_experiment_runner.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/research_experiment_runner.md:1>) | Research 实验运行与路线图:字段与 Markdown 格式冻结 | - 状态: 设计基线(2026-09-01 用户逐点定调;数据模型经同日二次收敛后冻结) |
| [docs/design/research_library.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/research_library.md:1>) | 独立研究课题库 |  |
| [docs/design/research_mode.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/research_mode.md:1>) | research 模式设计:独立深度研究模式(文献+仓库,论文级产出) | - 状态: **设计基线**(2026-08-16 §2 定调点全部经用户过审转正;先行对照证据见 \[research_mode_prior_art.md\](research_mode_prior_art.md)) |
| [docs/design/research_mode_prior_art.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/research_mode_prior_art.md:1>) | research mode 先行对照:已有方案调查(prior art) | - 状态: 完成(2026-08-16 三路调查全部落盘);本文档是 research_mode.md 重写与 R-273~R-276 登记的证据基座 |
| [docs/design/research_workspace.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/research_workspace.md:1>) | research 工作台前端设计(R-276 批1 设计稿) | - 状态: 2026-09-06 的空间导航、侧栏、课题会话及分屏方案由 \[workspace_information_architecture.md\](workspace_information_architecture.md) 接替。以下保留 R-276/R-277 的历史设计及工件交互依据。 |
| [docs/design/run_metrics_task_granularity.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/run_metrics_task_granularity.md:1>) | 运行画像按执行任务关闭粒度 | - 状态：草案 |
| [docs/design/run_metrics_task_migration.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/run_metrics_task_migration.md:1>) | 运行画像任务历史兼容、对账与回滚 | - 状态：实施基线 |
| [docs/design/session_state_and_line_runtime.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/session_state_and_line_runtime.md:1>) | 会话运行态、并行线路与任务设置统一设计 | - 状态：已确认，作为 R-197 的实现基线 |
| [docs/design/softwire_context_workbench.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/softwire_context_workbench.md:1>) | Softwire：上下文、记忆、工具与就地操作台 | - 状态：交互与接线设计，附可操作原型；尚未接入正式运行服务。 |
| [docs/design/softwire_runtime_refactor.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/softwire_runtime_refactor.md:1>) | Softwire 重构：以可持续交付的工作为中心 | - 状态：总体重构设计，尚未实施引擎改造；交互原型仅模拟状态。 |
| [docs/design/subagent_management.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/subagent_management.md:1>) | 子代理管理体系扩展方案（R-058） | 状态：方案已验证，按低风险顺序实施 |
| [docs/design/subagent_presentation.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/subagent_presentation.md:1>) | 子代理呈现:一张卡贯穿一次委派 | - 状态: 设计基线,2026-09-26 起草(用户原话:「关于子代理的呈现,我喜欢 claude 的这种感觉,需要你出一版设计方案」);同日按用户第二条原话「子代理也弄成侧栏，参考claude的结构呢？弹出也自动化一点」修订 §5.6/§7,活动与子代理合成停靠的「后台任务」侧栏(修订记录见 §16) |
| [docs/design/tier1_handoff_20260811.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/tier1_handoff_20260811.md:1>) | 任务级并行 · 2026-08-11 交接 |  |
| [docs/design/tier1_implementation_plan.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/tier1_implementation_plan.md:1>) | 第一梯队实施方案(D-267 / R-183 / R-177 / R-182 内容②) | - 状态: **本轮发布范围已执行完成** —— D-267/F0/F2/F5 已按用户定调作废；D-269、F4/F6/F7/F9/F11/F13、R-182 内容①与 R-184 基础 A/B 面已交付。F10/F12 无人值守 CLI 与 R-184 增强 UI 不在本次发布范围 |
| [docs/design/tool_edit_recovery.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/tool_edit_recovery.md:1>) | 工具编辑恢复与结果语义 |  |
| [docs/design/tracker_evidence_ledger.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/tracker_evidence_ledger.md:1>) | Tracker 状态可信化:改动面账本与两轴状态 | - 状态: 设计基线(2026-09-02 用户就三个方向定调后冻结 §3;§4 为实施拆解) |
| [docs/design/ui_chat_backdrop.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/ui_chat_backdrop.md:1>) | 对话背景:星座渲染器 | - 状态: 设计基线(2026-09-26 起草,随 release/2026-09-26-ui2 分支 ui2/bg 实施;同日按独立复核意见修订,本文描述的是修订后已落地的实现) |
| [docs/design/ui_color_semantics.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/ui_color_semantics.md:1>) | 界面配色:表面层级与颜色语义 | - 状态: 设计基线(2026-09-26 起草,随 release/2026-09-26-ui2 分支实施;本文描述的是已落地的实现) |
| [docs/design/ui_esm_migration.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/ui_esm_migration.md:1>) | 前端迁移原生 ESM:勘察结论与迁移前置条件 | - 状态: **B1/B2 前置条件已完成，正式 ESM 迁移未收口**。对应条目 R-264(P3)。 |
| [docs/design/ui_surface_stack.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/ui_surface_stack.md:1>) | 弹层技术栈:一种写法、一处外观、一道门禁 | - 状态: 设计基线(2026-09-26 起草并随 release/2026-09-26-ui 分支实施,本文描述的是已落地的实现);同日 UI2-0926 #4「很多的弹出窗口不可以修改拖拽，不是可变的」补 §4.6 可调框与分隔条,#14 把两块常驻浮层合成停靠的后台任务侧栏 |
| [docs/design/voice_interaction.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/voice_interaction.md:1>) | 本地语音交互 |  |
| [docs/design/weakness_register_20260820.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/weakness_register_20260820.md:1>) | 弱点登记与 Agent 减负方向(2026-08-20) | - 状态: 弱点登记 + 减负方向立项(用户发起) |
| [docs/design/work_item_context.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/work_item_context.md:1>) | 条目切换时整理上下文 | - 状态：已验证（源码与模拟模型集成） |
| [docs/design/work_unit_foundation.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/work_unit_foundation.md:1>) | Work Unit 底座：把 Outcome、执行状态与历史分开 |  |
| [docs/design/workbench_first.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/workbench_first.md:1>) | 工作台优先：多项目统筹与轻量项目空间 | - 状态：设计草案；产品方向来自用户，导航细节、负载预算与迁移顺序为本轮建议，尚未接入正式应用。 |
| [docs/design/workspace_information_architecture.md](<C:/Users/kanzei/Documents/kanzei code/docs/design/workspace_information_architecture.md:1>) | 开发与研究空间信息架构 | - 状态: 源码实现、分层回归及完整 verify 已通过；发布结果以对应 GitHub Release 为准，原生桌面关键交互验收单独跟踪。 |
| [docs/prototypes/decision_console/README.md](<C:/Users/kanzei/Documents/kanzei code/docs/prototypes/decision_console/README.md:1>) | 多项目总控交互原型 |  |
| [docs/prototypes/softwire/README.md](<C:/Users/kanzei/Documents/kanzei code/docs/prototypes/softwire/README.md:1>) | Softwire 操作台体验版 · V4 |  |
| [docs/prototypes/workbench/README.md](<C:/Users/kanzei/Documents/kanzei code/docs/prototypes/workbench/README.md:1>) | 工作台优先交互原型 |  |
| [docs/reference/deepseek_harness_reference_20260814.md](<C:/Users/kanzei/Documents/kanzei code/docs/reference/deepseek_harness_reference_20260814.md:1>) | deepseek_harness_reference_20260814 |  |
| [docs/reports/2026-08-07-frontend-review.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-08-07-frontend-review.md:1>) | kanzei 桌面端前端评估报告 |  |
| [docs/reports/2026-08-08-harness-static-analysis.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-08-08-harness-static-analysis.md:1>) | Harness 静态分析报告 |  |
| [docs/reports/2026-09-27-decision-console-integration.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-09-27-decision-console-integration.md:1>) | R-379 第一批接入验证 |  |
| [docs/reports/2026-09-27-reader-verification-lab.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-09-27-reader-verification-lab.md:1>) | 手机 Markdown 阅读器：后台验证与自主决策实验 |  |
| [docs/reports/2026-09-28-bootstrap-stall-and-subagents.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-09-28-bootstrap-stall-and-subagents.md:1>) | 自举长时间不提交与子代理协作审计 |  |
| [docs/reports/2026-09-28-softwire-experience-v4.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-09-28-softwire-experience-v4.md:1>) | Softwire V4 交互体验版 |  |
| [docs/reports/2026-09-28-softwire-ui-acceptance.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-09-28-softwire-ui-acceptance.md:1>) | Softwire V3：交互与视觉验收 | - 形态与状态：主执行中心、候选虚线、归档虚线关系、完成勾、等待标记。名称和必要短状态仍可读；完整名称同时提供给辅助技术。 |
| [docs/reports/2026-09-28-work-item-context.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-09-28-work-item-context.md:1>) | 条目上下文优化实现报告 | - 交付状态：已实现、验证、同步主工作目录并发布 build-584a1f42；发布补验见文末。 |
| [docs/reports/2026-10-02-bootstrap-fixes.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-10-02-bootstrap-fixes.md:1>) | D-772 / D-773 / D-774 自举修复 |  |
| [docs/reports/2026-10-02-web-cleanup.md](<C:/Users/kanzei/Documents/kanzei code/docs/reports/2026-10-02-web-cleanup.md:1>) | 网页整理项实现与验收 |  |
| [docs/使用手册.md](<C:/Users/kanzei/Documents/kanzei code/docs/使用手册.md:1>) | kanzei 使用手册 |  |
| [docs/目录.md](<C:/Users/kanzei/Documents/kanzei code/docs/目录.md:1>) | 文档入口 |  |

## 资源和其他文件

| 文件 | 字节数 |
|---|---:|
| [.gitignore](<C:/Users/kanzei/Documents/kanzei code/.gitignore:1>) | 2024 |
| [Cargo.lock](<C:/Users/kanzei/Documents/kanzei code/Cargo.lock:1>) | 163920 |
| [crates/kanzei-app/app-icon.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/app-icon.png:1>) | 24777 |
| [crates/kanzei-app/binaries/kz-x86_64-pc-windows-msvc.exe](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/binaries/kz-x86_64-pc-windows-msvc.exe:1>) | 26324480 |
| [crates/kanzei-app/icons/128x128.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/128x128.png:1>) | 2716 |
| [crates/kanzei-app/icons/128x128@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/128x128@2x.png:1>) | 5586 |
| [crates/kanzei-app/icons/32x32.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/32x32.png:1>) | 744 |
| [crates/kanzei-app/icons/64x64.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/64x64.png:1>) | 1479 |
| [crates/kanzei-app/icons/Square107x107Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square107x107Logo.png:1>) | 2270 |
| [crates/kanzei-app/icons/Square142x142Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square142x142Logo.png:1>) | 3005 |
| [crates/kanzei-app/icons/Square150x150Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square150x150Logo.png:1>) | 3176 |
| [crates/kanzei-app/icons/Square284x284Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square284x284Logo.png:1>) | 6055 |
| [crates/kanzei-app/icons/Square30x30Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square30x30Logo.png:1>) | 735 |
| [crates/kanzei-app/icons/Square310x310Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square310x310Logo.png:1>) | 6589 |
| [crates/kanzei-app/icons/Square44x44Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square44x44Logo.png:1>) | 1005 |
| [crates/kanzei-app/icons/Square71x71Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square71x71Logo.png:1>) | 1601 |
| [crates/kanzei-app/icons/Square89x89Logo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/Square89x89Logo.png:1>) | 1910 |
| [crates/kanzei-app/icons/StoreLogo.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/StoreLogo.png:1>) | 1144 |
| [crates/kanzei-app/icons/android/mipmap-hdpi/ic_launcher.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-hdpi/ic_launcher.png:1>) | 2267 |
| [crates/kanzei-app/icons/android/mipmap-hdpi/ic_launcher_foreground.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-hdpi/ic_launcher_foreground.png:1>) | 3397 |
| [crates/kanzei-app/icons/android/mipmap-hdpi/ic_launcher_round.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-hdpi/ic_launcher_round.png:1>) | 2048 |
| [crates/kanzei-app/icons/android/mipmap-mdpi/ic_launcher.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-mdpi/ic_launcher.png:1>) | 2213 |
| [crates/kanzei-app/icons/android/mipmap-mdpi/ic_launcher_foreground.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-mdpi/ic_launcher_foreground.png:1>) | 2301 |
| [crates/kanzei-app/icons/android/mipmap-mdpi/ic_launcher_round.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-mdpi/ic_launcher_round.png:1>) | 2018 |
| [crates/kanzei-app/icons/android/mipmap-xhdpi/ic_launcher.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xhdpi/ic_launcher.png:1>) | 4721 |
| [crates/kanzei-app/icons/android/mipmap-xhdpi/ic_launcher_foreground.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xhdpi/ic_launcher_foreground.png:1>) | 4635 |
| [crates/kanzei-app/icons/android/mipmap-xhdpi/ic_launcher_round.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xhdpi/ic_launcher_round.png:1>) | 4371 |
| [crates/kanzei-app/icons/android/mipmap-xxhdpi/ic_launcher.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xxhdpi/ic_launcher.png:1>) | 7338 |
| [crates/kanzei-app/icons/android/mipmap-xxhdpi/ic_launcher_foreground.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xxhdpi/ic_launcher_foreground.png:1>) | 6971 |
| [crates/kanzei-app/icons/android/mipmap-xxhdpi/ic_launcher_round.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xxhdpi/ic_launcher_round.png:1>) | 6594 |
| [crates/kanzei-app/icons/android/mipmap-xxxhdpi/ic_launcher.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xxxhdpi/ic_launcher.png:1>) | 9963 |
| [crates/kanzei-app/icons/android/mipmap-xxxhdpi/ic_launcher_foreground.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xxxhdpi/ic_launcher_foreground.png:1>) | 9416 |
| [crates/kanzei-app/icons/android/mipmap-xxxhdpi/ic_launcher_round.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/android/mipmap-xxxhdpi/ic_launcher_round.png:1>) | 8712 |
| [crates/kanzei-app/icons/icon.icns](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/icon.icns:1>) | 66035 |
| [crates/kanzei-app/icons/icon.ico](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/icon.ico:1>) | 11675 |
| [crates/kanzei-app/icons/icon.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/icon.png:1>) | 11165 |
| [crates/kanzei-app/icons/ios/AppIcon-20x20@1x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-20x20@1x.png:1>) | 432 |
| [crates/kanzei-app/icons/ios/AppIcon-20x20@2x-1.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-20x20@2x-1.png:1>) | 790 |
| [crates/kanzei-app/icons/ios/AppIcon-20x20@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-20x20@2x.png:1>) | 790 |
| [crates/kanzei-app/icons/ios/AppIcon-20x20@3x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-20x20@3x.png:1>) | 1111 |
| [crates/kanzei-app/icons/ios/AppIcon-29x29@1x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-29x29@1x.png:1>) | 601 |
| [crates/kanzei-app/icons/ios/AppIcon-29x29@2x-1.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-29x29@2x-1.png:1>) | 1104 |
| [crates/kanzei-app/icons/ios/AppIcon-29x29@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-29x29@2x.png:1>) | 1104 |
| [crates/kanzei-app/icons/ios/AppIcon-29x29@3x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-29x29@3x.png:1>) | 1637 |
| [crates/kanzei-app/icons/ios/AppIcon-40x40@1x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-40x40@1x.png:1>) | 790 |
| [crates/kanzei-app/icons/ios/AppIcon-40x40@2x-1.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-40x40@2x-1.png:1>) | 1493 |
| [crates/kanzei-app/icons/ios/AppIcon-40x40@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-40x40@2x.png:1>) | 1493 |
| [crates/kanzei-app/icons/ios/AppIcon-40x40@3x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-40x40@3x.png:1>) | 2157 |
| [crates/kanzei-app/icons/ios/AppIcon-512@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-512@2x.png:1>) | 20572 |
| [crates/kanzei-app/icons/ios/AppIcon-60x60@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-60x60@2x.png:1>) | 2157 |
| [crates/kanzei-app/icons/ios/AppIcon-60x60@3x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-60x60@3x.png:1>) | 3097 |
| [crates/kanzei-app/icons/ios/AppIcon-76x76@1x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-76x76@1x.png:1>) | 1400 |
| [crates/kanzei-app/icons/ios/AppIcon-76x76@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-76x76@2x.png:1>) | 2597 |
| [crates/kanzei-app/icons/ios/AppIcon-83.5x83.5@2x.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/icons/ios/AppIcon-83.5x83.5@2x.png:1>) | 2895 |
| [crates/kanzei-app/mobile-pwa/icon-192.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/mobile-pwa/icon-192.png:1>) | 4101 |
| [crates/kanzei-app/mobile-pwa/icon-512.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/mobile-pwa/icon-512.png:1>) | 11165 |
| [crates/kanzei-app/ui/assets/oc/base-workwear-v7-alpha.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/base-workwear-v7-alpha.png:1>) | 399867 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/aside.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/aside.mp4:1>) | 879315 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/blocked.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/blocked.mp4:1>) | 959712 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/complete.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/complete.mp4:1>) | 890170 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/executing.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/executing.mp4:1>) | 1209677 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/idle-breath.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/idle-breath.mp4:1>) | 1568895 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/idle-observe.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/idle-observe.mp4:1>) | 1698562 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/idle-quiet.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/idle-quiet.mp4:1>) | 1741265 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/listening.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/listening.mp4:1>) | 834084 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/replying.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/replying.mp4:1>) | 1040356 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/thinking.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/thinking.mp4:1>) | 1390063 |
| [crates/kanzei-app/ui/assets/oc/clips-v7/warm.mp4](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/clips-v7/warm.mp4:1>) | 906999 |
| [crates/kanzei-app/ui/assets/oc/demo-speech-c-v7.wav](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/demo-speech-c-v7.wav:1>) | 1440044 |
| [crates/kanzei-app/ui/assets/oc/detail-reference-v7.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/detail-reference-v7.png:1>) | 1663445 |
| [crates/kanzei-app/ui/assets/oc/master-workwear-v7-alpha.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/master-workwear-v7-alpha.png:1>) | 369723 |
| [crates/kanzei-app/ui/assets/oc/mouth-soft-v6.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/mouth-soft-v6.png:1>) | 1639863 |
| [crates/kanzei-app/ui/assets/oc/reference.png](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-app/ui/assets/oc/reference.png:1>) | 1221193 |
| [crates/kanzei-tools/assets/palettes/fixtures/sample.ase](<C:/Users/kanzei/Documents/kanzei code/crates/kanzei-tools/assets/palettes/fixtures/sample.ase:1>) | 70 |
| [extras/visual-authoring/oc-h3/.gitignore](<C:/Users/kanzei/Documents/kanzei code/extras/visual-authoring/oc-h3/.gitignore:1>) | 13 |
| [package-lock.json](<C:/Users/kanzei/Documents/kanzei code/package-lock.json:1>) | 39538 |
| [scripts/oc-h3/profile-soft-v5.json.local](<C:/Users/kanzei/Documents/kanzei code/scripts/oc-h3/profile-soft-v5.json.local:1>) | 859 |
| [scripts/oc-h3/profile.json.local](<C:/Users/kanzei/Documents/kanzei code/scripts/oc-h3/profile.json.local:1>) | 841 |
| [scripts/voice/.gitignore](<C:/Users/kanzei/Documents/kanzei code/scripts/voice/.gitignore:1>) | 13 |
