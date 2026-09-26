# 设计文档身份索引

本索引只记录入口和结构化时效元数据；设计正文仍是各主题的内容真源。身份由治理工具消费，不由模型凭自然语言推断。

字段约定：

- `live_design`：仍约束当前实现；必须有 `last_verified_commit`。
- `validated_design`：主体已交付，剩余边界必须映射到 tracker；必须有 `last_verified_commit`。
- `historical_snapshot`：保留某个时点的事实或审计结论；使用 `as_of_commit`，不因当前 tracker 变化失败。
- `superseded`：正文不再是当前方案；必须给出有效 `superseded_by`，默认上下文不得注入其正文。

## live_design

- [identity: live_design; last_verified_commit: 250fb219] [`cc_codex_alignment_20260925.md`](../../../docs/design/cc_codex_alignment_20260925.md)：Claude Code / Codex 能力对照与对齐清单(复刻清单 v1;接口定义与用户筛选结论,R-364~R-367、R-369~R-377、D-748、D-751 承接)。
- [identity: live_design; last_verified_commit: 250fb219] [`cc_codex_alignment_impl_maps.md`](../../../docs/design/cc_codex_alignment_impl_maps.md)：CC/Codex 对齐条目实施地图(行号、批次、陷阱与裁决;勘察加对抗核对产出)。
- [identity: live_design; last_verified_commit: 250fb219] [`doc_reference_graph.md`](../../../docs/design/doc_reference_graph.md)：文档引用标记、引用历史与引用图(R-368;D-749、D-750)。
- [identity: live_design; last_verified_commit: 534e6be0] [`oc-playback.md`](../../../docs/design/oc-playback.md)：预渲染透明视频、解码时钟、角色开关与运行性能证据。
- [identity: live_design; last_verified_commit: aa9c924a] [`oc.md`](../../../docs/design/oc.md)：角色外观、性格与连续动作表现基线。
- [identity: live_design; last_verified_commit: aa9c924a] [`oc-production.md`](../../../docs/design/oc-production.md)：完整视频角色包、嘴型跟踪、动作衔接及验收范围。
- [identity: live_design; last_verified_commit: aa9c924a] [`oc-idle-direction.md`](../../../docs/design/oc-idle-direction.md)：呼吸、眨眼、视线变化与待机循环的制作和检查。
- [identity: live_design; last_verified_commit: aa9c924a] [`oc-h3-deployment.md`](../../../docs/design/oc-h3-deployment.md)：H3 固定版本、同区双卡部署与素材生成记录。
- [identity: live_design; last_verified_commit: aa9c924a] [`oc-voice-direction.md`](../../../docs/design/oc-voice-direction.md)：角色音色方向、C 配音选择及交接约定。
- [identity: live_design; last_verified_commit: 860f7ff7] [`ui_chat_backdrop.md`](../../../docs/design/ui_chat_backdrop.md)：对话背景星座渲染器——kanzei 标志笔画转星座(主干 / 记忆 / 行动三种边,光点按事件语义流动)、北斗 / 猎户 / 仙后真实星表投影、上传图片只存导出的点集;画在正文两侧沟槽(768 列两侧只剩约 120px 时缩成窄沟小徽记,正文列 evenodd 剪掉),连窄沟都放不下才退水印——水印整张画进离屏层后以单一不透明度合成,正文对比度与叠几层无关;空闲 ≤8 帧/秒、流式事件不饿死、失败会话静止、隐藏零定时器;设置页「对话背景」与 app.json backdrop 字段;浏览器冒烟实测帧预算与正文下像素(UI2-0926 #10)。
- [identity: live_design; last_verified_commit: 250fb219] [`research_library.md`](../../../docs/design/research_library.md)：独立课题身份、存储及开发项目可选关联(R-363；该提交为变更前基线，工作树增量已验证，待提交验收)。
- [identity: live_design; last_verified_commit: 3ce8805b] [`ui_surface_stack.md`](../../../docs/design/ui_surface_stack.md)：弹层技术栈——dialog/popover/锚点定位/base-select 顶层原语、组件层 --surface-* token、唯一的 00-surface.js(一个栈、Esc 只关栈顶、点外关闭、焦点规则)、ESLint + ui-surface-rules 静态门禁与样例页浏览器冒烟(UI-0926 #9;截图 6 白色下拉的根因与修复);§4.6 可调框与分隔条唯一入口 00-frame.js(data-kz-frame*、installSplit,几何经 ui_prefs.ui_layout 持久化)与停靠侧栏 .k-panel[data-dock]/.k-scrim(UI2-0926 #4/#14)。
- [identity: live_design; last_verified_commit: 2f51f485] [`preview_pane.md`](../../../docs/design/preview_pane.md)：网页预览面板(UI2-0926 #8,对标 ChatGPT/Codex 内置浏览器)。§前端:停靠面板布局与窄屏页签、preview_set_bounds/set_visible 上报口径(带 processId 供代理路由)、onSurfaceChange 遮挡冻结与 --surface-safe-right、地址栏规范化、控制台与错误页、工具截图缩略图/交付卡片/代码块预览/localhost 链接、批注进附件、与后台任务侧栏并存、门禁与接缝。
- [identity: live_design; last_verified_commit: ee1d9492] [`subagent_presentation.md`](../../../docs/design/subagent_presentation.md)：子代理呈现——主对话单卡(字形/人格/描述/实时计数/≤3 行尾迹)、并行成组、侧栏总览与详情、状态与数据契约;复用第一波 .kz-glyph/.k-panel/05-tool-summary 原语,后端 meta trace、稳定终态码与整轮停止补发 ToolEnd(UI-0926 #8);§5.6/§7.1 活动与子代理合成停靠的「后台任务」侧栏(三段、Claude 式委派卡、纯策略模块 06-side-policy.js 自动开合,UI2-0926 #14)。
- [identity: live_design; last_verified_commit: 3ce8805b] [`ui_color_semantics.md`](../../../docs/design/ui_color_semantics.md)：界面配色——深色表面按 Codex 实测分层(主区最深、侧栏亮一档、输入区浮起、标题纯白)与语义色表(橙=进行中、琥珀=需要注意、绿=成功收尾、红=失败与 P0、灰=其余、蓝只给代码),ui-a11y-smoke ③b 叠色对比度(胶囊底 ∘ 卡底 ∘ 悬停合成后算)、⑥ 颜色语义门禁与运行时守卫(UI2-0926 #2#3,含复核修复;⑥w 输入区鞭挞组圆点与阶段字,UI2-0926 #11 复核)。
- [identity: live_design; last_verified_commit: ab34592f] [`architecture_diagrams.md`](../../../docs/design/architecture_diagrams.md)：架构图——Mermaid 12(ESM 分块版懒加载,ELK 分层布局,配色只由 --diagram-* token 注入,strict)为架构图与全站 markdown 图的唯一渲染器(04-diagram.js,renderMarkdownInto 唯一入口、未闭合围栏不渲染);crate 依赖图由 arch_diagram.rs 从 Cargo 清单实时生成(传递约简、分组、节点点击),手写图是 docs/architecture/*.md、agent 用 architecture diagrams 动作自查(D1–D9 lint);架构页标签页/适应缩放/错误卡;verify 新步 ui_diagram(无头 Edge 真渲染 + 自检反例)(UI2-0926 #7)。
- [identity: live_design; last_verified_commit: 8f632702] [`memory_knowledge_graph.md`](../../../docs/design/memory_knowledge_graph.md)：记忆知识图谱——记忆页「列表 | 图谱」,按架构(AreaRegistry 层带)渲染记忆、条目、代码区域与共享失败指纹的力导向图;refgraph 投影 + stat 缓存 + 可选 area 字段;图可视化统一走 vendored force-graph 共享渲染器 24-graph-view.js(取代 doc_reference_graph 的「不引入图库/手写 SVG」);配色、降级与门禁(UI2-0926 #9,含复核修复:区域行随改动重取、减少动效、确定性布局、竖排页签栏)。
- [identity: live_design; last_verified_commit: ced41f3c] [`project_workspace.md`](../../../docs/design/project_workspace.md)：工作目录管理——新建项目对话框(名称/位置/默认建 Git 库、有身份时首提交、.kanzei/.gitignore 运行时忽略规则、描述进草稿)、项目状态事实 core/project-state(空项目/Git 三态/技术栈/工具链,与桌面端 project_facts 同源)与事实横幅/「无 Git」芯片/并行线入口、路径形态唯一实现 path_form 与 schema v25 去 `\\?\` 前缀迁移、git 工具只操作自己的仓库与 init 动作、bash UTF-8 与新鲜 PATH、鞭挞 Stop(AwaitingUser)与按项目状态生成的 Nudge、结伴线续跑按结伴档(UI2-0926 #13)。

- [identity: live_design; last_verified_commit: 788dc43e] [`voice_interaction.md`](../../../docs/design/voice_interaction.md)：本机语音识别、流式播报、插话打断、人物嘴型与安装验证边界。
- [identity: live_design; last_verified_commit: 64953559] [`files_editor.md`](../../../docs/design/files_editor.md)：文件页编辑与可拖拽伸缩——统一路径解析 resolve_in_root(词法拒绝 + 真实路径包含)、只读策略(.git / 托管 / 内部状态 / 二进制 / 超 4MB / 非 UTF-8)、BOM 与换行保真、file_stat / file_write 按内容指纹比较并交换与覆盖留证(quarantine files-overwrite)、写日志供跨树围栏吸收用户手改;前端保存 / 脏标记 / 冲突横幅(比较 / 用磁盘版本 / 覆盖)/ 外部改动轮询 / 切项目草稿 / 新建文件 / 按行定位,文件树分隔条沿用 installSplit 与 ui_layout(UI2-0926 #6)。
- [identity: live_design; last_verified_commit: 568adcc8] [`memory_feedback_reliability.md`](../../../docs/design/memory_feedback_reliability.md)：记忆观测、恢复证据与信息呈现改造(R-361；568adcc8 为审计基线，首批代码在工作树完成定向验证，收益对照与任务上下文改造待推进)。

- [identity: live_design; last_verified_commit: 02342e17] [`agent_visualization_tools.md`](../../../docs/design/agent_visualization_tools.md)：Agent 绘图工具统一设计草案(R-335；架构图与 research 科学图表 API、验证、产物和迁移边界；架构图一侧 2026-09-26 已定为 Mermaid 默认、节点可点击,见 architecture_diagrams.md,科学图表的引擎组合仍待用户评审)。
- [identity: live_design; last_verified_commit: d374cb9f] [`app_icon.md`](../../../docs/design/app_icon.md)：图标设计规范与资产清单(R-061 done,规范仍有效)。
- [identity: live_design; last_verified_commit: d374cb9f] [`bootstrap_quality_audit.md`](../../../docs/design/bootstrap_quality_audit.md)：自举质量波次审计 SOP，规定只读审计、证据替身、最后一公里接线与注释承诺检查。
- [identity: live_design; last_verified_commit: d374cb9f] [`context_supply_bill_20260821.md`](../../../docs/design/context_supply_bill_20260821.md)：R-312 B1 真实 session 上下文注入账单；记录块级字符占比、粗 token 估算及当前测量缺口。
- [identity: live_design; last_verified_commit: d374cb9f] [`deepseek_harness_upgrade.md`](../../../docs/design/deepseek_harness_upgrade.md)：Typed Session Events、Surface Projection、Tool Pipeline/Spill 与 LineRuntime 的升级草案(R-241～R-246,A-012 待转 accepted)。
- [identity: live_design; last_verified_commit: d374cb9f] [`design_freshness_audit_20260820.md`](../../../docs/design/design_freshness_audit_20260820.md)：设计文档时效审计与 R-318 治理基线；审计结论和四类身份契约仍约束本轮治理。
- [identity: live_design; last_verified_commit: d374cb9f] [`direction_taste.md`](../../../docs/design/direction_taste.md)：方向基线——可替代区复刻优先、创新只投护城河；取活与验收判据。
- [identity: live_design; last_verified_commit: d374cb9f] [`harness_m1.md`](../../../docs/design/harness_m1.md)：Harness 五注册表(commands 已按 D-748 删除) + 拦截器链 + dev/research 双 profile 架构基线，并接入 R-317 执行层权威。
- [identity: live_design; last_verified_commit: d374cb9f] [`memory_control_plane.md`](../../../docs/design/memory_control_plane.md)：Memory 控制平面——证据账本/编译器/召回控制器/反事实评估四模块(R-161~R-167,D-229~D-231)。
- [identity: live_design; last_verified_commit: d374cb9f] [`memory_system.md`](../../../docs/design/memory_system.md)：Memory 系统设计基线(R-103~R-107,现行实施依据)。
- [identity: live_design; last_verified_commit: 1e2347e3] [`model_autonomy_and_harness_intensity.md`](../../../docs/design/model_autonomy_and_harness_intensity.md)：模型自治与门禁强度——结伴/自主两档门禁、模型停机权与编排抽象层(R-322/R-323,D-661/D-662;2026-08-21 外部七点评估的逐点定调)。
- [identity: live_design; last_verified_commit: d374cb9f] [`phase2_system_upgrade.md`](../../../docs/design/phase2_system_upgrade.md)：自举二期 research/memory/运行体验/动画/voice 的依赖、波次、Go/No-Go 与联合验收总纲(R-283)。
- [identity: live_design; last_verified_commit: f6b57f9b] [`auto_research.md`](../../../docs/design/auto_research.md)：AUTO research 完整流程：调研地图、用户选题、MVP、本机/SSH 环境、完整实验、分析与论文 PDF；两例真实 GPU 验收（R-363，当前工作树增量）。
- [identity: live_design; last_verified_commit: 1ebbb218] [`research_experiment_runner.md`](../../../docs/design/research_experiment_runner.md)：Research 实验运行与路线图的字段与 Markdown 格式冻结(两层模型、@@kanzei 回调、本机+SSH、环境策略分档与路线图投影;A-014~A-019,R-343~R-348 承接)。
- [identity: live_design; last_verified_commit: d374cb9f] [`research_mode.md`](../../../docs/design/research_mode.md)：研究模式设计基线草案(2026-08-12 八维度审计维度 8 产出；八个定调点待用户确认,R-221 承接)。
- [identity: live_design; last_verified_commit: dbafb50f] [`run_metrics_task_granularity.md`](../../../docs/design/run_metrics_task_granularity.md)：R-337 运行画像按执行任务关闭粒度的审计与设计草案；B1 已完成现状证据，B2 任务级方案待评审。
- [identity: live_design; last_verified_commit: e3d77ea4] [`run_metrics_task_migration.md`](../../../docs/design/run_metrics_task_migration.md)：R-339 历史 task 兼容、legacy/未归属对账、旧 API 过渡与 SQLite 备份回滚边界。
- [identity: live_design; last_verified_commit: d374cb9f] [`readme.md`](../../../docs/design/readme.md)：docs/design 的记录规范与文档模板；定义设计正文最小结构和方案变更规则。
- [identity: live_design; last_verified_commit: d374cb9f] [`session_state_and_line_runtime.md`](../../../docs/design/session_state_and_line_runtime.md)：会话状态与线路运行态设计(状态持久化、恢复与并发线路隔离)。
- [identity: live_design; last_verified_commit: d374cb9f] [`subagent_management.md`](../../../docs/design/subagent_management.md)：子代理管理四层方案(R-058 done,策略层未实施)。
- [identity: live_design; last_verified_commit: d374cb9f] [`weakness_register_20260820.md`](../../../docs/design/weakness_register_20260820.md)：弱点登记与 Agent 减负方向(2026-08-20 两轮外部评估对照；R-310~R-313、D-575/D-577/D-578,§六 为需求发现实测复核,减负方案待 R-312 勘察后评审)。
- [identity: live_design; last_verified_commit: 7823d6f6] [`tracker_evidence_ledger.md`](../../../docs/design/tracker_evidence_ledger.md)：tracker 状态可信化——改动面账本、意图态/交付态两轴、门禁判据换成本条目改动面、调度只提示不封禁(D-736 已落地 §3.4)。
- [identity: live_design; last_verified_commit: c3222943] [`work_unit_foundation.md`](../../../docs/design/work_unit_foundation.md)：Work Unit 底座——Outcome/执行状态/历史三分离的事件存储、投影与迁移契约(R-317)。

## validated_design

- [identity: validated_design; last_verified_commit: 5c9e1df] [`architecture_browser.md`](../../../docs/design/architecture_browser.md)：可视化架构浏览与记忆设置——技术栈选型评估(R-122 done,方案 A:既有 classic script + 目录树复用)。
- [identity: validated_design; last_verified_commit: c0ea88d] [`ci_release_evidence_chain.md`](../../../docs/design/ci_release_evidence_chain.md)：CI 与发布证据链——本地门禁 + commit 锚定(R-152/R-146/R-156/R-298 done)。
- [identity: validated_design; last_verified_commit: e791536] [`continue_prompt_dissection.md`](../../../docs/design/continue_prompt_dissection.md)：继续文案拆解与鞭挞引擎化——实施前拆解与 R-128/R-157/R-169/R-170 交付映射。
- [identity: validated_design; last_verified_commit: e791536] [`deep_parallel_dev.md`](../../../docs/design/deep_parallel_dev.md)：任务级并行基线——一线一 worktree、diff/合并/恢复与模型隔离(R-177/R-178/R-179/R-182 done)。
- [identity: live_design; last_verified_commit: 3ce8805b] [`chat_presentation_contract.md`](../../../docs/design/chat_presentation_contract.md)：主对话区分层契约——正文/轨迹/后台三层,工具一行可展开、轮间留白、单行思考不成块;附 state.db 复核的正文与轨迹字节比(R-350~R-352,D-725);§4.4 单列与工具组——列宽唯一真源 --chat-col、连续工具调用合成一行、失败常驻、ui-column-layout-smoke 浏览器量边(UI2-0926 #12)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`interaction_modes.md`](../../../docs/design/interaction_modes.md)：双人格与对话为主布局设计(R-036 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`m2_sqlite_store.md`](../../../docs/design/m2_sqlite_store.md)：SQLite 会话存储 Schema v1(R-003 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`memory_decision_sufficiency.md`](../../../docs/design/memory_decision_sufficiency.md)：Memory 判据层升级——决策充分性(R-145/R-150 done,含边界拍板与实证修正记录)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`monolith_decomposition.md`](../../../docs/design/monolith_decomposition.md)：巨石拆解方案——app/main.rs、ui/main.js、core/runner.rs、core/store.rs 分文件拆解(R-153~R-156,A-008)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`monolith_decomposition_round2.md`](../../../docs/design/monolith_decomposition_round2.md)：第二轮巨石拆解计划(R-253~R-258 的符号级批次地基)。
- [identity: validated_design; last_verified_commit: 1aed3e83] [`parallel_lines_ui.md`](../../../docs/design/parallel_lines_ui.md)：多线协作可见性——协作上下文、并列线路状态、文件冲突预警及收活流程(R-184/R-185/R-222 done)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`r059_mobile_agent_communication.md`](../../../docs/design/r059_mobile_agent_communication.md)：主代理/子代理消息与通知演进设计(R-059 dropped、R-270/R-271 done，R-288 真机 E3 仍在范围)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`r108_ai_design_decision_records.md`](../../../docs/design/r108_ai_design_decision_records.md)：设计记录规范的真实示例(R-108 done)。
- [identity: validated_design; last_verified_commit: 8ed3256f] [`r310_repo_map_design.md`](../../../docs/design/r310_repo_map_design.md)：代码地图形态与 token 成本对比——symbols 实时按需查询胜出(R-310 B3 done)。
- [identity: validated_design; last_verified_commit: 9d784f21] [`workspace_information_architecture.md`](../../../docs/design/workspace_information_architecture.md)：开发与研究独立空间、课题绑定会话及研究页面；源码、分层回归和完整 verify 已通过，原生桌面关键交互验收仍待执行(R-360/D-742)。
- [identity: validated_design; last_verified_commit: d374cb9f] [`reliability_usability_self_hosting_quality.md`](../../../docs/design/reliability_usability_self_hosting_quality.md)：可靠性、可用性与自举质量不变量、验证证据和 R-317 执行模型权威。
- [identity: validated_design; last_verified_commit: d374cb9f] [`ui_esm_migration.md`](../../../docs/design/ui_esm_migration.md)：前端 ESM 迁移评估(结论:保持有序 classic script,A-008)。

## historical_snapshot

- [identity: historical_snapshot; as_of_commit: 3e510b1] [`audit_20260812_eight_dimensions.md`](../../../docs/design/audit_20260812_eight_dimensions.md)：2026-08-12 八维度审计记录(巨石/记忆/验证/协作等维度的方法论与发现源)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`context_compaction.md`](../../../docs/design/context_compaction.md)：上下文压缩设计(历史基线；当前压缩实现见 runner compaction)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`frontend_phase3.md`](../../../docs/design/frontend_phase3.md)：前端能力差距与需求整理记录(R-031~R-051 系列)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`metrics_baseline.md`](../../../docs/design/metrics_baseline.md)：巨石度量基线快照(R-258 批2,`kz metrics` Top-30 榜单与阈值读数)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`parallel_read_serial_write_orchestration.md`](../../../docs/design/parallel_read_serial_write_orchestration.md)：R-171 阶段编排历史基线；R-182 已取代其实现阶段全串行部分。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`research_mode_prior_art.md`](../../../docs/design/research_mode_prior_art.md)：research 模式先行调研与同类系统对照资料，作为 R-221/R-277 的设计输入。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tier1_handoff_20260811.md`](../../../docs/design/tier1_handoff_20260811.md)：第一梯队交付移交记录(2026-08-11)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tier1_implementation_plan.md`](../../../docs/design/tier1_implementation_plan.md)：第一梯队实施计划(R-001~R-020 等首批条目)。
- [identity: historical_snapshot; as_of_commit: 3e510b1] [`tool_edit_recovery.md`](../../../docs/design/tool_edit_recovery.md)：edit 工具恢复机制设计(匹配失败诊断与恢复策略,M-021/M-022)。

## superseded

- [identity: superseded; as_of_commit: e08eb0a; superseded_by: workspace_information_architecture.md] [`research_workspace.md`](../../../docs/design/research_workspace.md)：原研究工作台设计；工件交互与历史依据保留，当前导航和布局以新空间设计为准。

- [identity: superseded; as_of_commit: 3e510b1; superseded_by: deep_parallel_dev.md] [`r030_process_decoupling.md`](../../../docs/design/r030_process_decoupling.md)：多进程解耦设计(R-030 done)；其 worktree/深并行残余已由 `deep_parallel_dev.md` 接替，正文保留历史决策和兼容边界。
