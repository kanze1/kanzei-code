# Requirements Archive

## R-001 harness 双模式 dev/research profile [done]

## R-002 Tauri 桌面端(类 VSCode 布局) [dropped]

## R-004 本地模型跑并行子代理(M4) [done]

## R-005 桌面端基础件:多项目管理/运行状态/设置页 [done]

## R-006 桌面端 UI 美化(用户反馈:现在有点丑) [done]

## R-008 自举:用 kanzei 开发 kanzei [dropped]
- 备注: 自举是持续工作方式而非可完结需求,由长期目标 G-001 承载;并入后关闭

## R-009 对话历史记录持久化 [dropped]
- 备注: 与 R-003 是同一事件日志/投影,并入 R-003 一并交付;关联缺陷 D-008 已修复

## R-011 Agent 通用工具能力对齐 Codex 与 Claude Code [dropped]
- 备注: 伞形需求,已由 2026-08-06 工具审计具体化:检索=R-026、子代理=R-012、todo=R-028、question=R-029、websearch=R-023、多模态=R-014/R-024;不再单独追踪

## R-012 将子Agent调度能力开放给主Agent [done]
- 实现: task 工具 + 只读 explore 子代理(read/glob/grep),同轮多 task 并行,fast/primary 双档位,E2E 验证通过

## R-015 对话全状态显示(diff/终端块/轮次/思考块/markdown/git 状态) [done]

## R-017 终端命令执行不弹出黑色控制台窗口 [done]

## R-019 支持设定目标并持久化长期工作 [done]

## R-020 编辑 diff 默认收纳并显示改变量摘要 [done]

## R-021 上下文自动压缩:超阈值自动总结并延续对话,压缩不丢数据 [done]

## R-022 LLM 请求瞬断自动重试(流未建立时退避重试) [done]

## R-026 glob/grep 检索工具(ripgrep 内核,head-limit 早停) [done]

## R-003 SQLite 事件溯源 + steer/queue 调度(M2) [done]
- 范围: SQLite 事件溯源(state.db)、steer/queue 双投递、运行中 queue drain、事件恢复消息历史;对话历史持久化(原 R-009)一并交付
- 已完成: SessionStore + 迁移、prompt admitted/promoted 事件、会话状态生命周期(running/idle/failed)、CLI/桌面端接入、事件恢复第一阶段、steer 输入入口与优先调度、运行中队列调度
- 下一步: 运行中 queue admission/drain 收尾,使 pending 输入在当前任务结束后自动提升执行
- 文档: docs/design/m2-sqlite-store.md
- refs: R-013 D-010
- 最新提交: 91d3f2b
- 进展: 已完成 queue/steer drain 的关键修复：promote_next_input 逐条 FIFO 提升 steer，避免后续 steer 丢失；新增连续 steer→queue 回归测试。cargo test --workspace 全部通过。剩余运行中 admission/drain 竞态与端到端覆盖。
- 完成说明: 已完成 SQLite 事件溯源、steer/queue admission 与桌面端运行中 drain；修复多个 steer 逐条 FIFO 提升问题，并通过 lifecycle 锁消除 queue admission 与 drain 收尾竞态。相关回归测试、M2 调度文档已更新，cargo test --workspace 全部通过。
- 验收: 运行结束边界提交的输入不会因 worker 在最后检查后直接退出而遗留 pending；steer/queue 按既定优先级与 FIFO 逐条提升。

## R-010 需求与缺陷分级及可编辑管理 [done]
- 范围: 需求/缺陷等级与字段编辑、按等级筛选排序、非法修改拒绝并提示
- 已完成: 侧边栏展开编辑、状态流转按钮、缺陷 severity 展示
- 剩余: 需求优先级枚举、列表筛选/排序
- 完成说明: 需求条目新增 P0/P1/P2/P3 优先级枚举并由 tracker 硬门禁校验；桌面端 docs_snapshot 透传优先级，支持需求按状态/优先级筛选及按优先级、状态、编号排序；编辑入口支持保存优先级。cargo test --workspace 与 node --check crates/kanzei-app/ui/main.js 全部通过。
- 验收: 非法优先级被拒绝并提示合法值；需求列表可按状态和优先级筛选，并按优先级、状态、编号排序；优先级持久化在 requirements.md 的“优先级”字段。

## R-013 支持回到之前的对话 [done]
- 范围: 会话列表、历史会话加载与继续对话(R-003 落地后的 UI 层)
- 验收: 历史列表可见；任意条目可加载消息并同步后端上下文；加载失败有可见错误；继续发送使用选中的历史上下文。
- refs: R-003
- 完成说明: 桌面端新增历史对话列表；用户可查看当前项目的持久化 conversation.updated 快照，打开任意历史快照恢复消息上下文并继续对话。启动、项目切换和新对话边界均已同步；cargo test --workspace 与 node --check 通过。

## R-007 复用订阅额度:Claude Code(OAuth)/Codex 凭证当 provider [done]
- 已完成: Codex 凭证(auth.json 刷新回写、Responses 协议、gpt-5.6 三兄弟)
- 剩余: Claude Code OAuth provider(~/.claude/.credentials.json)
- 进展: 已完成 Claude OAuth token 自动刷新：接入 console.anthropic.com/v1/oauth/token，使用 Claude Code client_id 与 refresh_token，在过期前 5 分钟刷新并回写 accessToken/refreshToken/expiresAt；构建请求继续复用 Anthropic OAuth headers。cargo test --workspace 全部通过。剩余真实 Claude Code 端到端验证。

## R-014 多模态模型支持上传图片和 PDF 等文件 [done]
- 已完成: 三协议的 image/document 消息映射(协议层就绪)
- 剩余: 前端上传/粘贴入口(与 R-024 一起做)
- refs: R-024
- 进展: 已完成桌面端多模态入口：支持文件选择、拖拽和剪贴板粘贴图片/PDF；附件以 base64 传入 runner，映射为 Image/Document Part；纯文本路径保持兼容。运行中排队附件明确报错，避免持久化 admission 丢失附件。已通过 node --check 与 cargo test --workspace。

## R-027 需求分析沟通模式与缺陷查找入口 [dropped]
- 范围: 需求澄清/边界/验收的结构化沟通模式;缺陷查找按钮与状态展示
- 验收: 具体交互方案与权限边界在实现前补充确认

## R-036 双状态 agent:自主推进(backlog驱动/连跑)与结伴开发(Claude式对话协作) [done]
- priority: P0
- 归属: kanzei
- 设计: docs/design/interaction-modes.md(含 pair 人格系统提示词草案,可直接用)
- 验收: 模式人格分离、显式 agent 路由、连跑边界均已落地。
- 进展: 已完成首个闭环：新增 dev-pair agent；桌面端默认提供结伴开发/自主推进/research 模式并把 agent 参数传入 run_prompt→select_agent；连跑仅允许自主推进，切换模式会关闭。已通过 cargo test --workspace 与 node --check。

## R-033 阅读体验:智能滚动跟随+回到最新按钮、消息一键复制、对话内搜索 [done]
- priority: P0
- 归属: kanzei
- 验收: 滚动跟随、回到最新、复制和对话搜索均已落地并接入实时消息与历史回放。
- 进展: 已完成阅读体验闭环：消息区智能跟随（用户上滚时暂停）、回到最新按钮、消息/工具一键复制、对话内搜索及上下匹配跳转。已通过 cargo test --workspace 与 node --check。

## R-023 research 模式补 websearch 工具 [done]
- priority: P2
- 归属: kanzei
- 已完成: webfetch(走代理、输出截断)
- 剩余: websearch 检索入口,结果可直接 source add
- 进展: 已完成 research 专属 websearch 工具：复用代理配置，调用 DuckDuckGo HTML 搜索，返回有界的 query/results/truncated JSON（title/url/snippet），限制查询长度、结果数和响应体，非 2xx/网络错误明确返回错误；新增解析单测。已通过 cargo test --workspace。
- 验收: research profile 已具备可调用且受权限控制的 websearch，结果结构化并有硬上限。

## R-029 question 工具:agent 结构化向用户提问(带选项),复用 ask 弹窗通道 [done]
- priority: P1
- 归属: kanzei
- refs: R-036
- 备注: 结伴开发人格的"拿不准就问"依赖此工具
- 进展: 已完成 question 工具闭环：新增结构化 question schema（question/options/default），runner 新增 AskRequest/AskResponse 并将答案作为工具结果回喂；桌面端复用 ask 队列和弹窗，支持选项、文本输入、取消，自动放行不跳过 question；CLI 支持终端输入。已通过 cargo test --workspace 与 node --check。
- 验收: agent 可通过 question 向用户发起带选项的结构化提问，桌面端和 CLI 均可回答，取消不会永久阻塞。

## R-028 todo 工具:运行内任务清单,长连跑会话的结构化计划 + 前端可视化 [done]
- priority: P1
- 归属: kanzei
- 进展: 已完成 todo 工具闭环：dev profile 新增 todowrite，整体替换当前运行计划，状态限定 todo/doing/done/dropped，最多 30 项并校验空字段；通过 ToolOutput.display.kind=todo 向桌面端发送结构化列表，右侧当前计划面板显示条目和完成比例。计划不写入项目 backlog。已通过 cargo test --workspace 与 node --check。
- 验收: 运行内结构化计划可由 agent 更新，桌面端实时可视化状态与完成比例，数据有界且不污染跨会话项目文档。

## R-016 kzapp 启动时自动完成 pending 自更新 [done]
- priority: P1
- 归属: kanzei
- 范围: 启动检测 kzapp.exe.pending 并自替换,发版后重启即新版
- 进展: 已完成 kzapp pending 自更新闭环：启动早期检测同目录 pending，派生独立 helper 等待旧进程释放文件锁，原子重命名旧版本为 previous、替换 pending、启动新版本，失败自动回滚；release.ps1 安装失败时改为提示下次启动自动更新，直接安装成功会清理旧 pending。新增 pending 路径单测。已通过 cargo test --workspace 与 node --check。
- 验收: release 生成的 kzapp.exe.pending 可在下一次启动自动替换，无需重新运行 release.ps1；替换失败保留旧版本并回滚。

## R-037 对话为主布局:主区只留对话与思考,工具活动收束到右侧活动面板 [done]
- priority: P0
- 归属: Claude
- 设计: docs/design/interaction-modes.md 附录
- 验收: 主区只保留用户消息/assistant 文本/思考头/轮次分隔,工具降为一行痕迹;右侧活动面板按序列出全部工具调用,详情(diff/终端)面板内展开
- 备注: 与 R-030 页签共用渲染状态重构,一起做

## R-038 需求列表按优先级着色(P0红/P1黄/P2蓝/P3灰,色条+徽标) [done]

## R-032 队列可视化:排队输入列表(内容/交付方式)+ 单条撤销 [done]
- priority: P1
- 归属: kanzei
- 验收: 运行中的 queue 输入显示内容与交付方式,支持单条撤销,状态与后端 admission 同步
- 进展: 开始检查后端 admission 队列事件、停止/取消接口和桌面端运行状态，优先落地队列可视化与单条撤销的最小闭环。
- 实现: 新增 list_pending_inputs/cancel_input Tauri command;前端新增排队输入面板、delivery 标识、单条撤销与生命周期刷新。
- 验收结果: 队列可视化与单条撤销完成;存储层只能取消 pending 输入,调度语义不变。
- 验证: cargo test --workspace; cargo check -p kanzei-app; node --check crates/kanzei-app/ui/main.js

## R-044 右侧活动面板保持稳定,禁止事件驱动自动开关 [done]
- 优先级: P0
- 内容: 右侧活动面板当前会因工具运行、事件刷新或视图变化出现自动打开/自动关闭，导致活动轨迹不稳定、用户无法持续观察。用户主动开启后，面板应保持开启，不得被事件流程擅自收起；关闭也必须由用户主动操作。
- 来源: 用户反馈:右侧活动开启后不要一会开一会关
- 验收: 1.用户打开右侧活动面板后,连续触发多个工具调用、完成运行、切换对话/视图,面板始终保持打开; 2.面板内容刷新不改变开关状态; 3.只有用户点击关闭/收起操作才关闭; 4.用户主动关闭后,后续普通工具事件不得自动打开; 5.重启后的默认状态按产品设定稳定,不出现闪烁或事件驱动的开关跳变。
- 优先级: P0
- 实现计划: 将右侧活动面板改为持久化用户开关,不再由 bg-list 内容或工具事件自动切换;新增顶栏活动开关,打开后保持显示,关闭后事件不得自动打开。
- 验收准备: 覆盖工具开始/结束、turn 清理、停止、项目切换和重启状态。
- 实现: 新增持久化的右侧活动面板用户开关与顶栏“活动”按钮;bgSync仅同步用户开关,工具开始/结束、turn清理、停止和项目切换不再擅自开关;活动列表增加滚动高度约束。
- 验收结果: 用户打开后事件刷新、内容清空、运行结束和视图变化均保持打开;用户关闭后工具事件不会自动打开;重启从localStorage恢复状态。
- 验证: cargo test --workspace; cargo check -p kanzei-app; node --check crates/kanzei-app/ui/main.js

## R-043 项目初始化与项目切换管理 [done]
- 优先级: P1
- 内容: 当前桌面端缺少完整的项目生命周期入口:首次使用时无法明确初始化项目配置/工作区,已配置多个项目时缺少项目切换入口,项目列表缺少新增、移除、重命名、路径查看与当前项目状态管理。需要建立项目管理 UI,并保证切换后对话、配置、运行上下文不会误串。
- 来源: 用户反馈:项目初始化,项目切换与管理能力几乎没有
- 验收: 1.首次启动或无项目时有明确的项目初始化引导,可选择/创建工作目录并完成基础配置; 2.顶栏或侧栏可查看当前项目并切换已登记项目; 3.支持新增、移除、重命名项目及查看项目路径,移除需确认且不删除磁盘文件; 4.切换项目后刷新项目相关文档、目标/需求、历史会话和运行状态,禁止跨项目串数据; 5.项目列表与当前项目选择重启后保持。
- 优先级: P1
- 实现计划: 在现有项目路径列表基础上增加向后兼容的名称映射;提供初始化目录命令与 UI 入口;支持项目重命名并持久化;切换/初始化后统一刷新会话、文档、模型、Git、队列和右侧面板状态。
- 验收准备: 覆盖空项目/初始化、切换、重命名、移除确认、重启读取与项目数据刷新。
- 实现: AppPrefs 增加向后兼容的 names 映射;新增 projects_init/projects_rename command;初始化创建项目目录与 .kanzei 配置目录;项目列表新增初始化和重命名入口,移除增加确认;切换/新增/初始化/移除当前项目刷新会话、文档、模型、Git、队列和面板。
- 验收结果: 初始化、切换、显示名重命名、路径查看、移除确认且不删磁盘、重启持久化及项目上下文刷新均已覆盖。
- 验证: cargo test --workspace; cargo check -p kanzei-app; node --check crates/kanzei-app/ui/main.js

## R-031 子代理轨迹透视:task 块可展开子代理完整工具轨迹,后台面板历史可回看 [done]
- priority: P1
- 归属: kanzei
- 验收: task 块可展开查看子代理完整工具轨迹,后台面板条目可回看,不因短时超时消失
- 实现计划: 先扩展 RunEvent::TaskProgress 携带子代理工具结构化 start/end 轨迹;桌面端 task 条目可展开子工具详情;活动面板跨轮保留历史,不再在每轮开始清空。之后补历史会话持久化回放。
- 验收准备: 实时 task 轨迹可展开,子工具顺序/状态/预览可见,活动面板跨轮不因短时事件清空。
- 进展: 已完成实时阶段:RunEvent::TaskProgress 携带子代理工具 start/end 结构化轨迹;task 条目可展开子工具名称、状态、预览和 display;活动面板跨轮保留历史,不再因每轮开始清空。剩余:将轨迹持久化并在历史会话回放中恢复。
- 验证: cargo test --workspace; cargo check -p kanzei-app; node --check crates/kanzei-app/ui/main.js
- 实现: TaskProgress 携带子代理工具 start/end 结构化轨迹;task 块可展开子工具名称、状态、预览和 display;活动面板跨轮保留;运行轨迹写入 run.trace 事件;新增 conversation_trace_get,历史加载按对话分段恢复 task/子工具轨迹。
- 验收结果: 实时与历史回放均可查看 task 内完整子工具轨迹,短时事件不会导致活动面板历史消失,新对话不会串入旧轨迹。

## R-039 权限弹窗队列化 + 连跑控制增强(暂停/跑完本轮停/上限可配) [done]
- 内容: 多 ask 排队时一次只显示一个弹窗,看不到「还有几条待确认」;需要队列感(计数角标/列表)。连跑控制:上限写死 10、无暂停、无「跑完这轮就停」按钮。来源: docs/design/frontend-phase3.md §二.3/§二.4
- 来源: docs/design/frontend-phase3.md
- 验收: 权限弹窗显示排队数与下一条预览;连跑支持暂停/继续与「本轮后停止」
- 优先级: P2
- 实现计划: 补充 ask 当前/总数与待处理预览;连跑增加暂停/继续、本轮后停止、1-100 上限并持久化;增加 autoContinueTimer 与停止代次校验,防止停止后旧 timer 重启。
- 验收准备: 覆盖多 ask 排队显示、连跑暂停恢复、本轮后停止、上限修改、停止后不自动重启。
- 实现: 权限弹窗增加当前/总数与待处理预览;连跑新增暂停/恢复、本轮后停止、1-100 可配置上限并持久化;新增自动续跑 timer 取消与 generation 校验,停止/错误/关闭连跑不会被旧 timer 重启。
- 验收结果: 多 ask 排队有队列感;连跑可暂停/恢复、跑完本轮停止、上限可改;停止后不会因旧 timer 自动再次启动。
- 验证: cargo test --workspace; cargo check -p kanzei-app; node --check crates/kanzei-app/ui/main.js

## R-035 diff 查看器升级:语法高亮、并排视图、多文件改动汇总 [done]
- priority: P3
- 归属: kanzei
- 实现计划: 扩展 write/edit diff payload 增加 language 与结构化 lines,保留旧 diff 字段兼容;前端统一 diff renderer 支持 unified/split 切换、行号和常见语言轻量高亮;活动面板聚合当前会话多文件 additions/deletions。
- 验收准备: 单文件 diff 可切换统一/并排并显示路径/统计/行号;常见扩展名有基础高亮;同一运行多文件改动有汇总。

## R-052 发行版安装包与应用内更新:NSIS setup.exe(scripts/package.ps1),GitHub Releases 为更新源,设置页检查更新/下载安装 [done]
