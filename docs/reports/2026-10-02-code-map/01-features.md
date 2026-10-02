# 当前功能地图

每行是一组实际代码能力。入口列用于定位使用界面，逻辑列说明系统怎样做事；详细规则在对应的后续文档。路径以仓库根为基准，所有文件均可在 [全量文件索引](08-file-index.md) 点击打开。

## 项目和用户工作空间

| 功能 | 入口 | 实际逻辑和代码归属 |
|---|---|---|
| 项目创建与登记 | 设置与项目选择器 | `projects.rs` 创建目录、可选 git init、登记用户偏好；Git 失败作为警告返回，目录创建不因此丢失 |
| 项目切换与隔离 | `03-workspaces.js`、`09-sessions.js` | 按主根和会话身份恢复运行；项目资产读取主根，线内代码操作使用 cwd |
| 项目概览 | `12-docs-pages.js` | `workspace_snapshot` 聚合 tracker、Work Unit、决策、验证和运行状态；跨项目视图由 `12-decision-console.js` 消费 |
| 原始想法 | 文档页 | I 条目直接收件；用户发起拆解子代理生成 R/D；未拆想法不进入取活队列 |
| 需求和缺陷 | `10` 至 `14` 文档模块 | DocStore 解析 Markdown，tracker 管 ID、字段、状态、依赖和终态归档 |
| 测试记录 | 文档测试页、`test_record` | 执行证据、覆盖范围、源码版本和测试状态进入记录，终态归档 |
| 项目规范 | `15-conventions.js` | 读取、用户 CAS 保存、agent 新建或生成建议稿，专用工具定点 patch |
| 架构浏览 | `19-arch.js` | 扫描 Cargo 依赖和手写 Mermaid 图，读取架构索引；受控写入和链接校验 |
| 文件导览 | `17-files.js` | 统一扫描器输出树、大小和用途；用途标注调用 fast 模型 |
| 文件编辑 | `17-files-editor.js` | Monaco 编辑、hash 比对保存、路径边界检查、历史 checkpoint；托管资产转入专门界面 |

## 对话和运行

| 功能 | 入口 | 实际逻辑和代码归属 |
|---|---|---|
| 流式对话 | `08-compose-runtime.js` | `run_prompt → run_task → run_once_with_parts`；事件同步到 UI、事实库和指标 |
| 思考和工具展示 | `05-chat-render.js`、`05-tool-summary.js` | 文本、思考、工具开始/结束与输出分开渲染，保留详情和失败类型 |
| 图片与文档附件 | 输入区、`run/assembly.rs` | 转换为 LLM Part；按 provider 能力投递或明确拒绝 |
| 会话恢复与重置 | `conversation.rs`、`15-views-misc.js` | 默认读 typed 事件投影，按 segment 恢复；保留 legacy 读路径回退开关 |
| 输入排队和 steering | `commands/run.rs`、`store/inbox.rs` | 输入先准入，再按投递模式提升；输入事实和运行状态不靠前端临时数组单独维护 |
| 停止整轮与单子任务 | 停止按钮、`stop_run`、`stop_task` | CancellationToken、运行代数和任务取消注册表分别处理，防旧停止回调终止新轮 |
| 权限请求 | `kz:ask`、`answer_ask` | allow/deny/ask 后等待或按非交互策略处理；“总是允许”写入成功才生效 |
| 多模型和角色模型 | `08-model-picker.js` | 合成线覆盖、agent、项目、全局、默认五层，并显示字段来源 |
| 按需工具加载 | `tool_search` | 部分 schema 延迟加载；搜索选择后加入本轮 catalog，历史工具调用可恢复加载 |
| 子代理派发 | `task`、`05-subagents.js` | 独立快照和上下文、fast/primary 档、后台结果、逐任务取消和 transcript 恢复 |
| 子代理容器 | 无独立容器入口；`agent_directory.rs` | 原模块只保存 JSON 清单，注册/升级/回滚命令和设置按钮已移除；Agent 目录和 Harness 的实际模型/工具快照保留，既有用户文件不迁移 |
| 勘察复核流水线 | `phase_pipeline.rs`、`task` | 固定角色、读写槽和复核屏障作为编排策略；子代理执行、超时与取消统一复用 core/runner/subagent.rs，不再复制运行配置或超时包装 |
| 自主推进 | `auto_run.rs`、`08-auto.js`、`runner/drive/batch.rs` | 后端决定 Continue/Nudge/Verify/Stop/Retry；前端调度下一轮。无限主代理写入后每 32 步或 15 分钟触发运行内收口，验证提交或保存未完成检查点后自动继续 |
| 用户目标与完成交接 | 线路设置、`work handoff` | 完成范围区分事项、批次、请求和目标，核对当前委托后才能停止整个委托 |
| 运行画像和上下文账单 | `13-memory.js`、活动面板 | token/cache/工具耗时/压缩/召回/关闭与失败等事件形成画像 |

## 工作交付和决策

| 功能 | 入口 | 实际逻辑和代码归属 |
|---|---|---|
| 确定性取活 | `work next` | 合成队列模式、WIP、依赖、阻塞、停车、认领人，返回唯一选择及理由 |
| 原子认领 | `work claim` | 文件锁保护取活和认领，避免各线同时拿同一项 |
| Work Unit | 需求工作页、`work` 工具 | 显式开启 `work_units_v1` 的需求拆成执行单元；事件构建当前投影 |
| Checkpoint 和证据 | `work checkpoint/evidence/verify` | 保存范围、下一步、版本观察和逐项验收证据；完成与验证分开 |
| 后台验证 | `bash.verification` | 冻结源码、独立 worker、共享资源排队，绑定验收原文和源码指纹 |
| 验证完成唤醒 | `verification_monitor.rs` | 用 Work Unit 所属线映射原会话，写稳定系统输入，唤醒空闲线 |
| 用户验收交付 | 决策台 | `work_delivery_accept` 绑定 unit 和 source sequence，记录用户复核事实 |
| 自主决策 | 自动模式 `question` | 模型提交简短决定、理由、影响、偏好引用；缺外部事实则记 NeedsInput |
| 决策接受与纠正 | `12-decision-console.js` | revision 校验、保存复核；纠正原会话入队，默认一次性 |
| 项目与全局偏好 | 决策复核范围选项 | 仅显式选择长期范围时同步 preference；保留同问题合并和旧复核拒绝覆盖 |
| 交付附件 | `deliver` | 项目路径校验后产生文件/图像交付卡；用户可直接预览 |
| 执行事故 | `incident` | 追加过程事实，和正式缺陷分开，不自动分配 D 编号 |

## 记忆和长期上下文

| 功能 | 入口 | 实际逻辑和代码归属 |
|---|---|---|
| 记忆真源 | 记忆页、MemoryStore | 一条 Markdown 文件一条记忆，frontmatter 管元数据，索引可重建 |
| 分级检索 | `memory_search` | 指纹精确、FTS5/BM25 和可选 dense 通道；混合结果用 RRF |
| 常驻记忆与开跑召回 | Profile、运行装配 | 注入偏好和有预算的索引；以当前输入或当前工作标题召回 |
| 失败触发召回 | runner/recall.rs | 根据可用失败分类/指纹召回修复经验，保留调用和采纳观测 |
| 草稿收件箱 | `memory_note` | 主模型投递草稿，manager 决定 add/update/merge/stale/discard |
| 机械纠错 | `memory_note action=correct` | 主模型仅能定点修正既有 title/description/body；不因此获得任意生命周期写权 |
| 记忆整理和晋升 | memory-manager | 受控准入、重复/subject 检查、候选 reconciliation、归档与账本 |
| 记忆专用对话 | `18-optional-ui.js` → `13-memory-chat.js` | 首次打开才加载，继续复用 memory manager 的既有工具；加载失败可重试，切换项目后不重放旧选择 |
| 记忆图谱 | `18-optional-ui.js` → `24-memory-graph.js` | 首次切到图谱才加载；从记忆、tracker、refs 和区域信号生成只读图，补入决策偏好溯源 |
| 主动压缩和 overflow 恢复 | `runner/compaction.rs`、`runner/drive/context_budget.rs` | 先管理预算，再摘要/裁剪/归档旧工具结果；切点移动到完整工具配对边界，无合法切点时保留历史和重试机会；超限时走应急恢复 |
| 条目切换上下文整理 | runner/item_context | 识别成功认领边界，只归档较旧大观察，持久化确认后才替换请求表面 |
| 回放评估 | `kz replay-eval` | 六臂检索策略、真实 LLM replay 和指标；不执行历史外部工具 |
| 存储占用与清理 | `kz artifacts/quarantine/shadow` | 计划、确认、备份、清理与投影差异诊断分别处理 |

## 研究

| 功能 | 入口 | 实际逻辑和代码归属 |
|---|---|---|
| 研究库与 topic | 研究空间 | 全局 library 登记身份，topic 保持原存储目录；可绑定研究会话 |
| 先行方案对照 | `prior_art` | 创建受控工件、统计搜索事实、验证双方方案和引用；复用现有研究基础设施 |
| 研究计划与审批 | `research_plan`、`research_control.rs` | 旧课题保留计划树与审批；有 AUTO 时计划入口只读并投影主预算，审批和推进由主流程控制 |
| 检索阅读反思 | `research_loop`、`research_control.rs` | 任务、证据和反思可恢复；AUTO 的阶段、暂停与预算统一约束旧入口和外部检索任务，旧课题仍需批准计划 |
| 来源与发现 | `source/finding` | topic Markdown；finding 引用有效 source，避免把无来源结论直接归档 |
| 全文与统一搜索 | `research_index` | topic Tantivy 索引，重新索引和断点恢复 |
| 引用核验 | `research_verify` | source 全文和代码 file:line@hash 核验，预算和未核验状态明确保留 |
| 分节写作 | `research_write`、`latex_tool/paper.rs` | 复用 AUTO 论文目录和编译服务；AUTO 写作受阶段限制，编译和修复回到主流程数值核验；旧课题保留分节兼容路径 |
| 探索路线图 | `19-research.js`、core/research | E 探索 Markdown 真源，下挂结果，图是投影 |
| 本机和 SSH 实验 | `research_runner` | 使用登记环境、policy/lease、callback、heartbeat、日志和 terminal 事实 |
| AUTO 研究流程 | `research_workflow`、`research_control.rs` | 调研→地图→用户选题→MVP→完整实验→分析→论文→检查→实际编译；阶段、暂停、预算和恢复信息为研究主控制面 |
| 环境准备 | research_workflow/compute | 准备 Python 环境，保留准备快照和运行路径；新路线需登记环境和预算 |
| 论文与数值证据表 | `research_workflow/paper.rs`、`latex_tool/paper.rs` | 绑定 result/metric/value；分节与整体写作共用工件，数值和引用核验后才编译，失败修复必须重新核验 |
| 科研绘图与调色板 | `plot`、`plot_tool/adapters.rs`、`21-palette.js` | 默认 Vega-Lite；PGFPlots/matplotlib 只在显式选择对应 engine 时加载执行；共用调色板和产物合同 |
| LaTeX 模板与编译 | research_latex、`latex`、`latex_tool/paper.rs` | topic 模板和受控 workdir；研究工具、AUTO 与桌面共用带锁编译，清理旧 PDF，核对 PDF 头/大小与未解析引用；系统 TeX 优先并支持 Tectonic |

## 桌面服务和工程交付

| 功能 | 入口 | 实际逻辑和代码归属 |
|---|---|---|
| 并行开发线 | `20-lines.js`、processes | 独立 worktree、分支、模型与运行目录；共享主根项目资产 |
| 协作和冲突预警 | collaboration、worktree | 共享线状态、文件集合交集、Git 文本冲突预检、no-ff 合并 |
| 结构化 Git 交付 | `git` | 逐文件 stage、版本/CAS、覆盖计划、fmt/clippy/测试和 finalize |
| 前端实查 | ui_dom/ui_console/ui_style/ui_screenshot | 通过真实 UI 探针和窗口像素读取状态，非截图替代物 |
| 应用内网页预览 | `24-preview.js` | Tauri child WebView、进程内 CDP、静态服务、控制台和设备尺寸 |
| 无头浏览器 | `browser` | Node/Playwright 辅进程，统一动作 schema；面板不可用时走无头 |
| Mobile PWA | `16-mobile.js`、mobile-pwa | 移动设置首次展开才加载；服务默认不启动，LAN 配对/token、消息、审批和通知游标保留；电脑仍是运行主机 |
| 手机系统通知 | mobile_notify、移动设置 | 独立勾选 KDE Connect 通知，启用前检查依赖；桥停止时关闭通知，普通桌面事件不再反复探测 CLI |
| 本地语音 | `18-optional-ui.js` → `23-voice.js` | 首次检查或开启才加载界面与运行时；loopback ASR/TTS 按会话拥有请求，关闭后停止轮询；安装与下载仍是显式操作 |
| 伴随视觉和状态动画 | `22-neural-flow.js` → `22-visual-runtime.js` | 轻量状态文字常驻；复杂视觉统一开关且默认关闭，开启才加载渲染器，关闭销毁循环与监听；素材制作在 extras/visual-authoring |
| 中英文与表面组件 | `02-i18n.js`、`00-surface.js` | UI 翻译、对话框、toast、布局与偏好保存 |
| 更新与安装 | update、release/package 脚本 | Release 检查、下载、pending 接力、唯一安装位置与 bundled CLI 同步 |
| 验证与 CI | verify-policy、verify.ps1、ci.yml | 按改动面分类、产生绑定 HEAD 的验证证据；当前 CI 是手动触发 |

## 明确分开的状态

正式源码已接入不代表本机当前安装包已包含。`docs/prototypes/workbench`、`docs/prototypes/softwire` 是交互原型；总体 Softwire 引擎改造仍在设计文档中。决策台和后台验证已经有实际源码接线，不能再统称为原型，也不能用原型证明总体重构完成。
