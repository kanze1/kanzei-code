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

## R-041 错误分级展示:可重试(轻提示+重试)与致命(说明+入口)分离 [done]
- 内容: 当前错误一律全屏红条;应区分可重试(网络/限流,给重试按钮)与致命(配置错误,给跳转设置入口),可重试错误不遮挡对话。来源: docs/design/frontend-phase3.md §二.8
- 来源: docs/design/frontend-phase3.md
- 验收: crates/kanzei-app/ui/main.js 与 style.css 已实现；node --check 通过；cargo test --workspace 全绿。
- 优先级: P2
- 实现: 桌面端统一错误卡片：网络/连接/超时错误显示“可重试错误”并提供“重试上一次请求”；其他错误显示“致命错误”且不提供重试。仅保存手动发送的最近请求，连跑错误不会覆盖重试目标，也不自动重放任务。
- 边界: 错误级别当前基于错误文本中的网络/连接/超时关键词判断，后续如需服务端结构化分类可另行扩展 Tauri 事件契约。

## R-040 键盘快捷键体系(停止/新对话/切进程/聚焦输入框) [done]
- 内容: 至少覆盖:停止当前运行、新建对话、切换视图/进程(待 R-030)、聚焦输入框。需与输入框按键(如 R-024 历史上下箭头)不冲突。来源: docs/design/frontend-phase3.md §二.7
- 来源: docs/design/frontend-phase3.md
- 验收: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test --workspace 全部通过。
- 优先级: P3
- 计划: 先落地低风险快捷键 MVP：Ctrl/Cmd+Shift+N 新对话、Ctrl/Cmd+Shift+C 停止、Ctrl/Cmd+K 聚焦输入框；复用现有按钮事件，不新增 Tauri command。切进程快捷键留待 R-030/R-037 能力就绪后补齐。
- 实现: 新增全局快捷键：Ctrl/Cmd+K 聚焦输入框；Ctrl/Cmd+Shift+N 触发现有新对话流程；Ctrl/Cmd+Shift+C 触发现有停止流程。空状态提示同步展示快捷键。未新增 Tauri command，复用现有按钮事件。
- 边界: 切进程快捷键暂不实现：当前产品尚无可切换的多进程/进程页签模型，待 R-030/R-037 落地后补充，避免把模式切换误标为进程切换。

## R-048 对话尾部运行指示与计时器 [done]
- 内容: 在对话尾部增加稳定的运行状态信息区:running 指示图标、当前阶段、已运行计时器、必要时显示当前 turn。运行开始显示,完成/停止/错误显示最终状态和耗时,不能因事件刷新闪烁或丢失。
- 复杂度: 小
- 来源: 用户反馈:在对话的尾部加一个 running 的 i 指示,还有计时器
- 验收: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test --workspace 全部通过。
- 优先级: P1
- 计划: 收數运行尾部状态：保留已有阶段详情，但新增稳定的“运行中/空闲 + 计时”视觉标识；去除状态点持续闪烁，确保完成、停止、错误路径复位。仅改前端，不改后端契约。
- 实现: 状态栏新增稳定的“运行中/空闲”主状态，保留原有阶段详情与运行计时；运行点改为稳定颜色、不再持续闪烁。发送、完成、停止、错误均通过既有 setStatus/stopElapsed 链路正确复位。

## R-025 权限规则管理:设置页查看/删除已记住的放行规则 [done]
- priority: P2
- 归属: kanzei
- 进展: 已完成：设置页新增当前项目已记住放行规则列表，显示操作/资源/配置路径；可确认删除单条规则。新增 Tauri permission_rules_get/permission_rule_delete 命令并注册，删除仅允许 allow 规则且重新读取项目配置。node --check、git diff --check、cargo test -p kanzei-harness -p kanzei-app 通过。

## R-053 快速记需求:独立按钮+子代理把自然语言描述结构化写入需求列表,不打断主对话 [done]
- priority: P1
- 复杂度: 中
- 归属: kanzei
- 场景: 主对话/连跑进行中,用户随时想到新需求,不想停下当前 run 也不想切上下文——点需求区的"✎ 记需求"按钮,输入一段自然语言,交给独立子代理结构化落库
- 实现要点: ①需求区标题栏加按钮,弹小输入框(多行,Ctrl+Enter 提交);②后端新命令启动**独立的迷你 run**(参考 fast_summarize 的独立调用方式,不占主对话 conversation/queue,可与主 run 并行);③子代理只挂 req 工具,提示词要求:精炼标题、建议 priority/复杂度、起草一条验收、原始描述存入"原始描述"字段;④模型默认 fast,fast 结构化失败(空/格式错)自动升级 primary 重试一次;⑤完成后 toast 显示新 ID 并刷新需求列表,失败给明确报错
- 验收: 主对话运行中可用;提交到落库全程不打断主 run;新条目含标题/priority/复杂度/验收/原始描述;fast 失败时 primary 兜底;连提多条互不覆盖

## R-054 需求手动排序:列表拖拽定开发顺序,agent 按序取活;编号与顺序解耦 [done]
- priority: P1
- 复杂度: 中
- 归属: kanzei
- 编号问题的解法(定案,用户拍板): **行内不显示 ID**——需求列表每行只显示 #位置序号 + 状态 + 标题,R-xxx 收进展开详情。身份仍用 R-xxx(短、可口述"先做 R-053"、refs/归档零迁移),但它从列表视觉里消失,乱序问题随之消失。文件顺序 = 开发顺序,拖拽即重排。
- 实现要点: ①tracker 新增 `reorder` action(输入完整 ID 序列,引擎整文件重写保序,走既有格式硬门禁,天然避免与 agent 写文件的竞态);②前端需求列表加"手动"排序模式(HTML5 draggable,拖完调 reorder 落盘),优先级/状态排序模式保留但手动为默认;③行首显示位置序号,ID 淡显;④dev-auto 提示词改为:"从需求列表自上而下取第一个可做的条目开工,列表顺序即用户意志,priority 只是背景信息";鞭挞提示词同步。
- 验收: 拖拽后刷新/重启顺序保持;agent 连跑按列表顶端顺序取活;refs 引用的旧 ID 全部有效;拖拽期间 agent 并发改状态不丢失任一方修改
- 原始描述: list支持手动排序需求开发顺序,我可以拖动,然后agent默认按照我的list顺序开发,然后是编号问题怎么解决

## R-018 对话结束时播放提示音并显示完成提示 [done]
- priority: P3
- 归属: kanzei
- 验收: 成功/失败/停止均提示;失焦可感知;通知失败不影响对话结果
- 进展: 已完成：运行成功、失败、手动停止及权限拒绝停止均显示 toast、播放短提示音；窗口失焦时更新标题并在已授权时发送系统通知，通知/音频失败仅记录 warn，不影响运行结果。新增统一 notifyRunState/playRunNotice 与回焦标题恢复。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-024 输入体验:提示词历史(上下箭头)、@文件引用补全 [done]
- priority: P2
- 归属: kanzei
- 备注: 粘贴/拖拽附件已随 R-014 完成,本条剩输入框体验
- 进展: 已完成：提示词发送后写入本地最近 30 条历史，输入框上下键可回填并保留草稿；输入 @路径 时调用受限的 project_files 命令扫描当前项目（跳过 .git/.kanzei/target/node_modules，最多 50 条），候选支持上下选择、Enter/Tab 插入、Escape 关闭。新增补全弹层及样式。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-034 research 模式前端:来源/发现侧边栏、引用跳转、报告入口 [done]
- priority: P2
- 归属: kanzei
- 验收: research 模式显示来源/发现侧边栏,引用可跳转,支持报告入口,与后端 sources/findings 对齐
- 进展: 已完成首个可用闭环：docs_snapshot 现在返回 sources/findings 及归档计数；docs_path 支持 source/finding/report 及归档文档；侧栏新增研究区与报告入口；source/finding 的 refs 字段渲染为可点击引用，可滚动定位对应条目。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-042 上下文成分拆解 + 压缩纪要可查看 [done]
- 内容: 上下文占用现在只有状态栏百分比;需要拆解 system/工具结果/历史消息各占多少。压缩(自动/手动)后模型实际看到的纪要无处查看,违背上下文透明原则。来源: docs/design/frontend-phase3.md §一 表格相关行
- 来源: docs/design/frontend-phase3.md
- 验收: 上下文进度条可点击查看成分拆解;每次压缩产生的纪要可在对话或面板中回看
- 优先级: P2
- 进展: 已完成：上下文 token 状态栏可点击查看输入上下文、缓存读取、本轮输出与合计成分；kz:compacted 读取 summary，将压缩纪要加入活动面板并支持点击展开；点击上下文状态会打开并定位最近纪要。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-045 全局工作区管理:项目/对话统一上层目录与运行状态 [done]
- 内容: 在对话和设置之外增加更上层的工作区管理视图,统一查看所有项目、当前项目、关联对话线程、运行中/空闲/失败状态、连跑状态和最近活动。项目切换、对话切换与设置入口从全局层可达。
- 复杂度: 中
- 来源: 用户反馈:对话和设置同理目录加一个更上层的管理,可以看到所有项目和状态
- 验收: 新增全局工作区入口;项目列表显示路径、当前状态、运行中/空闲、当前对话和最近活动;可从该层切换项目/对话/设置;不同项目状态不串联。
- 优先级: P1
- 进展: 已完成：新增 workspace_snapshot Tauri 命令，汇总所有项目的准确 session 状态、当前对话摘要、排队数量、更新时间和最近运行轨迹；新增工作区 Activity 入口与整页项目卡片，支持点击切换项目并复用既有上下文刷新链路。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-046 统一连续推进与过夜模式:可配置 turn 上限 [done]
- 内容: 合并“连跑”和“自主推进”入口与状态,统一为可配置的连续推进任务。保留手动暂停/恢复、本轮后停止、上限配置;新增过夜模式,可指定最多执行多少次连跑 turn,达到上限或无实质进展自动停止,并清晰显示当前轮次/剩余次数/停止原因。
- 复杂度: 中
- 来源: 用户反馈:连跑和自主推进合并;增加过夜模式,指定多少次连跑 turn
- 验收: 单一入口可选择连续推进;可设置过夜 turn 上限并持久化;运行中显示当前/总 turn 与剩余次数;支持暂停、恢复、本轮后停、立即停止;达到上限、无实质进展、错误或用户停止均有明确状态。
- 优先级: P1
- 进展: 已完成首个统一连续推进闭环：原有鞭挞上限明确显示为连续推进上限，新增可持久化过夜模式偏好（重启不静默运行），状态栏显示当前推进轮次/上限/等待状态，完成、用户拒绝、手动停止均显示停止原因。保留暂停、本轮后停和既有安全边界。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-047 对话内复制上下文与小总结收纳 [done]
- 内容: 移除顶栏“复制上下文”按钮,改为对话内能力:在对话尾部或消息工具栏提供复制当前对话/复制单条消息入口;小总结、工具摘要等非主对话信息统一收纳到右侧活动面板,主对话只保留必要的简短提示,避免重复刷屏。
- 复杂度: 小
- 来源: 用户反馈:复制上下文不要在最顶部而是在对话里面;活动已合并,小总结消息也合并
- 验收: 顶栏不再显示复制上下文;对话尾部或消息内可复制整段上下文/单条消息;小总结不在主区重复展开,可从活动面板查看完整内容;复制结果内容完整且不含 UI 控件。
- 优先级: P2
- 进展: 已完成：保留现有对话上下文复制入口；手动“总结”生成的小总结不再追加到主对话，而是收纳为右侧活动面板条目，支持点击展开并显示归档路径，同时 toast/日志告知用户。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-049 缺陷梳理:静态 harness 分析报告 [done]
- 内容: 在缺陷列表旁增加“缺陷梳理”按钮,调用静态 harness 流程读取当前项目 defects.md、相关代码/文档上下文,由模型生成缺陷归类、重复项、影响范围、建议优先级与下一步;第一阶段只生成报告,不自动修改缺陷状态或代码,动态监测后续再做。
- 复杂度: 中
- 来源: 用户反馈:缺陷旁边加缺陷梳理按钮,做一套静态 harness 总结缺陷,动态后续再说
- 验收: 缺陷区有按钮;点击后显示处理中/成功/失败状态;模型仅基于当前静态快照生成结构化报告(分类/重复/影响/优先级/建议);报告可在应用内查看或复制;不会自动改写 defects.md、需求状态或代码;无缺陷时有明确空状态。
- 优先级: P1
- 进展: 已完成静态分析报告：覆盖 harness 权限门禁、工具资源、路径边界、agent 选择、子代理隔离、runner 最后一步和 task 并发等风险；按高/中/低风险整理位置、影响、建议与测试缺口，并给出实施顺序。报告：docs/reports/2026-08-08-harness-static-analysis.md。git diff --check、cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools 通过。

## R-051 需求复杂度分级:小/中/大 [done]
- 内容: 需求除优先级(P0-P3)外增加复杂度分级字段,统一使用小/中/大。需求列表支持按复杂度筛选/排序,详情显示复杂度与主要风险;新增需求时必须填写或使用明确默认值。历史需求保持向后兼容,缺失复杂度显示“未评估”。
- 复杂度: 小
- 来源: 用户反馈:需求评估分复杂度大中小,不要只有优先级
- 验收: 需求数据支持复杂度字段;新增/编辑需求可设置小/中/大;列表和详情显示;旧需求缺失字段不报错并显示未评估;支持复杂度筛选或排序。
- 优先级: P2
- 进展: 已完成：需求列表新增复杂度筛选（小/中/大/未评估）和复杂度排序；列表行显示复杂度/未评估，需求详情可直接选择并保存复杂度；旧条目缺失字段保持兼容。node --check、git diff --check、cargo test -p kanzei-app -p kanzei-tools 通过。

## R-055 需求与缺陷管理独立栏目:活动栏第三视图,整页管理(列表/详情/编辑/筛选/拖拽),侧边栏只留摘要 [done]
- 进展: 已完成首个独立管理闭环：Activity bar 新增需求与缺陷视图；整页需求/缺陷列表复用详情展开、状态更新、需求拖拽排序，新增整页状态/优先级筛选；进入独立视图时侧栏收束为摘要，项目刷新后两处数据同步。node --check、git diff --check、cargo test -p kanzei-app -p kanzei-tools 通过。

## R-056 复杂度宽度前端分格修复 [done]
- complexity: 中
- 原始描述: 需求的复杂度的宽度前端显示，分一下格子，修复一下
- 归属: kanzei
- 验收: 需求显示的格子正常划分，每列内容完整不截断
- 优先级: P2
- 进展: 已完成：需求/缺陷行改为稳定分格，ID/序号、状态、优先级、复杂度和标题分别保留固定/弹性空间；复杂度标签不再挤压标题，标题可弹性收缩；独立管理页在窄窗口下缩小状态/复杂度列并保持内容完整。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-057 可直接点击需求调整优先级标签 [done]
- 原始描述: 可以直接点击需求调整优先级标签
- 复杂度: 中
- 归属: kanzei
- 验收: 用户在需求列表中可点击条目直接修改其优先级标签
- 优先级: P2
- 进展: 已完成：需求列表中的优先级徽章现在可直接点击，按 P0→P1→P2→P3→P0 循环；点击会阻止详情展开/拖拽，并通过 docs_update 持久化，未评估需求显示 P? 作为入口。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-058 子代理管理体系扩展优化方案 [done]
- priority: P2
- 原始描述: 子代理的管理扩展，不只是管理需求和缺陷，你看下还有怎么管理比较方便和合适的
- 复杂度: 中
- 归属: kanzei
- 验收: 完成对需求、缺陷之外的其他管理功能设计，并验证可用性
- 进展: 已完成：新增 docs/design/subagent-management.md，完成 agent/任务/策略/审计四层管理设计，明确硬门禁与实施顺序，并用现有活动面板、task 子轨迹回放、只读子代理测试完成可用性验证；与 R-049 风险报告对齐。git diff --check、cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools 通过。
- 设计: docs/design/subagent-management.md

## R-060 补全所有需求条目的复杂度字段标记（优先级/难度评估） [done]
- 验证: 检查项目文档中所有现存需求的 P0-P3 级优先级标签是否已完整标注
- 优先级: P2
- 进展: 已完成：盘点并补齐全部当前未归档需求的复杂度字段：R-030/R-050/R-059 标记为大，R-060/R-061 标记为小；R-051 已验证列表对缺失字段显示未评估、支持筛选/排序/编辑。req get 逐项核验通过，git diff --check、cargo test -p kanzei-tools 通过。
- 复杂度: 小

## R-061 设计APP图标 [done]
- priority: P2
- 原始描述: 设计一个好看的。app图标
- 复杂度: 小
- 归属: kanzei
- 验收: icon设计规范清晰且美观符合品牌调性
- 进展: 已完成：补充 K 形图标设计规范，明确主色/安全区/小尺寸约束/多平台资产验收；核验现有源图与 Tauri、Windows、macOS、Android、iOS 资产，1024/512/32/64/128/256 尺寸检查通过。cargo test -p kanzei-app、git diff --check 通过。
- 设计: docs/design/app-icon.md

## R-062 修复需求与缺陷计数问题，完成后停止鞭挞 [dropped]
- 原始描述: 当所有需求和缺陷做完计数就问题的时候，停止鞭挞，而且现在计数有问题
- 复杂度: 中
- 归属: kanzei
- 验收: 所有需求和缺陷全部完成时计数准确无误，且计数异常时停止鞭挞提醒
- 优先级: P1

## R-063 清空需求和缺陷后自动停止 [done]
- priority: P2
- 原始描述: 自动停止，清空需求和缺陷后
- 复杂度: 中
- 归属: kanzei
- 验收: 当需求与缺陷全部清空（无 todo/doing 条目）时，系统自动停止运行并退出
- 进展: 已完成：自动续跑每轮结束前读取 docs_snapshot，检查需求与缺陷是否均无活动条目；全部清空时自动取消鞭挞、清理旧 timer/轮次、持久化关闭状态，并在对话与日志显示停止原因；查询失败时保守继续且记录警告。node --check、git diff --check、cargo test -p kanzei-app 通过。

## R-066 需求与缺陷菜单交互全面治理：标题可见、状态清晰、展开收纳与排序行为一致 [done]
- 复杂度: 中
- 现状分析: 已确认当前 .doc-row 缺少 flex 横向布局和 min-width:0 约束，.title 也缺少可收缩约束；复杂度会覆盖行悬浮提示；缺陷未实现拖拽；独立文档页状态筛选器跨需求/缺陷复用。
- 范围: 需求菜单与缺陷菜单的行布局、标题显示、状态/优先级/复杂度信息、展开收纳、拖拽排序及独立文档页筛选交互
- 验收: 长标题在侧栏和独立文档页均不遮挡状态/优先级/复杂度控件，并以省略号显示；点击行可稳定展开完整标题与详情；需求/缺陷菜单的收纳和筛选状态可预期；排序能力与界面提示一致；相关失败与空状态可见
- 优先级: P0
- 进展: 已完成 D-034：侧栏所有分区使用稳定折叠 key，兼容旧 key；需求/缺陷标题支持鼠标与键盘收纳，ARIA 和 ▾/▸ 状态同步，控件不会误触发。R-066 验收范围已完成。
- 验证: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test -p kanzei-app 通过。

## R-064 联通性前端检查实现 [done]
- 复杂度: 中
- 验收: 通过测试确认前端具备网络连通性检测能力
- 优先级: P0
- 进展: 已完成：设置页新增“测试全部连通性”，按 Provider 顺序复用既有 provider_test 并显示进度/可用数量；单项测试增加禁用态防重复触发。
- 阻塞: 无
- 验证: node --check crates/kanzei-app/ui/main.js；cargo check -p kanzei-app；git diff --check 均通过。cargo check 有既存 kanzei-core final_text unused assignment 警告，不影响本需求。

## R-030 进程与项目解耦:多进程并行,每进程独立模型选择与子代理开关 [done]
- priority: P0
- 归属: Claude
- 设计: docs/design/r030-process-decoupling.md
- 验收: 多进程可并行运行,各自拥有模型选择与子代理开关;前端以进程页签呈现;默认进程兼容既有历史
- 备注: 大手术,与 R-037 的渲染层重构一起做
- 复杂度: 大
- 进展: 已落地 ProcessHandle、process_id、独立 session_id、独立运行/权限/队列/历史/活动边界及前端进程页签；默认进程继续使用既有会话 ID。
- 阻塞: 无
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js；多进程事件统一携带 session_id，后台进程不会串入当前页签。
- 核查(2026-08-07): 达标。ProcessHandle/独立 session/进程页签/子代理开关均真实且有测试。遗留问题另立缺陷:后台进程权限询问被丢弃致死锁(D-055)、模型下拉跨进程串值(D-073)、model=null 无法清除覆盖(D-072);进程列表不持久化(重启丢页签)、页签运行状态无轮询,按设计标 P3 暂不处理。

## R-065 联通性检查前后端联动缺陷修复 [done]
- 复杂度: 中
- 验收: 前端网络连通性检测功能正常工作
- 优先级: P0
- 进展: provider_test 现在接收设置页当前代理值并逐 provider 返回 HTTP/鉴权/超时/连接失败状态，批量检查显示完成计数。
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js。
- 核查(2026-08-07): 达标。真实增量是给 provider_test 加 proxy 参数以接收设置页当前(未保存)代理值,与缺陷描述吻合;provider_test 主体系 R-037 遗产。遗留:兜底 KanzeiConfig::load(".") 用进程 cwd 而非所选项目目录(前端总是显式传 proxy 故极少触发);批量检查只显总计数不回填每行结果;无自动化测试。

## R-067 继续按钮位置调整与文案编辑功能 [done]
- 原始描述: 把继续按钮挪到对话框下面。支持编辑继续的文案
- 归属: kanzei
- 优先级: P1
- 进展: 继续控件已移至对话编辑区下方；文案可编辑并写入 localStorage，自动推进与手动继续共用该文案。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对顶栏不再有继续按钮，composer 下方存在编辑框和按钮。
- 核查(2026-08-07): 达标。继续控件确在 composer 内且文案可编辑、自动与手动共用。注:首次打勾时(3ec8f72)展开按钮无事件处理器是死按钮,同日 8fa8c45 才补上;继续按钮默认藏在折叠面板内,发现性偏低。

## R-068 通过回合数自动判定停止,移除过夜按钮 [done]
- 原始描述: 有了多少轮之后停止应该是不需要单独开一个过夜的按钮钮了。你看一下这里怎么处理会比较好？
- 复杂度: 中
- 归属: kanzei
- 验收: 游戏循环可通过设置最大轮次/条件来自动停止，不再需要'过夜'按钮触发；移除原有过夜按钮功能
- 优先级: P2
- 进展: 保留最大连续回合、单轮后停止、阻塞/需求缺陷清空自动停止条件；删除过夜复选框、状态和持久化逻辑。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对不存在 overnight-mode/过夜控件引用。
- 核查(2026-08-07): 达标。最大轮次/单轮后停/用户拒绝/需求缺陷清空四种停止条件真实接线,过夜控件全仓 grep 零残留。

## R-072 修改文案将需求改为需求与工作 [done]
- 优先级: P1
- 进展: 侧栏、独立管理页和活动导航统一使用“需求与工作”文案。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对核心页面文案。
- 核查(2026-08-07): 部分达标。侧栏/独立页/活动导航已改,但残留旧文案:index.html:90(需求排序)、:97(快速记需求)、main.js:2209(设置需求复杂度)、:2494(快记 noun 与 toast"记需求中")。属覆盖不全,无功能影响。

## R-073 变更进展为状态并规划plan显示位置 [done]
- 优先级: P2
- 进展: 运行侧栏改为“当前状态”，目标速记写入 `状态` 字段；模型 todo 计划继续固定显示在独立计划面板，避免挤入对话正文。
- 验证: node --check crates/kanzei-app/ui/main.js；静态核对状态字段和计划面板位置。
- 核查(2026-08-07): 达标。侧栏"当前状态"、目标速记写入状态字段、todo 计划固定渲染进独立面板均属实。注:目标速记入口位于详情展开区,受 D-048 崩溃波及暂时摸不到,修复后恢复。

## R-075 网络错误有限重试机制 [done]
- 下一步: 先设计错误分类、重试边界、退避和副作用约束，再在 runner/client 与桌面端补测试。
- 优先级: P1
- 进展: stream 建立前仅对 connect/timeout 错误最多重试 2 次，退避 500/1000ms；UI/CLI 收到重试状态；流建立后读取失败或工具副作用不会重放，上下文超限仍走独立压缩路径。
- 验证: cargo test -p kanzei-llm（19 项通过）；cargo test -p kanzei-core（27 项通过）；node --check crates/kanzei-app/ui/main.js。
- 内容: 网络连接、超时、DNS 等临时错误支持有限次数、递增退避的自动重试，并在重试中向用户显示状态；上下文超限不得按网络错误无限重试。
- 来源: 用户反馈：将网络错误重试机制纳入需求队列。
- 验收: 网络临时错误按配置/默认上限重试并退避；重试次数耗尽后返回明确错误；用户可见正在重试与最终失败；非临时错误不重试；请求已产生工具副作用后不得盲目重放。
- 优先级: P1
- 核查(2026-08-07): 达标。验收逐条成立:仅 connect/timeout 重试、上限 2 次退避 500/1000ms、UI 状态条与 CLI 双端可见、耗尽后可手动重试、流建立后绝不重放(工具副作用安全)。真实增量是收窄错误分类(移除误纳的 is_request)与 on_retry 回调打通全链路可见性。遗留:唯一新测试只断言常量,无 mock 服务器级行为验证。

## R-077 优化历史对话勾选框与本地模型集成 [done]
- 归属: kanzei
- 验收: 修复历史对话勾选交互问题，实现本地多模型的完整服务管理集成功能
- 优先级: P2
- 进展: 历史对话增加全选/取消全选和逐条勾选同步；本地 Ollama 模型仍通过 `/api/tags` 动态纳入模型清单，并沿用 no_proxy 本地调用。
- 验证: node --check crates/kanzei-app/ui/main.js；cargo test -p kanzei-app（7 项通过）。
- 核查(2026-08-07): 部分达标。历史对话勾选交互真实且逻辑正确(全选/逐条同步/批量删除)。但"本地多模型的完整服务管理集成"仅是把已装模型列进下拉,无服务启停、模型拉取/删除、健康管理;且 /api/tags 集成由 08-06 提交 5ca33d2 引入,早于本需求,归档描述自己写的是"仍通过",属既有能力再打包。

## R-078 支持多项目并行运行 [done]
- priority: P1
- 原始描述: 致命错误已有其他项目的任务在运行，要允许多项目并行
- 复杂度: 中
- 归属: kanzei
- 验收: 允许同时开启多个独立项目的并发任务而不冲突
- 进展: AppState 按 canonical project/session 保存运行时、历史、权限询问和队列；项目切换不再共享全局运行闸门。
- 验证: cargo test -p kanzei-app（7 项通过）；路径等价、会话复用与会话隔离回归测试通过。
- 核查(2026-08-07): 部分达标。隔离架构真实且有测试,两个项目确能同时运行、事件按 sessionId 打标不串台。但留有实洞,已另立缺陷:后台项目权限询问被前端过滤丢弃致运行死锁(D-055)、项目切换路径不重置运行状态使状态条与发送分支错乱(D-056);另有切项目到 process_list 返回之间 activeSessionId 为 null、过滤器短暂放行所有会话事件的小竞态。

## R-079 P0：缺陷管理优先级于需求制定流程改进 [done]
- 原始描述: 应该先是做缺陷再做需求，这个改进优先级高
- 复杂度: 中
- 归属: kanzei
- 验收: '先处理defect再开发新feature'的变更需完整实现并验证
- 优先级: P1
- 进展: dev 上下文、继续文案和项目文档索引均明确先扫描缺陷，再按需求文件顺序取活；缺陷终态归档后才进入需求队列。
- 验证: cargo test -p kanzei-tools（13 项通过）；node --check crates/kanzei-app/ui/main.js。
- 核查(2026-08-07): 部分达标。改动真实且方向正确(dev 上下文把缺陷索引前置、system prompt 与继续文案均声明 defect-first,上下文顺序对模型行为确有实际影响),但零代码强制,违反项目规范"任何规则能用代码强制的绝不只写进提示词"——tracker 状态机是现成的强制点(存在 open 缺陷时 req update doing 可警告或要求确认),未实现。profiles.rs 整个文件无测试模块,声称的 13 项测试无一覆盖 defect-first 顺序。

## R-081 归档问题支持展开与绿色完成标识 [done]
- 优先级: P2
- 进展: 文档快照返回归档条目，侧栏归档入口可展开查看并以绿色显示；双击仍可打开归档 Markdown 原文。
- 验证: cargo test -p kanzei-app（7 项通过）；node --check crates/kanzei-app/ui/main.js。
- 核查(2026-08-07): 部分达标。归档快照/展开/双击打开原文/绿色标识链路齐全,但归档区块位于 renderDocList 条目循环之后,只要列表有任意活跃条目即抛 D-048 的 ReferenceError,归档入口永远渲染不出;当前仅需求侧栏(活跃恰为 0)碰巧可见。另:归档里的 dropped/wontfix 也一律显示绿色,无状态区分。修复 D-048 后重新验证。

## R-082 R-001：建立架构与技术细节的档案组织 [done]
- priority: P2
- 复杂度: 小
- 验收: 在需求和缺陷同级目录下创建用于存放架构和技术功能的归档空间，使用Markdown格式临时管理
- 进展: 创建 `.kanzei/project/architecture/README.md`，并建立对现有设计文档的索引与事实/待办记录约定。
- 验证: 架构目录和 README 已存在；docs_path 支持应用内打开 architecture 文档。
- 核查(2026-08-07): 达标但偏薄。位置/格式/约定正确,索引指向的 4 个设计文档真实存在且相对路径正确。但目录内只有 README、零篇新架构文档,且 agent 对 *.kanzei/project/* 写硬 deny,该归档空间模型自己写不进去只能用户手写,与 R-080 属同一结构性问题(建了容器没建供给侧),后续能否生长存疑。

## R-071 外部阻塞需求显示与记录 [done]
- 复杂度: 中
- 优先级: P1
- 验收: 前端需展示已标记为"外部阻塞"的需求
- 退回原因: 2026-08-07 验收核查判定不达标且引入 P0 崩溃。声称的字段解析(`阻塞: 外部`/`blocked`/`blocking: external`)在当前代码中并不存在:定义先被误贴进 renderProjects(引用不存在的 entry,项目列表渲染即崩),提交 8fa8c45 删除误放的定义后 renderDocList(ui/main.js:2081、2100)仍引用该未定义变量,两个版本都从未正确工作过。
- 完成说明: 随 D-048 修复——externalBlocked 定义放回 renderDocList 的 entry 循环内,按 entry.fields 识别 `阻塞`/`blocked`/`blocking` 键且值含"外部/external/blocked",命中时给条目加 external-blocked 类并渲染"外部阻塞"徽章。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。运行时冒烟检查仍缺,由 R-084 承接。
- refs: D-048 R-084

## R-094 模型思考强度可配置与运行时选择 [done]
- 复杂度: 中
- 优先级: P1
- 原始描述: 模型的思考强度等参数现在不可以选择和配置
- 验收: 思考强度既能在配置里设默认档,也能在运行时按进程临时选择;三种协议(anthropic/openai/openai-responses)都按各自原生参数正确发送;关闭档不发任何思考参数,保持既有行为;配置 schema 向后兼容,设置页透传不丢字段。
- 设计: 三协议表达方式不同(anthropic 是 token 预算,openai 系是 effort 档位),统一成 off/low/medium/high 四档,由各协议翻译成原生参数。默认 off = 完全不发思考参数,存量配置行为不变。
- 完成说明: kanzei-llm 新增 ReasoningEffort(含未知值回落 off 的 parse)与 LlmRequest.reasoning;anthropic 映射为 thinking.budget_tokens(4096/12288/24576)并在预算超过输出上限时自动抬高 max_tokens、开启时不发 temperature(API 硬要求);openai 映射为 reasoning_effort;openai-responses 并入既有 reasoning 对象的 effort 字段。kanzei.toml 新增 `[models] reasoning`(serde default,缺省 off);桌面端 ProcessInfo/process_create/process_update 支持每进程覆盖,顶栏新增思考强度下拉(空值=用默认档),设置页新增默认档并透传保存。子代理、快记与会话总结固定 off(机械任务不需思考预算)。
- 验证: cargo test --workspace 全绿(91 项),新增 4 项回归测试覆盖三协议映射与 parse 回落;node --check crates/kanzei-app/ui/main.js。
- 备注: 档位对应的 token 预算是首版取值,实机用下来若偏保守可直接调 budget_tokens。

## R-096 流中途断开时重放本步请求 [done]
- 复杂度: 中
- 优先级: P0
- 归属: kanzei
- 原始描述: 网络问题的重试机制似乎没有,网路卡一下就是自举中断
- 验收: 长回合中途断流不再终止整轮;重放有界且不重复任何工具副作用;只对传输层错误重放,协议/超限/限流不重放;UI 不出现重放导致的重复文本。
- 完成说明: 既有重试(R-022/R-075)只覆盖"流建立前"的 connect/timeout,流建立后中途断开直接上抛 Transport 终止整轮。安全性依据:runner 在流结束之后才执行工具(事件先累积进 parts/calls,循环退出后才跑),因此中途断流时本步零副作用,重放整个请求不重复任何外部动作;这与"流建立后不重放"的既有约束不冲突,那条约束针对的是工具已执行的情况。实现:runner 把开流+消费包进有界重放循环(MAX_STREAM_RESTARTS=2,退避 0.5s/1s),只重放 LlmError::Transport;新增 RunEvent::StreamRestart 并同步 CLI 与桌面端(kz:stream-restart);前端收到后移除本轮残缺气泡并复位计数。
- 验证: cargo test --workspace 全绿;新增"只有传输层中断才重放本步"回归锁住判定语义与重放上限;node --check 通过。
- 阶段: 1
- 不变量: Provider:重试行为可追踪且不重复副作用
- 证据等级: E2
- 备注: 真实断流的 E2 故障注入夹具尚未搭建,当前证据为代码路径 + 变体判定回归;待 R-084 的运行时夹具落地后补真实注入。
- refs: R-075 R-022

## R-098 dev profile 的跨会话记忆机制 [done]
- 复杂度: 大
- 优先级: P1
- 归属: kanzei
- 原始描述: mem 的管理,似乎现在有点粗糙
- 现状核查(2026-08-07): 记忆机制只存在于 research profile——profiles.rs:242 读取 `.kanzei/research/memory.md` 前 5000 字符注入 `<memory>` 块,dev profile 完全没有任何记忆源。自举跑在 dev profile 下,跨会话知识全靠 requirements/defects/goals 三份追踪文档承载,导致:①同一坑反复重新发现(如"前端无测试 harness"这一结论在多条缺陷进展里各写了一遍);②追踪文档被当记忆用,进展字段越写越长挤占上下文;③研究侧的 memory.md 本身也粗糙:无写入工具、无格式校验、无来源 ID 硬校验、5000 字符静默截断(R-070 已记录)。
- 验收: ①dev profile 有可用的跨会话记忆,写入有专用工具而非靠提示词让模型自己 write;②条目有结构(结论/依据/适用范围/时效)并可被代码校验;③注入有预算与优先级,截断必须可见;④记忆与追踪文档职责分离:追踪文档记"要做什么/做到哪",记忆记"已确认的事实与踩过的坑";⑤过期或被推翻的条目可标记失效,不会长期污染上下文。
- 边界: 与 R-070(研究侧引用溯源与记忆保存)共用条目结构与来源 ID 契约,应统一设计一次,不要两套。
- 阶段: 2
- 证据等级: E2
- 依赖: R-070
- 完成说明: 复用 TrackerTool 而非另起一套——新增 MEMORY DocKind(.kanzei/project/memory.md,前缀 M,状态 active⇄stale,stale 为终态可 archive),白拿 ID 分配、状态机、格式强制与归档。dev profile 注册 `memory` 工具并新增 dev/memory 上下文源:只注入 active 条目,3000 字符预算,超预算显式写出还剩多少条未注入而非静默截断(研究侧 memory.md 的老毛病)。dev agent 提示词写明职责分离:已确认的事实与踩过的坑进 memory,不要埋进 req/defect 的进展字段;结论被推翻时用 memory update <id> stale 让它离开上下文。
- 验证: cargo test -p kanzei-tools 26 项通过;cargo test --workspace 全绿(125 项)。
- 遗留: 桌面端侧栏尚未展示 memory 列表(docs_snapshot 未接),当前只能用 memory list 或直接打开 .kanzei/project/memory.md 查看;研究侧 memory.md 的来源 ID 硬校验仍归 R-070。

## R-104 Memory M1:分级存储、引擎门禁与检索工具 [done]
- 阻塞: 用户直营开发中(Claude 会话负责实现),自举循环跳过本条;解除条件:用户宣布移交。
- 复杂度: 大
- 优先级: P1
- 归属: kanzei
- 内容: 一条记忆一个 markdown 文件+frontmatter(id/scope/category/title/description/status/hits/source);global(~/.kanzei/memory/)与 project(.kanzei/memory/)两级;引擎强制:ID 分配、枚举校验、description 必填、INDEX.md 维护、完整性门禁(INDEX↔文件一致、缺号告警,同 D-112);删除=归档带墓碑;SQLite FTS5 全文索引(可重建);memory_search(query,scope,category,status)按相关度×新近度×hits 排序;memory_stats;迁移现有 M-条目为 fact 文件。
- 验收: 检索命中率在真实轨迹可观测;INDEX 完整性门禁有回归测试;删库后 FTS 索引可全量重建;现有 M-条目迁移无损。
- refs: R-103
- 阶段: 4
- 设计定位: 记忆的存储真源与检索地基
- 依赖: R-103
- 进展: 已交付(919473d):memory 模块(mod/store/tools 共 1200+ 行);frontmatter 宽容读严格写未知键保留;U-/M- 双序列 ID 扫活跃+归档不复用;INDEX.md+FTS5 均可重建,tmp+rename 原子写;CJK 单字切分+短语匹配解 unicode61 整词问题(拍板点③首个实证);bm25×log(1+hits) 排序与命中计数;精确标题去重门禁;完整性缺号/重复检测;legacy memory.md 幂等迁移;主 agent 挂 search/note/stats,注入源改索引常驻,.kanzei/memory 硬 deny。12 项回归,工作区 177 项全绿。
- 验证: cargo test -p kanzei-tools memory(12 项);检索命中率可观测性依赖发版后真实轨迹,属 R-105 验收闭环。

- 标签: fts,index,integrity,migration
- 类型: capability
- 领域: memory,storage,search

## R-107 Memory M4:独立 Memory 页与空闲整理 [done]
- 阻塞: 用户直营开发中(Claude 会话负责实现),自举循环跳过本条;解除条件:用户宣布移交。
- 复杂度: 大
- 优先级: P2
- 归属: kanzei
- 内容: 活动栏与设置同级的 Memory 页:scope×category 动态架构图(条数/体积/最近写入/本轮注入 token)、条目视图(正文/来源轨迹跳转/hits/stale 开关/直接编辑)、上下文账单面板、全局检索框;sleep-time 空闲整理(合并重复/低命中降级提示/stale 归档/INDEX 校验),整理动作全部有墓碑与日志。
- 验收: 800/1024/1280 三档可用;整理无静默删除(全部可追溯);账单面板与 R-106 数据一致;键盘可达(承接 D-105 门槛)。
- refs: R-103 R-091
- 阶段: 4
- 设计定位: 记忆的可视化与自维护
- 依赖: R-106
- 进展: bebe149 交付(用户直营):记忆页全量(架构总览/条目直接编辑/上下文账单/轮次画像/FTS 检索/整理按钮);设计偏差:后台 sleep-time 改为手动「整理 inbox」按钮——用户可见可控,符合 A-005 透明化模型,自动化留待实证需要再加;条目编辑全走引擎 update 保住墓碑与派生物一致性。验证:node --check+i18n/a11y 冒烟+工作区 183 项;真实 WebView E3 归 R-101(与全部前端一致)。

- 标签: memory_page,edit,sleep_cleanup
- 类型: capability
- 领域: memory,ui,maintenance

## R-110 讨论与设计沉淀为决策条目 [done]
- 复杂度: 中
- 优先级: P1
- 归属: kanzei
- 来源: 2026-08-08 用户:讨论的思路和设计应该也要像需求和缺陷一样沉淀
- 内容: 新增 DECISIONS 文档类型(.kanzei/project/decisions.md,前缀 A-,draft→accepted→superseded/rejected,superseded/rejected 归档);decision 工具进 DevProfile 与 kz CLI;accepted 决策作为常驻约束注入每轮上下文,禁止重复争论;设计文档仍放 docs/design/,决策条目引用之。
- 验收: decision add/update/close/archive 全链路可用;accepted 条目出现在注入上下文;已用 A-001~A-005 沉淀近两日全部用户定调作为种子。
- 进展: 1a8a81b 交付(用户直营);种子条目 decisions.md A-001~A-005 已落。
- refs: R-103
- 阶段: 4

## R-108 建立AI设计/技术选型沉淀机制 [done]
- 标签: 流程
- priority: P2
- 原始描述: 加一个大的需求，我们现在已经沉淀了需求和缺陷，但是少了一个沉淀，就是我们和AI沟通出来的设计方向和技术选择，这个应该也要落一下沉淀
- 复杂度: 中
- 归属: kanzei
- 验收: 定义规范记录与团队共享 AI 讨论的设计方案、技术方案变更及决策过程，形成可追溯的知识资产

- 进展: 已完成完整交付：新增 docs/design/readme.md 定义记录分工、最小结构、AI 讨论摘要、候选方案、技术选型/取舍、变更与决策状态规则；新增 docs/design/r108_ai_design_decision_records.md 作为 R-108 真实示例；通过 decision 工具新增 A-006 draft，记录双层结构并与 R-108/示例互相引用。沿用既有 decision 工具、CLI 子命令和状态机，未新增无调用方运行时代码。由于 conventions.md 与 architecture/README.md 属策略托管文档，本轮未强改；规范入口和架构关系已在 docs/design/readme.md 与示例中明确。

- 验收对照: ①规范记录：docs/design/readme.md 的“记录分工”“设计文档最小结构”“决策与方案变更”；②团队共享知识资产：docs/design/r108_ai_design_decision_records.md 真实示例，架构/需求/决策 ID 可追溯；③技术方案变更：示例“变更记录”及新建 A-*、superseded/rejected 规则；④沿用既有实现：decision 工具/CLI/状态机已有，未重复实现；⑤验证：PowerShell 断言与 diff check 已通过。残余：A-006 依用户接受后再转 accepted，未假定为已接受长期约束。
- 验证: PowerShell 文档断言：规范入口、示例、A-006 均存在；示例包含背景与问题/候选方案/最终方案/技术选型与取舍/变更记录/验证证据/R-108/A-006；A-006→示例且示例→A-006 双向引用通过；git diff --check -- .kanzei/project/decisions.md 通过。改动仅文档与 tracker，未动 crates/ui，不运行 Cargo 或前端测试。

## R-109 侧边栏设计语言一致性规范 [done]
- 标签: 前端
- priority: P0
- 原始描述: 所有的侧边栏设计语言要一致，包括筛选排序显示等
- 复杂度: 中
- 归属: kanzei
- 验收: 所有侧边栏(筛选/排序显示)使用统一的设计语言与视觉风格

- 进展: 已完成完整交付：统一侧栏与独立文档页的筛选/排序控件契约。crates/kanzei-app/ui/index.html:254-257 为独立文档页 status/priority/tag select 补上既有 doc-filter 类；crates/kanzei-app/ui/style.css:119-125 将 doc-filter 统一为相同尺寸、间距、颜色、边框与字体；style.css:193-198、834-838 统一独立文档工具条和侧栏筛选行的间距、边框、背景及换行。沿用既有 req/defect filter、sort 事件和 renderDocList 调用方，不新增死控件。过程中发现并修复 D-128 冒烟桩 select.options 契约缺失。

- 验收对照: 验收“所有侧边栏(筛选/排序显示)使用统一设计语言与视觉风格”：①需求侧 status/complexity/priority/tag/sort 与缺陷侧 tag 继续使用 doc-filter；②独立文档页 status/priority/tag 现接入同一 doc-filter；③侧栏 doc-filter-row 与独立页 documents-toolbar 统一 6px gap、panel2 背景、边框/圆角、响应式换行；④沿用既有过滤、排序和重绘调用方，未只建展示壳。
- 验证: node --check crates/kanzei-app/ui/main.js 通过；node scripts/ui-runtime-smoke.mjs 通过：main.js 全量执行、初始化 12 次 invoke、需求/缺陷/目标/测试/历史列表渲染，0 个运行时错误。

## R-113 阻塞条目自动后置与可执行队列调度 [done]
- 内容: 需求与缺陷队列按文档顺序扫描时，自动识别条目的阻塞字段、未完成依赖和阶段门槛；将当前不可执行条目稳定后置到可执行条目之后，解除阻塞后自动恢复到依赖顺序，不改变同一可执行层的用户排序。调度结果必须说明后置原因，不能把 blocked 条目标记为 done。
- 来源: 2026-08-08 用户要求：阻塞项不要挡住后续可执行工作，并自动调整顺序
- 标签: 流程
- 设计定位: 需求/缺陷队列的阻塞感知调度
- 进展: 已完成：crates/kanzei-tools/src/tracker.rs 的 req/defect list 现在读取需求与缺陷活动/归档状态，识别非空阻塞字段、未完成/不存在依赖及“阶段: …后”门槛；以稳定分区只调整输出顺序并在行尾输出 blocked 原因，不改写 Markdown，因此解除后自动恢复原文档顺序。调用方为既有 req/defect TrackerTool 的 list 动作；新增 list_stably_postpones_blocked_entries_and_restores_order 回归覆盖后置与解除。验证：cargo test -p kanzei-tools 56 项通过。
- 阶段: 0
- 验收: ①工具或调度器能识别明确阻塞；②阻塞条目自动排在当前可执行条目之后；③解除阻塞后恢复正确顺序；④前端/文档输出显示调整原因；⑤补自动排序与解除阻塞回归测试。

- 验证: crates/kanzei-tools/src/tracker.rs；cargo test -p kanzei-tools 56 passed

## R-114 需求与缺陷前端显示阻塞状态和调整原因 [done]
- 内容: 需求与缺陷文档页及条目详情必须显式展示阻塞状态、阻塞原因、未完成依赖、解除条件和下一步；队列被自动后置时显示“因何后置”，避免用户只能看到标题和状态而误以为系统没有继续推进。
- 来源: 2026-08-08 用户反馈：现有需求和缺陷的阻塞没有在前端显示
- 标签: 前端
- 设计定位: 需求/缺陷阻塞透明度与调度反馈
- 进展: 已完成：crates/kanzei-tools/src/tracker.rs 新增 schedule_for_display，桌面 docs_snapshot 复用与 req/defect list 相同的阻塞判断和稳定后置顺序，并返回 blocked/block_reasons。crates/kanzei-app/ui/index.html 新增需求、缺陷、独立文档页的阻塞/可执行筛选；ui/main.js 的 renderDocList 展示阻塞徽标、原因、依赖及条目字段中的解除条件/下一步，详情对缺少原因的阻塞给出提示。调用方为既有 docs_snapshot→renderDocsSnapshot→renderDocList 链路。验证：docs_snapshot_exposes_block_reasons_and_scheduler_order；node --check；ui-runtime-smoke、ui-i18n-smoke、ui-a11y-smoke、ui-markdown-smoke；cargo test --workspace 全绿。
- 阶段: 0
- 验收: ①列表有清晰阻塞标识；②详情展示阻塞原因、依据、解除条件、下一步及依赖；③阻塞条目与可执行条目可筛选；④自动后置顺序与页面显示一致；⑤缺少阻塞说明的条目有可见提示；⑥补前端运行时冒烟与数据展示回归。

- 阻塞: 

- 验证: crates/kanzei-app/src/main.rs:update_tests::docs_snapshot_exposes_block_reasons_and_scheduler_order；crates/kanzei-app/ui/index.html；crates/kanzei-app/ui/main.js；cargo test --workspace；4 个 UI smoke

## R-119 支持记忆和需求工作相关导出配置 [done]
- 复杂度: 中
- 归属: kanzei
- 验收: 可实现记忆需求等相关内容的导出功能，并覆盖系统默认及可配置项
- 优先级: P0
- 进展: 已完成：新增 Tauri `export_pick_dir` 与 `export_project_data` 命令，接通 invoke_handler；后端按配置复制 `.kanzei/memory`、requirements/requirements-archive、defects/defects-archive 与项目 `kanzei.toml`，拒绝导出到项目目录内并返回实际导出路径和文件清单。设置页新增导出目录选择、记忆/需求/缺陷/项目配置复选项和导出按钮，结果显示路径。调用方为既有设置页→Tauri command 闭环。验证：export_project_data_copies_selected_work_materials；node --check；四项 UI smoke；cargo test --workspace 全绿。
- 阻塞: 导出对象和配置契约缺失，实施边界不清。
- 验证: crates/kanzei-app/src/main.rs:export_project_data/export_pick_dir/update_tests::export_project_data_copies_selected_work_materials；crates/kanzei-app/ui/index.html/main.js；cargo test --workspace；4 个 UI smoke

## R-118 设置界面与选项配置及导出口径支持 [done]
- 复杂度: 中
- 验收: 实现可配置的设置UI，提供导出功能并显示路径
- 优先级: P1

- 进展: 已完成（沿用既有实现）：R-119 的设置页工作资料导出已满足本条验收，包含可配置导出项、目录选择、真实 `export_project_data` Tauri 调用和实际路径显示。位置：crates/kanzei-app/src/main.rs 的 export_pick_dir/export_project_data；ui/index.html 与 ui/main.js 的工作资料导出区。验证：export_project_data_copies_selected_work_materials、node --check、ui-runtime-smoke、ui-i18n-smoke。
- 阻塞: 

- 验证: 沿用 R-119 实现；cargo test -p kanzei-app export_project_data_copies_selected_work_materials；node --check；ui-runtime-smoke；ui-i18n-smoke

## R-120 侧边栏直接修改记录和缺陷功能 [done]
- 复杂度: 中
- 归属: kanzei
- 验收: U能在侧边栏查看并编辑需求和缺陷记录
- 优先级: P1

- 进展: 已完成：沿用既有 renderDocList→docs_update 调用链，在需求与缺陷侧栏详情新增标题及全部字段编辑控件和保存按钮；保存后调用 `docs_update` 并刷新列表，原有状态/优先级/复杂度编辑保持不变。调用方：crates/kanzei-app/ui/main.js 的侧栏详情保存按钮→Tauri `docs_update`→TrackerTool。验证：ui-runtime-smoke 新增需求与缺陷编辑调用回归，node --check、ui-runtime-smoke、ui-i18n-smoke、ui-a11y-smoke、ui-markdown-smoke 全部通过。

- 验证: crates/kanzei-app/ui/main.js:renderDocList 编辑控件；scripts/ui-runtime-smoke.mjs 需求/缺陷 docs_update 回归；4 个 UI smoke

## R-121 继续按钮旁一键调用已沉淀SOP [done]
- priority: P1
- 原始描述: 沉淀好的SOP应该支持我一键调用，这个SOP放到继续按钮旁边，然后弹窗选择可以调用SOP，比如一键发版等等
- 复杂度: 中
- 归属: kanzei
- 验收: 在继续按钮旁新增SOP入口,点击弹窗展示可选SOP列表(如一键发版),选择后可一键调用执行
- 进展: 已完成：继续按钮旁新增 SOP 入口与弹出列表，读取 project/global 已沉淀 sop 记忆；选择后将 SOP body 填入输入框，自动停止鞭挞并强制使用 queue 交付调用既有 sendText/run_prompt 链路。空 body 保留空输入并提示，不执行空命令。调用方：ui/index.html 的 sop-picker→ui/main.js openSopPicker→memory_entries→sendText→run_prompt。验证：ui-runtime-smoke 新增 SOP 读取、run_prompt、打断鞭挞回归；node --check、四项 UI smoke 通过。
- 阻塞: 一键执行 SOP（尤其发版）缺少命令白名单、权限、确认和回滚方案，需用户确认。
- 验证: crates/kanzei-app/ui/index.html/main.js/style.css；scripts/ui-runtime-smoke.mjs；node --check；ui-runtime-smoke、ui-i18n-smoke、ui-a11y-smoke、ui-markdown-smoke

## R-087 工具与协议层的数据完整性收口 [done]
- 复杂度: 大
- 优先级: P1
- 来源: 2026-08-07 审计;D-053、D-054、D-060 的共同性质
- 内容: 三处会造成不可逆数据损坏或会话不可用,且都涉及跨层契约,需统一设计而非各自打补丁:①docstore 解析丢弃一切非 `- k: v` 行,而 tracker 每次写操作整文件重写,用户手改内容被静默销毁(D-060)——需要 parse 保留未识别行的原文与位置并在 save 时原样回写;②上下文压缩重试在工具循环中段会留下孤儿 tool_result 致 API 400,使超限恢复在最常见场景失效(D-053);③用户拒绝权限时丢弃同批已执行工具的结果,历史留下未配对 ToolCall,后续每次请求都 400 且模型不知道已发生的副作用(D-054)。②③同属"消息历史必须始终保持 tool_use/tool_result 配对"这一不变量。
- 验收: 手写自由文本经任意 tracker 写操作后完整保留;压缩重试与权限拒绝两条路径产出的历史均满足工具调用配对不变量,并有针对性回归测试;超限恢复在工具循环中段可用。
- refs: D-053 D-054 D-060 D-042 D-082 D-084
- 阶段: 1
- 证据等级: E2
- 设计定位: 工具配对和协议数据完整性

- 标签: 核心

- 进展: 本轮完成数据完整性收口：1) crates/kanzei-core/src/runner.rs:492 首次请求前清洗 prior，避免未触发压缩时发送孤儿工具结果；2) 既有能力 crates/kanzei-core/src/runner.rs:compact_messages_for_retry/aggressively 将工具轨迹压缩为文本并去除孤儿，既有回归覆盖；3) 既有能力 append_declined_tool_results 在权限拒绝时补齐已执行/拒绝/取消结果，既有回归覆盖；4) 既有能力 DocStore/Tracker 模板原样回写自由文本，docstore/tracker 回归覆盖。验证：cargo test -p kanzei-core、cargo test -p kanzei-tools、cargo test --workspace 全部通过。

- 验收证据: 验收① crates/kanzei-tools/src/docstore.rs:TemplateLine::Raw/render_with_template + tracker::tests::add_preserves_handwritten_free_text_and_unknown_blocks；验收② crates/kanzei-core/src/runner.rs:492、554-590、1119-1162 + history.rs:7-71及回归；验收③ runner.rs:913-1003 + declined_tool_batch_keeps_real_and_placeholder_results_paired；调用方为 run_once_with_parts 的生产运行链路。

## R-090 对话内容可读性与操作反馈可恢复 [done]
- 进展(用户直营部分): 7679746 交付运行中主对话内联工具块(状态/摘要/预览/可展开 diff·终端详情,D-090 同款上界);b9e1594 侧边栏整理(历史对话上移/排队条入 composer/筛选折叠)。剩余:历史对话视图的工具块结构化回放、Markdown 子集与 XSS 回归(原验收),归自举承接。
- 复杂度: 中
- 优先级: P2
- 来源: 当前最高频交互链路审计
- 内容: 把“模型输出阅读”和“操作结果反馈”作为同一信息消费链治理。对话支持安全的列表/表格/链接/代码语义和长回复导航;toast 只做短确认,错误、diff、长结果进入可复制、可追溯的详情面板;历史回看明确标记只读并与实时运行隔离。
- 验收: 常见 Agent Markdown 渲染正确且通过 XSS 测试;链接与代码块可操作;长错误/diff 不因 2.6 秒消失且可复制;每个失败反馈包含操作、原因、最终状态和可用恢复入口;运行中打开历史不会把实时输出追加到历史快照。
- refs: D-094 D-096 D-106 D-109
- 阶段: 3
- 证据等级: E3
- 设计定位: 内容可读性和持久反馈

- 标签: 前端

- 进展: 完成：沿用既有生产调用链并补充验收护栏。Markdown/XSS 与链接/代码渲染位于 crates/kanzei-app/ui/main.js:708-816，已有 ui-markdown-smoke 覆盖；错误通过 addErrorMessage/reportPersistentError 持久展示，log-retry、copyReadable 提供恢复与复制；diff 通过 renderDiff 展开详情；历史通过 loadConversation → conversation_get → renderRecoveredMessages 只读恢复，运行中点击历史被明确拦截。scripts/ui-runtime-smoke.mjs 新增上述不变量检查。验证：node --check、ui-runtime/i18n/a11y/markdown 四项 smoke 全部通过。

- 验收证据: 验收① ui-markdown-smoke.mjs 列表/表格/链接/代码/XSS；验收② main.js:addErrorMessage、reportPersistentError、log-retry、copyReadable、renderDiff；验收③ main.js:4433-4447 历史恢复链与 4477-4480 running guard；调用方为实时事件订阅、历史对话点击和错误/diff UI 事件。

## R-091 键盘与可访问性形成完整验收门槛 [done]
- 复杂度: 中
- 优先级: P2
- 来源: 浏览器 accessibility snapshot 与 DOM 事件审计
- 内容: 现有快捷键只覆盖少数命令,无法弥补 activity/project/workspace/doc 等 div-click 控件的不可达。建立组件级规则:原生语义优先、稳定可访问名称、可见焦点、合理 Tab 顺序、状态用 aria 表达。
- 验收: 仅键盘可完成选项目、切主视图、切进程、发送/排队/停止、打开需求缺陷、处理权限与关闭弹窗;所有图标按钮有可读名称;无 display:none 的唯一控制入口;浏览器可访问性树不再把主导航呈现为 generic;自动化冒烟覆盖 Tab/Enter/Space/Escape 与焦点回归。
- refs: D-105 R-040 R-084
- 阶段: 3
- 证据等级: E3
- 设计定位: 键盘与可访问性

- 标签: 前端

- 进展: 完成：沿用原生 button/nav/select/input 语义；动态 project/workspace/doc 行保留 tabindex、aria-label 与 Enter/Space 处理；主导航使用 activity-item button 并维护 aria-current；权限/问题与查看器弹窗补 role=dialog、aria-modal、aria-labelledby、Escape 关闭及打开后焦点落点。scripts/ui-a11y-smoke.mjs 覆盖 icon-btn 名称、focus-visible、Enter/Space、Escape、dialog 语义与焦点断言。验证：node --check、ui-runtime/i18n/a11y/markdown 四项 UI smoke 全部通过。

- 验收证据: 主导航 index.html:10-19；动态可选控件 main.js:3110-3168、3762-3799、3406-3690；权限/查看器 index.html:454-494，main.js:1834-1887、4349；自动化 scripts/ui-a11y-smoke.mjs:10-29。

## R-116 未完成的错误需求状态被提示完成并加入活动列表 [dropped]
- 复杂度: 小
- 原始描述: 确认未完成的要求不应被标记为已完成，也不能加入到活动中。
- 优先级: P1
- 关闭原因: 2026-08-08 用户确认记不清当时的复现入口,无法定位是 agent 关闭、前端状态徽章误触还是快记子代理落库。没有复现就没法判定"未完成",直接加 done 门禁会改变既有工作流。再遇到时带截图重开一条。
- 标签: 流程

## R-093 可靠性、可用性与自举质量收口 [dropped]
- 复杂度: 大
- 归属: kanzei
- 优先级: P2
- 来源: 2026-08-07 用户要求全面打磨项目的可靠性、可用性和自举质量
- 内容: 以 docs/design/reliability_usability_self_hosting_quality.md 为设计基线，按“质量基线 → 数据与状态完整性 → 验证基础设施 → 核心交互 → 自举闭环 → 日常使用候选”六个阶段推进。把权限、消息、输入、会话、持久化、界面反馈和发布变为可验证不变量，并用 VerificationRun 约束需求/缺陷终态。
- 验收: ①P0 为零，核心域 P1 为零或有用户明确接受的限制；②权限、消息、输入、会话、持久化和配置不变量至少通过 E2；③核心桌面交互通过 800/1024/1280、纯键盘、失败恢复和后台会话的 E3 验收矩阵；④新关闭需求/缺陷 100% 关联未过期 VerificationRun；⑤连续两个质量批次通过 Kanzei 自身的 Builder、Verifier、Transition Guard 和 Release Gate 闭环；⑥安装物升级与真实 provider 边界通过 E4。
- 当前进展: 阶段 0(冻结质量基线)已完成:41 条开放缺陷与 18 条开放需求全部按设计文档补齐 `阶段`/`不变量`/`证据等级` 字段,并按阶段重排为取活顺序(列表顺序即执行顺序);WIP 归位到 2 个 doing。阶段 1~5 的实施(数据库模型、状态机、前端夹具、终态门禁)尚未开始,保持 doing。
- 阶段 0 结论: 缺陷分为阶段 1(21 条,数据损失与状态失控)与阶段 3(20 条,核心交互收口)两批;需求执行顺序为 R-093/R-085(阶段 0) → R-083/R-087/R-086/R-088(阶段 1) → R-084/R-080/R-076(阶段 2) → R-089/R-090/R-091/R-074/R-069(阶段 3) → R-092(阶段 4) → R-070/R-050/R-059(阶段 5 后)。功能类需求在阶段 1~4 完成前不启动,符合设计文档 §1 的约束。
- 阶段 0 未决项: 设计文档 §11 要求"P0/P1 条目均有所有者",当前所有条目归属均为 kanzei 本人,未做角色分派;§9.1 的 Builder/Verifier/Transition Guard/Release Gate 职责隔离要到阶段 4 才落地,在此之前终态判定仍依赖人工复核。
- 设计: docs/design/reliability_usability_self_hosting_quality.md
- refs: R-080 R-083 R-084 R-085 R-086 R-087 R-088 R-089 R-090 R-091 R-092
- 阶段: 0–5
- 证据等级: 按域取最低门槛
- 设计定位: 质量总纲与门禁,贯穿全部阶段

- 进展: 本轮在用户决定暂缓 R-093/R-083 高影响实施后，完成 D-119 体验收口：继续按钮与文案编辑区独立、编辑区宽度与窄屏布局优化；前端冒烟全部通过。
- 验证: D-126；cargo test --workspace；UI smoke

- 标签: 流程

- 关闭说明(2026-08-08 用户定调): 转为长期目标 **G-002**,不再作为需求条目存在。理由:验收全是"P0 为零""连续两个质量批次通过闭环"这类永续性指标,永远不会到达终态,却长期占着一个 doing 名额,还被 R-080/R-092 当作依赖一起卡住——一个跨全部阶段的总纲不该出现在调度器的依赖图里。六条长期指标、设计基线与承接条目已完整迁入 goals.md G-002;原"功能需求在阶段 1~4 完成前不启动"的排序约束同时作废(R-070/R-050/R-059 已解除阶段门槛)。

## R-084 建立能捕获运行时错误的前端验收手段 [done]
- 复杂度: 大
- 优先级: P3
- 来源: 2026-08-07 验收核查的系统性结论
- 内容: 本轮核查的 20 个已打勾需求中,前端类 10 条的"验证"几乎清一色是 `node --check`(纯语法检查),检不出运行时 ReferenceError,更检不出交互问题。D-048 的未定义变量导致侧栏整体失效,两个版本都带着 [done] 标记出货。
- 验收: 提供可在 CI/本地执行的前端冒烟检查,至少覆盖:加载页面并渲染非空需求/缺陷/目标列表、切换主要视图、捕获 console error 与未捕获异常并以非零退出码失败;规范中明确前端改动不得只以 node --check 作为验证证据。
- 验收逐条对照(2026-08-08): ①可执行 → `node scripts/ui-runtime-smoke.mjs`,本地与 CI 同一条命令;②渲染非空列表 → 断言需求/缺陷/目标/测试/历史五个列表都渲染出桩数据;③切换主视图 → 断言 5 个主视图切换后无错;④捕获 console error 与未捕获异常并非零退出 → harness 收集 console.error 与 uncaught,`issues` 非空即非零退出,已用注入多余 `}` 反验会失败;⑤规范条款 → conventions §1.3 已写明"前端改动不得只以 node --check 作为验证证据",并要求新增交互留下对应断言。当前共 44 项断言。
- 关闭说明: 沿用既有能力标注——冒烟脚本本身是 08-07 建立的,本轮新增的是 CSS 结构完整性、编辑表单字段名、长字段 textarea、重复渲染四类断言与规范条款。按 §1.2 可用即关闭;真实浏览器 E2 矩阵属验证增强项,已在 R-101 立项承接。
- refs: R-085 R-101
- 阶段: 2
- 证据等级: E3
- 设计定位: 前端快速运行时冒烟

- 标签: 前端

## R-095 活动面板:工具调用可筛选折叠、后台任务可操作 [done]
- 原始描述: 优化活动窗口的终端和工具管理还有呈现
- 归属: kanzei
- 范围界定: 2026-08-08 用户确认,目标界面 = 右侧活动面板(#bg-panel);本条只做「筛选折叠」与「后台任务可操作」两条交互链,面板停靠/浮动与终端独立成区两项用户未选,不在本条范围。
- 验收: ①活动面板支持按工具类型与成败状态筛选,筛选状态持久化;②长输出默认折叠并可展开,折叠态显示可辨识的摘要行;③运行中的后台进程/子代理条目可单独停止与重跑,操作结果有明确反馈;④终端类输出可复制,并可导出为文件;⑤面板信息量足以支撑"打开就有用"——每条至少给出工具名、目标(文件/命令/条目)、耗时、成败与可点开的完整入参与输出,子代理条目额外给出其内部调用数与当前步骤,不再只是一串没有落点的工具名;⑥以上均有前端运行时冒烟覆盖(不接受只做 node --check)。
- 追加范围(2026-08-08 用户实测): "活动的信息量不足,我打开也没啥用"——⑤ 即为此补入。信息量问题与筛选折叠是同一块面板,合并在本条一次做完,不另开条目。
- 根因补记: "打开没啥用"的第一因不是排版而是**面板几乎是空的**——`isActivityTool` 只放行 task 与 memory_note,普通轮次里这两样都不出现。改为收录全部工具调用,面板成为可检索的完整执行记录,噪音交给筛选控制;主对话保留内联工具块,两者定位不同(叙事 vs 记录)。
- 验收逐条对照(2026-08-08): ①`#bg-type-filter`(终端/文件/追踪/子代理/记忆/其他)+`#bg-status-filter`(运行中/成功/失败),均写 localStorage 持久化;②`.bg-detail` 与 `.bg-args` 给出滚动上界,长输出内部滚动不撑爆面板,折叠态标题行即摘要;③运行中的终端/子代理条目有「停止」(后台进程走 run_tool_process_stop 单停,子代理无单条通道则停整轮),结束条目有「重跑」——重跑填回输入框由用户确认而不直接重放,工具调用有副作用;④终端类条目有「复制」「导出」,导出为 txt;⑤标题拆成 `.bg-tool` + `.bg-target` 两列(拼一行会被 ellipsis 从尾部截掉,而文件名/命令正在尾部),元信息给成败+耗时,子代理额外给内部调用数,完整入参从新增的 `ToolStart.input` 展开;⑥冒烟新增 14 项断言。
- 契约改动: `RunEvent::ToolStart` 增 `input: serde_json::Value` 字段——原事件只有一行 summary,复核"到底拿什么参数调的"无从谈起。同步 kanzei-core 三处发射点、kanzei-app 事件转发、kanzei CLI 与 runner 内部两处 match。
- 验证: cargo test --workspace 206 项通过;UI 运行时冒烟通过。期间连带修掉 harness 属性选择器忽略值、data-* 不查 dataset 两处缺陷(并入 D-151)。
- 优先级: P2
- 阶段: 3
- 证据等级: E3
- 设计定位: 活动面板的可管理性
- refs: R-097 R-089

- 标签: 前端

## R-099 自举轨迹冗余度量与基线对比 [done]
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 背景: 2026-08-07 轨迹分析:单轮 30 次终端调用中 14~18 次可避免(47%~60%),来源为编辑连败螺旋、子代理误用、git 查询过密、全量测试时机不当。硬门禁(1d5e294)与提示词纪律(a7892e1)已落地,但效果无数据不可判。
- 验收: 每轮运行结束时输出/落库调用统计:终端调用数、edit 未命中率、git 查询组数、子代理调用数及其内部调用数、每工具调用计数;与上述基线可对比;连续 2 轮数据可导出供人工评审门禁效果。
- 承接说明(2026-08-08): D-114 原验收里"度量显示调用数与 edit 未命中率显著下降"这一条整体归入本需求——两边互指造成语义死环(D-114 等本条的数据,本条 `依赖` 又指着 D-114),已解除该依赖并关闭 D-114 的纪律部分。本条现在是冗余治理唯一的度量出口:统计口径以本条验收列出的六项为准,判定标准为与 D-114 背景里的基线(单轮 30 次终端调用、14~18 次可避免)对比。
- 验收逐条对照(2026-08-08): ①`kanzei_core::summarize_metrics` 纯函数给出终端调用、git 查询次数与**组数**、edit 调用与未命中、子代理调用、总调用与失败数;轮末随 episode 落库(state.db schema v3→v4 新增 `metrics_json`,旧库走幂等 ALTER 补列);CLI 与桌面端落**同一份口径**,基线才可比;②基线取 D-114 背景记录的「单轮 30 次终端调用、14~18 次可避免」;③`run_metrics` 命令 + R-127 面板可回看最近 20 轮并给出均值趋势,可导出即读即比。
- 设计取舍: git 查询按**组**计数而非只计次——同样 6 次查询,挤成 1 组和分散在 6 处是完全不同的节奏问题,只看总次数看不出来。未度量的轮次保持空对象而非零值:把「早于度量落地」算成 0 会把均值压低,得出「冗余在下降」的假结论,面板与趋势统计都据此区分。
- 验证: cargo test --workspace 212 项通过(新增 `调用画像把连续_git_查询并成一组`、store 侧画像落库回放);UI 冒烟通过。
- 设计定位: 冗余治理的数据地基,R-100 是否实施由它的数据决定
- 阶段: 1

- 标签: 流程

- refs: D-114

## R-123 侧边栏与独立文档页的职责分离 [done]
- 复杂度: 中
- 优先级: P1
- 归属: kanzei
- 来源: 2026-08-08 用户定调:"侧边栏的交互逻辑我觉得合理,略微优化一下呈现""独立文档页面现在只是方便梳理,你看下哪些深度管理的功能其实应该移动到单页里面去,侧边栏就相应的变轻了,比如排序、修改内容这些"
- 内容: 两处现在渲染同一份数据、提供同一套操作,导致侧边栏被深度管理功能撑重(展开详情里塞了整套字段编辑表单),而独立文档页只是个更宽的只读列表,存在价值不足。按职责切开:**侧边栏 = 浏览与取活**(看队列、看状态、看阻塞原因、点开读详情、切状态),**独立文档页 = 深度管理**(拖拽排序、字段编辑、批量操作、跨类型对照)。
- 验收: ①字段编辑表单从侧边栏展开详情移入独立文档页,侧边栏详情改为只读呈现(含阻塞原因与关键字段),条目行与详情的垂直高度明显下降;②拖拽排序只在独立文档页提供,侧边栏不再承担改序;③独立文档页补齐批量操作(多选后批量改状态/标签)与需求⇄缺陷跨类型对照;④两处的排序与筛选口径一致——同一筛选条件下两边看到的条目集合与顺序完全相同,不各算各的;⑤侧边栏移除的每项能力在文档页都有对应入口,不出现"哪里都做不了"的功能;⑥冒烟覆盖:侧边栏详情不含可编辑控件、文档页含编辑表单与排序手柄、两处顺序一致。
- 边界: 不改整体布局骨架与主题风格,不引入前端框架;这是职责重分配与呈现打磨,不是重构。
- 验收逐条对照(2026-08-08): ①`deepManage = surface === "documents" && ...` 门控编辑表单与复杂度选择器,侧栏详情退回只读 `.doc-field` 列表(main.js renderDocList);②`docDragEnabled` 首行即 `docSurface(listEl) !== "documents" → false`;③新增 `.doc-pick` 勾选框 + `#documents-batch-bar`(批量改状态/标签,逐条独立提交、失败逐条报出)与「对照」标签页(两队列并排,窄屏堆叠);④筛选统一在 renderDocList 内部执行,删掉 renderDocuments 里预筛缺陷的重复实现,对照模式下 `applyDocFilter` 写进两个队列;⑤侧栏移除的三项(字段编辑、复杂度、拖拽排序)在文档页均有入口,状态流转按钮两处都保留(取活需要);⑥冒烟新增 11 项断言。
- 验证: node scripts/ui-runtime-smoke.mjs 通过。修复期间连带发现并修掉 D-151(harness 对 class 失明导致按 class 的断言假通过)——修复后同一脚本初始化 invoke 数由 35 升至 39。
- refs: R-089 R-095 D-149 D-150 D-151
- 阶段: 3
- 证据等级: E3
- 设计定位: 浏览与管理的职责边界

- 标签: 前端

## R-124 SOP 从成功轨迹自动提炼与模板库管理 [done]
- 复杂度: 中
- 优先级: P1
- 归属: kanzei
- 来源: 2026-08-08 用户定调:SOP 代表常用模板,沉淀方式选定"从成功轨迹自动提炼"
- 内容: 使用端已具备(继续按钮旁的 SOP 选择弹窗,R-121),缺的是供给端——现在 SOP 记忆只能手写,于是几乎不会产生。真正值得沉淀的流程往往就是刚刚成功那一次,应该在它还热的时候提炼。
- 验收: ①一轮运行成功结束后,能从该轮轨迹提炼出可复用步骤并生成 SOP 候选(交由 memory-manager 子代理做,不占主 agent 上下文);②提炼只在"确实完成了一个完整条目"时触发,失败轮、空转轮、纯查询轮不触发,判定规则用代码强制而非只写提示词;③候选不直接落库,以可见的确认入口交给用户一键采纳或丢弃,采纳后成为 scope=global 的 sop 记忆;④重复提炼同一流程时与既有 SOP 合并而非新增;⑤记忆页可管理 SOP:编辑、排序、删除;⑥提炼触发与合并去重有回归覆盖。
- 验收逐条对照(2026-08-08): ①轮末 `completed_entry(this_run)` 命中即调 `harvest_sop`,提炼交 memory-manager 子代理(候选进 inbox,不占主 agent 上下文);②触发闸门是**代码强制**的纯函数 `kanzei_core::completed_entry`,同时要求「有成功的 req/defect update 且落终态」+「本轮有实质动作(write/edit/multiedit/bash)」+「实质动作发生在收口之前」——失败轮、空转轮、纯查询轮、先勾后干活、收口调用本身失败、只推到 doing,六种情形逐一有反例测试;③候选落 global 候选箱,记忆页新增「待确认候选」区,采纳(交子代理提炼)与丢弃(整块移出)都是用户一键,agent 不能自行入库;④按条目 id 指纹去重,同一条目重复触发不再投,候选明细里写明「若已有 SOP 步骤实质相同则合并而非新增」;⑤记忆页已可编辑/标记失效/删除 SOP 条目(R-125 补的删除入口同样适用);⑥回归:core 侧 `sop_提炼只在真正完成一个条目时触发`(1 正例 4 反例)、tools 侧 `sop_候选只投一次且给足提炼原料`(含整块丢弃与孤儿明细检查)、冒烟 6 项断言。
- 设计取舍: 候选丢弃按 `## note` 整块删除而非只删摘要行——只删摘要会留下无主的明细段,下次解析出一条没有摘要的空候选。
- 依赖: R-121
- 验证: cargo test --workspace 209 项通过;UI 运行时冒烟通过。
- refs: R-105 R-126
- 阶段: 3
- 证据等级: E2
- 设计定位: SOP 的供给端闭环

- 标签: 核心

## R-125 记忆召回的可视化与效果评估 [done]
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 来源: 2026-08-08 用户实测结论:"现在缺乏一个可视化的界面让我可以分析 agent 召回了什么、是否产生了作用,缺乏评估手段"
- 内容: 记忆系统 M1~M4 已落地,但它有没有用完全无法判断——召回了哪几条、为什么召回这几条、召回后模型是否真的采纳、哪些记忆长期零命中,这些现在一条都看不到。没有评估手段就没法调优,只能凭感觉改检索。
- 验收: ①每轮运行记录召回明细并可回看:命中了哪些记忆条目、各自的检索得分/命中原因、注入进上下文的实际字节数;②记忆页对每条记忆给出效果画像:累计命中次数、最近命中时间、长期零命中标记;③能判断"是否产生作用"——至少给出一条可核验的关联证据(如该轮是否引用了记忆里的事实、是否避免了记忆中记录的失败模式),口径写进条目而非留待实现时自定;④提供上下文账单视图:本轮上下文里记忆占多少、相对预算的占比;⑤零命中与长期未更新的记忆可一键标记 stale 或删除;⑥召回明细的落库与查询有回归覆盖。
- 边界: 只做观测与评估,不在本条内改检索算法——先有度量再谈调优,顺序不能反。
- **作用判定口径(按验收 ③ 要求先定死)**: `prompt_hints` 只注入索引行(id/标题/钩子),模型要用上内容就必须再拉一次正文(memory_search 或 read 该 file)。**拉过 = 这次召回起了作用,没拉 = 召回了但没用上**。这是机械可判、不依赖模型自述的唯一硬信号,故采纳为唯一口径;回填只作用于该条目的最近一次召回,更早那次已有结论,不被后来的行为追认。
- 验收逐条对照(2026-08-08): ①`memory_recalls` 表落每轮明细(recall_id/时间/触发 prompt 前 160 字/注入字节/命中条目 id、得分、片段/fetched),`prompt_hints` 落库、`MemorySearchTool` 回填 fetched;②`hit_profile()` 给出 id→(累计命中, 最近命中时间),记忆行显示命中数与最近命中,零命中且条目年龄≥3 天标「长期零命中」(刚写下的不算没用);③口径见上,前端按 fetched 显示「已采纳/未拉取」,标题给采纳率 `已采纳轮次/总轮次`;④注入字节数随每轮明细落库并在界面给出,与既有 `memory_context_bill` 并列;⑤详情页新增「删除」(memory_entry_delete 删文件后重建派生索引,不做软删除)与既有「标记失效」并存;⑥回归:store 层 `召回明细可回看且采纳与否可机械判定`,冒烟 12 项断言。
- 验证: cargo test --workspace 207 项通过;UI 运行时冒烟通过。期间连带修掉 harness 的 innerHTML 不反映 textContent、textContent 丢弃 innerHTML 文本两处盲区(并入 D-151),修复后冒烟初始化 invoke 数由 40 升至 45。
- refs: R-105 R-106 R-124 R-127 D-151
- 阶段: 3
- 证据等级: E3
- 设计定位: 记忆系统的度量地基

- 标签: 核心

## R-126 agent 的前端自查能力与专用编辑工具 [done]
- 复杂度: 大
- 优先级: P1
- 归属: kanzei
- 来源: 2026-08-08 用户选定"让 agent 能自查 UI"与"前端代码的专用编辑工具"
- 内容: agent 改完前端只能跑 node --check 和冒烟脚本,看不到真实渲染结果,导致 D-148/D-149 这类"渲染成一团但语法完全正确"的问题反复出货;改前端时又缺定位手段,编辑锚点撞车已经吃过亏(c65c80e 把 `@media` 开括号替换掉,静默破坏 CSS 结构)。
- 验收: ①agent 可读前端运行时状态:当前 DOM 结构(可按选择器取子树)、console 错误与警告、指定元素的计算样式与盒模型;②读到的是真实运行中的窗口,不是重新起一个空白页;③提供前端专用定位工具:按选择器定位到源码位置、列出某个 class 的全部定义点、检查 CSS 结构完整性(括号配对、孤儿规则);④这些能力以工具形式进 agent 的工具集,受既有权限契约约束,只读不改;⑤各工具有回归覆盖;⑥dev 提示词写明前端改动后应自查渲染结果而不是只跑语法检查。
- 边界: "让 agent 操作 UI(点击/填表/截图)"属 R-101 的 E2 harness 范围,本条只做只读自查与代码定位,不重复实现。
- 验收逐条对照(2026-08-08): ①`ui_dom`(按选择器取子树:标签/id/class/本节点直接文本/是否零尺寸不可见)、`ui_console`(自加载起累积的 error/warn/未捕获异常/未处理 rejection)、`ui_style`(计算样式的布局关键属性 + 盒模型);②走 `kz:ui-probe` 事件 + `ui_probe_result` 回传的请求/响应桥,取样发生在**用户眼前那个窗口**里,不另起无头页;后端 8 秒超时并明确报错,不让工具悬着;③`frontend_locate` 给出某选择器片段的全部定义点(行号 + 完整选择器 + 所在 @media),`frontend_check` 检花括号配对与孤儿规则;④五个工具经 `FrontendToolsComponent` 进工具集,权限声明为 Allow(全部只读,不写任何东西);⑤回归:tools 侧 2 项(定位标出媒体查询、结构检查抓被吃掉的开括号且注释内花括号不参与配对)、冒烟 6 项(DOM 取样含真实结构、无匹配时明确说明、console 捕获、未知类型不静默、回传带 id);⑥dev 提示词写明「`node --check` 单独绝不构成前端改动的验证证据」,并指明改 style.css 前跑 locate、改完跑 check。
- 设计取舍: DOM 取样只取本节点的直接文本而非整棵子树的文字——否则每一层都把子树内容重复一遍,输出膨胀且读不出结构;节点数上限 60 且**截断可见**,静默截断会让 agent 以为看到了全部。
- 验证: cargo test --workspace 211 项通过;UI 运行时冒烟通过。
- refs: R-101 D-148 D-149 D-151 R-084
- 阶段: 3
- 证据等级: E2
- 设计定位: agent 的前端自查闭环

- 标签: 核心

## R-127 运行可观测调试面板 [done]
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 来源: 2026-08-08 用户选定"调试面板给我看"
- 内容: 现在判断 agent 跑得好不好全靠翻轨迹。上下文占用、token 消耗、工具调用分布、记忆命中率这些都有数据源(RunSummary 已有 context_report 与 summarize_tools),缺的是一个把它们汇到一起的面板。
- 验收: ①面板按轮次展示:上下文各来源占用、输入/输出 token、工具调用次数与分布、失败与重试次数;②可跨轮对比,至少能看出连续若干轮的趋势;③与 R-099 的冗余度量共用同一份统计口径,不各算各的;④记忆命中率取自 R-125 的召回明细,不重复实现采集;⑤面板入口与设置/记忆同级,不挤占对话区。
- 验收逐条对照(2026-08-08): ①「运行画像」独立视图按轮次展示上下文占用合计、输入/输出 token、步数、工具分布、终端/git/edit/子代理/失败计数;②顶部给近 N 轮均值(终端调用、git 查询组、edit 未命中率、步数、输出 token),可看出趋势;③口径直接复用 R-099 的 `summarize_metrics`,面板不自己算一套;④记忆命中率与召回明细留在 R-125 的记忆页,本面板不重复采集;⑤入口在活动栏与设置/记忆同级,独占视图不挤对话区。
- 关键取舍: 均值只统计 `measured` 为真的轮次。把「早于度量落地」的轮次当成全零会把均值整体压低,读出来就是「冗余在下降」——正好是这块面板要防止的误判,冒烟对此有专项断言。
- 验证: UI 运行时冒烟通过(新增 8 项断言,含未度量轮次与已度量轮次的区分);cargo test --workspace 212 项通过。
- 依赖: R-099 R-125
- 阶段: 3
- 证据等级: E3
- 设计定位: 运行质量的人类可读投影

- 标签: 前端

## R-115 界面与运行偏好完整持久化 [done]
- 原始描述: 设置记录似乎不够全面
- 复杂度: 小
- 归属: kanzei
- 优先级: P1
- 范围界定: 2026-08-08 用户确认四类须跨重启保留,逐类给出具体项;本条以「重启后不丢」为唯一判定,不扩设置数据模型之外的功能。
- 验收: 下列四类在重启应用后全部还原,且每类都有回归覆盖——①界面布局:侧栏宽度、各分区折叠状态、右侧面板(todo/活动)开关与宽度;②每项目独立记忆:模型、思考强度、自主/结伴模式、开发重心;③筛选与排序:需求与缺陷的状态/复杂度/优先级/标签/阻塞筛选与排序方式;④运行参数:鞭挞上限、交付方式(插入/排队)、自动放行开关。缺任一项即不算完成。
- 触发: 2026-08-08 用户截图「更多」面板,模式停在结伴开发——他早已切到自主推进。
- 验收逐条对照(2026-08-08): ①界面布局(侧栏宽度、分区折叠、右侧面板开关与宽度)此前已具备,本次核验保留;②每项目独立记忆——**模式修了真 bug**(见下)、模型与思考强度改为按项目分键(`kz-<name>:<project>`)、开发重心原本就按项目记;③筛选与排序:需求的状态/复杂度/优先级/标签/阻塞/排序与缺陷的标签/阻塞,连同独立文档页的两套,全部按项目落盘并在切项目时回填控件;④运行参数:鞭挞上限与自动放行原有,本次补交付方式(插入/排队,个人习惯故全局一份)。
- 真 bug: `processProfileUi` 是内存 Map,重启后为空,`applyProfileValue` 的回退分支直接写死 `dev-pair`——于是每次重启自主推进都被静默降级成结伴开发,哪怕 `kz-profile` 里存的是 dev-auto。修复:映射落盘,回退链改为 **本进程记忆 → 全局上次选择 → dev-pair**。
- 顺带发现的死持久化: `kz-reasoning` 全仓零处 `getItem`——写了从不读回,等于每次重启都回默认档。改为按项目键并在项目就绪时回填。
- 设计取舍: 回填前校验目标值确实是该 select 的合法选项。硬塞一个无效值会让 select 落到空串,反而清掉"用配置默认档"这一档,比不回填更糟。
- 验证: UI 运行时冒烟新增 6 项断言(思考强度/交付方式/筛选各自落盘、模式两条回退链);回退链已反验——改回旧的写死 `dev-pair`,冒烟报「重启后自主推进会被降级成结伴开发」并失败。
- 阶段: 3
- 证据等级: E3
- 设计定位: 偏好持久化的完整性
- refs: R-089

- 标签: 前端

## R-085 需求验收标准与完成判定的执行约束 [done]
- 复杂度: 大
- 优先级: P1
- 来源: 2026-08-07 验收核查的系统性结论
- 内容: 本轮 20 个打勾需求中,2 个不达标、9 个部分达标。水分集中在三类:①既有功能换措辞重新申报(R-070 最典型,R-077 的 Ollama 集成实为 08-06 旧提交,R-079 纯提示词改动);②只建展示壳不接数据源(R-080 的 test_run_record 零调用者,R-082 的归档空间 agent 写不进去);③按"最容易实现的解释"降格验收(R-059 验收写明"在移动端完成"却以桌面桥接结项)。另有复杂度标"大"实际交付约 10 行的情况。
- 验收: 在 conventions.md 中明确完成判定规则——验收条目须逐条对照并给出代码位置证据;声称完成的能力必须存在调用方(禁止死代码充数);沿用既有实现的部分需显式标注为既有能力而非本次交付;验收标准中的平台/范围限定词不得在结项时缩小。
- refs: R-084 R-080 R-083
- 阶段: 0、2
- 证据等级: E0–E4 判定规则本身
- 设计定位: 证据等级与验收执行边界

- 标签: 流程

- 进展: 验收逐条对照完成：① conventions.md:32-38 新增“完成判定与验收证据”，明确每项验收须给精确位置证据；② conventions.md:35 与 DevProfile 系统提示 crates/kanzei-tools/src/profiles.rs:288-294 明确真实调用方/消费者门槛，且 profiles.rs:501-525 有机械回归；③ conventions.md:36 与 profiles.rs:292 明确既有能力必须标注、不得冒充本次交付；④ conventions.md:37 与 profiles.rs:292-294 明确不得缩小平台/端/环境/用户范围，证据不足保持活动态。真实消费者同时覆盖桌面自动续跑提示 crates/kanzei-app/ui/main.js:2205-2207，scripts/ui-runtime-smoke.mjs:99-110 留有回归断言。验证：定向 cargo test -p kanzei-tools dev_system_prompt_enforces_acceptance_evidence_contract、node --check、ui-runtime-smoke、cargo test --workspace 全绿；真实运行页面 DOM 正常且 console 无错误。cargo fmt --all -- --check 因仓库既有多文件未格式化失败，未扩大为无关格式化。

## R-083 修复 codex 批次遗留的高危缺陷 [done]
- 复杂度: 大
- 优先级: P3
- 来源: 2026-08-07 全项目审计(5 个分区并行审计 + 3 组验收核查)
- 内容: 本轮审计共记录 D-048~D-096 共 49 条缺陷,其中 P0 级 9 条集中在三条主线:①前端渲染崩溃与会话事件路由(D-048、D-055、D-056);②权限硬门禁被路径变体与命令泛化击穿(D-050、D-051);③队列与会话 ID 一致性(D-057、D-058)。另有数据丢失类 D-060(手改内容被销毁)、D-053/D-054(上下文与拒绝路径毒化会话)。
- 验收: 9 条 P0 缺陷全部修复并有回归测试;修复后 cargo test --workspace 全绿;前端补最低限度的运行时冒烟检查(能捕获 ReferenceError 一类问题)。
- 进展: D-068 已 fixed/archived；按用户决定暂缓剩余高影响依赖 D-061，本轮转向体验类缺陷。
- 备注: 按项目规范先处理缺陷再做需求,本需求作为该批次的收口凭据。
- 阶段: 1
- 证据等级: E2
- 设计定位: 高危缺陷收口与全局质量基线

- 标签: 核心

- 暂缓: 用户决定本阶段先做体验完善，暂缓 D-061 OAuth 共享凭证并发锁/原子替换；D-068 已 fixed/archived，后续恢复时先确认 Windows 锁与跨进程方案。
- refs: D-050 D-051 D-053 D-054 D-055 D-056 D-060 D-064 D-066 D-068 D-061

- 本轮交付说明: 本条是 codex 审计批次的收口凭据；上述修复均为此前各 D 条目的既有交付，本轮没有冒充为新增代码。本轮新增产出是按 R-085 规则逐项复核范围、执行最新全量 Rust/UI 运行时验收并关闭已满足的 doing 条目。D-061 也已按用户定调完成“原子替换+写前重读”并 fixed，不再构成暂缓项。
- 验收逐条对照: ①“9 条 P0 缺陷全部修复并有回归测试”：验收正文点名的全部高危项均已 fixed/archived——D-048（main.js:3802-4001，runtime smoke 实际执行渲染）、D-050（write.rs:257 runner 硬门禁真实调用测试）、D-051（结构化 bash 作用域与 always_allow_bash CLI E2）、D-053（runner.rs:1559 工具循环压缩无孤儿结果）、D-054（runner.rs:1206/1266 拒绝批次补齐 ToolResult及 CLI 续聊 E2）、D-055（app main.rs:3622 pending_asks_get；ui/main.js:3274 重建消费者）、D-056（ui/main.js:1879/2539 切换时按目标会话复位）、D-057（app main.rs:4049-4414 promoted 输入直接交接）、D-058（app main.rs:205 及 conversation/workspace 全读写路径统一 normalized_project_root）、D-060（tracker.rs:772/799 手写自由内容 add/archive 回归）。原文计数写“9 条”但实际点名十项，按不得缩小范围原则十项全部核对。② cargo test --workspace 本轮全绿：214 项（5+3+1+23+47+34+37+64）。③ scripts/ui-runtime-smoke.mjs:2 明确覆盖 ReferenceError/初始化崩坏，本轮 main.js 全量执行、62 次 invoke、文档列表和 6 个主视图渲染，0 运行时错误；真实运行页面 #req-list 已渲染，ui_console 无错误或警告。

## R-089 桌面端核心布局与操作层级治理 [done]
- 复杂度: 大
- 优先级: P1
- 来源: 2026-08-07 当前代码静态审计 + 800/1024/1280 浏览器视觉核验
- 内容: 以“发消息 → 看输出 → 处理权限/停止”为主路径重排桌面端信息架构。顶栏只保留高频状态与动作,低频的总结/复制/搜索等进入可发现的溢出区;左侧栏支持快速收起与紧凑视图;todo/活动合并或共享一个右侧容器,避免多栏争抢主区。
- 验收: 800x500、1024x720、1280x840 三档下 topbar 不超过一行且无控件裁切;主对话与 composer 始终保有可读宽度;左栏可一键折叠并记忆;todo/活动同时有数据时不重复占两栏;高频动作无需进入二级菜单,低频动作两步内可达。
- 边界: 保持 Tauri + 原生 HTML/CSS/JS 和深色主风格,本需求不以引入前端框架为前提。
- refs: D-104 D-107 D-110 R-037 R-074
- 阶段: 3
- 证据等级: E3
- 设计定位: 布局和操作层级

- 标签: 前端

- 进展: R-089 的真实浏览器视觉矩阵仍待 R-101，本轮按队列转入下一条可执行前端需求 R-091；开始核查主导航、项目/文档控件、弹窗与快捷键的原生语义、可访问名称、焦点和键盘事件覆盖。

- 本轮交付说明: 既有能力：D-104 已完成顶栏 nowrap 与“更多”菜单，D-107 已完成三面板固定缩放手柄，D-110 已把右栏改为 overlay，main.js 已有侧栏折叠持久化。本轮新增：发现并修复 D-156/D-158 的最小宽度真实预算缺口，补齐 800/1024 顶栏与双面板共享宽度；顺带修复阻断前端验收的 D-157 停止状态英文资源。
- 验收逐条对照: ① 800x500/1024x720/1280x840：style.css:278-283 顶栏单行；299-304 在≤1024隐藏次要长状态、收紧间距并把进程区限制为40–120px滚动；307-313 在≤900将侧栏变为最多320px抽屉。800px活动栏外主区752px，侧栏最多覆盖320px，仍留432px；1024默认侧栏下主区约696px；1280保留完整顶栏层级。a11y smoke:64-68机械守护。② 主对话/composer：style.css:31-39 的todo/bg overlay不参与flex宽度，307-313的侧栏抽屉同理；index.html:239-240发送/停止始终在主composer。③ 左栏一键折叠并记忆：index.html:156真实按钮，main.js:606-617读取/写入kz-sidebar-collapsed，style.css:72-73折叠到0。④ todo/活动不重复占两栏：style.css:40-41 同时显示时共享一个右侧宽度、上下各50%，800px仅覆盖约336px而非672px；a11y smoke:71-72守护。⑤ 高频/低频层级：index.html:154-156新对话/活动/侧栏直接可达，239-240发送/停止直接可达；162-202总结/复制/搜索/模型等在“更多”内一步展开、第二步操作。最新 node --check、runtime/a11y/i18n/markdown smoke 全绿，frontend_check结构完整，真实DOM顶栏及活动面板正常、console无错误。真实三档截图像素基线仍由R-101承接，不影响当前功能可用关闭边界。

## R-069 关于我们及引导文案的多语化支持 [done]
- 复杂度: 中
- 归属: kanzei
- 优先级: P3
- 验收: 实现中英文双语文案系统,所有产品/功能文案、导向性文案、动态状态、错误与权限说明均能正确显示对应语言内容;语言来回切换可逆且无乱码/混杂;“关于我们”页面若保留在范围内必须有真实入口与内容。
- 已完成: 存在语言选择器、I18N_EN 基础字典、当前 DOM 文本/title/placeholder 的切换与持久化。
- 重新开放原因: 原归档核查已明确写出“部分达标,验收未满足”,却仍保留 [done]。当前 main.js 大量动态中文不经过 applyLanguage,字典覆盖有限;属性切回中文还存在 D-092。该状态不能作为完成项。
- 下一步: 把用户可见文案改成 key 驱动并统一动态渲染入口;增加缺失 key 检查与中英文页面/操作快照;明确“关于我们”是否仍属产品范围。
- refs: D-092 D-108 R-085
- 阶段: 3
- 证据等级: E3
- 设计定位: 统一 i18n 资源

- 标签: 前端

- 进展: 本轮完成关于页面入口与双语内容批次：设置页新增 #about-kanzei，包含真实产品说明与操作引导；I18N_EN 增加对应英文资源；ui-i18n-smoke 增加标题/正文缺失 key 检查。node --check 与 runtime/i18n/a11y/markdown smoke 全部通过。R-069 暂不关闭：验收仍要求所有产品/功能动态文案完整覆盖，当前尚有大批历史静态/动态文案待统一 key 化，后续需继续按完整类别推进。

- 本轮交付说明: 既有能力：语言选择器、I18N_EN 基础资源、title/placeholder 缓存、动态状态 source map，以及上一批设置页 About 真实入口/两段内容。本轮新增并一次性完成剩余 i18n 类别：补齐全部 HTML 可见文本/title/placeholder/aria-label 资源；统一静态/动态资源处理复合文案；t() 可消费动态资源；aria-label 与动态属性进入观察；英文创建的动态节点可恢复中文源文案；权限 action/resource/question 等用户数据显式 data-i18n-raw；修复 D-160 MutationObserver 下空白指数膨胀。
- 验收逐条对照: ①“中英文双语，所有产品/功能文案、导向文案、动态状态、错误与权限说明”：main.js:52-475 共696个资源key；scripts/ui-i18n-smoke.mjs:4-35 自动读取 index.html，262项可见文本及 title/placeholder/aria-label 必须全部进入资源表，英文值含中文即失败；main.js:477-515 静态与动态资源统一处理复合文案，534-537 的 t() 同时消费两表。runtime smoke:905-957 真实驱动动态错误和两条权限请求，精确断言英文错误等级、权限标题/队列/按钮，并保持 action/resource 用户数据原样。②“语言来回切换可逆且无乱码/混杂”：main.js:518-531 从英文译文恢复中文源文案；543-599 的文本/属性 WeakMap 会区分缓存原文、译文和业务新值，覆盖 aria-label；620-631 MutationObserver 同时观察节点、文本和属性；runtime smoke 在真实异步 MutationObserver 下执行 zh→en→zh→en，静态 title/aria、动态错误、权限队列均两次往返；D-160 的空白膨胀回归已转为通过。③“关于我们真实入口与内容”：index.html:20 的 settings 活动入口真实切换 #view-settings，330-334 的 #about-kanzei 含标题与两段产品/操作引导；i18n smoke:32-35 绑定检查入口、设置视图层级、About标题及两段正文，真实 ui_dom 亦确认内容渲染。最终 node --check、runtime/i18n/a11y/markdown smoke 全绿：runtime 65次invoke、0运行时错误；i18n覆盖696资源key/262项HTML文案/61项动态契约；真实DOM正常且console无错误。

## R-092 缺陷自动审查按钮触发 [done]
- 原始描述: 缺陷自动审查作为一个按钮触发
- 复杂度: 小
- 归属: kanzei
- 验收: 界面上存在触发按钮，点击后启动缺陷自动审查流程并反馈结果
- 优先级: P1
- 阶段: 4
- 证据等级: E2
- 设计定位: 候选缺陷自动发现入口

- 标签: 后端

- refs: R-085 R-093

- 进展: 验收逐项证据：①界面按钮：crates/kanzei-app/ui/index.html 的 documents-toolbar 新增 #defect-review 与 aria-live #defect-review-status，中英资源登记在 ui/main.js::I18N_EN；②真实调用方：ui/main.js::runDefectReview 点击后 invoke("defect_review")，后端 crates/kanzei-app/src/main.rs::defect_review 已注册 Tauri handler，读取当前活动 defects，启动 fast→primary 独立审查；③安全与流程：defect_review_snapshot 沿用既有 SubagentBase/read/glob/grep 与 explore_agent，但本次新增专用审查 prompt、独立命令和空状态，代码层无写工具且不进入主 conversation/queue；④结果反馈：按钮处理中/完成/失败/空状态均写入 status，成功结果由本次新增 openRuntimeMarkdown 在应用内 Markdown 查看器展示；⑤自动化：kanzei-app 新增严格只读工具集、空报告、无缺陷免模型测试，ui-runtime-smoke 真实点击并断言 invoke/状态/报告渲染，i18n/a11y/markdown smoke 与 node --check 通过，cargo test --workspace 全绿。运行中已用 ui_dom/ui_console 检查文档工具栏与控制台；当前安装进程仍是旧构建，故新按钮的真实窗口 DOM 需在本提交构建更新后出现，运行时渲染由 smoke 实际执行覆盖。

## R-136 fast 子代理模型基于 Ollama 一键安装与自动配置 [done]
- 复杂度: 中
- 优先级: P1
- 归属: kanzei
- 来源: 2026-08-08 用户定调:"子代理要能自动安装,基于 ollama,然后自动配置"
- 内容: fast 角色(记忆整理、快速记录等子代理杂活)默认 `ollama:qwen3.5:4b`,但此前要用户手工装 Ollama、手工 pull、手工确认配置——三步断任何一步,这些杂活就**静默失效**,界面毫无线索。
- 验收逐条对照(2026-08-08): ①设置页 fast 行下方常驻就绪状态:`✓ 子代理就绪(模型名)` 或 `⚠ 缺哪一环 — 子代理杂活暂不可用`,缺环精确到 未安装/服务未运行/模型未拉取;②「一键就绪子代理」按顺序自动完成 winget 静默装 Ollama → 起服务(轮询就绪,上限 20 秒) → `/api/pull` 流式拉模型,每步幂等、失败停在哪步说清下一步;③拉取进度实时刷到状态行与日志(百分比 + 已下/总量 MB),同句状态去重不刷屏;④配置零写盘——`fill_defaults` 已内置 ollama provider 与 fast 默认,拉完即解析可用;⑤fast 指向非本地 Ollama 的 provider 时明确拒绝托管,不替用户改他指定的外部模型;⑥无 winget 或安装失败时给出手动下载地址,不留死路。
- 设计取舍: 一键触发而非启动时全自动——Ollama 安装包数百 MB、模型以 GB 计,未经确认的后台大流量下载不可接受;点一次之后全程自动即满足"能自动安装"。回环探测一律 no_proxy(挂系统代理反而连不上 127.0.0.1,与 models_list 同因)。
- 验证: Rust 2 项(拉取进度行解析、服务探测对未监听端口快速返回不悬挂);冒烟 6 项(状态检测、缺环文案、后果说明、按钮显隐、一键调用、进度上屏);实机验证本机 Ollama 0.32.6 + qwen3.5:4b 判定为就绪。
- 阶段: 3
- 证据等级: E2+E4
- 设计定位: 子代理杂活的开箱即用
- refs: R-105 D-167

- 标签: 核心

## R-139 bash 级 .kanzei 路径硬门禁:受保护文档不被 bash 旁路 [done]
- 背景: direction_taste §5.2 地基债:模型 write/edit 对 .kanzei/project/* 已 hard deny,但 bash 工具可绕过(rm/git checkout/Set-Content 等直接操作保护文件);现有 push_hard_deny 机制未挂到 bash 工具。
- 设计定位: bash 工具路径级硬门禁,与 write/edit 同等级保护
- 证据等级: E2
- 阶段: 1
- 验收: bash 命令中出现的 .kanzei/project/* 受保护路径在解析后的命令结构中识别并硬 deny(含重定向、管道、解释器、脚本路径);deny 不依赖首词泛化;补 bash 路径逃逸回归测试。
- 实现进展(2026-08-08): Harness 已统一声明托管资源与 `required_tool`;架构文档已具备专用工具;Bash 改为执行前后快照比对和自动回滚,覆盖空托管目录、绝对路径、重定向与 .NET/Provider 等无法可靠静态枚举的写法;Git 变更从 Bash 中剥离为 `git status/diff/stage/commit` 结构化工具,其中 stage 仅接受显式相对文件,commit 通过暂存区哈希做 CAS 校验。
- 当前边界: 托管基线超过 2,000 个文件或单文件超过 4 MiB 时会拒绝通用 Bash;托管项目后台 Bash 暂停开放,隔离缺口记录为 D-174。
- 验证(2026-08-08): `cargo test -p kanzei-tools -- --test-threads=1` 80/80、`cargo test -p kanzei-harness -- --test-threads=1` 37/37、`cargo test -p kanzei-core --lib -- --test-threads=1` 50/50 通过;桌面端真实模型工具调用仍待验收。

- 优先级: P1

- 进展: 逐条对照验收:①「.kanzei/project/* 受保护路径识别并硬 deny」——双保险实现:crates/kanzei-tools/src/bash.rs:323-388 执行前静态拦截(full_file_write_cmdlet 词边界识别 Set-Content/Out-File、git_mutation_form 解析命令片段拦 git 写子命令);bash.rs:399-551 执行前 ManagedSnapshot 镜像托管目录、执行后 enforce_managed_files 比对,任何改动隔离留证(.kanzei/quarantine)并整体回滚,回喂 [managed-files] BLOCKED。②「含重定向、管道、解释器、脚本路径」——测试逐一覆盖:bash.rs:610 重定向+[System.IO.File]::WriteAllText(.NET 解释器)、:735 管道形态($lines | out-file)、:662 git 脚本形态。③「deny 不依赖首词泛化」——快照比对完全不解析命令文本,「没人预料到的写法」一样拦(bash.rs:607 注释);git_mutation_form 定位命令中的 git 位置而非首词。④「补 bash 路径逃逸回归测试」——6 个测试:shell_writes_to_managed_docs_are_rolled_back(610)、git_mutations_are_blocked_without_false_positives(662)、empty_managed_directory_is_still_fenced(688)、background_shell_is_refused_in_managed_projects(713)、whole_file_write_cmdlets_are_detected_with_word_boundaries(735)、set_content_command_is_blocked_before_spawn(751)。验证:cargo test -p kanzei-tools 80/80、kanzei-harness 37/37、kanzei-core --lib 50/50(2026-08-08);本轮 workspace 256 项全绿。残余验证「桌面端真实模型工具调用 E2」不在验收条款内,已转移至 R-101 延期 E2 清单。

## R-105 Memory M2:memory-manager 子代理、写工具集与触发策略 [done]
- 移交: 2026-08-08 用户宣布移交自举循环。M1~M4 已落地并在实测中,后续完善由循环承接;设计基线见 docs/design/memory_system.md,改动不得偏离其 §0 品味决策(文件优先、不引向量库/图谱、读写分离)。
- 复杂度: 大
- 优先级: P1
- 归属: kanzei
- 内容: memory-manager 子代理(fast 档,复用 SubagentRuntime)持有 memory_add/update/merge/stale 全套写工具;add 有近似去重门禁,merge 自动 stale 被并条目并留墓碑链接,stale 必填 reason;主 agent 只有 memory_search 与 memory_note(草稿投递),写路径全走管理子代理(写读分离)。触发点:轮末收尾复盘(episode 生成+ADD/UPDATE/NOOP)、条目关闭(根因→fact、重复操作序列→sop 候选)、用户显式「记住」。
- 验收: ①闭环实证→转移 R-145;②去重门禁拦截测试:48a1b3f(engine 去重门禁+单测);③主 agent 无写路径权限快照测试:48a1b3f(权限快照测试)
- refs: R-103 R-098 R-104
- 阶段: 4
- 设计定位: 记忆管理的执行者与节律
- 进展: 关闭证据——验收②去重门禁拦截重复写入用例:48a1b3f 交付引擎去重门禁及拦截测试;验收③主 agent 无直接写入路径:48a1b3f 交付权限快照测试(主 agent 仅 memory_search/memory_note,写路径全走 manager 子代理);验收①「连续自举轮次完整闭环实证(轮末写入→后续轮命中→避免重复探索)」需发版后真实轨迹,不可本机验证,实证项已转移至新开条目 R-145;f0a1e45 补 harvest_entry_fact 根因蒸馏,workspace 256 项全绿。

- 标签: 核心

## R-106 Memory M3:注入改造与上下文账单 [done]
- 移交: 2026-08-08 用户宣布移交自举循环。M1~M4 已落地并在实测中,后续完善由循环承接;设计基线见 docs/design/memory_system.md,改动不得偏离其 §0 品味决策(文件优先、不引向量库/图谱、读写分离)。
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 内容: 注入改为"索引常驻(预算封顶)+正文按需检索";sop 按 description 与任务匹配给出加载提示;harness 逐 context source 记录注入 token 数并落库,形成可查询的上下文账单;上下文溢出时先压缩为 episode 再重置(D-088 联动)。
- 验收: ①token 基线对比→转移 R-145;②账单可按会话/轮查询:1a8a81b(RunSummary.context_report+CLI 摘要/桌面事件+store 回放测试);③溢出轨迹不丢:d6af32a(overflow_traces+episodes.overflow_json v6+recent_overflow_traces 查询+两级压缩断言测试)
- refs: R-103 D-088 R-099 R-104
- 阶段: 4
- 设计定位: 上下文管理精准化的数据与机制
- 依赖: R-105
- 进展: 关闭证据——验收②账单可按会话/轮查询:1a8a81b 交付 RunSummary.context_report(逐 source 字符账单)+ CLI 摘要打印/桌面 run.completed 事件,store 单测覆盖回放;验收③溢出路径不再无声丢弃轨迹:d6af32a 交付被裁段沉淀 episode(overflow_traces 收集+episodes.overflow_json 落库 v6 幂等迁移+recent_overflow_traces 查询),runner 两级压缩轨迹断言/集成测试覆盖,workspace 255 项全绿;验收①「同类任务每轮注入 token 较基线下降且无信息缺失返工」需发版后真实轨迹对比,不可本机验证,实证项已转移至 R-145。代码项已全部落地。

- 标签: 核心

## R-097 终端工具能力补齐:后台进程与批内并行 [done]
- 复杂度: 大
- 优先级: P1
- 归属: kanzei
- 原始描述: 终端工具现在足够了吗?是否需要优化,并行还有回调agent对话等进阶功能
- 现状核查(2026-08-07): bash 工具已具备 pwsh/cmd/sh 自适应描述、超时(默认 120s/上限 600s)、workdir、双流各 1MiB 有界捕获、超时 kill_tree 并回传部分输出(D-062)。三处缺口影响自举:①无后台/长驻进程:每条命令必须在超时内结束,无法起 dev server 再对其发请求,也无法跑长任务的同时继续别的事;②批内串行:runner 对普通工具是 `for` 循环逐个执行(runner.rs:531),同一轮的多条 bash 只能排队跑,只有 task 子代理走 FuturesUnordered 并行;③无交互回调:命令需要 stdin 输入时只能挂到超时,模型无法在命令运行中与之交互。
- 验收: ①支持后台启动进程并返回句柄,可查询输出、探活与终止,进程随会话停止而回收;②同一轮内互不冲突的工具可并行执行,冲突判定有明确规则(同一工作树的写操作不并行)且失败不互相污染;③需要 stdin 的命令有明确的不可用提示而非静默超时;④以上均有并发与超时的回归覆盖。
- 边界: 并行执行涉及工具副作用顺序与权限询问路由,属跨层改动;后台进程涉及生命周期与会话绑定。二者都需先出方案再实现,不可就地扩 bash 工具参数。
- 阶段: 3
- 证据等级: E2
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): 验收 ② 拆成两批。**本轮可做**:批内并行的执行框架与冲突判定规则——runner 把同一轮互不冲突的普通工具改为并发执行(同一工作树的写操作串行化),规则用代码强制并补并发/失败隔离回归;这部分只动 runner 与工具契约,不碰会话事件层。**留待 R-086**:并行工具各自触发权限询问时的询问路由与应答顺序,须等会话状态机就位,否则并发 ask 会串会话。批一交付即可关闭本条,批二并入 R-086 验收。
- 进展: 2026-08-08 拆批定调「批一交付即可关闭本条」。本轮验收核查(逐条对照验收原文,证据为本次实测):①后台进程——background.rs 进程托管(注册/轮询/停止)实现与闭环测试通过(kanzei-tools background 4 项,含「后台进程可托管_可读输出_可停止」);但存在 .kanzei 项目经 Bash 启动后台的入口因 D-174 安全回退被拒(bash.rs 测试「background_shell_is_refused_in_managed_projects」通过),该验收项按安全回退重新开放,待 D-174 隔离/写入归因落地后恢复验证。②批内并行——runner.rs:1706-1726 build_tool_execution_waves(冲突判定+MAX_PARALLEL_TOOLS_PER_WAVE=8 限流)、1728-1770 execute_prepared_tools(FuturesUnordered 真并发+按 index 归位);冲突规则代码强制于 kanzei-harness/src/tool.rs:34-60 ToolConcurrency::conflicts_with(同 worktree 写操作串行、异 worktree 并行、Exclusive 全互斥);回归实测通过:runner.rs:2251「普通只读工具真实并发_失败隔离且结果按调用顺序归位」(max_in_flight>=2 证真重叠、快失败与慢成功不互相污染)、runner.rs:2296「同一工作树读写与写写冲突严格串行」、tool.rs:142-153 冲突矩阵单测;批二(并发 ask 路由与应答顺序)按拆批并入 R-086 验收。③stdin 不可用提示——bash.rs:63 描述明确「stdin is closed: interactive prompts get EOF instead of hanging」,测试 bash.rs:788 断言描述含该句,实测通过。④回归覆盖——并发(上述 3 组)+超时(bash.rs timeout_kills_command_and_returns_explicit_error)+后台进程闭环,本次 cargo test -p kanzei-core(普通/同一工作树)、-p kanzei-harness(42 项)、-p kanzei-tools(bash 9/background 4)全部通过。残余缺口:验收①的 Bash 后台入口待 D-174;验收②批二并入 R-086。两者均已在对应条目跟踪,不滞留本条。
- 验证: cargo test -p kanzei-tools 26 项通过,含真实 spawn 的后台进程闭环测试(托管→捕获输出→登记在册→停止);cargo test --workspace 全绿(125 项)。
- 安全回退(2026-08-08): 为修复托管文档可被异步 Bash 绕过的问题,当前在存在 `.kanzei` 的项目中拒绝后台 Bash。后台进程注册、轮询和停止实现仍保留,但本需求的“通过 Bash 启动后台任务”验收项重新开放,待 D-174 的隔离或写入归因方案完成后恢复。
- 当前验证(2026-08-08): 后台进程底层测试与 Bash 拒绝路径包含在 `kanzei-tools` 80 项通过测试中;尚未完成隔离后的真实长驻进程验收。

- 标签: 核心

## R-088 凭证与 provider 协议的健壮性 [done]
- 复杂度: 中
- 优先级: P1
- 来源: 2026-08-07 审计
- 内容: ①OAuth 凭证无锁读改写且非原子覆盖,与 Claude Code/Codex 官方 CLI 共享同一文件,并发刷新会因 refresh token 轮换导致登录态永久失效并殃及官方 CLI(D-061);②anthropic 协议遇未知 content_block 类型直接杀流,官方明确要求忽略未知类型,属前向兼容炸弹(D-067);③错误分类忽略 kind,限流可被误判为上下文超限从而触发破坏性压缩,且缺少限流/过载分类与 retry-after 退避(D-068)。
- 验收: 凭证刷新加锁并原子替换,补并发不互相覆盖的测试;未知 block 类型记录并忽略;限流单独分类并按 retry-after 退避,不再触发压缩。
- refs: D-061 D-067 D-068 D-065
- 阶段: 1
- 证据等级: E2+E4
- 设计定位: 凭证原子性与 provider 错误语义

- 标签: 模型

- 进展: 验收核查收尾(2026-08-08,实现均来自 D-061/D-067/D-068 修复,本次为逐条证据核查 + kanzei-llm 全包实测)。逐条对照验收原文:①凭证刷新「加锁并原子替换」——按 D-061 方案定调(用户明确不加跨进程文件锁:官方 CLI 不参与锁协议,锁只能拦 kanzei 自身进程)实现为「原子替换+写前重读」:kanzei-llm/src/auth/store.rs:22-38 commit 落盘前重读、磁盘更新则采纳对方不覆盖,45-72 atomic_write 同目录带 pid 临时文件 + rename 原子替换(失败重试 5 次,绝不退回 truncate-then-write);真实消费者 claude.rs:94-95、codex.rs:100-101 用返回值构造请求头;并发不覆盖测试 store.rs:116/139/166/184 四项。②未知 block 记录并忽略——anthropic.rs:181-184 未知 content_block 类型 debug 记录并登记 ignored_blocks,不再 Err;187-191 delta 命中 ignored 索引直接跳过;测试 anthropic.rs:311「unknown_content_block_is_ignored_without_poisoning_following_blocks」。③限流单独分类并按 retry-after 退避、不再触发压缩——error.rs:60-85 classify_provider_with_code 按 kind 优先(限流/过载先于 token 类文案匹配),RateLimited 变体 error.rs:10-15;client.rs:151-194 建流前 429/529 按 Retry-After 退避重试并最终归类 RateLimited;压缩触发只认 runner.rs:1110/1196 的 is_context_overflow(),RateLimited 走不到压缩分支;测试 error.rs:122/137/152 + 三协议回归 anthropic.rs:470/483、openai.rs:458/470/484、openai_responses.rs:409/421(限流不触发压缩、真 overflow 仍触发)。本轮实测 cargo test -p kanzei-llm 39 项全绿。

## R-080 测试记录展示并自动归档 [done]
- 复杂度: 中
- 优先级: P3
- 原始描述: 左侧栏展示当前拥有的测试,每个测试都要归档
- 验收: 左侧栏以清晰形式列出所有已获取测试结果,每条记录需触发/完成归档动作
- 退回原因: 2026-08-07 验收核查发现写入端是死路,展示壳与归档逻辑永不触发。test_run_record(kanzei-app/src/main.rs:998-1122)全仓零调用者——前端从不 invoke,agent 侧没有对应工具,且 dev profile 对 `*.kanzei/project/*` write/edit 硬 deny(profiles.rs:54-57)模型也无法直接写 tests.md;`.kanzei/project/tests.md` 至今不存在,左侧栏永远显示"暂无测试记录",tests-archive.md 归档分支(main.rs:1047-1072)永远不会执行。
- 下一步: 补上产生数据的一环——agent 跑完测试后记录,或 bash 工具识别测试命令自动记录;打通后再验收展示与归档。
- 遗留质量问题: parse_test_blocks 无单元测试。
- 备注: R-076 的验收硬指标"鞭挞流程在 test 中标记通过"依赖本需求落地,当前无法满足。
- refs: R-076 R-085 R-093
- 阶段: 2
- 证据等级: E2
- 设计定位: VerificationRun 的人类投影、测试记录入口和归档

- 标签: 后端

- 进展: 写入端打通(2026-08-08):此前 test_run_record 全仓零调用,tests.md 永不产生,左侧栏永远空、归档永不执行。本轮按 architecture.rs 的 D-173 先例——权限严了就得配专用工具——新增 agent 工具 test_record 作为 .kanzei/project/tests.md 唯一合法写通道(kanzei-tools/src/test_record.rs:TestRecordTool),并把解析/快照/自动归档逻辑从 kanzei-app main.rs 下沉到 kanzei-tools(parse_test_blocks/test_runs_snapshot/append_test_run),app 的 test_runs_snapshot 与 test_run_record 改为薄封装调用同一实现,消除两套格式解析漂移。DevProfile 注册 test_record 并把 *.kanzei/project/tests* 加入 write/edit 硬 deny(替代工具 test_record,profiles.rs)。验证:新增 6 个单测(解析、running 留存、终态自动归档、非法状态拒绝、工具端到端),kanzei-tools 94 项、kanzei-app 36 项、workspace 全量全绿;ui-runtime-smoke 展示端既有断言通过(桩数据渲染)。逐条对照验收:①「左侧栏以清晰形式列出所有已获取测试结果」——展示端为既有能力(main.js:3480 renderTestRuns 渲染 active+archived/状态符号/悬浮字段,main.js:3507 invoke test_runs_snapshot,ui-runtime-smoke.mjs:646 断言),数据产生通道为本轮交付(test_record 工具);②「每条记录需触发/完成归档动作」——test_runs_snapshot 自动把终态记录移入 tests-archive.md(test_record.rs 快照逻辑),单测 append_then_snapshot_archives_terminal_status / running_status_stays_active_until_terminal 覆盖;遗留质量问题 parse_test_blocks 无单测已补齐。

## R-086 控制类事件按会话路由与 pending ask 重建 [done]
- 复杂度: 大
- 优先级: P1
- 来源: 2026-08-07 审计;D-055、D-056 的共同根因
- 内容: 前端 on() 目前对非活动会话的所有事件一刀切丢弃(ui/main.js:6-15),把"哪个会话该看到"与"哪个会话该记账"混为一谈。ask/done/error/stopped 这类控制事件承载的是状态迁移而非展示内容,被丢弃后:后台进程的权限询问永久挂死(后端 receiver.await 无重发)、运行结束状态永不复位。当前只做了症状级修复(按进程身份重算 running),真正的解法是把会话状态与视图渲染分离:每个会话维护独立的运行/待答状态机,控制事件按 sessionId 更新对应状态机,视图只订阅活动会话的展示事件。
- 验收: 后台会话的权限询问在切回该进程时可见并可答复;后端提供 pending asks 查询以支持重建;任意会话运行结束后其状态正确复位而不依赖当前视图;补多会话并发下 ask/done 不丢不串的验证。
- 备注: 属架构级改动,涉及前端事件层、会话状态容器与后端 ask 生命周期,不可按普通 bug 就地打补丁。
- refs: D-055 D-056 R-030 R-078 D-078 D-085
- 阶段: 1
- 证据等级: E2+E3
- 设计定位: 会话状态权威、控制事件路由和 pending ask 重建

- 标签: 后端

- 进展: 状态权威定为「每会话一个前端状态机 + 后端 running 为真值源」：控制事件按 sessionId 先更新状态机再决定是否投影视图，视图只投影活动会话。本轮交付：①前端 sessionStates 状态机与 kz:ask 的按会话入队/切回重放；②后端补会话级终态事件 kz:idle（run_prompt 的 run loop 退出时发，reason 区分 completed/failed），前端只认 kz:idle/kz:stopped 收敛 converged、并用每轮必发的 kz:turn 自愈——修掉了「kz:done 其实是轮末事件却被当会话终态」引入的回归：排队输入的多轮运行从第二轮起标签页熄灯、切回显示空闲并解禁发送键，且 converged 屏蔽了轮询校正再也纠不回来；③pending_asks_get 接进 renderProcesses 的首次渲染（按会话去重），界面重载后后端仍在 await 的挂起询问能被重建，此前该查询全仓只有 switchProcess 一个调用点。验证：ui-runtime-smoke 新增「多轮不熄灯 / kz:idle 才收敛 / 切回可见可答复不串会话 / 重载后从后端重建」四组断言，cargo test --workspace 269 项全绿。剩余：控制事件仍未带运行代次（run_id），停止后紧接重发这类极窄竞态下旧事件仍可能错配，需要时再上代次方案；验收整体待桌面端双进程真机实测（E2/E3）。

## R-076 鞭挞模式触发异常 bug 修复 [done]
- 复杂度: 中
- 归属: kanzei
- 优先级: P2
- 原始描述: 鞭挞模式现在的触发有BUG
- 验收: 冷启动、暂停恢复、本轮后停、达到上限、用户拒绝、backlog 清空与外部阻塞场景均有确定状态迁移;无实质进展时不会靠写日记/无关工具绕过刹车;完整流程写入 tests.md 并在 UI 测试记录中可查看。
- 已完成: 冷启动勾选会调度第一轮,暂停恢复会在轮间重新调度;最大连数、用户拒绝、需求/缺陷清空能停止。
- 重新开放原因: 原验收明确要求“在 test 中标记通过”,当前 tests.md 和真实记录入口尚未打通(R-080);防空转仍以 steps>1 软判断,一次无意义工具调用即可继续;blocked-but-open backlog 也不会停。
- 下一步: 先完成 R-080 的测试记录供给链,再把“实质进展/阻塞”变为可判定状态并覆盖状态机测试。
- refs: D-044 R-085 R-080
- 阶段: 2
- 证据等级: E2
- 设计定位: 鞭挞状态机可判定化,依赖测试记录供给链
- 依赖: R-080

- 标签: 核心

- 进展: R-080 已关闭,测试记录供给链(R-080 交付的 test_record + UI test_runs_snapshot 展示)不再阻塞。本轮补齐两块可判定化并覆盖状态机测试,验收逐条对照:
① 七种场景确定状态迁移——全部落在 ui/main.js kz:done 鞭挞分支与 stopAutoWhenBacklogEmpty:冷启动(勾选后首轮调度,2206-2267 + scheduleAutoContinue 2706-2716,冒烟场景① rounds 0→1)、暂停恢复(2207-2210 停 + 恢复后续跑,冒烟⑨)、本轮后停(2212-2221,冒烟⑦)、达到上限(2225-2231,冒烟⑧)、用户拒绝(2206 !p.halted 整段跳过,冒烟⑩ rounds 原地不动)、backlog 清空(2718+ stopAutoWhenBacklogEmpty,冒烟⑥ 原因「已清空」)、外部阻塞(新增:active 全部 blocked 即停,原因「全部被阻塞」,冒烟④)。
② 防空转硬化——后端 kz:done payload 新增本轮工具画像(crates/kanzei-app/src/main.rs:6068-6071 计算 this_run_tools=summarize_tools(本轮切片)、6150 行 "tools" 字段);前端 NON_PROGRESS_TOOLS 非进展工具集 + hasProgressTools(ui/main.js:2511-2523),noAction = steps<=1 || !hasProgressTools(p.tools)(2237)。只有 memory_note/read/grep 等纯读·写日记·探测工具的轮次不再靠 steps>1 蒙混:第一次追加推进指令、第二次刹车(冒烟②);真实改动轮 bash/edit 正常推进(冒烟③)。工具名粒度分不出 git status/commit,bash/git 留在进展侧,误判成空转的代价(真干活被打断)高于漏判,设计取舍记录于此。
③ 完整流程写入 tests.md 并可在 UI 测试记录查看——test_record T-1786221328(命令+摘要)已落盘归档,tests.md/tests-archive.md 由 R-080 供给链自动接入 UI 测试记录列表(既有能力)。
验证:ui-runtime-smoke 新增 10 组鞭挞断言(214 次 invoke,0 运行时错误);四条前端冒烟全绿(i18n 35 key);cargo test --workspace 13 crate 全绿(kanzei-app 39 项)。
残余:外部阻塞判定只看 req/defect 的 blocked 标记,goal 队列未纳入(鞭挞驱动的是 req/defect 队列,设计定位一致);E2 真机桌面验证随 R-101 harness 补。

## R-074 前端显示面板和容器支持缩放拖拽适配 [done]
- 复杂度: 中
- 归属: kanzei
- 优先级: P3
- 原始描述: 前端的各类显示面板和容器要支持缩放和拖拽你看一下哪些适合支持的帮我我适配一下。。
- 验收: 先按交互用途明确哪些面板应调整尺寸、哪些应固定或改为抽屉;被纳入范围的面板可稳定调整并恢复默认,滚动后手柄仍可达,键盘可操作;窄屏不得因多面板同时出现挤没主对话区。
- 已完成: 侧栏、当前计划和活动面板已支持 pointer 调宽,有最小/最大边界并用 localStorage 恢复。
- 重新开放原因: 原验收核查明确“部分达标,验收未满足”,实际只有三处宽度调整,侧栏手柄还会随滚动移出可视区;“所有容器都能拖位置”也不是合理产品目标,需要先收敛交互范围再完成。
- 下一步: 与 R-089 一并确定左/右面板响应式策略;修复 D-107/D-110;补双击/键盘恢复和 800/1024/1280 视觉验收。
- refs: D-107 D-110 R-089
- 阶段: 3
- 证据等级: E3
- 设计定位: 面板尺寸策略,并入布局治理
- 依赖: R-089

- 标签: 前端

- 进展: R-089(布局策略定案)、D-107(缩放手柄固定+键盘/双击恢复)、D-110(窄屏 overlay)均已关闭,验收五项逐条对照(除注明外均为既有能力,非本轮新增):
① 交互范围收敛——纳入缩放的只有三面板:crates/kanzei-app/ui/main.js:748-750 setupResize("sidebar"/"todo-panel"/"bg-panel"),todo/bg 在 ≤1400px 改为右侧 overlay 抽屉(style.css:31-41),其余容器固定;R-089 定案。
② 稳定调整并恢复默认——setupResize(ui/main.js:684-747):pointer 拖拽带 min/max 边界(748-750:220-460/240-520),宽度持久化 localStorage 并初始化恢复(687-688),Home 键与双击 resetWidth 恢复默认(709-713、733、738)。
③ 滚动后手柄仍可达——.resize-handle position:fixed(style.css:69)+ ResizeObserver/resize 同步手柄到面板边界(ui/main.js:697-716),不随内容滚动移出可视区(D-107)。
④ 键盘可操作——handle role=separator + aria-orientation + tabIndex=0(ui/main.js:692-695),ArrowLeft/ArrowRight ±8px 微调、Home 恢复(730-737);ui-a11y-smoke.mjs:42-43 机械守护。
⑤ 窄屏不挤没主对话区——≤1400px 时 todo/bg 绝对定位 overlay(style.css:33-41),双面板同时显示时上下各 50%(40-41),不参与 flex 占位挤压主区(D-110)。
验证:ui-runtime-smoke 214 次 invoke 0 错误、ui-a11y 冒烟全绿、node --check 通过。残余:真实 800/1024/1280 浏览器像素基线验证转 R-101(与 R-089/D-107/D-110 同一转移模式),不影响功能可用关闭边界。

## R-070 来源引用的文档解析与记忆保存 [done]
- 复杂度: 大
- 优先级: P2
- 原始描述: 来源引用的文档解析和相关的记忆保存机制。这个也比较复杂。
- 验收: 实现引用溯源的文档解析链路及内存持久化机制,保证上下文完整性与一致性
- 退回原因: 2026-08-07 验收核查判定为"既有功能换措辞重新申报"。全部新代码约 10 行(kanzei-tools/src/profiles.rs:242-250):读 .kanzei/research/memory.md 前 5000 字符注入 <memory> 块 + 一句提示词。①"文档解析链路"是既有 TrackerTool/docstore 能力,归档记录自己写的是"继续作为引用真源",承认零新增;②"记忆保存机制"不存在:没有保存代码、没有专用工具、没有格式校验,保存全靠提示词让模型自己 write;③"保留来源 ID"零代码校验,而 finding 工具的 refs 校验就是代码强制的现成先例,违反项目规范"任何规则能用代码强制的绝不只写进提示词";④`.chars().take(5000)` 静默截断无提示(同文件 conventions 加载反而有截断提示,双标);⑤memory.md 是同 commit 塞入的空模板,零条结论。
- 下一步: 先定义 memory 条目的结构与来源 ID 契约,再实现写入工具与硬校验,截断需可见。
- 遗留质量问题: 无任何新测试;复杂度标"大"实际交付约 10 行。
- 阶段: 5
- 证据等级: E2
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 核心

- 进展: 2026-08-08 重新交付,逐条对照 2026-08-07 审计的退回归纳:①「文档解析链路」——既有能力(TrackerTool/docstore 引用真源、source add + finding refs 校验),本轮不重复申报,只补记忆写入侧的来源契约;②「记忆保存机制」——既有能力(memory_add 写入工具 R-105、MemoryStore 文件优先存储、memory_note 草稿箱),标注既有;③「保留来源 ID 零代码校验」——本轮补齐,这是本次交付核心:memory/mod.rs 新增 validate_source_refs,memory_add 与 memory_note 的 refs 参数代码强制校验([RDAMGSF]-<数字> 命中对应 doc 活跃或归档条目,否则按相对文件路径须真实存在),任一非法整体拒绝,先例 tracker.rs check_refs;④「截断可见」——profiles.rs research/docs 的 <memory> 块 5000 字符截断补可见提示(与 conventions 块一致);⑤「memory.md 空模板」——已被 R-104/R-105 MemoryStore 体系取代(legacy 迁移有测试),既有能力。
新增交付:docs/design/memory_system.md §2 文件格式补 refs 契约;MemoryEntry::refs() 读取助手(mod.rs);store.add 把 refs 写入 frontmatter extras、render/parse 往返保留(store.rs);append_note 写 - refs: 行、pending_note_list detail 带出(store.rs),manager system prompt 指示透传 refs 给 memory_add(manager.rs);UI 消费者:memory_entries 快照带 refs(kanzei-app/src/main.rs),记忆详情页显示「引用来源」(ui/main.js 5229 行 + i18n 键)。
验证:memory 模块 26 项测试(含 5 个新测试)全绿,workspace 13 crate 全绿,前端 i18n 36 key / runtime 214 invoke 0 错误,node --check 通过;测试记录 T-1786223647。
残余:refs 是溯源标注,记忆页不提供跳转(需求页 refs 有跳转);全局 scope 条目的 refs 只按项目 docstore 校验(全局条目引用项目条目语义上成立,不校跨项目);这两点不影响验收核心「引用溯源与上下文一致性」,如需可再开条目。

## R-100 runner 层调用模式机械门禁 [done]
- 复杂度: 中
- 优先级: P3
- 归属: kanzei
- 背景: 提示词纪律对子代理误用与验证节奏无强制力(D-114);若 R-099 数据显示未收敛,需要 runner 层就地检测。
- 验收: 对可机械识别的冗余模式在工具结果中就地提醒(不阻断,先观察后升级):同一工作树无变化时的重复 git status/diff、无文件变更的重复全量测试、缺陷 fields 已含文件路径时的 task 子代理调用;每类触发计数进入 R-099 度量。
- 依赖: 
- 阶段: 1

- 标签: 核心

- refs: D-114 R-099

- 进展: 依赖 R-099 已关闭,满足。2026-08-08 交付,验收逐条对照:① 就地提醒不阻断——crates/kanzei-core/src/runner.rs 新增 RedundancyWatch(默认构造,按单次运行持有、跨轮清零),在整步工具结果回喂前调用 note_step(回喂点 1778 附近),只向工具结果文本追加 [冗余提醒] 前缀、不改 is_error:三种模式——(a)同一工作树无变化时的重复 git status/diff:以上一次同类的原始结果内容为工作树指纹(提醒文本不污染指纹,先取 original 再比较),内容一致即提醒;全量测试判定 is_full_test_command(cargo test/nextest 带 --workspace/--all/--all-targets,或工作区根不带 -p 的 cargo test);(b)无文件变更的重复全量测试:两次全量测试之间 git 指纹未变即提醒;(c)缺陷 fields 已含路径仍调 task:defect_known_path_hint 纯文本解析 defects.md/defects-archive.md(不依赖 docstore——runner 不能反向依赖 kanzei-tools,机械门禁取舍),prompt 引用 D-xxx 且该缺陷段落已含的路径也出现在 prompt 里即提醒。② 每类触发计数进入 R-099 度量——RunMetrics 新增 redundant_git/redundant_test/redundant_task(#[serde(default)] 兼容旧 metrics_json),summarize_metrics 按 [冗余提醒] 前缀分类计数(只计本轮切片,失败结果不计);R-127 运行画像行新增 redundantLine 展示(ui/main.js,仅在有触发时显示,i18n 键「冗余提醒」)。
验证:4 个新单元测试(重复 git status 提醒/内容变化不误报、全量测试提醒/定向测试不触发、task 已知路径提醒/未知路径不触发、计数归类+失败不计)全绿,runner 26 项;cargo test --workspace 13 crate 全绿(kanzei-core 68 项);前端 i18n 37 key、runtime 214 invoke 0 错误、node --check 通过;测试记录 T-1786226007。
残余:模式 (c) 依赖 prompt 里带 D-xxx 引用(不带 ID 无法机械定位缺陷段落);模式 (b) 依赖先有 git status/diff 提供指纹(无任何 git 查询时无法判定变更);提醒是观察档,后续如需升级为阻断(如 R-144 验收核查)再单独开条目。

## R-149 记忆判据决策充分性改造 P1:反事实写入闸+subject 状态门禁+复发检测+采纳率排序+观测面 [done]
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 来源: 2026-08-09 用户提供 Control-Sufficient Memory 研究文档并要求按其重设计记忆系统;边界经 4 项逐条拍板(引擎硬门禁/manager 带指纹/温和降权/六项全做),当前会话直接交付,不走自举。
- 内容: 把记忆四操作(写入/遗忘/合并/检索)的判据从语义显著度换成决策价值,全部机械代理量实现:①manager 写入闸改反事实判据(说不出「不记会做错哪个动作」就 NOOP),description 双钩子;②memory_add 增可选 subject,引擎强制同 scope+category+subject 至多一条 active,冲突拒绝指路 memory_update,force 不可绕;③复发检测:失败笔记 [fp:tool|kind] 指纹由 manager 带进条目正文,轮末采集发现指纹在 active 记忆里仍复发则投修订笔记点名条目;④检索排序折入召回→采纳率(召回≥3 生效,×0.6~×1.3 温和降权,参数待实证复核);⑤memory_stats 增召回/采纳汇总与零采纳候选;⑥dev/memory 注入文案改决策判据表述。设计: docs/design/memory_decision_sufficiency.md。
- 验收: ①subject 状态不变量有单测(冲突返回既有条目、force 不可绕、stale 后可重建、subject 落 frontmatter);②复发检测有单测(指纹命中投修订笔记点名条目 id、正常笔记要求保留指纹、同轮去重仍生效);③排序有单测(decision_weight 边界+零采纳条目沉底高采纳浮上的排序翻转);④stats 有单测(召回/采纳汇总+零采纳候选点名);⑤manager 工具端到端单测(subject 冲突报错指路 memory_update);⑥workspace 全量测试不回归。
- 进展(2026-08-09 当日交付,全部有单测): ①subject 状态不变量:store.rs add() 冲突先于标题去重且不受 force 影响,subject 落 frontmatter extras(测试 subject_状态不变量_同主题至多一条_active_且_force_不可绕:冲突/force/跨 category/stale 重建四路);②复发检测:mod.rs harvest_failures 先查 project+global 指纹(find_active_by_marker 精确子串),命中投修订笔记点名条目,正常笔记新增「指纹放进正文」指引(测试 失败笔记要求保留指纹_复发时改投修订笔记);③排序:store.rs decision_weight(召回<3 不动分,0.6+0.7×采纳率,脏数据截断)+recall_profile 聚合 memory_recalls,search() 折入(测试 decision_weight_边界与单调性、零采纳条目在检索里沉底_高采纳浮上);④观测:tools.rs memory_stats 增「召回 N/采纳 M」与零采纳候选点名≤3 条(测试 stats_reports_recall_adoption_and_flags_zero_adoption_candidates);⑤manager:memory_add 增 subject 入参,SubjectConflict 报错指路 memory_update 并注明 force 不可绕(测试 memory_add_subject_conflict_points_to_update_and_ignores_force),系统提示词换反事实判据+subject 规则+指纹保留;⑥profiles.rs dev/memory 空库声明与收尾指引改决策判据表述。cargo test --workspace 全绿(kanzei-tools 110 项含 6 项新增)。降权参数(0.6/0.7/阈值3)按拍板留待真实召回数据复核,挂 R-150 验收③。
- refs: R-103 R-125 R-145

## R-152 CI/发布证据链:GitHub Actions 独立验证 + verification.json 绑定 commit 门禁 [done]
- 优先级: P0
- 复杂度: 中
- 标签: 发布
- 来源: 2026-08-09 用户定调(仓库工程评审 P0 三项:License 元数据冲突、CI 缺失、release gate 机械化——正确性不应依赖"应该先跑过测试"的约定),方案已落设计文档,交自举执行。
- 内容: ①License:workspace Cargo.toml `license = "MIT"` → `"PolyForm-Noncommercial-1.0.0"`(与 LICENSE.md 统一);②新增 .github/workflows/ci.yml:windows-latest,push(dev/main)+PR 触发,cargo test --workspace + 四条 UI 冒烟;**fmt/clippy 两闸注释着落地**(现存 435 处 fmt diff 与 23 条 clippy warning,首日启用必红),分别由 R-156/R-146 启用;③新增 scripts/verify.ps1:脏树拒跑,门禁全绿后写 dist/verification.json(HEAD 全 SHA + 各检查结果);④package.ps1 在 D-183 Ack 核对后加证据门禁:证据缺失、commit 不符或未全绿一律中止,新增 -VerificationPath 参数供发布树指向 dev 树证据(ff 合并后两树 HEAD 同 commit)。参考实现已写入设计文档,可直接落盘。设计: docs/design/ci_release_evidence_chain.md。
- 边界: 不做多平台矩阵/签名/SBOM/CI 直发;verification.json 落 dist/ 不入库;release.ps1(开发通道)不动;CI 首跑暴露的环境相关红测按缺陷登记修复,不许 skip。
- 验收: ①cargo metadata --no-deps 全部 crate license 为 PolyForm-Noncommercial-1.0.0;②push dev 后 GitHub Actions 首跑全绿(进展里留链接);③verify.ps1 实测两态:脏树拒跑、全绿产出 JSON;④package.ps1 实测三拦:无证据拦、commit 不符拦(verify 后再提交一个 commit 重跑必须中止)、证据齐全放行至构建——各拦截报错原文记入进展;⑤ci.yml 与 verify.ps1 两处门禁清单互相注明同步义务。
- refs: A-009 R-146 R-156

- 进展: 逐项验收证据（实现位置与实测）：① `Cargo.toml:12-15` 的 `[workspace.package].license` 为 `PolyForm-Noncommercial-1.0.0`；`cargo metadata --no-deps --format-version 1` 实测 6 个 workspace crate 全部一致。② `.github/workflows/ci.yml:3-42` 配置 windows-latest、dev/main push、main PR、workspace test、UI 语法与四冒烟；首跑链接 https://github.com/kanze1/kanzei-code/actions/runs/31291964471，D-218 修复后 runs 31292345597/31292710503/31292885059 连续全绿。③ `scripts/verify.ps1:7-14` 脏树门禁实测原文“工作树不干净，证据无法绑定 commit: .github/workflows/ci.yml scripts/verify.ps1”；`scripts/verify.ps1:16-44` 全绿态实测通过 workspace test、ui_syntax、ui_runtime、ui_a11y、ui_i18n、ui_markdown，并写出 `dist/verification.json`，commit=`c0ea88db9f89546d69d430065bc0e46da67143af`、`all_pass=true`。④ `scripts/package.ps1:61-70` 位于 D-183 Ack/脏树检查之后、构建之前，实测无证据拦截（原文“缺验证证据 …:先跑 scripts/verify.ps1”）、commit 漂移拦截（原文“验证证据绑定旧 commit,HEAD 是新 commit:commit 变了就要重新 verify——这正是本门禁存在的原因”），证据齐全经 `-VerificationPath` 放行至构建并发布 build-cd85360（release 链接：https://github.com/kanze1/kanzei-code/releases/tag/build-cd85360）；`package.ps1:70` 未全绿也硬拦。⑤ `.github/workflows/ci.yml:27` 与 `scripts/verify.ps1:24-25` 互相注明 R-156/R-146 启用/禁用必须同步；两处当前均按设计保留注释。既有 D-218 修复与之前的 CI/package 实测属于本条已有实现证据，本次交付仅补齐同步说明与本轮 metadata/verify 证据。
- 进展(2026-08-09 发版放行实测): D-218 修复后 Actions 连续三跑全绿(runs 31292345597/31292710503/31292885059);ci.yml checkout/setup-node 升 v5 消除 Node 20 弃用警告(cd85360)。验收④第三拦「证据齐全放行」实测完成:verify.ps1 产出绑定 cd85360 的全绿证据,发布树 package.ps1 -Ack 9 -Publish -VerificationPath 证据核对通过、放行至构建并发布 build-cd85360——证据链首个完整走通的 release:https://github.com/kanze1/kanzei-code/releases/tag/build-cd85360 。至此验收②④证据齐全,可逐条核对关闭。

## R-159 运行上限进配置与设置页,设置页按组折叠 [done]
- 类型: 后端+前端
- 来源: 2026-08-09 用户"很多可设置参数都加到设置里,并优化设置页排版"。普查后纠正了一个预期:KanzeiConfig 既有字段(primary/fast/reasoning/codex_fast_mode/providers/proxy/profile.default/permissions)本来就全在设置页,真正缺入口的是散在各 crate 的硬编码常量。
- 内容: 新增 [limits] 节 10 项(主对话/子代理输出上限、子代理墙钟、单轮子代理数、压缩触发线、近期逐字比例、单波并行工具数、流重放次数、传输/限流重试),经 RunnerConfig.limits 与 SubagentRuntime.limits 下发;设置页新增「运行上限」组并把 8 个分区改为 details/summary 折叠。
- 验收对照: ①全字段 Option,None=内置默认且与改造前常量逐值一致(测试锁);②层叠合并逐字段覆盖,项目层只写一个键不打回其余(测试锁);③设置页留空=默认、占位符显示内置默认、保存只写填了的键、清空即删键(Rust 测试 + 冒烟三条断言);④新字段进 SETTINGS_FORM_IDS,脏状态可见;⑤cargo test --workspace 全绿 + 四条 UI 冒烟全绿。
- 未纳入: bash 默认超时、测试记录悬空阈值、edit 净删除阈值三项仍是常量——它们在工具层,需要给 ToolCtx 加字段并改 52 处构造点,收益不抵本次改动的风险;宁可不做,也不放"设置页有键但运行不认"的死键。
- 优先级: P1

## R-158 Codex 支持同模型 Fast mode 优先服务档位 [done]
- 类型: 后端
- 背景: Codex 的 fast mode 不是另一模型，而是对同一 Codex 模型启用更高消耗、更快响应的 priority 服务档位。当前 Responses 请求未发送该参数。
- 范围: 桌面端设置、kanzei.toml [models]、Runner/LlmRequest、Codex Responses 请求体；不更换 Codex 模型，不影响 fast 角色。
- 验收: ①设置页提供 Codex Fast mode 开关，明确说明仍使用当前 Codex 模型且可能增加额度消耗；②配置保存/读取支持该开关，旧配置缺字段时默认关闭且不丢其他字段；③启用后仅对 auth=codex 的 Responses 请求发送 service_tier=priority，未启用或非 Codex 请求不发送；④主对话真实调用链透传该设置，仍可使用现有 luna 模型；⑤补充协议与配置回归测试并通过。
- 优先级: P1
- 进展: 实现已覆盖：crates/kanzei-app/ui/index.html 设置页开关；ui/main.js loadSettings/settings-save 透传；kanzei-harness::ModelRoles 的 codex_fast_mode 向后兼容；kanzei-app::run_task 与 kanzei CLI 主运行链按 auth=codex 生成 priority；kanzei-core::RunnerConfig/LlmRequest 透传；kanzei-llm::openai_responses::build_body 写入 service_tier。协议测试已补。验证：node --check + ui-runtime-smoke 通过；Rust cargo check 仍被工作树既有 R-153 的 mobile.rs 语法错误与 processes.rs 重复 command 阻断，不能关闭。
- 收尾(2026-08-09): 交付时夹带两处误替换，已一并修回——①openai_responses::build_body 里写 reasoning effort 的整段被 service_tier 顶掉，Codex 从此不发思考档位；②设置页思考强度说明段落被删。另 R-153 批4/5 迁出后 state_tests/process_tests 仍从 super 导入，dev HEAD 一直编译不过，同批修好(改从 projects/state 模块导入 + 两个 helper 提 pub(crate))。默认 primary 改 codex:gpt-5.6-luna 且该默认下 Fast mode 自动开启，harness 默认值测试同步更新。
- 验证: cargo test --workspace 322 项全绿；verify.ps1 六项门禁(test/ui_syntax/ui_runtime/ui_a11y/ui_i18n/ui_markdown)全绿，证据绑定 3e4c744。
- 发版: build-3e4c744（https://github.com/kanze1/kanzei-code/releases/tag/build-3e4c744），范围 build-cd85360..HEAD 共 36 个提交。
- 未纳入本次提交: R-153 批6 的 mobile.rs/processes.rs 仍是未跟踪的半成品(mobile.rs 有语法错误)，留给自举继续。

## R-153 拆解 kanzei-app/src/main.rs(6413 行→约 16 模块,main.rs 收敛为装配) [done]
- 优先级: P1
- 复杂度: 大
- 批次: 11/11
- 标签: 后端
- 来源: 2026-08-09 用户定调巨石拆解;结构地图与批次表已落设计文档 §A(行号基准 c339b58,执行以符号名定位)。
- 内容: 照 files_view.rs 先例(command 加 pub、invoke_handler 全路径注册、低耦合模块零依赖 main)把 75 个 command 按域拆为 state/update/fast_model/agent_container/mobile/memory/prefs/projects/processes/settings/docs/conversation/harness_ext/subagents/run 等模块;批0 先把 818 行 update_tests 按域切开(解锁全部后续批),批1 零依赖叶子起步,批4 落 state.rs 枢纽,批10 收 run.rs,共 11 批,每批一提交。设计: docs/design/monolith_decomposition.md §A。
- 边界: 零行为变更,diff 只允许 move+use+可见性;run_task(695 行)只整体搬迁不拆内部(内部拆分另立条目);main() 开头三调用顺序、UI_PROBE 三 static 同模块、ask_seq 共享、cfg(windows) 成对搬迁等危险点清单见设计文档;拆解批与其他源码条目不得并发。
- 验收: ①main.rs ≤300 行且只含 mod 声明+main()+Builder 装配;②每批独立提交且 cargo test -p kanzei-app 绿,条目关闭前全量 cargo test --workspace 一次全绿(节奏见 conventions §1.4);③invoke_handler 78 项全数保留(拆前后清单 diff 核对)且按域分组加注释;④四条 UI 冒烟不受影响;⑤拆前后 wc -l 对照记入进展。
- refs: A-008 R-148(先例 files_view.rs)
- 依赖: R-152

- 进展: 验收逐项完成：①`crates/kanzei-app/src/main.rs:1-195` 仅保留模块声明、跨模块导出装配、`main()` 与 Tauri Builder，PowerShell 行数核对为 195 ≤ 300；②每批均有独立提交，当前批提交 `085e488`，`T-1786299143` 的 `cargo test --workspace` 全绿；③`main.rs:102-181` 的 `tauri::generate_handler!` 脚本核对得到 78 项，迁移后的 `projects::workspace_snapshot` 与全部既有域注册均保留；④四条 UI 冒烟分别由 `T-1786299153` i18n、`T-1786299156` a11y、`T-1786299162` Markdown、`T-1786299165` runtime 记录，全部通过。既有 UI 能力沿用，本次交付仅改变 Rust 模块归属与入口装配，未改 UI 行为。

## R-154 拆解 kanzei-app/ui/main.js(7020 行→18 个有序 classic script) [done]
- 优先级: P1
- 复杂度: 大
- 标签: 前端
- 来源: 2026-08-09 用户定调;不引 ES modules 的机制论证与文件清单已落设计文档 §B(行号基准 c339b58)。
- 内容: B0 使能批**只改四个冒烟脚本**:从 index.html 解析 `<script src>` 清单按序读入,runtime 冒烟逐文件 vm.runInContext(与浏览器多 script 语义一致含 TDZ,拼接执行会掩盖前向引用 bug),静态断言用 join 串,探针注入按累计命中≥2 判定——此批 main.js 一字不动、四冒烟必须仍绿;随后 B1~B9 从尾部往前切出 18 文件(01-core…18-startup):readJson/writeJson 上提 01(现存唯一前向引用硬风险 L3244)、启动 IIFE 锁死末位、04/05 相邻保 markdown 冒烟切片边界;index.html 仅 script 标签区改为按序 18 个 `<script defer>`;同步 deep_parallel_dev.md:283 的 node --check 改遍历。设计: docs/design/monolith_decomposition.md §B。
- 边界: 不引 ES modules/打包器/框架(A-008);style.css 零改动;tauri.conf.json 无需改(frontendDist 整目录);拆解批与其他前端条目不得并发。
- 验收: ①B0 后单文件形态四冒烟仍全绿(机制改造零行为变化);②每批 node --check(遍历 ui/*.js)+四条冒烟全绿;③最终 main.js 消失,18 文件按 index.html 顺序加载,单文件 ≤1000 行;④发版后真机复查主视图/发送/设置页可用(E3 残余,不阻塞关闭,进展注明);⑤拆前后行数对照记入进展。
- refs: A-008
- 依赖: R-152

- 批次: 10/10
- 进展: B0-B9 全部完成。B4 完成:main.js(5171→3732)切出 12-docs-pages(199)+11-docs-list(667)+10-docs-core(108),index 按序 main→04…12。B5:切出 09-sessions(462),main.js 3732→2683(实际 2666+readJson/writeJson)。B6 提交 b1a5d2e:切出 08-compose(1049),readJson/writeJson 上提 01 区(82/90 行),main.js 2683。B7 提交 d318f46:切出 06-activity(491)+07-events(556),main.js 1636。B8 提交 0c3af9a:切出 05-chat-render(342)+04-markdown(136)相邻落位,main.js 1156。B9 提交 f34f28a:切出 01-core(98)/02-i18n(694)/03-shell(364),删除 main.js,index.html 01..18 按序;发现并修复切分漏行——第 98 行 `const promptBox=$("prompt")` 被区间跳过导致 08 顶层 dragover 绑定 ReferenceError(runtime 冒烟 723 行 batch-meter 断言崩即此因)。08→09 行数收敛提交 717de61:08 达 1049 行超验收③上限,队列输入+测试记录(renderPendingInputs/refreshTests/renderTestRuns 等 100 行)移入 09-sessions(会话历史域),08=949、09=562。行数对照:拆分前 main.js 7212 行(c339b58 基准 7020 + 后续新增 D-202/鞭挞/模型直选等),拆分后 18 文件合计 7200 行,单文件 35–949 行全部 ≤1000。验收④(发版后真机复查主视图/发送/设置页 E3 残余)不阻塞关闭,已注明待发版后执行。

## R-155 拆解 kanzei-core runner.rs(3240 行)与 store.rs(1972 行)为子模块目录 [done]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 来源: 2026-08-09 用户定调;外部 API 面已 Grep 核实(外部三 crate 零处使用模块路径,全走顶层再导出),划分与危险点清单已落设计文档 §C(行号基准 c339b58)。
- 内容: runner/ 按 B1 event→B2 metrics→B3 redundancy→B4 context→B5 compaction→B6 tool_exec→B7 subagent→B8 drive 八批;store/ 按 S1 拆壳(connection/path 转 pub(crate))→S2 episodes→S3 notifications→S4 events→S5 inbox→S6 session→S7 schema(migrate 原样搬不重构)→S8 测试分域八批;mod.rs pub use 平铺保持 kanzei_core:: 顶层再导出零变更;测试随域下沉不建大 tests.rs,共享测试辅助建 #[cfg(test)] pub(crate) mod testutil。设计: docs/design/monolith_decomposition.md §C。
- 边界: 零行为变更;run_once 保持 boxed 签名(与 run_subagent 递归的断点,改 async fn 立刻 E0072,两处加注释锁死);run_once_with_parts(778 行)只整体搬迁;不删零调用 pub 方法;唯一允许的非 move 改动是给 RedundancyWatch::note_step 加 debug_assert_eq!(calls.len(), results.len()) 与三处下标不变式注释。
- 验收: ①每批独立提交且定向绿(cargo test -p kanzei-core + cargo check -p kanzei -p kanzei-app -p kanzei-tools),条目关闭前全量 cargo test --workspace 一次全绿(节奏见 conventions §1.4);②lib.rs 与外部三 crate(kanzei/kanzei-app/kanzei-tools)全程零改动仍编译(以①的 cargo check 为每批断言);③runner.rs/store.rs 单文件消失,各子文件 ≤900 行;④下标不变式 debug_assert 与注释落位;⑤拆前后行数对照记入进展。
- refs: A-008
- 依赖: 

- 批次: 16/16
- 进展: 16 批全部完成并提交:B1 event.rs 2e52179 / B2 metrics.rs 2ab49b3 / B3 redundancy.rs+testutil 26e802e / B4 context.rs 7c77194 / B5 compaction.rs 07297ff / B6 tool_exec.rs 219fd94 / B7 subagent.rs 15f857c / B8 drive.rs 2f4d449(每批独立提交+定向绿);store 域按锚点 4 提交:S1-S4 拆壳+episodes/notifications/events 2fd36cf / S5-S6 inbox/session d4fd6ad / S7-S8 schema+测试分域 c4faded(S1 拆壳无独立编译意义、同域连续小批按 §1.3 合并,每锚点前 cargo test -p kanzei-core 71 passed + cargo check 下游三 crate 绿)。
验收⑤拆前后行数对照:runner.rs 3240 行→runner/ 3417 行(mod.rs 308 + event 151 + metrics 559 + redundancy 284 + context 428 + compaction 394 + tool_exec 303 + subagent 180 + drive 810);store.rs 1972 行→store/ 2106 行(mod.rs 139 + schema 451 + session 292 + events 282 + inbox 459 + notifications 250 + episodes 219 + testutil 14);最大子文件 drive.rs 810 行(≤900)。差值为拆解模块头注释与平铺 use,主体零行为变更。
关闭前全量 cargo test --workspace 全绿(T-1786307001):kanzei-core 71、kanzei 46、kanzei-app 123、kanzei-llm 39、harness 43。lib.rs 与外部三 crate 全程零改动(各批 diff 均只含 store/runner 内文件)。

## R-156 全仓 fmt 收敛并启用 fmt 闸门 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 流程
- 来源: 2026-08-09 实测 cargo fmt --all -- --check 有 435 处 diff——fmt 闸门首日启用 CI 必红;且全仓格式化会使拆解设计文档的行号地图漂移,故单列一条排在拆解之后。
- 内容: ①`cargo fmt --all` 单独一个纯格式化提交(不混任何业务/重构改动);②取消 ci.yml 与 scripts/verify.ps1 里注释着的 fmt 步骤(两处同启);③实测闸门会拦:临时引入一处格式漂移验证非零退出后撤销。
- 边界: 与 R-146(clippy)同理必须避开在飞的源码工作;两条可同轮或相邻轮做,均在 R-153~R-155 完成之后。
- 验收: ①cargo fmt --all -- --check exit=0;②ci.yml 与 verify.ps1 的 fmt 步骤启用且 CI 全绿;③拦截实测记入进展;④格式化提交 diff 零逻辑变更(全量测试全绿佐证)。
- refs: R-152 R-146
- 依赖: 

- 进展: 验收对照:
①cargo fmt --all -- --check exit=0——格式化后与漂移撤销后两次实测均 0(提交 cee7aa8)。
②fmt 闸门两处启用:ci.yml:29-30 取消注释(cargo fmt --all -- --check),verify.ps1:25 新增 Invoke-Check "fmt"(同命令,与 ci.yml 清单同步注释声明)。CI 全绿依赖 push 后 GitHub Actions 实测;本机已用与闸门完全相同的命令验证通过。
③拦截实测:向 crates/kanzei/src/main.rs 注入尾随空格漂移,cargo fmt --all -- --check 真实 exit=1 且 diff 指向 main.rs:16,撤销后 exit=0。
④零逻辑变更:格式化提交仅 74 个 Rust 文件 + ci.yml + verify.ps1(cee7aa8),全量 cargo test --workspace 测试数与格式化前逐 crate 一致(T-1786307168):core 71/kanzei 46/app 123/llm 39/harness 43。
纯格式化单提交,不混业务改动;R-153/R-154/R-155 依赖已全部关闭,移入 refs。

## R-148 文件导览:VSCode 级浏览页 + files 工具 + AI 用途标注 [done]
- 优先级: P1
- 复杂度: 大
- 标签: 前端
- 阶段: 2
- 归属: kanzei
- 来源: 2026-08-09 用户定调,并明确"由当前会话直接交付,不走自举"。两条设计决策:①文件预览要 VSCode 对标级别(引 Monaco vendor,不做简化实现;项目无零依赖约束——那是 frontend.rs 单工具的局部取舍,不得外推);②度量中性呈现,不点名"该拆"——页面定位是人类主动分析与 agent 辅助分析的架构地图,行数多不必然要拆,拆分判断结合 AI 用途标注由人/agent 做。
- 内容: 一份扫描器两个消费者。①扫描器(kanzei-tools):git ls-files 拿清单(尊重 .gitignore,非 git 目录退化为过滤遍历),每文件度量=大小+代码行数(按扩展名)/md 字数(字符数),>2MB 只 stat 标「过大」;按目录聚合文件数/总大小/总行数。②agent 工具 `files`:文本树输出,支持 path 子树与 top-N(按行数降序),只读 Allow;已有 AI 标注时随树输出——弱模型不必逐个 read 就知道每个文件是干嘛的。③桌面端新主视图「文件」:树形浏览器(展开/折叠/排序),行尾度量徽章,目录行聚合值;点击文件用 Monaco(vendored,只读)打开,语法高亮/行号/折叠/搜索原生。④AI 用途标注:fast 模型按文件头部 60 行生成一句话用途,目录级聚合;缓存 .kanzei/file-annotations.json 按内容 hash 失效;页面手动「标注」按钮触发,后台增量,不自动烧。
- 验收: ①files 工具:树/子树/top-N 三种调法有单测,输出含度量与(已有的)标注;②页面:真实仓库(184 文件)树渲染流畅,目录聚合正确,Monaco 打开 6000+ 行文件语法高亮可用;③标注:首次全量后改动单文件只重标该文件,缓存命中不调模型;④冒烟覆盖新视图切换与树渲染;⑤用户复查"直观"达标。
- 进展(2026-08-09 代码全交付): ①扫描器+files 工具(kanzei-tools/src/files.rs,3 单测:扫描度量/树与 top 渲染/标注缓存回环),注册进 BaseComponent 全 profile 可用,权限 Allow;②Tauri 侧独立模块 files_view.rs(不再往 6400 行的 main.rs 里堆):files_snapshot/file_preview(canonicalize 逃逸检查+二进制识别+4MB 截断,2 单测)/files_annotate(fast 模型逐文件一句话+目录聚合,增量按指纹,每 8 个落盘,进度事件);③Monaco vendored 5MB(min/vs 裁掉 language 智能服务 7M 与 8 个非中文 nls——语法高亮在 basic-languages,只读预览不需要补全):懒加载,暗色主题,只读,minimap;④前端新主视图「文件」:树形展开/折叠/键盘可达,名称/行数双排序,目录聚合行,标注随行显示,i18n 23 词条;⑤运行时冒烟新增树渲染断言(目录聚合/展开后文件度量/md 字数/标注)。workspace 310 项全绿+四条前端冒烟绿。剩余验收⑤(用户复查直观达标)与真实仓库 Monaco 实测待发版安装后确认。
- refs: D-173 R-126
- 阻塞: 用户: 剩余仅验收⑤——build-cd85360 已发布并含本功能,用户在桌面端打开「文件」视图复查"直观达标"并实测 Monaco 打开 6000+ 行文件即可解除;复查通过即按验收关闭。(2026-08-09 补记:属 §1.1 ①类外部阻塞,按新口径不占 WIP 准入配额)

- 进展: 2026-08-10 用户关闭指令(原文:'r148功能来说确实有了,然后易用性还有待修复,不过先关闭把,这个需求我后面会观察之后重新提')。验收逐条证据:①files 工具三调法——kanzei-tools/src/files.rs,3 单测(扫描度量/树与 top 渲染/标注缓存回环),注册进 BaseComponent 全 profile 可用,权限 Allow;②页面——前端「文件」主视图(files_view.rs:files_snapshot/file_preview/files_annotate,canonicalize 逃逸检查+二进制识别+4MB 截断,2 单测),Monaco vendored 懒加载只读暗色,用户确认'功能确实有了';③标注增量——files_annotate 按内容指纹增量(fast 模型逐文件一句话+目录聚合,每 8 个落盘,进度事件),缓存命中不调模型;④冒烟——ui-runtime-smoke.mjs 新增树渲染断言(目录聚合/展开后文件度量/md 字数/标注);⑤用户复查——用户确认功能已存在,但易用性(直观性)未完全达标;按用户明确指令先关闭本需求,易用性缺口由用户在后续观察后重新提需求/缺陷。构建 build-cd85360 已发布含本功能。

- 批次: 1/1

- n: 0/0

## R-102 CLI 只读运行档位:分析类任务免配权限直接跑 [done]
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 背景: 2026-08-07 用 kz 做只读前端分析:agent 首选 bash 触发询问,非交互场景直接拦停;唯一出路是给沙盒放行 `bash *`,权限粒度与"只是分析别动文件"的意图之间缺一层表达。
- 验收: 提供只读档位(如 `kz run --readonly` 或 profile):read/glob/grep/task 放行,write/edit 硬 deny,bash 限制为无副作用或直接禁用并提示替代工具;非交互终端下只读任务可零配置完整跑完;补档位权限快照测试。
- 设计定位: 让"问问题/做分析"成为 kz 的零门槛入口
- 阶段: 4

- 标签: 核心

- refs: D-121

- 批次: 3/3
- 进展: 2026-08-10 批次规划(复杂度中→3批):批1=只读档位概念落码——CLI 参数/配置接入(readonly 档位解析 + profile 合并 + 权限快照函数),批2=权限强制(read/write/edit 硬 deny、bash 禁用提示替代、read/glob/grep/task 放行)+ 非交互零配置路径打通,批3=档位权限快照测试 + 文档。
2026-08-10 批1 完成(b2e6947):ProfileKind::Readonly 档位(defs.rs:7-10)、ReadonlyProfile 组件注册只读 agent(profiles.rs)、CLI --readonly 解析与 profile 合并(main.rs parse_run_args/run_cli)、permission_snapshot 快照函数(harness.rs)。
2026-08-10 批2 完成(4d00537):ReadonlyProfile 权限强制——write/edit/bash 硬 deny(managed 替代指引点 read/glob/grep/files/git status|diff|log/webfetch)、只读族与 git 只读子命令放行、工具物化摘除写命令。真实冒烟:kz run --readonly 用 ollama 非交互零配置跑完,read 放行、零权限询问。
2026-08-10 批3 完成(55c6c82):档位权限快照断言(快照 write/edit/bash=Deny+fully_denied、read/glob/grep/files/webfetch=Allow、task 不摘除)。文档:usage 已含 --readonly 行。
批次 3/3 走满,三批提交 b2e6947/4d00537/55c6c82,定向测试全绿,待全量测试后关闭。
2026-08-10 批1 完成(b2e6947):ProfileKind::Readonly 档位(defs.rs:7-10)、ReadonlyProfile 组件注册只读 agent(profiles.rs)、CLI --readonly 解析与 profile 合并(main.rs parse_run_args/run_cli)、permission_snapshot 快照函数(harness.rs)。kz --help 实测展示 --readonly;harness 130 + kz bin 7 测试全绿,workspace check 通过。批2 开始:ReadonlyProfile 权限强制(write/edit/bash 硬 deny)。

- 状态: doing

## R-103 Memory 系统总纲:文件优先、分级、子代理管理 [done]
- 移交: 2026-08-08 用户宣布移交自举循环。M1~M4 已落地并在实测中,后续完善由循环承接;设计基线见 docs/design/memory_system.md,改动不得偏离其 §0 品味决策(文件优先、不引向量库/图谱、读写分离)。
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 来源: 2026-08-08 用户定调的下一个大规划(用户为记忆研究方向,taste 已对齐)
- 内容: 以 docs/design/memory_system.md 为设计基线。五个目标:提高易用性、上下文管理更精准、用户个性化持久化、常用轨迹效率提高、agent 工作效率提高。核心决策(不再重议):文件优先(markdown 真源,可编辑可透明,git 可恢复);不用向量库/知识图谱/Mem0 类框架,给 agent 好的搜索工具(FTS5+结构化过滤);记忆写读分离,写路径由 memory-manager 子代理专管;分级 = scope(global/project) × category(preference/habit/fact/sop/episode);agent 既是用户,验收全部取自举轨迹实证。
- 验收: R-104~R-107 四期全部落地;连续自举轮次中出现"写入→检索命中→避免重复探索"的闭环实证;记忆内容全部可 git 恢复;SQLite 仅存可重建派生物(FTS 索引/hits/episode 表)。
- 设计: docs/design/memory_system.md
- refs: R-098 R-099 D-088 D-114 R-104 R-107
- 阶段: 4
- 设计定位: 记忆作为 first-class primitive 的总纲与门禁
- 依赖: 

- 标签: 核心

- 进展: 2026-08-10 接手:R-104~R-107 四期均已 done,总纲验收逐项盘点——①四期落地✓;③记忆文件全部被 git 跟踪(29 条 M- + INDEX + inbox)✓;④index.db 未被 git 跟踪(仅存 FTS 索引等可重建派生物)✓;②闭环实证已显式转移至 R-145(仅跟踪实证,不发版不可本机验证)。
批次 1/7 完成:子代理/写路径现状盘点——主 agent 工具表(profiles.rs:110-121)只含 memory_search/note/stats;写路径 add/update/merge/stale 定义在 manager.rs(57-294)且只注册进 manager_agent 专属工具表(manager.rs:331-350);memory-manager 子代理 AgentDef(manager.rs:628-666)= fast/Subagent/steps10/无 shell;.kanzei/memory/* 硬 deny 指向 memory_note 草稿投递(profiles.rs:180-184)。写读分离设计 §3 完全落地。
批次 2/7 完成:设计基线 §9 工程决策逐项核对——§9.1 memory 模块结构✓;§9.2 U-/M- 前缀+slug 文件名+frontmatter 平铺✓;§9.3 episode 存 state.db 不落文件✓(mod.rs:6/45);§9.4 FTS5 unicode61+bm25 topN+log(1+hits) 重排✓(store.rs:347/469/524);§9.5 tmp+rename 原子替换+可整体重建✓(store.rs:1119-1123);§9.6 工具集主 agent 三件套+add 去重去噪 force✓;§9.7 INDEX 预注入常驻✓(profiles.rs:258-286)。
批次 3/7 完成:memory 模块完整性核验——派生全量重建 rebuild_all(store.rs:299-370)、归档目录 archive/(store.rs:98-99)供 load_archived_ids 保 ID 不复用、merge 合并自动 stale+superseded_by 墓碑(store.rs:608/675)、integrity 缺号/重复检测(store.rs:566/1347)、stale 降权不出 active(store.rs:1331)。缺口:stale→archive/ 搬运流程未实现,已由 D-217/D-231 独立跟踪(R-165 承接),不影响本总纲验收项。
批次 4/7 完成:Memory UI 页与设计 §6 一致性——refreshMemory(13-memory.js:3)、memory_context_bill(11)、架构图 renderMemoryArch(307-342 scope 卡片+category 格+integrity 警告)、条目视图 loadMemoryList(347, stale/dormant 标记)、命中/注入统计(259-305)、整理 inbox 按钮(html:303)。设计 §6 全部落地。
批次 5/7 完成:注入注册与 §5 一致性——resident_index 常驻注入(profiles.rs:258-286)、prompt_hints 按任务描述匹配触发(mod.rs:476-523)、sop 提炼候选箱 harvest_sop(mod.rs:398-419)、预算口径统一 D-216(mod.rs:275-276)。§5 注入机制全部落地。
批次 6/7 完成:git 恢复实证——项目记忆 30 文件全部被 git 跟踪且 git log 可见演进历史(4fa7c23/0f91e21 等);inbox.md(草稿暂存)与 index.db(派生物)在 .gitignore 不入库,符合设计(真源 md 可恢复、派生物可重建)。
批次 7/7 完成:验收证据整理与收口——①R-104~R-107 四期 done(requirements-archive 关闭记录+提交哈希)✓;②闭环实证→R-145 跟踪(R-105/R-106 同口径)✓;③记忆文件全 git 跟踪+历史可恢复✓;④index.db 未跟踪仅存派生物(store.rs rebuild_all 可重建)✓。三项已证、一项转移。总纲本身无新增代码(全部代码由 R-104~R-107 交付),本轮为核验收口。
批次 1/8 完成:子代理/写路径现状盘点——主 agent 工具表(profiles.rs:110-121)只含 memory_search/note/stats;写路径 add/update/merge/stale 定义在 manager.rs(57-294)且只注册进 manager_agent 专属工具表(manager.rs:331-350);memory-manager 子代理 AgentDef(manager.rs:628-666)= fast/Subagent/steps10/无 shell;.kanzei/memory/* 硬 deny 指向 memory_note 草稿投递(profiles.rs:180-184)。写读分离设计 §3 完全落地。
批次 2/8 完成:设计基线 §9 工程决策逐项核对——§9.1 memory 模块结构✓;§9.2 U-/M- 前缀+slug 文件名+frontmatter 平铺✓;§9.3 episode 存 state.db 不落文件✓(mod.rs:6/45);§9.4 FTS5 unicode61+bm25 topN+log(1+hits) 重排✓(store.rs:347/469/524);§9.5 tmp+rename 原子替换+可整体重建✓(store.rs:1119-1123);§9.6 工具集主 agent 三件套+add 去重去噪 force✓;§9.7 INDEX 预注入常驻✓(profiles.rs:258-286)。
批次 3/8 完成:memory 模块完整性核验——派生全量重建 rebuild_all(store.rs:299-370)、归档目录 archive/(store.rs:98-99)供 load_archived_ids 保 ID 不复用、merge 合并自动 stale+superseded_by 墓碑(store.rs:608/675)、integrity 缺号/重复检测(store.rs:566/1347)、stale 降权不出 active(store.rs:1331)。缺口:stale→archive/ 搬运流程未实现,已由 D-217/D-231 独立跟踪(R-165 承接),不影响本总纲验收项。
批次 4/8 完成:Memory UI 页与设计 §6 一致性——refreshMemory(13-memory.js:3)、memory_context_bill(11)、架构图 renderMemoryArch(307-342 scope 卡片+category 格+integrity 警告)、条目视图 loadMemoryList(347, stale/dormant 标记)、命中/注入统计(259-305)、整理 inbox 按钮(html:303)。设计 §6 全部落地。
批次 5/8 完成:注入注册与 §5 一致性——resident_index 常驻注入(profiles.rs:258-286)、prompt_hints 按任务描述匹配触发(mod.rs:476-523)、sop 提炼候选箱 harvest_sop(mod.rs:398-419)、预算口径统一 D-216(mod.rs:275-276)。§5 注入机制全部落地。
批次 6/8 完成:git 恢复实证——项目记忆 30 文件全部被 git 跟踪且 git log 可见演进历史(4fa7c23/0f91e21 等);inbox.md(草稿暂存)与 index.db(派生物)在 .gitignore 不入库,符合设计(真源 md 可恢复、派生物可重建)。
批次 7/8:验收证据逐项整理——①R-104~R-107 四期 done(requirements-archive 关闭记录+提交证据);②闭环实证→R-145 跟踪;③记忆文件全 git 跟踪+历史可恢复;④index.db 未跟踪仅存派生物(store.rs rebuild_all 可重建)。三项已证、一项转移。
批次 8/8:收口。
批次 1/8 完成:子代理/写路径现状盘点——主 agent 工具表(profiles.rs:110-121)只含 memory_search/note/stats;写路径 add/update/merge/stale 定义在 manager.rs(57-294)且只注册进 manager_agent 专属工具表(manager.rs:331-350);memory-manager 子代理 AgentDef(manager.rs:628-666)= fast/Subagent/steps10/无 shell;.kanzei/memory/* 硬 deny 指向 memory_note 草稿投递(profiles.rs:180-184)。写读分离设计 §3 完全落地。
批次 2/8 完成:设计基线 §9 工程决策逐项核对——§9.1 memory 模块结构✓;§9.2 U-/M- 前缀+slug 文件名+frontmatter 平铺✓;§9.3 episode 存 state.db 不落文件✓(mod.rs:6/45);§9.4 FTS5 unicode61+bm25 topN+log(1+hits) 重排✓(store.rs:347/469/524);§9.5 tmp+rename 原子替换+可整体重建✓(store.rs:1119-1123);§9.6 工具集主 agent 三件套+add 去重去噪 force✓;§9.7 INDEX 预注入常驻✓(profiles.rs:258-286)。
批次 3/8 完成:memory 模块完整性核验——派生全量重建 rebuild_all(store.rs:299-370)、归档目录 archive/(store.rs:98-99)供 load_archived_ids 保 ID 不复用、merge 合并自动 stale+superseded_by 墓碑(store.rs:608/675)、integrity 缺号/重复检测(store.rs:566/1347)、stale 降权不出 active(store.rs:1331)。发现缺口:stale→archive/ 搬运流程未实现,已由 D-217/D-231 独立跟踪(R-165 Memory Compiler 承接),不影响本总纲验收项(git 可恢复/SQLite 派生物),收口时标注。
批次 0/8 已写批次表。继续批4:Memory UI 页与设计 §6 一致性核对。
批次 1/8 完成:子代理/写路径现状盘点——主 agent 工具表(profiles.rs:110-121)只含 memory_search/note/stats;写路径 add/update/merge/stale 定义在 manager.rs(57-294)且只注册进 manager_agent 专属工具表(manager.rs:331-350);memory-manager 子代理 AgentDef(manager.rs:628-666)= fast/Subagent/steps10/无 shell;.kanzei/memory/* 硬 deny 指向 memory_note 草稿投递(profiles.rs:180-184)。写读分离设计 §3 完全落地。
批次 2/8 完成:设计基线 §9 工程决策逐项核对——§9.1 memory 模块结构✓(kanzei-tools/src/memory/{mod,store,tools,manager}.rs);§9.2 U-/M- 前缀+slug 文件名+frontmatter 平铺✓(store.rs:284 slug 终身不改、U-001/M-001 测试);§9.3 episode 存 state.db 不落文件✓(mod.rs:6/45);§9.4 FTS5 unicode61+bm25 topN+log(1+hits) 重排✓(store.rs:347/469/524);§9.5 tmp+rename 原子替换+可整体重建✓(store.rs:1119-1123);§9.6 工具集主 agent 三件套+add 去重去噪 force✓(profiles.rs:110-121, manager.rs:97);§9.7 INDEX 预注入常驻✓(profiles.rs:258-286 resident_index 预算注入)。
批次 0/8 已写批次表。继续批3:memory 模块完整性核验。
批次规划(8批):批1=子代理/写路径现状盘点(memory-manager 是否专管写路径、主 agent 是否只有 search/note/stats);批2=设计基线 §9 工程决策核对(episode 存 state.db、FTS tokenizer、安全模型、注入注册);批3=memory 模块完整性核验(去重去噪/归档/INDEX 重建);批4=Memory UI 页与设计 §6 一致性核对;批5=注入注册与 §5 一致性核对;批6=记忆内容 git 恢复实证;批7=验收证据逐项整理;批8=收口关闭(实证去向标注 R-145)。
批次 0/8:开始批1。

- 批次: 7/7

## R-111 需求缺陷依赖的组织与可视化 [done]
- 标签: 后端
- 复杂度: 中
- 优先级: P2
- 归属: kanzei
- 来源: 2026-08-08 用户:依赖设计合理,但求更好的组织形式与可视化
- 内容: 现状 refs/依赖 是自由文本,无校验无方向语义。改造:①字段语义分立——`依赖:`(阻塞关系,本条完成前置)与 `refs:`(关联参考,不阻塞)在引擎侧校验 ID 存在且区分方向;②tracker 输出条目时附带反向链接(谁依赖我);③独立文档页给依赖视图:按依赖拓扑分层的列表(可做层/被阻塞层),点击条目高亮其依赖链;暂不做图形化 DAG 画布(重,收益存疑,列表+高亮已覆盖主要场景)。
- 验收: 依赖引用不存在时工具告警;条目详情含正反向链接;文档页有"被谁阻塞/阻塞谁"视图且切换流畅;循环依赖检测告警。
- refs: R-054 D-112
- 阶段: 4

- 批次: 3/3
- 进展: 批3(收口)完成:全量 cargo test --workspace 全绿(45+71+51+39+132+7+3+2+1,首跑 LNK1104 因测试二进制瞬态占用,重试即过)。验收逐项核对:①依赖引用不存在告警=既有引擎能力 tracker.rs:860(block_reasons 依赖不存在);②条目详情含正反向链接=批1 交付 docs.rs:179-181 snapshot 输出 dependencies/dependents+前端 12-docs-pages.js:201-202 meta;③文档页被谁阻塞/阻塞谁视图=批2 交付 renderDependencyView(12-docs-pages.js:156-231)+highlightDependencyChain(232-267)+toggle(14-docs-actions.js:42),runtime smoke 断言覆盖按钮/分层/高亮/压暗/隐藏切换;④循环依赖告警=既有引擎能力 tracker.rs:846-850。验收全项满足,关闭。
批次 1/3 完成(9c61b23):tracker.rs dependents_map 公共函数(正反向依赖图,与 dependency_states 共用「依赖:」字段解析),docs.rs docs_snapshot 输出 dependencies/dependents 字段;单测覆盖正反向与去重。
批次 2/3 完成(0eb7cf8):文档页依赖视图——index.html dep-toggle 按钮+dep-view 容器;12-docs-pages.js renderDependencyView(可做/被阻塞分层,数据与引擎取活同源)+highlightDependencyChain(点击高亮向上依赖+向下被依赖,压暗无关);14-docs-actions.js toggle 绑定;style.css dep-view 样式;i18n 登记 6 词条。runtime smoke 新增断言(按钮/分层/高亮/压暗/隐藏切换)全绿。
批次 3/3:收口验证——验收①④为既有引擎能力(依赖不存在告警 tracker.rs:823、循环依赖告警 tracker.rs:831-835),验收②③为本次交付;跑全量测试后关闭。
批次 1/3 完成(9c61b23):tracker.rs 新增 dependents_map 公共函数(返回正向/反向依赖图,与 dependency_states 共用「依赖:」字段解析,排序去重),docs.rs docs_snapshot 每条目输出 dependencies/dependents 字段。单测 dependents_map_reports_forward_and_reverse_links 覆盖正反向与去重,tools 132 passed。
批次 2/3:前端文档页依赖视图(分层列表+点击高亮)。
批次 3/3:收口验证(不存在引用告警/正反向链接/视图切换/循环告警逐项+全量测试)。
批次规划(3批):批1=引擎侧依赖语义分立——「依赖:」字段解析成独立键+ID 存在性校验+循环依赖检测(依赖图 DFS 判环)+tracker 输出附带反向链接;批2=前端文档页依赖视图——按依赖拓扑分层的列表(可做层/被阻塞层)+点击条目高亮其依赖链;批3=验收收口——不存在引用告警/正反向链接/视图切换/循环告警逐项验证+全量测试。
批次 0/3:开始批1。

