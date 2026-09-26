// kanzei UI 预览:架构页夹具(UI2-0926 #7,docs/design/architecture_diagrams.md)。
// 由 architecture_snapshot 的真实形状生成的静态快照:架构索引保留 CRLF(磁盘上就是 CRLF)、含 kebab 名文档、
// docs/architecture 的两张手写图、crate 图的直接依赖/全部依赖两份源码。只作预览截图的视觉基线,
// 数据过期不影响正确性(真实数据在运行时由后端生成);要刷新就照 architecture_snapshot 的字段重写。
// 场景参数:tab=<标签 key>(crates / 01_runtime_loop / …)、full=1(全部依赖)、broken=1(追加一张写坏的图看错误卡)。
export const ARCH_SNAPSHOT = {
 "index_path": "C:/Users/kanzei/Documents/kanzei code/.kanzei/project/architecture/README.md",
 "index": "# 设计文档身份索引\r\n\r\n本索引只记录入口和结构化时效元数据；设计正文仍是各主题的内容真源。身份由治理工具消费，不由模型凭自然语言推断。\r\n\r\n字段约定：\r\n\r\n- `live_design`：仍约束当前实现；必须有 `last_verified_commit`。\r\n- `validated_design`：主体已交付，剩余边界必须映射到 tracker；必须有 `last_verified_commit`。\r\n- `historical_snapshot`：保留某个时点的事实或审计结论；使用 `as_of_commit`，不因当前 tracker 变化失败。\r\n- `superseded`：正文不再是当前方案；必须给出有效 `superseded_by`，默认上下文不得注入其正文。\r\n\r\n## live_design\r\n\r\n- [identity: live_design; last_verified_commit: 250fb219] [`cc_codex_alignment_20260925.md`](../../../docs/design/cc_codex_alignment_20260925.md)：Claude Code / Codex 能力对照与对齐清单(复刻清单 v1;接口定义与用户筛选结论,R-364~R-367、R-369~R-377、D-748、D-751 承接)。\r\n- [identity: live_design; last_verified_commit: 250fb219] [`cc_codex_alignment_impl_maps.md`](../../../docs/design/cc_codex_alignment_impl_maps.md)：CC/Codex 对齐条目实施地图(行号、批次、陷阱与裁决;勘察加对抗核对产出)。\r\n- [identity: live_design; last_verified_commit: 250fb219] [`doc_reference_graph.md`](../../../docs/design/doc_reference_graph.md)：文档引用标记、引用历史与引用图(R-368;D-749、D-750)。\r\n- [identity: live_design; last_verified_commit: 534e6be0] [`oc-playback.md`](../../../docs/design/oc-playback.md)：预渲染透明视频、解码时钟、角色开关与运行性能证据。\r\n- [identity: live_design; last_verified_commit: aa9c924a] [`oc.md`](../../../docs/design/oc.md)：角色外观、性格与连续动作表现基线。\r\n- [identity: live_design; last_verified_commit: aa9c924a] [`oc-production.md`](../../../docs/design/oc-production.md)：完整视频角色包、嘴型跟踪、动作衔接及验收范围。\r\n- [identity: live_design; last_verified_commit: aa9c924a] [`oc-idle-direction.md`](../../../docs/design/oc-idle-direction.md)：呼吸、眨眼、视线变化与待机循环的制作和检查。\r\n- [identity: live_design; last_verified_commit: aa9c924a] [`oc-h3-deployment.md`](../../../docs/design/oc-h3-deployment.md)：H3 固定版本、同区双卡部署与素材生成记录。\r\n- [identity: live_design; last_verified_commit: aa9c924a] [`oc-voice-direction.md`](../../../docs/design/oc-voice-direction.md)：角色音色方向、C 配音选择及交接约定。\r\n- [identity: live_design; last_verified_commit: 860f7ff7] [`ui_chat_backdrop.md`](../../../docs/design/ui_chat_backdrop.md)：对话背景星座渲染器——kanzei 标志笔画转星座(主干 / 记忆 / 行动三种边,光点按事件语义流动)、北斗 / 猎户 / 仙后真实星表投影、上传图片只存导出的点集;画在正文两侧沟槽(768 列两侧只剩约 120px 时缩成窄沟小徽记,正文列 evenodd 剪掉),连窄沟都放不下才退水印——水印整张画进离屏层后以单一不透明度合成,正文对比度与叠几层无关;空闲 ≤8 帧/秒、流式事件不饿死、失败会话静止、隐藏零定时器;设置页「对话背景」与 app.json backdrop 字段;浏览器冒烟实测帧预算与正文下像素(UI2-0926 #10)。\r\n- [identity: live_design; last_verified_commit: 250fb219] [`research_library.md`](../../../docs/design/research_library.md)：独立课题身份、存储及开发项目可选关联(R-363；该提交为变更前基线，工作树增量已验证，待提交验收)。\r\n- [identity: live_design; last_verified_commit: 3ce8805b] [`ui_surface_stack.md`](../../../docs/design/ui_surface_stack.md)：弹层技术栈——dialog/popover/锚点定位/base-select 顶层原语、组件层 --surface-* token、唯一的 00-surface.js(一个栈、Esc 只关栈顶、点外关闭、焦点规则)、ESLint + ui-surface-rules 静态门禁与样例页浏览器冒烟(UI-0926 #9;截图 6 白色下拉的根因与修复);§4.6 可调框与分隔条唯一入口 00-frame.js(data-kz-frame*、installSplit,几何经 ui_prefs.ui_layout 持久化)与停靠侧栏 .k-panel[data-dock]/.k-scrim(UI2-0926 #4/#14)。\r\n- [identity: live_design; last_verified_commit: ee1d9492] [`subagent_presentation.md`](../../../docs/design/subagent_presentation.md)：子代理呈现——主对话单卡(字形/人格/描述/实时计数/≤3 行尾迹)、并行成组、侧栏总览与详情、状态与数据契约;复用第一波 .kz-glyph/.k-panel/05-tool-summary 原语,后端 meta trace、稳定终态码与整轮停止补发 ToolEnd(UI-0926 #8);§5.6/§7.1 活动与子代理合成停靠的「后台任务」侧栏(三段、Claude 式委派卡、纯策略模块 06-side-policy.js 自动开合,UI2-0926 #14)。\r\n- [identity: live_design; last_verified_commit: 3ce8805b] [`ui_color_semantics.md`](../../../docs/design/ui_color_semantics.md)：界面配色——深色表面按 Codex 实测分层(主区最深、侧栏亮一档、输入区浮起、标题纯白)与语义色表(橙=进行中、琥珀=需要注意、绿=成功收尾、红=失败与 P0、灰=其余、蓝只给代码),ui-a11y-smoke ③b 叠色对比度(胶囊底 ∘ 卡底 ∘ 悬停合成后算)、⑥ 颜色语义门禁与运行时守卫(UI2-0926 #2#3,含复核修复;⑥w 输入区鞭挞组圆点与阶段字,UI2-0926 #11 复核)。\r\n- [identity: live_design; last_verified_commit: 02342e17] [`architecture_diagrams.md`](../../../docs/design/architecture_diagrams.md)：架构图——Mermaid 12(ESM 分块版懒加载,ELK 分层布局,配色只由 --diagram-* token 注入,strict)为架构图与全站 markdown 图的唯一渲染器(04-diagram.js,renderMarkdownInto 唯一入口、未闭合围栏不渲染);crate 依赖图由 arch_diagram.rs 从 Cargo 清单实时生成(传递约简、分组、节点点击),手写图是 docs/architecture/*.md、agent 用 architecture diagrams 动作自查(D1–D9 lint);架构页标签页/适应缩放/错误卡;verify 新步 ui_diagram(无头 Edge 真渲染 + 自检反例)(UI2-0926 #7)。\r\n- [identity: live_design; last_verified_commit: 8f632702] [`memory_knowledge_graph.md`](../../../docs/design/memory_knowledge_graph.md)：记忆知识图谱——记忆页「列表 | 图谱」,按架构(AreaRegistry 层带)渲染记忆、条目、代码区域与共享失败指纹的力导向图;refgraph 投影 + stat 缓存 + 可选 area 字段;图可视化统一走 vendored force-graph 共享渲染器 24-graph-view.js(取代 doc_reference_graph 的「不引入图库/手写 SVG」);配色、降级与门禁(UI2-0926 #9,含复核修复:区域行随改动重取、减少动效、确定性布局、竖排页签栏)。\r\n\r\n- [identity: live_design; last_verified_commit: 788dc43e] [`voice_interaction.md`](../../../docs/design/voice_interaction.md)：本机语音识别、流式播报、插话打断、人物嘴型与安装验证边界。\r\n- [identity: live_design; last_verified_commit: 568adcc8] [`memory_feedback_reliability.md`](../../../docs/design/memory_feedback_reliability.md)：记忆观测、恢复证据与信息呈现改造(R-361；568adcc8 为审计基线，首批代码在工作树完成定向验证，收益对照与任务上下文改造待推进)。\r\n\r\n- [identity: live_design; last_verified_commit: 02342e17] [`agent_visualization_tools.md`](../../../docs/design/agent_visualization_tools.md)：Agent 绘图工具统一设计草案(R-335；架构图与 research 科学图表 API、验证、产物和迁移边界；架构图一侧 2026-09-26 已定为 Mermaid 默认、节点可点击,见 architecture_diagrams.md,科学图表的引擎组合仍待用户评审)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`app_icon.md`](../../../docs/design/app_icon.md)：图标设计规范与资产清单(R-061 done,规范仍有效)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`bootstrap_quality_audit.md`](../../../docs/design/bootstrap_quality_audit.md)：自举质量波次审计 SOP，规定只读审计、证据替身、最后一公里接线与注释承诺检查。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`context_supply_bill_20260821.md`](../../../docs/design/context_supply_bill_20260821.md)：R-312 B1 真实 session 上下文注入账单；记录块级字符占比、粗 token 估算及当前测量缺口。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`deepseek_harness_upgrade.md`](../../../docs/design/deepseek_harness_upgrade.md)：Typed Session Events、Surface Projection、Tool Pipeline/Spill 与 LineRuntime 的升级草案(R-241～R-246,A-012 待转 accepted)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`design_freshness_audit_20260820.md`](../../../docs/design/design_freshness_audit_20260820.md)：设计文档时效审计与 R-318 治理基线；审计结论和四类身份契约仍约束本轮治理。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线——可替代区复刻优先、创新只投护城河；取活与验收判据。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`harness_m1.md`](../../../docs/design/harness_m1.md)：Harness 五注册表(commands 已按 D-748 删除) + 拦截器链 + dev/research 双 profile 架构基线，并接入 R-317 执行层权威。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md)：Memory 控制平面——证据账本/编译器/召回控制器/反事实评估四模块(R-161~R-167,D-229~D-231)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`memory_system.md`](../../../docs/design/memory_system.md)：Memory 系统设计基线(R-103~R-107,现行实施依据)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`model_autonomy_and_harness_intensity.md`](../../../docs/design/model_autonomy_and_harness_intensity.md)：模型自治与门禁强度——结伴/自主两档门禁、模型停机权与编排抽象层(R-322/R-323,D-661/D-662;2026-08-21 外部七点评估的逐点定调)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`phase2_system_upgrade.md`](../../../docs/design/phase2_system_upgrade.md)：自举二期 research/memory/运行体验/动画/voice 的依赖、波次、Go/No-Go 与联合验收总纲(R-283)。\r\n- [identity: live_design; last_verified_commit: f6b57f9b] [`auto_research.md`](../../../docs/design/auto_research.md)：AUTO research 完整流程：调研地图、用户选题、MVP、本机/SSH 环境、完整实验、分析与论文 PDF；两例真实 GPU 验收（R-363，当前工作树增量）。\r\n- [identity: live_design; last_verified_commit: 1ebbb218] [`research_experiment_runner.md`](../../../docs/design/research_experiment_runner.md)：Research 实验运行与路线图的字段与 Markdown 格式冻结(两层模型、@@kanzei 回调、本机+SSH、环境策略分档与路线图投影;A-014~A-019,R-343~R-348 承接)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`research_mode.md`](../../../docs/design/research_mode.md)：研究模式设计基线草案(2026-08-12 八维度审计维度 8 产出；八个定调点待用户确认,R-221 承接)。\r\n- [identity: live_design; last_verified_commit: dbafb50f] [`run_metrics_task_granularity.md`](../../../docs/design/run_metrics_task_granularity.md)：R-337 运行画像按执行任务关闭粒度的审计与设计草案；B1 已完成现状证据，B2 任务级方案待评审。\r\n- [identity: live_design; last_verified_commit: e3d77ea4] [`run_metrics_task_migration.md`](../../../docs/design/run_metrics_task_migration.md)：R-339 历史 task 兼容、legacy/未归属对账、旧 API 过渡与 SQLite 备份回滚边界。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`readme.md`](../../../docs/design/readme.md)：docs/design 的记录规范与文档模板；定义设计正文最小结构和方案变更规则。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`session_state_and_line_runtime.md`](../../../docs/design/session_state_and_line_runtime.md)：会话状态与线路运行态设计(状态持久化、恢复与并发线路隔离)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`subagent_management.md`](../../../docs/design/subagent_management.md)：子代理管理四层方案(R-058 done,策略层未实施)。\r\n- [identity: live_design; last_verified_commit: d374cb9f] [`weakness_register_20260820.md`](../../../docs/design/weakness_register_20260820.md)：弱点登记与 Agent 减负方向(2026-08-20 两轮外部评估对照；R-310~R-313、D-575/D-577/D-578,§六 为需求发现实测复核,减负方案待 R-312 勘察后评审)。\r\n- [identity: live_design; last_verified_commit: 7823d6f6] [`tracker_evidence_ledger.md`](../../../docs/design/tracker_evidence_ledger.md)：tracker 状态可信化——改动面账本、意图态/交付态两轴、门禁判据换成本条目改动面、调度只提示不封禁(D-736 已落地 §3.4)。\r\n- [identity: live_design; last_verified_commit: c3222943] [`work_unit_foundation.md`](../../../docs/design/work_unit_foundation.md)：Work Unit 底座——Outcome/执行状态/历史三分离的事件存储、投影与迁移契约(R-317)。\r\n\r\n## validated_design\r\n\r\n- [identity: validated_design; last_verified_commit: 5c9e1df] [`architecture_browser.md`](../../../docs/design/architecture_browser.md)：可视化架构浏览与记忆设置——技术栈选型评估(R-122 done,方案 A:既有 classic script + 目录树复用)。\r\n- [identity: validated_design; last_verified_commit: c0ea88d] [`ci_release_evidence_chain.md`](../../../docs/design/ci_release_evidence_chain.md)：CI 与发布证据链——本地门禁 + commit 锚定(R-152/R-146/R-156/R-298 done)。\r\n- [identity: validated_design; last_verified_commit: e791536] [`continue_prompt_dissection.md`](../../../docs/design/continue_prompt_dissection.md)：继续文案拆解与鞭挞引擎化——实施前拆解与 R-128/R-157/R-169/R-170 交付映射。\r\n- [identity: validated_design; last_verified_commit: e791536] [`deep_parallel_dev.md`](../../../docs/design/deep_parallel_dev.md)：任务级并行基线——一线一 worktree、diff/合并/恢复与模型隔离(R-177/R-178/R-179/R-182 done)。\r\n- [identity: live_design; last_verified_commit: 3ce8805b] [`chat_presentation_contract.md`](../../../docs/design/chat_presentation_contract.md)：主对话区分层契约——正文/轨迹/后台三层,工具一行可展开、轮间留白、单行思考不成块;附 state.db 复核的正文与轨迹字节比(R-350~R-352,D-725);§4.4 单列与工具组——列宽唯一真源 --chat-col、连续工具调用合成一行、失败常驻、ui-column-layout-smoke 浏览器量边(UI2-0926 #12)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`interaction_modes.md`](../../../docs/design/interaction_modes.md)：双人格与对话为主布局设计(R-036 done)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`m2_sqlite_store.md`](../../../docs/design/m2_sqlite_store.md)：SQLite 会话存储 Schema v1(R-003 done)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`memory_decision_sufficiency.md`](../../../docs/design/memory_decision_sufficiency.md)：Memory 判据层升级——决策充分性(R-145/R-150 done,含边界拍板与实证修正记录)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`monolith_decomposition.md`](../../../docs/design/monolith_decomposition.md)：巨石拆解方案——app/main.rs、ui/main.js、core/runner.rs、core/store.rs 分文件拆解(R-153~R-156,A-008)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`monolith_decomposition_round2.md`](../../../docs/design/monolith_decomposition_round2.md)：第二轮巨石拆解计划(R-253~R-258 的符号级批次地基)。\r\n- [identity: validated_design; last_verified_commit: 1aed3e83] [`parallel_lines_ui.md`](../../../docs/design/parallel_lines_ui.md)：多线协作可见性——协作上下文、并列线路状态、文件冲突预警及收活流程(R-184/R-185/R-222 done)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`r059_mobile_agent_communication.md`](../../../docs/design/r059_mobile_agent_communication.md)：主代理/子代理消息与通知演进设计(R-059 dropped、R-270/R-271 done，R-288 真机 E3 仍在范围)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`r108_ai_design_decision_records.md`](../../../docs/design/r108_ai_design_decision_records.md)：设计记录规范的真实示例(R-108 done)。\r\n- [identity: validated_design; last_verified_commit: 8ed3256f] [`r310_repo_map_design.md`](../../../docs/design/r310_repo_map_design.md)：代码地图形态与 token 成本对比——symbols 实时按需查询胜出(R-310 B3 done)。\r\n- [identity: validated_design; last_verified_commit: 9d784f21] [`workspace_information_architecture.md`](../../../docs/design/workspace_information_architecture.md)：开发与研究独立空间、课题绑定会话及研究页面；源码、分层回归和完整 verify 已通过，原生桌面关键交互验收仍待执行(R-360/D-742)。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`reliability_usability_self_hosting_quality.md`](../../../docs/design/reliability_usability_self_hosting_quality.md)：可靠性、可用性与自举质量不变量、验证证据和 R-317 执行模型权威。\r\n- [identity: validated_design; last_verified_commit: d374cb9f] [`ui_esm_migration.md`](../../../docs/design/ui_esm_migration.md)：前端 ESM 迁移评估(结论:保持有序 classic script,A-008)。\r\n\r\n## historical_snapshot\r\n\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`audit_20260812_eight_dimensions.md`](../../../docs/design/audit_20260812_eight_dimensions.md)：2026-08-12 八维度审计记录(巨石/记忆/验证/协作等维度的方法论与发现源)。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`context_compaction.md`](../../../docs/design/context_compaction.md)：上下文压缩设计(历史基线；当前压缩实现见 runner compaction)。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`frontend_phase3.md`](../../../docs/design/frontend_phase3.md)：前端能力差距与需求整理记录(R-031~R-051 系列)。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`metrics_baseline.md`](../../../docs/design/metrics_baseline.md)：巨石度量基线快照(R-258 批2,`kz metrics` Top-30 榜单与阈值读数)。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`parallel_read_serial_write_orchestration.md`](../../../docs/design/parallel_read_serial_write_orchestration.md)：R-171 阶段编排历史基线；R-182 已取代其实现阶段全串行部分。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`research_mode_prior_art.md`](../../../docs/design/research_mode_prior_art.md)：research 模式先行调研与同类系统对照资料，作为 R-221/R-277 的设计输入。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tier1_handoff_20260811.md`](../../../docs/design/tier1_handoff_20260811.md)：第一梯队交付移交记录(2026-08-11)。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tier1_implementation_plan.md`](../../../docs/design/tier1_implementation_plan.md)：第一梯队实施计划(R-001~R-020 等首批条目)。\r\n- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tool_edit_recovery.md`](../../../docs/design/tool_edit_recovery.md)：edit 工具恢复机制设计(匹配失败诊断与恢复策略,M-021/M-022)。\r\n\r\n## superseded\r\n\r\n- [identity: superseded; as_of_commit: e08eb0a; superseded_by: workspace_information_architecture.md] [`research_workspace.md`](../../../docs/design/research_workspace.md)：原研究工作台设计；工件交互与历史依据保留，当前导航和布局以新空间设计为准。\r\n\r\n- [identity: superseded; as_of_commit: 3e510b1; superseded_by: deep_parallel_dev.md] [`r030_process_decoupling.md`](../../../docs/design/r030_process_decoupling.md)：多进程解耦设计(R-030 done)；其 worktree/深并行残余已由 `deep_parallel_dev.md` 接替，正文保留历史决策和兼容边界。\r\n",
 "design_docs": [
  {
   "name": "agent_visualization_tools.md",
   "title": "Agent 绘图工具：架构图与研究科学图表统一设计",
   "bytes": 21720
  },
  {
   "name": "app_icon.md",
   "title": "kanzei APP 图标设计规范（R-061）",
   "bytes": 3553
  },
  {
   "name": "architecture_browser.md",
   "title": "可视化架构浏览与维护记忆设置——技术栈选型评估报告",
   "bytes": 6687
  },
  {
   "name": "architecture_diagrams.md",
   "title": "架构图:好看、agent 能改、一个渲染器",
   "bytes": 21928
  },
  {
   "name": "audit_20260812_eight_dimensions.md",
   "title": "八维度全面审计与改进计划(2026-08-12)",
   "bytes": 20054
  },
  {
   "name": "auto_research.md",
   "title": "AUTO research：从方向到实验和论文 PDF（R-363）",
   "bytes": 15151
  },
  {
   "name": "bootstrap_quality_audit.md",
   "title": "波次质量审计 SOP(手动触发)",
   "bytes": 4727
  },
  {
   "name": "cc_codex_alignment_20260925.md",
   "title": "Claude Code / Codex 能力对照与对齐清单(复刻清单 v1)",
   "bytes": 35914
  },
  {
   "name": "cc_codex_alignment_impl_maps.md",
   "title": "CC/Codex 对齐条目实施地图",
   "bytes": 391181
  },
  {
   "name": "chat_presentation_contract.md",
   "title": "主对话区呈现契约:什么算正文,什么算轨迹",
   "bytes": 14989
  },
  {
   "name": "ci_release_evidence_chain.md",
   "title": "CI 与发布证据链(独立验证者 + commit 绑定门禁)",
   "bytes": 12010
  },
  {
   "name": "context_compaction.md",
   "title": "上下文压缩重设计——分层压缩、滚动合并与可配置压缩模型",
   "bytes": 14417
  },
  {
   "name": "context_supply_bill_20260821.md",
   "title": "上下文供给账单（R-312 B1）",
   "bytes": 16951
  },
  {
   "name": "continue_prompt_dissection.md",
   "title": "继续文案拆解与鞭挞引擎化——保留必要性评估",
   "bytes": 10857
  },
  {
   "name": "deep_parallel_dev.md",
   "title": "深度并行开发模式与模型选择隔离 — 深度分析",
   "bytes": 34222
  },
  {
   "name": "deepseek_harness_upgrade.md",
   "title": "DeepSeek Harness 约束驱动的运行时升级",
   "bytes": 18348
  },
  {
   "name": "design_freshness_audit_20260820.md",
   "title": "设计文档时效审计与治理基线",
   "bytes": 6311
  },
  {
   "name": "direction_taste.md",
   "title": "方向基线:可替代区复刻优先,创新只投护城河",
   "bytes": 9351
  },
  {
   "name": "doc_reference_graph.md",
   "title": "文档引用标记、引用历史与引用图",
   "bytes": 16816
  },
  {
   "name": "frontend_phase3.md",
   "title": "前端三期:能力差距全面分析(2026-08-06)",
   "bytes": 7259
  },
  {
   "name": "harness_m1.md",
   "title": "Harness M1 设计稿",
   "bytes": 9819
  },
  {
   "name": "interaction_modes.md",
   "title": "交互模式沉淀:自主推进 vs 结伴开发(R-036)",
   "bytes": 6823
  },
  {
   "name": "m2_sqlite_store.md",
   "title": "M2 SQLite 会话存储",
   "bytes": 2510
  },
  {
   "name": "memory_control_plane.md",
   "title": "kanzei Memory 控制平面(Decision-Centric Memory Control Plane)",
   "bytes": 15173
  },
  {
   "name": "memory_decision_sufficiency.md",
   "title": "Memory 决策充分性改造(Control-Sufficient Memory)",
   "bytes": 13743
  },
  {
   "name": "memory_feedback_reliability.md",
   "title": "记忆收益与任务信息呈现",
   "bytes": 18830
  },
  {
   "name": "memory_knowledge_graph.md",
   "title": "记忆知识图谱:按架构渲染的记忆关系图",
   "bytes": 37799
  },
  {
   "name": "memory_system.md",
   "title": "kanzei Memory 系统设计",
   "bytes": 12600
  },
  {
   "name": "metrics_baseline.md",
   "title": "巨石度量基线快照(R-258 批2)",
   "bytes": 7085
  },
  {
   "name": "model_autonomy_and_harness_intensity.md",
   "title": "模型自治与门禁强度",
   "bytes": 23854
  },
  {
   "name": "monolith_decomposition.md",
   "title": "巨石文件拆解(app/main.rs · ui/main.js · core/runner.rs · core/store.rs)",
   "bytes": 24327
  },
  {
   "name": "monolith_decomposition_round2.md",
   "title": "巨石文件拆解 第二轮(app/run.rs · memory/store.rs · app/processes.rs)",
   "bytes": 33492
  },
  {
   "name": "oc-h3-deployment.md",
   "title": "OC 动画：H3 社区版本与双卡方案",
   "bytes": 11472
  },
  {
   "name": "oc-idle-direction.md",
   "title": "OC 待机与呼吸设计",
   "bytes": 4022
  },
  {
   "name": "oc-playback.md",
   "title": "角色动画播放与性能",
   "bytes": 7206
  },
  {
   "name": "oc-production.md",
   "title": "OC 角色与动画制作记录",
   "bytes": 4069
  },
  {
   "name": "oc-voice-direction.md",
   "title": "OC 音色方向",
   "bytes": 4370
  },
  {
   "name": "oc.md",
   "title": "OC 当前设计",
   "bytes": 8058
  },
  {
   "name": "parallel_lines_ui.md",
   "title": "任务级并行的前端：agent 身份贯穿全 UI",
   "bytes": 23348
  },
  {
   "name": "parallel_read_serial_write_orchestration.md",
   "title": "多进程代理编排：阶段流历史基线与任务级并行修订",
   "bytes": 31303
  },
  {
   "name": "phase2_system_upgrade.md",
   "title": "自举二期系统升级总纲",
   "bytes": 22125
  },
  {
   "name": "r030_process_decoupling.md",
   "title": "R-030 进程与项目解耦(多进程并行)设计",
   "bytes": 4640
  },
  {
   "name": "r059_mobile_agent_communication.md",
   "title": "R-059 移动端子代理通信与通知设计",
   "bytes": 13282
  },
  {
   "name": "r108_ai_design_decision_records.md",
   "title": "R-108 AI 设计讨论与技术决策记录",
   "bytes": 5086
  },
  {
   "name": "r310_repo_map_design.md",
   "title": "Repo Map Design for R-310 Batch 3",
   "bytes": 3739
  },
  {
   "name": "readme.md",
   "title": "设计与 AI 讨论记录规范",
   "bytes": 3266
  },
  {
   "name": "reliability_usability_self_hosting_quality.md",
   "title": "Kanzei 可靠性、可用性与自举质量打磨设计",
   "bytes": 26734
  },
  {
   "name": "research_experiment_runner.md",
   "title": "Research 实验运行与路线图:字段与 Markdown 格式冻结",
   "bytes": 23211
  },
  {
   "name": "research_library.md",
   "title": "独立研究课题库",
   "bytes": 2520
  },
  {
   "name": "research_mode.md",
   "title": "research 模式设计:独立深度研究模式(文献+仓库,论文级产出)",
   "bytes": 16544
  },
  {
   "name": "research_mode_prior_art.md",
   "title": "research mode 先行对照:已有方案调查(prior art)",
   "bytes": 15884
  },
  {
   "name": "research_workspace.md",
   "title": "research 工作台前端设计(R-276 批1 设计稿)",
   "bytes": 9021
  },
  {
   "name": "run_metrics_task_granularity.md",
   "title": "运行画像按执行任务关闭粒度",
   "bytes": 20885
  },
  {
   "name": "run_metrics_task_migration.md",
   "title": "运行画像任务历史兼容、对账与回滚",
   "bytes": 6640
  },
  {
   "name": "session_state_and_line_runtime.md",
   "title": "会话运行态、并行线路与任务设置统一设计",
   "bytes": 6486
  },
  {
   "name": "subagent_management.md",
   "title": "子代理管理体系扩展方案（R-058）",
   "bytes": 6100
  },
  {
   "name": "subagent_presentation.md",
   "title": "子代理呈现:一张卡贯穿一次委派",
   "bytes": 48253
  },
  {
   "name": "tier1_handoff_20260811.md",
   "title": "任务级并行 · 2026-08-11 交接",
   "bytes": 4293
  },
  {
   "name": "tier1_implementation_plan.md",
   "title": "第一梯队实施方案(D-267 / R-183 / R-177 / R-182 内容②)",
   "bytes": 98201
  },
  {
   "name": "tool_edit_recovery.md",
   "title": "工具编辑恢复与结果语义",
   "bytes": 2940
  },
  {
   "name": "tracker_evidence_ledger.md",
   "title": "Tracker 状态可信化:改动面账本与两轴状态",
   "bytes": 13409
  },
  {
   "name": "ui_chat_backdrop.md",
   "title": "对话背景:星座渲染器",
   "bytes": 25562
  },
  {
   "name": "ui_color_semantics.md",
   "title": "界面配色:表面层级与颜色语义",
   "bytes": 23604
  },
  {
   "name": "ui_esm_migration.md",
   "title": "前端迁移原生 ESM:勘察结论与迁移前置条件",
   "bytes": 8205
  },
  {
   "name": "ui_surface_stack.md",
   "title": "弹层技术栈:一种写法、一处外观、一道门禁",
   "bytes": 58303
  },
  {
   "name": "voice_interaction.md",
   "title": "本地语音交互",
   "bytes": 8103
  },
  {
   "name": "weakness_register_20260820.md",
   "title": "弱点登记与 Agent 减负方向(2026-08-20)",
   "bytes": 12304
  },
  {
   "name": "work_unit_foundation.md",
   "title": "Work Unit 底座：把 Outcome、执行状态与历史分开",
   "bytes": 7795
  },
  {
   "name": "workspace_information_architecture.md",
   "title": "开发与研究空间信息架构",
   "bytes": 7630
  }
 ],
 "diagrams": [
  {
   "id": "01_runtime_loop",
   "path": "docs/architecture/01_runtime_loop.md",
   "title": "运行时主循环",
   "summary": "一轮任务从输入区(或 kz 命令行)进来:kanzei-app 的 run_task 排队并装配本轮,执行循环依次做记忆预检索、勘察、主循环与复核;kanzei-core 的 run_once_with_parts 是主循环本体——装配工具与上下文、流式调模型、过权限门禁执行工具、超限时压缩。事件一路落库并推回界面。点节点打开对应实现。",
   "source": "flowchart LR\n  subgraph entry_grp[\"入口\"]\n    compose[\"输入区<br/>ui/08-compose.js\"]:::entry\n    cli[\"kz 命令行<br/>cli/run.rs\"]:::entry\n  end\n  subgraph app_grp[\"kanzei-app · 一轮任务\"]\n    run_task[\"run_task<br/>排队、装配、收尾\"]\n    exec_loop[\"执行循环<br/>预检索、勘察、复核\"]\n  end\n  subgraph core_grp[\"kanzei-core · 主循环\"]\n    drive[\"run_once_with_parts<br/>runner/drive.rs\"]:::focus\n    assembly[\"开跑装配<br/>工具、system、消息\"]\n    tool_exec[\"工具执行<br/>权限门禁、并行批\"]\n    compaction[\"上下文压缩<br/>摘要与溢出恢复\"]\n  end\n  llm[\"kanzei-llm<br/>流式调用模型\"]:::ext\n  pipeline[\"工具流水线<br/>guards、本体、策略\"]\n  store[(\"state.db<br/>事件存储\")]:::store\n  ui_events[\"界面事件<br/>ui/07-events.js\"]\n\n  compose --> run_task\n  cli --> drive\n  run_task --> exec_loop\n  exec_loop --> drive\n  exec_loop -.-> ui_events\n  exec_loop -.-> store\n  drive --> assembly\n  drive --> llm\n  drive --> tool_exec\n  drive --> compaction\n  tool_exec --> pipeline\n\n  click compose \"crates/kanzei-app/ui/08-compose.js\" \"输入区:经 run_prompt 发送一轮任务\"\n  click cli \"crates/kanzei/src/cli/run.rs\" \"kz run:命令行直接调 run_once\"\n  click run_task \"crates/kanzei-app/src/run/coordinator.rs\" \"run_task:装配、事件循环、轮末收尾\"\n  click exec_loop \"crates/kanzei-app/src/run/execution.rs\" \"run_execution_loop:记忆预检索 → 勘察 → 主循环 → 复核修正\"\n  click drive \"crates/kanzei-core/src/runner/drive.rs\" \"run_once_with_parts:单次运行主循环\"\n  click assembly \"crates/kanzei-core/src/runner/drive/assembly.rs\" \"开跑时的工具物化、system 分块与消息初始化\"\n  click tool_exec \"crates/kanzei-core/src/runner/drive/permissions.rs\" \"按规则集裁决允许 / 询问 / 拒绝;并行批见 drive/parallel_tools.rs\"\n  click compaction \"crates/kanzei-core/src/runner/compaction.rs\" \"主动压缩与上下文溢出恢复\"\n  click llm \"crates/kanzei-llm/src/lib.rs\" \"多协议 LLM、流式事件与认证,连到模型服务商\"\n  click pipeline \"crates/kanzei-harness/src/tool_pipeline.rs\" \"run_tool_pipeline:guards → 工具本体 → result policies → observers\"\n  click store \"crates/kanzei-core/src/store/events.rs\" \"事件存储(state.db)\"\n  click ui_events \"crates/kanzei-app/ui/07-events.js\" \"前端事件处理\"",
   "source_line": 6,
   "issues": []
  },
  {
   "id": "02_harness_registries",
   "path": "docs/architecture/02_harness_registries.md",
   "title": "Harness 注册表",
   "summary": "每次运行前,各档位组件往 HarnessDraft 的五个注册表里贡献条目(agents、tools、skills、上下文源、权限规则集),`Harness::resolve` 做能力覆盖校验后冻结成不可变快照;主循环每次调工具都走同一条流水线:guards → 工具本体 → 结果策略 → 观察者。详细设计见 docs/design/harness_m1.md。",
   "source": "flowchart LR\n  subgraph source_grp[\"组件来源\"]\n    dev_profile[\"dev 档组件<br/>profiles/dev.rs\"]:::entry\n    research_profile[\"research 档组件<br/>profiles/research.rs\"]:::entry\n    readonly_profile[\"只读档<br/>profiles/readonly.rs\"]:::muted\n    config[\"kanzei.toml<br/>全局 → 项目层叠\"]\n    defs[\"agent / skill 定义<br/>defs.rs\"]\n  end\n  subgraph draft_grp[\"HarnessDraft · 五个注册表\"]\n    agents[\"agents<br/>AgentDef\"]\n    tools_reg[\"tools<br/>Tool 实现\"]\n    skills[\"skills<br/>SkillDef\"]\n    context[\"上下文源<br/>ContextSource\"]\n    rules[\"权限规则集<br/>Ruleset\"]\n  end\n  resolve[\"Harness::resolve<br/>能力覆盖校验\"]:::focus\n  snapshot[\"HarnessSnapshot<br/>本次运行的不可变快照\"]\n  runner[\"kanzei-core 主循环<br/>runner/drive.rs\"]\n  pipeline[\"工具流水线<br/>guards → 本体 → policies → observers\"]\n  design_doc[\"设计文档<br/>harness_m1.md\"]:::ext\n\n  dev_profile --> tools_reg\n  dev_profile --> context\n  dev_profile --> rules\n  research_profile --> tools_reg\n  readonly_profile -.-> rules\n  config --> rules\n  defs --> agents\n  defs --> skills\n  agents --> resolve\n  tools_reg --> resolve\n  skills --> resolve\n  context --> resolve\n  rules --> resolve\n  resolve --> snapshot\n  snapshot --> runner\n  runner --> pipeline\n  snapshot -.-> design_doc\n\n  click dev_profile \"crates/kanzei-tools/src/profiles/dev.rs\" \"dev 档:注册工具、上下文源与权限规则\"\n  click research_profile \"crates/kanzei-tools/src/profiles/research.rs\" \"research 档组件\"\n  click readonly_profile \"crates/kanzei-tools/src/profiles/readonly.rs\" \"只读档:收紧写权限\"\n  click config \"crates/kanzei-harness/src/config.rs\" \"kanzei.toml:全局 ~/.kanzei 再叠项目 .kanzei\"\n  click defs \"crates/kanzei-harness/src/defs.rs\" \"AgentDef / SkillDef / ProfileKind\"\n  click agents \"crates/kanzei-harness/src/harness.rs\" \"HarnessDraft 的五个注册表\"\n  click tools_reg \"crates/kanzei-harness/src/tool.rs\" \"Tool trait\"\n  click skills \"crates/kanzei-harness/src/defs.rs\" \"SkillDef\"\n  click context \"crates/kanzei-harness/src/context.rs\" \"ContextSource:每轮渲染的上下文片段\"\n  click rules \"crates/kanzei-harness/src/permission.rs\" \"Ruleset:允许 / 询问 / 拒绝\"\n  click resolve \"crates/kanzei-harness/src/harness.rs\" \"组件按序贡献,装配末尾校验专用工具都已注册\"\n  click snapshot \"crates/kanzei-harness/src/harness.rs\" \"HarnessSnapshot\"\n  click runner \"crates/kanzei-core/src/runner/drive.rs\" \"run_once_with_parts\"\n  click pipeline \"crates/kanzei-harness/src/tool_pipeline.rs\" \"run_tool_pipeline:唯一流水线\"\n  click design_doc \"docs/design/harness_m1.md\" \"Harness 设计文档\"",
   "source_line": 6,
   "issues": []
  }
 ],
 "crates": {
  "members": [
   {
    "id": "kanzei_base",
    "name": "kanzei-base",
    "description": "原子写与文件锁",
    "group": "基础",
    "entry": "crates/kanzei-base/src/lib.rs",
    "dir": "crates/kanzei-base"
   },
   {
    "id": "kanzei_harness",
    "name": "kanzei-harness",
    "description": "注册表与拦截器",
    "group": "运行时",
    "entry": "crates/kanzei-harness/src/lib.rs",
    "dir": "crates/kanzei-harness"
   },
   {
    "id": "kanzei_llm",
    "name": "kanzei-llm",
    "description": "多协议模型适配",
    "group": "运行时",
    "entry": "crates/kanzei-llm/src/lib.rs",
    "dir": "crates/kanzei-llm"
   },
   {
    "id": "kanzei_core",
    "name": "kanzei-core",
    "description": "会话主循环与事件",
    "group": "运行时",
    "entry": "crates/kanzei-core/src/lib.rs",
    "dir": "crates/kanzei-core"
   },
   {
    "id": "kanzei_memory",
    "name": "kanzei-memory",
    "description": "记忆与文档存储",
    "group": "能力",
    "entry": "crates/kanzei-memory/src/lib.rs",
    "dir": "crates/kanzei-memory"
   },
   {
    "id": "kanzei_tools",
    "name": "kanzei-tools",
    "description": "内置工具与档位",
    "group": "能力",
    "entry": "crates/kanzei-tools/src/lib.rs",
    "dir": "crates/kanzei-tools"
   },
   {
    "id": "kanzei",
    "name": "kanzei",
    "description": "kz 命令行",
    "group": "入口",
    "entry": "crates/kanzei/src/main.rs",
    "dir": "crates/kanzei"
   },
   {
    "id": "kanzei_app",
    "name": "kanzei-app",
    "description": "Tauri 桌面端",
    "group": "入口",
    "entry": "crates/kanzei-app/src/main.rs",
    "dir": "crates/kanzei-app"
   }
  ],
  "edges": [
   {
    "from": "kanzei",
    "to": "kanzei-core",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei",
    "to": "kanzei-harness",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei",
    "to": "kanzei-llm",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei",
    "to": "kanzei-tools",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-app",
    "to": "kanzei-core",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-app",
    "to": "kanzei-harness",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-app",
    "to": "kanzei-llm",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-app",
    "to": "kanzei-tools",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-core",
    "to": "kanzei-base",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-core",
    "to": "kanzei-harness",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-core",
    "to": "kanzei-llm",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-harness",
    "to": "kanzei-base",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-llm",
    "to": "kanzei-base",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-memory",
    "to": "kanzei-base",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-memory",
    "to": "kanzei-core",
    "kind": "normal",
    "transitive": false
   },
   {
    "from": "kanzei-memory",
    "to": "kanzei-harness",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-memory",
    "to": "kanzei-llm",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-tools",
    "to": "kanzei-base",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-tools",
    "to": "kanzei-core",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-tools",
    "to": "kanzei-harness",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-tools",
    "to": "kanzei-llm",
    "kind": "normal",
    "transitive": true
   },
   {
    "from": "kanzei-tools",
    "to": "kanzei-memory",
    "kind": "normal",
    "transitive": false
   }
  ],
  "mermaid": {
   "reduced": "flowchart LR\n  %% 由 Cargo 清单生成(不落盘):只画直接依赖,可由传递得到的依赖已隐藏;dev/build 依赖不画\n  subgraph grp_1[\"入口\"]\n    kanzei[\"kanzei<br/>kz 命令行\"]:::entry\n    kanzei_app[\"kanzei-app<br/>Tauri 桌面端\"]:::entry\n  end\n  subgraph grp_2[\"能力\"]\n    kanzei_tools[\"kanzei-tools<br/>内置工具与档位\"]\n    kanzei_memory[\"kanzei-memory<br/>记忆与文档存储\"]\n  end\n  subgraph grp_3[\"运行时\"]\n    kanzei_core[\"kanzei-core<br/>会话主循环与事件\"]\n    kanzei_harness[\"kanzei-harness<br/>注册表与拦截器\"]\n    kanzei_llm[\"kanzei-llm<br/>多协议模型适配\"]\n  end\n  subgraph grp_4[\"基础\"]\n    kanzei_base[\"kanzei-base<br/>原子写与文件锁\"]\n  end\n  kanzei --> kanzei_tools\n  kanzei_app --> kanzei_tools\n  kanzei_core --> kanzei_harness\n  kanzei_core --> kanzei_llm\n  kanzei_harness --> kanzei_base\n  kanzei_llm --> kanzei_base\n  kanzei_memory --> kanzei_core\n  kanzei_tools --> kanzei_memory\n  click kanzei_base \"crates/kanzei-base/src/lib.rs\" \"kanzei-base · 原子写与文件锁\"\n  click kanzei_harness \"crates/kanzei-harness/src/lib.rs\" \"kanzei-harness · 注册表与拦截器\"\n  click kanzei_llm \"crates/kanzei-llm/src/lib.rs\" \"kanzei-llm · 多协议模型适配\"\n  click kanzei_core \"crates/kanzei-core/src/lib.rs\" \"kanzei-core · 会话主循环与事件\"\n  click kanzei_memory \"crates/kanzei-memory/src/lib.rs\" \"kanzei-memory · 记忆与文档存储\"\n  click kanzei_tools \"crates/kanzei-tools/src/lib.rs\" \"kanzei-tools · 内置工具与档位\"\n  click kanzei \"crates/kanzei/src/main.rs\" \"kanzei · kz 命令行\"\n  click kanzei_app \"crates/kanzei-app/src/main.rs\" \"kanzei-app · Tauri 桌面端\"\n",
   "full": "flowchart LR\n  %% 由 Cargo 清单生成(不落盘):实线 = 直接依赖,虚线 = 可由传递得到的依赖(不分组);dev/build 依赖不画\n  kanzei[\"kanzei<br/>kz 命令行\"]:::entry\n  kanzei_app[\"kanzei-app<br/>Tauri 桌面端\"]:::entry\n  kanzei_tools[\"kanzei-tools<br/>内置工具与档位\"]\n  kanzei_memory[\"kanzei-memory<br/>记忆与文档存储\"]\n  kanzei_core[\"kanzei-core<br/>会话主循环与事件\"]\n  kanzei_harness[\"kanzei-harness<br/>注册表与拦截器\"]\n  kanzei_llm[\"kanzei-llm<br/>多协议模型适配\"]\n  kanzei_base[\"kanzei-base<br/>原子写与文件锁\"]\n  kanzei -.-> kanzei_core\n  kanzei -.-> kanzei_harness\n  kanzei -.-> kanzei_llm\n  kanzei --> kanzei_tools\n  kanzei_app -.-> kanzei_core\n  kanzei_app -.-> kanzei_harness\n  kanzei_app -.-> kanzei_llm\n  kanzei_app --> kanzei_tools\n  kanzei_core -.-> kanzei_base\n  kanzei_core --> kanzei_harness\n  kanzei_core --> kanzei_llm\n  kanzei_harness --> kanzei_base\n  kanzei_llm --> kanzei_base\n  kanzei_memory -.-> kanzei_base\n  kanzei_memory --> kanzei_core\n  kanzei_memory -.-> kanzei_harness\n  kanzei_memory -.-> kanzei_llm\n  kanzei_tools -.-> kanzei_base\n  kanzei_tools -.-> kanzei_core\n  kanzei_tools -.-> kanzei_harness\n  kanzei_tools -.-> kanzei_llm\n  kanzei_tools --> kanzei_memory\n  click kanzei_base \"crates/kanzei-base/src/lib.rs\" \"kanzei-base · 原子写与文件锁\"\n  click kanzei_harness \"crates/kanzei-harness/src/lib.rs\" \"kanzei-harness · 注册表与拦截器\"\n  click kanzei_llm \"crates/kanzei-llm/src/lib.rs\" \"kanzei-llm · 多协议模型适配\"\n  click kanzei_core \"crates/kanzei-core/src/lib.rs\" \"kanzei-core · 会话主循环与事件\"\n  click kanzei_memory \"crates/kanzei-memory/src/lib.rs\" \"kanzei-memory · 记忆与文档存储\"\n  click kanzei_tools \"crates/kanzei-tools/src/lib.rs\" \"kanzei-tools · 内置工具与档位\"\n  click kanzei \"crates/kanzei/src/main.rs\" \"kanzei · kz 命令行\"\n  click kanzei_app \"crates/kanzei-app/src/main.rs\" \"kanzei-app · Tauri 桌面端\"\n"
  },
  "hidden_transitive": 14
 },
 "graph": []
};

export const ARCH_BROKEN_DIAGRAM = {
 "id": "03_broken_example",
 "path": "docs/architecture/03_broken_example.md",
 "title": "写坏的图(预览)",
 "summary": "预览用:标签里的括号没加引号。",
 "source": "flowchart LR\n  %% 预览:括号没加引号(注释与 click 行在前,错误卡的行号仍对)\n  click c \"docs/design/architecture_diagrams.md\"\n  a[\"入口\"] --> b[run(x)]\n  b --> c[\"出口\"]",
 "source_line": 5,
 "issues": [
  {
   "line": 8,
   "severity": "error",
   "code": "D6",
   "message": "标签 `run(x)` 含括号/竖线/引号却没加引号(mermaid 解析不了)",
   "hint": "写成 id[\"run(x)\"](整段标签加双引号)"
  }
 ]
};

/// 按场景参数给 architecture_snapshot:broken=1 时追加一张写坏的图。
export function archSnapshot(params = {}) {
  const snap = structuredClone(ARCH_SNAPSHOT);
  if (params.broken === "1") snap.diagrams.push(structuredClone(ARCH_BROKEN_DIAGRAM));
  return snap;
}
