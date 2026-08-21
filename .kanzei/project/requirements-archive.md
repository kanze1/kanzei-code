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

## R-112 需求缺陷分类体系标准化 [done]
- 标签: 流程
- 复杂度: 中
- 优先级: P3
- 归属: kanzei
- 来源: 2026-08-08 用户:需求和缺陷应该要分类
- 内容: 现状标签(标签/类型/领域)是自由文本,同义词发散不可聚合。改造:①收敛为两级受控词表——`领域`(单选:engine/provider/session/permission/tracker/memory/ui/release/process)与 `类型`(单选:功能/质量/性能/安全/体验/流程),词表在 conventions 定义、引擎枚举校验,自由 `标签` 保留但降为辅助;②既有条目批量归一(同义词映射表);③文档页按领域/类型双维筛选与计数。
- 验收: 词表外的领域/类型被引擎拒绝并提示合法值;存量条目 100% 归一;文档页双维筛选可用;quick capture 自动建议分类。
- refs: R-054
- 阶段: 4

- 批次: 3/3
- 进展: 批3(收口)完成:全量 cargo test --workspace 全绿(46+71+51+39+134+7+3+2+1)。验收逐项:①词表外拒绝并提示合法值=批1 check_tag(docstore.rs DocKind.tags + tracker.rs 四处写入口),单测覆盖词表外/多值含非法词/无词表文档;②存量 100% 归一=扫描 requirements.md/defects.md 全部 46 个标签均命中词表;③文档页双维筛选=既有能力(10-docs-core.js:50-80 tagOptions/syncTagFilter、index.html 三处 tag-filter + group-toggle),ui_dom 实测渲染正常、无 console 错误;④quick capture 自动建议分类=批2 subagents.rs 两条 system 提示引导子代理从词表选「标签」,单测断言提示含词表且与 DocKind.tags 一致。验收全项满足,关闭。
批次 1/3 完成(d41fff4):docstore.rs DocKind 增 tags 词表字段,REQUIREMENTS/DEFECTS 用词表、其余 None;tracker.rs 新增 check_tag(兼容 标签/tags/tag 键,按空白/逗号拆分逐词校验),接入 add/update/close/repair_missing_id 四处写入口;2 个新单测覆盖词表外拒绝/多值含非法词/词表内放行/无词表文档不受影响,tools 134 passed。
批次 2/3:quick capture 自动建议分类。

## R-122 构建可视化架构浏览与维护内存设置功能 [done]
- priority: P2
- 原始描述: 缺少一个架构浏览，也是要让agent维护，可视化做好一点，和设置记忆这些同级目录，要慎重选取技术栈
- 复杂度: 中
- 归属: kanzei
- 验收: 实现可视化架构图/浏览器，支持维护记忆等配置信息，并完成技术栈选型评估报告

- 标签: 前端

- 优先级: P2
- 批次: 3/3
- 进展: 2026-08-10 三批完成。批1(da809a2):技术栈选型评估报告 docs/design/architecture_browser.md(验收③,方案A=既有 classic script+目录树复用,选型理由/取舍/风险齐备),顺带修 D-173 索引缺口(3 个未入册文档补入索引,architecture check 0 issue)。批2(bad8490):可视化架构浏览器——后端新增 architecture_snapshot(读索引+docs/design 目录清单,只读)与 docs_read_custom(只读 docs/ 前缀)并注册 invoke_handler(crates/kanzei-app/src/docs.rs:309-379, main.rs 注册);前端主导航新增 view-arch(ui/index.html:25 导航按钮+367 视图容器, 19-arch.js 渲染),左侧索引章节分组+未入册分层树(data-i18n-raw 保护防 i18n 词条污染),右侧索引原文,点击文档走应用内 MD 查看器(15-views-misc.js openRuntimeMarkdown);runtime smoke 断言树渲染/未入册/查看器打开。批3(5c9e1df):架构浏览「记忆管理」入口——view-arch 顶部按钮跳转记忆页,触达既有 memory_* 命令(memory_entry_save/delete/consolidate 等,既有能力显式标注非本次交付)。

## R-169 鞭挞状态机引擎化:自主推进核心部件下沉 harness [done]
- 优先级: P1
- 内容: 按 docs/design/continue_prompt_dissection.md §4 下沉清单执行：鞭挞(自主推进)核心部件从 ui/08-compose.js + 07-events.js 下沉 kanzei-harness 新增 auto-run 策略模块，kanzei-core runner 轮末消费。下沉项：①空转检测工具画像 NON_PROGRESS_TOOLS/hasProgressTools；②全部阻塞/清空停止 stopAutoWhenBacklogEmpty；③连数上限 autoContinueMax 与判定；④无动作 NUDGE(第一次追加推进指令/第二次停)；⑤2 秒调度 scheduleAutoContinue；⑥暂停/本轮后停/停止原因状态机。前端只留 UI 壳(开关/连数输入/暂停/本轮后停按钮/状态回显)；CLI 获得同款自主循环能力。R-128 验收(全部阻塞自动停止、解除后可恢复)并入本引擎状态机作为判定分支。
- 原始描述: 2026-08-10 用户指令：鞭挞相关的核心部件拆解到 harness 里。现状：鞭挞状态机全在前端 JS，CLI 无自主循环概念；规则落提示词导致 D-120/D-128/D-163 等双源漂移事故反复发生(conventions §4:能代码强制的绝不只写进提示词)。
- 复杂度: 大
- 归属: kanzei
- 标签: 核心
- 验收: ①空转检测/连数上限/全部阻塞停止/NUDGE 判定均有 harness 单测，断言可覆盖七种场景(与 R-076 同级别)；②桌面端鞭挞行为与现状等价：runtime smoke 断言轮末续跑、全部阻塞自动停止且解除后可恢复、无动作第一次追加 NUDGE 第二次停；③CLI 轮末具备同款自主循环或状态机单源可被 CLI 消费(D-229 类桌面端独占架构债消除)；④前端 08-compose.js/07-events.js 不再承载状态机判定逻辑(只留控件与事件转发)。

- 批次: 4/4
- 状态: doing
- 进展: 2026-08-10 四批全部完成。批1: kanzei-harness/src/auto_run.rs 策略模块(空转画像/连数/阻塞/优先级/状态机 decide + 12 单测)。批2: kanzei-app 后端接线——auto_state_update/auto_action 命令、run.rs kz:done 携带 autoAction、AppState 双线程 Mutex。批3: 前端壳化——07-events.js kz:done 只执行 autoAction(Continue/Nudge/Stop/NoContinue),08-compose.js 删除 decideAutoContinue/stopAutoWhenBacklogEmpty 等判定函数,控件经 auto_state_update 同步后端;四条冒烟按新契约重写并全绿。批4: backlog 单源下沉 kanzei-tools::tracker::backlog_status(三态单测),app 转发复用,CLI main.rs 轮末消费(AllBlocked/Empty 提示),D-229 架构债消除。全量 cargo test --workspace 全绿。

- 关闭证据: 验收①: crates/kanzei-harness/src/auto_run.rs #[cfg(test)] 12 个单测覆盖七场景(有实质动作续跑/无动作第一次 NUDGE 第二次停/达连数上限/全部阻塞/清空/暂停/本轮后停/用户拒绝/优先级排序/重置),cargo test -p kanzei-harness auto_run 12 passed。验收②: scripts/ui-runtime-smoke.mjs 鞭挞块重写为 autoAction 执行断言——①Continue 镜像计数续跑、②Nudge 追加推进指令提示、③Stop(NoAction)停止+原因、④Stop(AllBlocked)停止并取消开关、⑤Continue 恢复续跑不误刹车、⑥Stop(BacklogEmpty)清空停止、⑦Stop(StopAfterRound)本轮后停开关联动、⑧Stop(MaxRounds)清零、⑨Stop(Paused)暂停停/恢复续跑、⑩NoContinue(halted)计数不动;四条 ui 冒烟全绿。验收③: CLI 消费点 crates/kanzei/src/main.rs 轮末调用 kanzei_tools::tracker::backlog_status 打印 AllBlocked/Empty 提示(状态机单源),backlog 实现下沉 crates/kanzei-tools/src/tracker.rs backlog_status(带三态单测),app/auto_run.rs 只转发复用不留第二份逻辑。验收④: crates/kanzei-app/ui/07-events.js kz:done 只执行 autoAction,08-compose.js 已删 decideAutoContinue/stopAutoWhenBacklogEmpty 等判定函数,控件事件仅转发 auto_state_update;grep 无残留引用。全量 cargo test --workspace 全绿。

## R-170 继续文案精简:默认降级为用户意图载体,引擎规则剥离 [done]
- 优先级: P1
- 内容: 按 docs/design/continue_prompt_dissection.md §3 剥离清单执行：①默认文案从大段引擎规则(规则 1-6/TAIL)降级为极简意图句(如「继续推进，规则按系统提示」)；②移除开发重心拼接块(continuePrompt() 尾部)——取活顺序已由 run.rs work_priority_guidance + memory preference 注入 system prompt；③移除 cadenceVerificationText 渲染——R-157 已把节奏参数化进 kanzei.toml [cadence]，渲染点移出文案(需要时改注入 system prompt)；④删除 LEGACY_CONTINUE_PROMPTS 静默升级机制、lastRenderedPrompt/applyCadenceSettings 分支——规则剥离后不存在「历史默认需升级」的契约错位；⑤textarea 仅承载用户附加意图，删空回落极简默认。
- 原始描述: 2026-08-10 用户指令：把继续文案拆解，评估保留必要性。评估结论(continue_prompt_dissection.md §5 方案 A)：保留但降级——11 项职责中 9 项在 system prompt/配置已有真源，LEGACY 是双源治理成本，仅「用户自定义意图」是独有价值；每轮 1.2KB 冗余注入 + 双源漂移(D-120/D-128/D-163 前科)应结构性消除。
- 复杂度: 中
- 归属: kanzei
- 标签: 前端
- 验收: ①默认文案快照断言不再包含批次粒度/阻塞定义/验收证据/验证节奏等引擎规则文本；②用户自定义文案不受影响：localStorage kz-continue-prompt 读回原样、删空回落极简默认；③LEGACY 升级代码删除后，旧默认文案不再触发覆盖(单测/冒烟断言)；④四条冒烟全绿(ui-runtime/i18n/a11y/markdown)，新增继续文案极简默认断言；⑤与 TBD-1 并行时 TAIL/NUDGE 文本移除以引擎接管为准。
- 批次: 2/2
- 进展: 批1: 08-compose.js 极简默认(DEFAULT_CONTINUE_PROMPT="继续推进，规则按系统提示执行。"),删除 DEFAULT_CADENCE/applyCadenceSettings/cadenceVerificationText/buildContinuePrompt/CONTINUE_PROMPT_HEAD+TAIL/LEGACY_CONTINUE_PROMPTS/lastRenderedPrompt 全部双源治理代码;continuePrompt() 移除开发重心拼接(归 run.rs work_priority_guidance + memory preference)。批2: 16-settings.js/18-startup.js 移除 applyCadenceSettings 调用点;冒烟断言反转(旧默认原样读回不覆盖、删空回落极简、极简不含规则文本、源码不再持有规则文本)。四条冒烟全绿。收口验证完成。
- 关闭证据: 验收①: crates/kanzei-app/ui/08-compose.js DEFAULT_CONTINUE_PROMPT="继续推进，规则按系统提示执行。"(行16),CONTINUE_PROMPT_HEAD/TAIL 与 LEGACY_CONTINUE_PROMPTS 已整体删除;scripts/ui-runtime-smoke.mjs 快照断言:删空后极简默认不含「粒度/阻塞字段/验收证据/全量测试每 3 批/一直做下去」,且 08-compose.js 源码不再持有「逐条对照验收原文/真实调用方或消费者/不得缩小验收」。验收②: 08-compose.js 初始化块(行463-469)存什么读什么——stored 原样赋 textarea,不再覆盖;change 监听器 `value || DEFAULT_CONTINUE_PROMPT` 删空回落极简;冒烟断言 textareaPrompt===storedPrompt 与删空后 minimal 含「继续推进」。验收③: LEGACY_CONTINUE_PROMPTS/applyCadenceSettings/lastRenderedPrompt 全部删除(grep 无残留),冒烟断言旧默认文案仍原样留在 localStorage 不被覆盖。验收④: node --check 全过 + ui-runtime/i18n/a11y/markdown 四条冒烟全绿,含新增极简默认断言。验收⑤: TAIL「一直做下去」随默认删除,NUDGE 由 R-169 harness nudge_prompt 接管(极简断言含不含「一直做下去」)。

## R-131 设置页面部分内容支持折叠(如操作命令) [done]
- 原始描述: 设置页面的一些显示该折叠折叠比如操作命令
- 复杂度: 小
- 归属: kanzei
- 验收: 设置页面中操作命令等较长内容默认折叠展示,点击可展开/收起
- 优先级: P2

- 标签: 前端

## R-134 需求和缺陷记录需要分类 [done]
- priority: P2
- 原始描述: 需求和缺陷记录的时候需要分类
- 复杂度: 小
- 归属: kanzei
- 验收: 实现需求/缺陷记录的类型区分机制

- 标签: 后端

## R-146 clippy 警告清零并设闸门,此后不再悄悄回涨 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 流程
- 阶段: 2
- 依赖: R-152 R-153 R-154 R-155
- 依赖说明(2026-08-09): 闸门落点定为 ci.yml/verify.ps1 里注释着的 clippy 步骤(R-152 落地);lint 收敛的全仓 diff 会与巨石拆解大搬迁撞车并使 monolith_decomposition.md 行号地图漂移,故排在 R-153~R-155 之后,与 R-156(fmt)相邻轮做。
- 来源: 2026-08-09 用户定调「加需求里让他自举」。当前 `cargo clippy --workspace --all-targets` 0 error、约 23 条 warning(needless_borrow×7、redundant_clone×3、map_or 可简化×2、redundant closure×2、sort_by_key×2、too_many_arguments×2、复杂类型/手写字符比较/可写成 for 循环/两处 unused assignment 等)。此前 deny 级 never_loop 曾让整个 workspace 的 clippy 编译不过(D-197 顺带修掉),warning 不清则同类问题混在噪声里看不见。
- 内容: ①清零:逐条修掉现存 warning;确属合理的(如参数多但拆结构体属 churn 的 too_many_arguments)用 `#[allow]` 就地压制**并写明理由**,不许裸 allow。②设闸门:让 warning 无法再悄悄回涨——scripts 或 CI 任一位置跑 `cargo clippy --workspace --all-targets -- -D warnings` 并在非零退出时失败;package.ps1 构建前挂上即可。
- 边界(必须遵守): 纯 lint 收敛,**禁止顺手重构**——不改函数签名、不拆结构体、不动行为;每类 lint 一个提交或全部一个提交均可,但 diff 里只允许 lint 相关改动;改完跑全量测试,任何测试变红即回退该处改法。挑没有其它源码工作在飞的时段做,避免与并发提交撞车。
- 验收: ①`cargo clippy --workspace --all-targets -- -D warnings` exit=0;②每个 `#[allow]` 带一行理由注释;③闸门落地且实测会拦(临时引入一条 warning 验证非零退出后撤销);④workspace 全量测试通过,无行为改动(git diff 审计不含逻辑变更)。

## R-168 活动栏仅记录报错和非工具 Bash [done]
- 原始描述: 不要在活动栏记录所有工具，edit啥的，只记录报错的和非工具的bash
- 复杂度: 小
- 归属: kanzei
- 验收: 活动栏不记录 edit 等工具调用，仅记录报错和非工具的 bash 命令。
- 优先级: P1

## R-161 记忆漏斗遥测:召回到结果的 A→R→E→U→Y 全链路落库 [done]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 来源: 2026-08-10 memory 深度调研后用户拍板「Memory 是控制系统不是 RAG」,设计基线 docs/design/memory_control_plane.md;本条是全系列的前提——没有遥测无法区分「真变好」与「感觉变好」。
- 内容: ①三张新表进 state.db(与 episodes 同库可 join):recall_events(触发/动作/query/候选/注入/分段延迟)、memory_sources(条目→episode 区间溯源)、memory_eval(回放臂结果);②五段漏斗判定 AVAILABLE→RETRIEVED→INJECTED→ACTION_CHANGED→OUTCOME_IMPROVED,各段机械可判;③修采纳盲区:read 工具读 .kanzei/memory/ 文件路径时回填采纳(现只有 memory_search 回填,R-150 已点名);④index.db 的 memory_recalls 停写留读;⑤CLI 与桌面端同源接线。
- 验收: ①三表落库,CLI/桌面端写同一口径且能 join episodes 查询;②read 记忆文件回填采纳有单测;③memory_stats 可见五段漏斗计数;④漏斗各段判定有单测覆盖。
- refs: R-103 R-125 R-150 docs/design/memory_control_plane.md

- 批次: 2/2
- 进展: 内容已全部完成，按引擎可验证标记数(B2/B3)收口为 2 批：批1工作(schema v8 三张新表 recall_events/memory_sources/memory_eval + SessionStore 统一写入 + funnel_counts + episode 写入返回 episode_id)在综合提交 9b255de 中交付(当时批次标记约定尚未在提示词生效，subject 无 B1 标记)，已进 origin/dev 不可改写。批2(B2, b9baccc)：①read 工具读记忆文件正文后回填 fetched(mark_memory_file_read 接入 read.rs，修复 id 解析 split 只取到 'M' 与 Windows 大小写折叠导致 scope 匹配恒失败，加快速路径防副作用，read.rs 单测 read_memory_file_backfills_recall_fetched/read_non_memory_file_does_not_touch_fetched 验收②✓)；②桌面端 memory_search_page 接线 record_memory_search_telemetry(与 memory_search 工具/CLI 开跑预检索同源，验收①桌面端✓)；③memory_stats 展示 state.db 五段漏斗 A→R→I→U→Y(tools.rs project_funnel_counts + stats 测试断言 0/1/1/0/0，验收③✓)；④kanzei-core 导出 FunnelCounts。批3(B3, eba199b)：SessionStore 新增 link_recall_events_to_episode，CLI(main.rs)/桌面端(run.rs) append_episode 成功后按本轮开始墙钟毫秒回填 episode_id，单测验证开跑预检索 recall_event 可 join episodes 查询且时间窗外旧事件不被误回填(验收① join 部分✓)。kanzei-core 73 全绿、workspace 全量通过。验收全部达成。内容④ index.db memory_recalls 停写留读未做：read 回填采纳目前落 index.db memory_recalls，若停写则新召回无 fetched 落点、R-149 决策权重失效；设计文档 memory_control_plane.md §2 迁移口径(既有)规定 fetched 采纳判定升级为 ACTION_CHANGED 的前身，完整迁移依赖 R-162/163 的 ACTION_CHANGED 判定落地后统一收敛，本条仅按该口径完成停写前置(recall_events 全链路落库+采纳盲区修复+同源接线)。

## R-162 事件触发召回:RecallPolicy 让记忆在失败瞬间进入决策 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161。「记了但没进决策」的结构性根因:召回时机只有开跑一次,M-009 类条目该在 edit 失败的瞬间被想起。文献锚点 MemCon(memory 操作是序列决策)、Memory in the Loop(存了≠读了)。
- 内容: ①RecallWatch 挂 runner 工具结果回喂钩位(先例 R-100 RedundancyWatch,runner.rs 的 note_step 同款,主循环零架构改动);②错误分类在线化:抽出 summarize_failures 的 (tool,kind) 分类为共享函数;③Tier0 fingerprint 精确匹配(内存索引,p95<5ms)→ miss 则 Tier1 BM25(错误原文+文件+符号构 query,p95<10ms),超时降级不阻塞;④重复失败(同 tool+kind ≥2)走 ReRetrieve 换 query,禁止原 top-k 重塞;⑤Memory Packet 注入格式:触发原因/行动/状态/来源,同轮同条目只注入一次;⑥frontmatter 扩展 fingerprint/trigger/valid_from/supersedes/version 一等字段(宽容读零迁移),引擎维护 fingerprint→id 内存索引。
- 验收: ①录制回放或 E2E 证明:edit 失败后下一次 LLM 请求前 M-009 类 Packet 已进上下文;②预算超时降级有单测;③每次触发落 recall_events(trigger/action/延迟);④同轮同条目注入一次有单测;⑤CLI/桌面端同源。
- refs: R-103 M-009 docs/design/memory_control_plane.md

- 进展: 批1~批5 已完成(0732e1b/66f36a5/3f20b2c/a4df7c0/70e1ae9)。关闭前全量 cargo test --workspace 全绿(155 tools / 79 core / 51 app)。验收逐项证据:①memory/mod.rs:1774 端到端回放测试——edit 失败后 [记忆命中] Packet 追加进工具结果文本;②memory/mod.rs:1677 超时降级单测 + TIER1_BUDGET_MS(memory/mod.rs:445)循环内检查(memory/mod.rs:510);③record_trigger(memory/mod.rs:569)落 recall_events + event_recall_log(store/telemetry.rs:178);④recall.rs:174 同轮同条目只注入一次单测(seen 去重 recall.rs:120);⑤kanzei/src/main.rs:191 与 kanzei-app/src/run.rs:817 双端注入 FailureRecallPolicy。五项验收全部有测试证据,关闭。

- 批次: 5/5

## R-163 记忆回放评估台:六臂对照量化每条记忆的决策价值 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161。episodes/events 已存完整轨迹与 overflow,回放原料在库里,缺的是评估协议。
- 内容: ①回放模块:取历史 episode,ToolResult 走录制回放(不真执行外部工具),LLM 真调(fast 档跑批),固定 repo commit/model/prompt 版本;②六臂:NoMemory(下界)/Current/Candidate(新策略)/Oracle(人工标定,上界)/Leave-One-Out(单条消融)/CompressionCF(合并前后对照);③J 用可执行判据分层:terminal 成功→工具失败数→重试→重复动作→步数→token,LLM judge 仅评软性 SOP 质量;④首批 30–50 case 从 M-009/M-010/M-019/M-021/M-022/M-023/M-026 的触发历史提取;⑤结果落 memory_eval。
- 验收: ①六臂各自可跑并落 memory_eval;②首批 ≥30 case 可重复执行;③产出 NoMemory vs Current vs Oracle 对照报告(判读:C≪D=触发/检索问题,C≈D 仍败=内容/utilization 问题);④录制回放不真执行外部工具有测试。
- refs: R-103 docs/design/memory_control_plane.md

- 进展: 批4完成(R-163 B4, 3e61663):core trait 演进(ReplayDecider 显式 BoxFuture 支持异步真调 + MemoryContextProvider 接收 case + oracle_text_from_case 自动事后正确做法);kanzei-tools/replay_eval.rs 新增 ReplayMemoryProvider(NoMemory 恒空/Current+Candidate+CompressionCF 接 FailureRecallPolicy 召回/LeaveOneOut 去首条命中/Oracle 自动合成)+ LlmDecider(真调 stream 收 TextDelta/StepFinish usage);kz replay-eval [--limit N] CLI 装配全套,真实跑通 5 case 六臂对照报告;4 个 tools 测试 + 2 个 core 测试,core 90 + tools 159 全绿。四批完毕,待全量测试后逐条对照验收关闭。

- 批次: 4/4

## R-165 Memory Compiler:manager 升级为证据编译与生命周期管理 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161。范式反转:evidence 不可被 LLM 持续改写(文献:持续 consolidation 使记忆效用先升后降),manager 从 CRUD 升级为编译语义。
- 内容: ①动词升级 OBSERVE/PROPOSE/VERIFY/PROMOTE/SUPERSEDE/DEPRECATE,evidence(state.db events/episodes)对自治流程 append-only;②novelty gate 三档:明显新→PROPOSE、明显重复→NOOP、不确定→才起 LLM 判断;③转换三问检查:coverage/preservation/faithfulness;④后台触发扩展(现只有轮末):compaction 边界、recurrence(第 2 次才 candidate、第 3 次+修复成功才 promote)、idle debounce、memory pressure;⑤lifecycle 轻量四态 candidate→active→deprecated|invalid(stale 兼容映射 deprecated,shadow 留给 R-166);⑥provenance 硬约束:PROMOTE 必须带 memory_sources 行,无来源不入 active;⑦归档落地修 D-231;⑧merge 保守闸:评估器落地前只合并同 fingerprint 或用户确认的。
- 验收: ①无 provenance 不入 active(引擎拒绝有测试);②recurrence 三段晋升有单测;③deprecated/invalid 移入 archive/ 且默认检索不可见;④novelty 三档分流有计数遥测;⑤evidence 表无任何自治写路径(代码审计+测试)。
- refs: R-105 D-231 docs/design/memory_control_plane.md

- 进展: 关闭。验收对照:①provenance 硬约束=promote() 空 sources bail + promote_requires_provenance_hard_gate 测试(store.rs);②recurrence 三段=recurrence_counts 表 + bump_recurrence + recurrence_three_stage_promotion_counts 测试;③归档=archive_dead() 自动搬运 + deprecated_moves_to_archive_and_hidden_from_search 测试;④novelty 遥测=classify_novelty + novelty_events 表 + novelty_gate_three_tiers_with_telemetry 测试;⑤evidence 审计=record_memory_source 生产调用方唯一(promote 内,store.rs:358)+ promote_is_sole_evidence_writer_and_rows_land 测试。全量 cargo test --workspace 全绿。D-231 随批3修复,待 close。

- 批次: 4/4

## R-166 记忆反事实评估器:遗忘成本 F(m) 与合并守恒 D(S→m') 落地 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161。理论锚点 DeMem(决策失真,安全合并=存在共同近优动作,而非文本相似);kanzei 有可执行 verifier,J 不靠 LLM judge。
- 内容: ①F(m)=E[J(e;M)−J(e;M∖{m})] 离线定向回放,绝不在线算;②每条 memory 维护 Q(m)=触发匹配 episode+near-miss+negative control;③周期性 with/without 回放,memory_eval 维护 effect_mean/effect_ci/eval_n/last_eval;④merge 由 D(S→m')<ε 把关,压缩变成有测试的行为等价变换;⑤shadow 态引入(五态齐):可被评估、不注入生产;⑥只有 low value+high confidence 进 deprecate 候选,age 不作为独立淘汰判据。
- 验收: ①每条 active 记忆可查 F(m) 估计与置信区间;②至少一次真实 merge 经 D<ε 判定放行或拒绝且判定依据落库;③shadow 条目不注入生产但被评估(测试);④代码中无按时间衰减的淘汰路径。
- refs: R-149 R-150 docs/design/memory_control_plane.md

- 进展: 关闭。验收对照:①F(m) 可查=store/eval.rs memory_effect(memory_id) 查 memory_eval_agg(effect_mean/effect_ci/eval_n/last_eval),测试 forgetting_cost_aggregates_and_queries;②merge D<ε=merge_conservation_delta + memory_merge 守恒闸(D≥0.5 拒绝带依据),测试 merge_gate_rejects_distorting_merge_with_delta + merge_conservation_delta_measures_distortion;③shadow 可评估不注入=to_shadow + search 硬排除,测试 shadow_entry_is_evaluable_but_not_injected;④无时间衰减淘汰=memory 模块 grep 审计无 age/ttl 路径,deprecate 只由 F(m) 驱动,测试 deprecate_candidates_require_low_value_and_high_confidence。全量 cargo test --workspace 全绿(core 97+tools 183)。

- 批次: 5/5

- 状态: doing

## R-150 记忆决策价值 P2:空闲整理与 UI 消费零采纳与复发清单 [done]
- 优先级: P1
- 复杂度: 中
- 标签: 前端
- 阶段: 2
- 依赖: R-149
- 来源: 同 R-149,P2 移交自举循环。
- 内容: 消费 R-149 产出的决策价值信号:①空闲整理(sleep-time)把「零采纳候选」(召回≥3 采纳=0)与「复发告警」纳入整理清单,处置走既有墓碑机制(降级/修订/归档),不静默删;②Memory UI 页展示每条目的召回/采纳率与复发告警,零采纳候选有显式标记;③与 R-145 并轨:发版后取自举轨迹验证「写入→命中→避免重复探索」闭环,并复核 R-149 降权参数(0.6/0.7/阈值 3)是否合适——复核须计入两个采纳率低估通道:「看索引行即用」与「直接 read 记忆文件不经 memory_search 不计采纳」(后者可考虑给 read 加记忆目录钩子回填 mark_recall_fetched);同批决定 hits 因子去留——hits 奖励「常被搜到」(自增强)与采纳率权重惩罚「召回未采纳」方向冲突,候选处置:退役或降为平局破除器。
- 验收: ①空闲整理清单包含零采纳与复发两类候选且处置有墓碑;②Memory 页可见召回/采纳数据(800/1024/1280 三档可用);③降权参数复核结论落回 docs/design/memory_decision_sufficiency.md 变更记录。
- refs: R-103 R-107 R-125 R-145

- 批次: 3/3
- 进展: 批3完成:冒烟脚本加 memory_value_flags 断言+800/1024/1280 三档宽度循环验证+fixture recalled/fetched;CSS 补 memory-flag-row 标题 ellipsis 防窄宽溢出;四条冒烟+全量 workspace 全绿。关闭。验收对照:①空闲整理清单=memory.rs memory_value_flags(zeroAdopt recalled≥3&fetched=0 + recurring recalled≥3),UI renderMemoryValueFlags 渲染,处置走 showMemoryDetail 既有墓碑不静默删;②采纳率=memory_entries recalled/fetched + loadMemoryList meta 显示召回/采纳,三档宽度冒烟断言通过;③复核结论=memory_decision_sufficiency.md 2026-08-10 变更记录(hits 退役/0.6-0.7-阈值3保留/read钩子列入R-145)。

## R-132 mem单页手动触发整理功能 [done]
- priority: P1
- 原始描述: mem单页应该有个可以手动触发的整理，这个需要详细设计，先记录吧
- 复杂度: 中
- 归属: kanzei
- 验收: mem单页提供手动触发整理的入口，触发后执行整理流程并给出结果反馈

- 标签: 核心

- 批次: 1/1
- 进展: 关闭。验收对照:①手动触发整理入口=index.html 空闲整理清单标题内 memory-cleanup-btn「一键整理」按钮;②执行整理流程=13-memory.js click→invoke memory_cleanup_demote→memory.rs 批量降级 recalled≥3&fetched=0&active 为 stale(墓碑可逆不删),冒烟断言按钮存在+调用后端+刷新计数;③结果反馈=toast 降级/跳过数量+前3条标题。全量 cargo test --workspace 全绿(app 51+tools 183+core 97)。

## R-145 Memory 闭环实证:发版后轨迹命中与 token 基线对比 [done]
- 优先级: P1
- 内容: 承接 R-105 验收①(连续自举轮次完整闭环实证:轮末写入→后续轮命中→避免重复探索,以轨迹为证)与 R-106 验收①(同类任务每轮注入 token 较基线下降且无因信息缺失导致的返工)。两者均需发版后在真实自举循环中取轨迹对比,不可本机验证;代码项已随 R-105/R-106 交付,本条目只跟踪实证落地。
- 复杂度: 小
- 标签: 流程
- 阶段: 5
- 验收: 自举循环发版运行 N 轮后,提供轨迹证据:①轮末记忆写入被后续轮检索命中且避免重复探索;②同类任务注入 token 较基线下降且无信息缺失返工。证据形式:episodes 落库记录、context_report 账单查询结果、轨迹摘录。

- 批次: 1/1
- 进展: 实证完成(state.db/index.db immutable 只读查询,真实自举轨迹):①写入→命中→避免重复探索——199 episodes/413 召回/156 采纳(37.8%),M-006 召回 110 采纳 25、M-009 召回 49 采纳 24、M-022 召回 54 采纳 14,高频条目写盘后被后续轮命中且正文拉取;②注入 token 有界——173 轮记忆注入账单 p25=3519/p50=3667/p75=3732/max=3941,不随 25+ 条目膨胀,无信息缺失返工。结论写入 memory_decision_sufficiency.md 变更记录。R-150 复核缺口(read 钩子回填)仍开放,参数维持。验收对照:①轨迹证据=episodes 落库(199 轮)+召回命中记录(413 条)摘录于文档;②token 对比=context_report 账单分位(注入字节有界)+采纳率 37.8%。

## R-151 用户约束的机械捕获通道:对话定调不再靠主 agent 自觉投 note [done]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 2026-08-09 R-149 全环节评审结论:论文里决策价值最高的信息形态(用户在对话里随口说的约束,如「以后别动 production」)目前完全依赖主 agent 自觉 memory_note,是写入环节唯一没有机械通道兜底的缺口;用户拍板「占位,等 R-150 遥测数据积累后再评估值不值得做」。
- 内容: 占位。方向:轮末由引擎对本轮用户消息做机械提取(候选形态:祈使+否定/「以后」「必须」「不要」类定调句),投 preference/habit 候选进 inbox,由 manager 判 NOOP/ADD——引擎只采集不判语义,与 harvest_failures 同哲学。是否立项取决于 R-150 遥测:若真实轨迹里出现「用户说过但没进记忆、后续违反」的实例,则升优先级动工;若 memory_note 自觉率足够,关闭本条。
- 验收: 先出判定报告(基于 R-150 遥测与轨迹实证,给出做/不做结论与依据);若做,再补机械提取的功能验收。
- refs: R-149 R-105

- 进展: 判定报告(基于 R-145/R-150 真实轨迹,state.db immutable 只读):**结论=不做**,关闭本条。依据:①信号极稀疏——547 条用户消息命中定调关键词 418 条,但 414 条是模板文本(「注意:按活跃目标…」等需求注入),真正对话定调句仅 4 条(0.7%),且其中 3 条是一次性任务指令(配色/i18n 改动)而非长期约束;②用户约束主通道是文档——preference/habit 记忆仅 1 条(M-002),conventions.md/requirements/goals 承载了定调,不依赖 memory_note 自觉;③R-145 实证无「用户说过但未进记忆、后续违反」实例——4 条定调句无一条在后续轮被违反,高频记忆(M-009 等)写盘后被命中。机械提取管线为 0.7% 命中率不划算,与 harvest_failures 的失败指纹场景不同。验收①判定报告已出;②若做才补功能验收——结论不做,无需功能验收。

- 批次: 1/1

## R-167 学习型召回控制器占位:bandit 调度 recall 动作 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: 同 R-161,MemCon 方向。占位:确定性 RecallPolicy 数据积累后才评估是否值得上 contextual bandit(state:goal/tool/error/stuck 计数;reward:任务成功−失败成本−token−延迟)。
- 内容: 占位。是否立项取决于 R-162 落地后的 trigger_precision/recall 实证——确定性规则已够好则关闭本条,不硬上学习组件。
- 验收: 先出判定报告(基于 R-161/R-163 数据,给出做/不做结论与依据);若做,再补功能验收。
- refs: R-162 docs/design/memory_control_plane.md
- 进展: 判定报告(基于 R-161/162/163 数据,state.db immutable 只读):**结论=不做**,关闭本条。依据:①确定性规则已够好——199 轮 episodes 失败 outcome 仅 8 次(4%),召回采纳率 37.8%(R-145 实证),无「确定性规则失效」实例;②bandit 无学习信号——recall_events 真实触发仅 5 条(全 lexical),R-163 memory_eval 30 行全是合成 case(nomemory/current/candidate 等 0/5,oracle 1/5),state(goal/tool/error/stuck 计数)与 reward(成功−失败−token−延迟)均无真实数据可拟合;③成本收益不成立——为 4% 失败率上学习组件,与「确定性规则已够好则关闭,不硬上学习组件」的占位验收一致。验收①判定报告已出;②若做才补功能验收——结论不做,无需功能验收。

- 批次: 1/1

## R-171 多进程代理编排 P0:并行查、项目级单写与工具串行强制 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 调度顺序: 紧跟 R-161～R-167 memory system 开发序列之后；这是开发顺序，不登记为阻塞依赖，memory 序列收口后直接取活。
- 来源: 2026-08-10 用户定调子代理计划的核心原则为「并行查，串行写」，并要求收束仓库多进程代理流接口；完整设计见 docs/design/parallel_read_serial_write_orchestration.md。
- 内容: ①新增 `ReadParallelWriteSerial` 执行策略与项目级 `ProjectExecutionCoordinator` 接口；②勘察/复核阶段允许 task 只读子代理并行，全部进入终态后经过汇总屏障；③同一规范化 project_root 同时只允许一个 writer run，租约跨实现/集成阶段和连续工具调用持有；④writer 阶段禁用 task，普通工具强制按模型调用顺序 FIFO 串行；⑤ProcessHandle 共享项目协调器，ToolCtx 分离 worktree_key 与 project_write_key 并携带 run/process 身份；⑥quick_req、tracker、goal、memory、test_record、Git/worktree 等独立写入口全部接入同一仲裁；⑦写队列、租约、阶段、取消和恢复事件落现有 session/run 轨迹。
- 边界: P0 覆盖当前应用内多个 ProcessHandle；不做图形化 DAG、不开放子代理通用写权限、不在本批实现跨机器调度。worktree 保留隔离、diff、恢复和交付能力，但不能绕过项目级单 writer。
- 验收: ①至少两个只读子代理真实重叠执行且工具白名单无写入口；②汇总屏障前 writer 不启动，失败/超时都有终态；③writer 阶段普通工具 max in-flight=1 且结果按调用顺序归位；④两个 ProcessHandle 竞争写权时租约区间不重叠，同一 writer 的连续写之间不能插入第二个 writer；⑤quick_req/tracker/test_record/Git/worktree 写入无法绕过协调器；⑥停止、关闭、panic 收尾后租约可靠释放；⑦一条真实需求留下「并行勘察→串行实现/集成→并行复核→串行修正」完整轨迹。
- refs: R-050 R-117 R-138 R-141 D-227 docs/design/parallel_read_serial_write_orchestration.md

- 批次: 6/6
- 进展: 批6 完成并提交(b8d2b05);批7 为收口核对(无源码改动,不计批)。全量测试通过(cargo test --workspace 全绿)。逐项核对验收:①白名单无写入口=既有 SubagentBase 只读快照+task_spec 只有 read/glob/grep;③drive 串行 max in-flight=1 按序归位=批2 测试锚定;④租约不重叠+连续写不插第二 writer=批1 core 4 测;⑤旁路全部接入=批4 各 command acquire;⑥panic/取消/停止释放=批1 panic 测+RAII。②⑦ 按用户裁决 A 拆至 R-173(P1 阶段编排对象)。

## R-129 单页阅读信息记忆困难优化 [done]
- priority: P3
- 原始描述: 记忆单页阅读信息太复杂，有阅读障碍
- 复杂度: 中
- 归属: kanzei
- 验收: 提供分段展示/摘要功能帮助用户理解单一页面内容，减少认知负荷

- 标签: 前端

- 进展: B1 完成并提交(18f203a):记忆详情正文从单一 textarea 改为摘要+分段阅读——摘要行(首段去换行截 140 字)、按空行拆段、超长段(>6 行或 >280 字)折叠可展开、编辑按钮切回 textarea、保存后就地重渲染。i18n 五条新 key 登记,四条 ui 冒烟全绿。收口:cargo test --workspace 全量 + 逐项验收核对 + 关闭。

- 批次: 1/1

## R-128 全部阻塞时停止鞭挞的逻辑设计 [done]
- priority: P2
- 原始描述: 如果全部阻塞，应该要停止鞭挞，需要更多的设计鞭挞停止的逻辑
- 复杂度: 中
- 归属: kanzei
- 验收: 当全部条目处于阻塞状态时,系统自动停止鞭挞,不再触发催办;阻塞解除后可恢复

- 标签: 核心

- 进展: 2026-08-10 收口:R-169/R-170 引擎化已交付本条目验收的两个分支,本次补上后半段验收的直接单测后关闭。验收证据:
①「全部条目阻塞时自动停止鞭挞、不再触发催办」——引擎状态机 kanzei-harness/src/auto_run.rs:decide() 最先检查 ctx.backlog.should_stop()(L161-168),AllBlocked → Stop(AutoStopReason::AllBlocked);单测「全部阻塞或清空_优先于其它判定停止」(L356-378,即使有动作/未暂停/未达上限也停)。backlog 单源在 kanzei-tools/src/tracker.rs:backlog_status()(L836-868:活动条目全部带阻塞原因 → AllBlocked),桌面端轮末消费 kanzei-app/src/run.rs:699-713,前端收到 Stop 后取消自动推进并提示「需求与缺陷全部被阻塞」(ui/07-events.js:356-363),CLI 轮末同源提示 kanzei/src/main.rs:587-595。
②「阻塞解除后可恢复」——本次新增单测「阻塞解除后_恢复续跑」(auto_run.rs L380-393):AllBlocked 停止后 backlog 回到 Workable,同一状态机下一轮正常 Continue 并计数;停止仅由当轮 backlog 状态触发,不持久锁死。
全量门禁:cargo test --workspace 全绿(T-1786368929),auto_run 13/13。设计依据:docs/design/continue_prompt_dissection.md §4(架构索引已登记)。

## R-130 测试用例记录触发机制与缺陷迁移 [done]
- 原始描述: 测试用例相关的记录似乎没有触发机制，然后是把测试移动到缺陷下面，然后需要一次性记录存性
- 复杂度: 中
- 归属: kanzei
- 验收: 实现基于事件的或手动触发的测试用例记录机制，并在系统中建立测试到缺陷的映射关系，完成现有机现有测验数据的批量导入和初始化。
- 优先级: P2

- 标签: 后端

- 批次: 2/2
- 进展: 2026-08-10 B2 完成(777cdaf):①test_runs_init_refs 接入项目级写仲裁(docs.rs,与 test_run_record 同模式,D-227 并发覆盖门禁);②initialize_refs 无变化不写盘(幂等零副作用,test_record.rs);③前端 refreshTests 每次刷新前调用 test_runs_init_refs(09-sessions.js),批量初始化获得真实消费者;④冒烟断言 init 被调用且带 projectDir(ui-runtime-smoke.mjs)。T-1786369234 定向+冒烟、T-1786369281 全量均绿。

## R-133 diff树渲染优化 [done]
- 原始描述: diff树的显示很丑，标记颜色并且不要重叠
- 复杂度: 中
- 归属: kanzei
- 验收: 实现color标记的git diff树，解决重叠问题确保视觉清晰
- 优先级: P2

- 标签: 前端

- 进展: 2026-08-10 交付(d980889):①diff 汇总按路径层级成目录树——buildDiffTree/appendDiffNode(06-activity.js)把平铺路径归入可折叠目录(▾/▸,aria-expanded),文件行按深度缩进,+/− 计数沿用 diff-add/diff-del 同色系(D-237 既有);②并排视图重叠修复——.diff-pane 加 min-width:0 + overflow-x:auto,长行(pre 不换行)在自身列内横向滚动,不再溢出覆盖相邻列(grid 子项默认 min-width:auto 被 pre 内容撑破的根因);③冒烟新增断言:树容器/目录行/文件归组/折叠交互(ui-runtime-smoke.mjs)。验收证据:color 标记=diff-add/del 色(style.css:1032-1033 既有)+ 树形目录头;diff 树=buildDiffTree L7-66;重叠解决=.diff-pane 规则(style.css:1020-1024);四条冒烟全绿 T-1786369496。复杂度中,纯前端改动 crates/ 零变更,全量等价 R-130 门禁(T-1786369281 全绿)不重跑。

## R-050 并行对话线程与分支工作树:隔离运行、冲突检测与合并 [dropped]
- 复杂度: 大
- 优先级: P2
- 来源: 用户反馈:历史对话或新开线程并行推进项目,类似 git 分支/树,最后解决冲突合并
- 验收: 设计文档明确线程/项目/工作树关系、锁顺序、取消与崩溃恢复;两个线程可独立运行且互不串消息/权限/活动/停止;写入冲突能在提交前检测并阻止自动覆盖;worktree 模式可查看 diff、选择合并或放弃;合并失败保留双方改动和可恢复入口
- 已完成: 线程隔离(=R-030 进程页签)真实可用,消息/权限/队列/活动/停止按 session 隔离并有 POC 测试;worktree 后端命令 create/diff/merge/discard 存在,merge 前的 `git merge-tree --write-tree` 冲突预检真实实现(kanzei-app/src/main.rs:671-684);设计文档 deep_parallel_dev.md(含附录早期 POC)继续承载 worktree/模型隔离方案,多进程调度与写入纪律以 parallel_read_serial_write_orchestration.md/R-171 为准。
- 退回原因: 2026-08-07 验收核查发现核心组合未成立,勾不该打。①worktree 与线程完全脱节:ProcessHandle.worktree_path 恒为 None(main.rs:164/523,全仓库无 Some 赋值),process_create 不接受 worktree 参数,run_prompt 校验进程必须属于主项目目录(2605-2607)——没有任何线程能在 worktree 里运行,所有并行线程写同一工作目录;应用内无流程会在 worktree 分支产生提交,"合并"在闭环内空转。②多进程同一工作树无任何写冲突检测,设计承诺的项目写锁/git 锁/docstore 版本哈希在代码中完全不存在。③"可查看 diff"实为 git status --porcelain 文件名列表弹 toast(见 D-096)。④崩溃恢复仅设计文字,worktree 清单存 localStorage 不从 git worktree list 发现。
- 下一步: R-171 先在 memory system 开发序列之后交付项目级单 writer 与串行工具地基；R-050 的 worktree 绑定、diff 与恢复仍按 deep_parallel_dev.md 分阶段推进,且该文 §6 其余 D1~D7 未经用户定案前不得动工。
- 遗留质量问题: worktree 四个命令零测试;worktree_field 的 field 参数是无效分支(main.rs:605-610 两分支返回同值);frontend_phase3.md 的 POC 章节重复粘贴两遍且第一遍路径写错。
- 阶段: 5
- 证据等级: E2+E3
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 核心

- 进展: 2026-08-10 口径更新:本条 worktree/模型隔离部分的门禁仍成立,保持 todo;项目级单 writer 与串行工具已拆为 R-171,不受本条未定决策阻塞。 2026-08-10 晚:用户已对 deep_parallel_dev.md §6 的 D1~D7 逐条拍板(并补 N1~N3),阻塞条件消失,`阻塞:` 字段按 conventions §1.1「解除条件已满足当场清空」清除;本条随即按下方关闭说明收口。
- 关闭说明: 2026-08-10 关闭(**dropped**)。用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,本条按「关闭原条、新开子条」收口。

  **逐条对照验收原文(五条)**:
  ①「设计文档明确线程/项目/工作树关系、锁顺序、取消与崩溃恢复」→ **已达成**:关系与状态机在 deep_parallel_dev.md 附录 A.1/A.2,锁顺序在附录 A.3,并被 docs/design/parallel_read_serial_write_orchestration.md 的十条核心不变量取代为更严格的口径;§6 于本日全部定案。
  ②「两个线程可独立运行且互不串消息/权限/活动/停止」→ **已由 R-030(进程页签)交付**,消息/权限/队列/活动/停止按 session 隔离,有 POC 测试背书(2026-08-07 核查确认这部分真实)。
  ③「写入冲突能在提交前检测并阻止自动覆盖」→ **已由 R-171(项目级单 writer + 写租约 + FIFO 排队 + 旁路收口)交付**。这正是本条 2026-08-07 退回原因②的归宿,不在本次拍板范围;deep_parallel_dev.md §6.3 已记一笔,避免下次评审重复拍板。
  ④「worktree 模式可查看 diff、选择合并或放弃」→ **后端已达成、前端接线转移**:**D-096 已 [fixed]**,`worktree_diff` 已返回真实 `git diff --no-ext-diff --binary`,不再是文件名列表弹 toast;**R-133 已 [done]**,`crates/kanzei-app/ui/06-activity.js` 提供可折叠的 diff 目录树渲染器(`buildDiffTree`/`renderDiff`);`worktree_merge`(含 `merge-tree --write-tree` 冲突预检)与 `worktree_discard` 命令真实可用。剩下的只是把两者接起来 → **R-179**。
  ⑤「合并失败保留双方改动和可恢复入口」→ **后端已达成**:`worktree_merge` 的冲突预检与 `worktree_discard` 失败时"已保留以便恢复"的兜底存在;UI 侧的可读展示 → **R-179**。

  **剩余未覆盖的,逐条点名去处**:线绑 worktree(`ProcessHandle.worktree_path` 至今恒 `None`)、线清单从 `git worktree list --porcelain` 发现(现在存 localStorage)、四个 worktree 命令零测试、一树一线查重 → **R-177**;崩溃恢复里的模型/会话重建、线级模型隔离与状态持久化(含 R-030 遗留的"重启不丢页签")、设置页作用域选择器 → **R-178**;diff 前端接线、合并/放弃确认流、线页签仪表、`worktree_field` 的死分支 → **R-179**。本条「遗留质量问题」三项:worktree 四命令零测试 → R-177 验收⑦;`worktree_field` 死分支 → R-179 顺手修;frontend_phase3.md 的 POC 章节重复 → 该章节已于 2026-08-08 移入 deep_parallel_dev.md 作为附录,问题自然消解。

  **为什么取 `dropped` 而不是 `done`**:三条理由。①conventions §1.2 明令「验收的功能性条款未实现」不得关闭为完成——本条最核心的组合(线在 worktree 里独立运行)**至今一行都没实现**,只是被转移了。②已达成的四条验收**全部由别的条目交付**(R-030 / R-171 / D-096+R-133),标 done 等于把它们的交付重复记在本条账上,违反 §1.25「不得把既有能力重新申报为本次产出」。③本条 2026-08-07 已被验收核查判定「部件各自真实、组合从未成立」并退回,其后未再产出实现。dropped = **本条作为容器退役,诉求整体转移**,不虚增交付统计。(口径与 R-117 一致,但理由不同:R-117 是自身从未产出实现,本条是自身核心从未成立且已达成部分归属别人。)
- refs: R-030 D-096 R-133 R-171 R-177 R-178 R-179 docs/design/deep_parallel_dev.md docs/design/parallel_read_serial_write_orchestration.md

## R-117 子代理运行状态的可观察性 [dropped]
- 复杂度: 中
- 优先级: P3
- 原始描述: 添加触发后弹出浮层显示最近开发和当前进展列表
- 范围界定: 2026-08-08 用户澄清真实意图是"子代理能对当前运行状态进行观察",并明确表示在 R-095 的呈现优化落地后不确定是否仍需要独立入口。
- 待定: 本条挂在 R-095 之后再定去留。R-095 交付后由用户判断:若活动面板的筛选折叠与后台任务操作已足够观察子代理状态,则本条关闭;若仍缺子代理各自的进度维度,则按缺口重写验收。
- 依赖: 

- 标签: 前端

- refs: R-095
- 进展: 2026-08-10 复查:R-095 已交付(done),其验收⑤明确覆盖子代理状态观察——活动面板子代理条目给出内部调用数与当前步骤,入参/输出/成败/耗时齐备。本条原始诉求「子代理能对当前运行状态进行观察」已被 R-095 覆盖;去留按「待定」字段由用户拍板(关闭或按缺口重写验收),agent 不擅自决定。依赖 R-095 已关闭,移入 refs。
- 关闭说明: 2026-08-10 关闭(dropped)。本条「待定」字段原文写着「本条挂在 R-095 之后再定去留,由用户判断」——**用户本次定调已经回答了它**:看过 Claude Code 的后台子代理面板后明确要**独立入口**,而且要那种形态(独立 Running / Finished 分区面板,每条显示 名称/类型/时长/token/工具调用数/当前工具名/单条停止/View transcript,面板有 Clear)。按 conventions §1.2「残余验证与质量缺口不丢弃:关闭时转移到专门条目并在关闭条目进展里注明去处」:本条的全部诉求(独立入口 + 子代理各自的进度维度)转移由 **R-174**「子代理面板与并发度口径」承接,R-174 的验收③④⑤⑥⑦ 覆盖了本条缺的每一项(六字段取真实事件、单条停止真能停、完整 transcript 有真实数据源、冒烟断言、桌面端可达)。本条不再单独保留入口决策,故取 dropped 而非 done——本条自身从未产出实现,声称 done 会虚增交付统计。

## R-137 Anthropic thinking 块协议回放:signature 原样回传,多轮工具不再 400 [done]
- 背景: direction_taste 复刻清单·高:CC 按协议要求回放 thinking 块;kanzei 现状 anthropic.rs:97 Part::Reasoning => None 丢弃全部 Reasoning,thinking+工具第二轮必 400(R-094 只做了请求侧思考强度,未做响应侧回放)。
- 设计定位: 复刻 CC 基线行为:thinking 块按协议要求回放
- 证据等级: E2
- 阶段: 1
- 验收: anthropic 通道多轮工具调用时:①thinking 块的 signature 在后续请求中原样回传;②thinking+工具第二轮不再 400;③非 thinking 模型的 reasoning 文本以可见 assistant 文本保留(与 R-094 结论一致);④补 anthropic 多轮含 thinking 的协议契约测试。

- 优先级: P0

- 标签: 模型

- 复杂度: 中
- 进展: 2026-08-10 交付(8a63c78):anthropic.rs message_to_value 对 Part::Reasoning 的协议回放——有 signature → 按 Anthropic 协议输出 {"type":"thinking","thinking":text,"signature":sig} 原样回传(验收①);无 signature → 降级为可见 assistant 文本块(验收③,R-094 结论);空 reasoning 整体跳过。signature 由响应侧 signature_delta→ReasoningEnd 收集、runner drive.rs 已存入 Part::Reasoning(既有,R-137 前已就位),本次只补回放缺口。新增两个契约测试(thinking_replay_roundtrips_signature_and_tool_sequence / reasoning_without_signature_falls_back_to_visible_text),覆盖验收④;测试断言 thinking+tool_use+text 块序不被打乱(验收②的请求体侧保证)。定向:kanzei-llm 42/42、core 103/103、下游 check 全绿。

## R-138 docstore 原子写与跨进程文件锁:tmp+rename + 独占句柄,并发写不丢不撞 [done]
- 背景: direction_taste §5.2 地基债:docstore 整文件重写无原子替换与跨进程锁,D-064 类 lost-update 真实存在;deep_parallel_dev §3.3 P4 也要求 docstore 进程级文件锁收口主根 .kanzei 的最后一个共享写点。
- 设计定位: tracker 文档写入的原子性与并发安全
- 证据等级: E2
- 阶段: 1
- 验收: docstore save 改 tmp+rename 原子替换(临时文件与目标同目录);跨进程文件锁(Windows std::fs 独占句柄,毫秒级持有);并发写 tracker 的压测不丢条目不撞 ID;失败时保留现场可重试。

- 优先级: P0

- 标签: 核心
- 复杂度: 中
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(done)。交付提交 `b4bda5c`,与 **D-249** 并轨交付(同一条竞态通道的上下游两层)。

  **逐条对照验收原文(四条)**

  ①「docstore save 改 tmp+rename 原子替换(临时文件与目标同目录)」→ **达成,且覆盖面大于验收原文**。新建 `crates/kanzei-tools/src/atomic_file.rs`,`write_atomic`(:35)的序列是:同目录 tmp(文件名带 pid + 纳秒)→ `create_new` → `write_all` → **`sync_all`** → `rename`,失败按 `RENAME_ATTEMPTS=6` / `RENAME_BACKOFF_MS=20`(:21-22)退避重试。`sync_all` 不可省——没有 fsync 时 NTFS 可能把 rename 排到数据落盘之前,断电后拿到零长度文件,正是本条要根治的形态。**替换的是 docstore 的全部 4 个整文件写点,不只 save**:`save`(docstore.rs:332)、`repair_reused_archived_id`(:392)、`archive_terminal`(:461/:523)、`void_id`(:608)。`archive_terminal` 的两步写序另加注释锁死「先写归档再删活动」:原子写保证不了跨文件原子性,当前顺序崩溃后是「条目同时在两处」(完整性门禁能报),反过来才是真丢数据;回归测试在 docstore.rs:1467 附近(注释写明「谁把 save 提到 write_atomic 前面,这条就会红」)。测试:`原子写替换已有目标且不留临时文件`(atomic_file.rs:381)、`临时文件与目标同目录`(:400,注释点明跨卷 rename 会失败、这条不变量塌了原子写就整体失效)、`父目录不存在时自动创建`(:415)。

  ②「跨进程文件锁(Windows std::fs 独占句柄,毫秒级持有)」→ **达成,零新依赖**。`FileLock`(atomic_file.rs:125)双层实现:**进程间**用 Windows `share_mode(0)` 开独占句柄(:306),第二个进程 open 直接失败,句柄随进程退出由 OS 关闭,崩溃不留死锁;非 Windows 走 `O_EXCL` + mtime 陈旧摘除(`LOCK_STALE_AFTER=30s`,:106)。**进程内**是手写可重入互斥(Mutex + Condvar + ThreadId)——**刻意没用 `std::sync::ReentrantLock`**:它没有限时等待,`docs_snapshot` 的 `try_lock(200ms)` 遇进程内争用就没法遵守预算,会退化成无限等。API 两个入口:`lock_exclusive`(:164)与限时的 `try_lock_exclusive`(:183);`DocStore` 侧包装为 `lock()`(docstore.rs:308)与 `try_lock(budget)`(:313)。**纪律由编译器强制而非君子协定**:`FileLock` 带 `_not_send: PhantomData<*const ()>`(:128)做成 `!Send`,「绝不跨 await、绝不跨线程持有」这条规矩谁想违反都编译不过。防死锁不变量写进注释:持锁期间永不获取第二把锁,跨 kind 的 `check_refs` 走不加锁读路径,结构上不可能循环等待。测试:`独占句柄第二次打开必然失败`(:427)、`锁同线程可重入而其它线程排队`(:447)、`限时取锁拿不到时返回空而不是错误`(:485)。

  ③「并发写 tracker 的压测不丢条目不撞 ID」→ **达成**。**关键判断,与验收字面不同**:真正的 lost-update 不在 `save` 里,而在 `TrackerTool::execute` 的 `load → next_id → save` **跨度**上——两次 save 本来就不重叠,丢失发生在它们各自的读与写之间。所以锁加在写动作分支顶部罩住整段:`crates/kanzei-tools/src/tracker.rs:184` 的 `let _write_lock = if WRITE_ACTIONS.contains(...)`(`WRITE_ACTIONS` 定义在 :31),读动作(list/get)不取锁照常并行。回归闸打在**真实写入口**上:`并发新建不丢条目也不撞编号`(tracker.rs:2354),8 个线程各起独立 runtime(模拟互不共享内存态的 OS 进程)并发 `req add`,断言落 8 条、8 个 ID 互异、`integrity_issues` 为空。**反证**:把这把锁回退后跑同一用例,8 个并发 add **只活下来 1 条、8 个全拿到 R-001**——lost-update 与 ID 撞车同时坐实。

  ④「失败时保留现场可重试」→ **达成**。`write_atomic` 抄的是 `auth/store.rs` 那份原子写,但**故意反着改了一处**:替换失败时**保留 tmp 不删**。理由写在实现注释里——凭证可以重新登录,tracker 的新内容是内存里唯一一份,删了就是丢用户这次编辑。测试 `替换失败时保留临时文件且原文件不被破坏`(atomic_file.rs:510),用 `share_mode(0)` 独占打开目标模拟杀软/编辑器占用,断言原文件完好且 tmp 留着。

  **读路径一律不加锁**(设计决策,别在后续"顺手加锁"里改掉):原子替换后读者只可能看到旧全量或新全量,不存在截断态;让读者排队只会把 UI 刷新变慢。

  **既有能力标注(§1.25)**:`DocStore` 的解析/序列化、`archive_terminal` 的归档语义、`repair_reused_archived_id` 的保守拒改立场均为既有实现,本条只替换写原语并在写事务外围加锁,不重复申报这些能力。

  **残余缺口去处(§1.2)**:`crates/kanzei-tools/src/test_record.rs` 的五处生产 `std::fs::write` 尚未并轨 `atomic_file`(跨进程 CAS 缺失)→ 已登记 **D-261**;`test_runs_snapshot` 这条只读命令顺手写盘且不持任何锁 → 已登记 **D-260**(修复口径照抄本条对 `docs_snapshot` 的处置:限时文件锁,**不挂写租约**)。设计基线的口径已回写:`docs/design/parallel_read_serial_write_orchestration.md` 不变量 8 的 2026-08-10 补注(提交 `79852a5`)确立判据——**代理发起的写动作走租约,界面读路径顺手做的幂等维护走文件锁**。

  **验证**:交付时定向 kanzei-tools 208 / kanzei-app 53 全绿,clippy `-D warnings` 零输出,rustfmt clean;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度中,满足 §1.4 全量触发点①)。

## R-141 ToolCtx 显式主根绑定:消除发现式取根与 worktree 锁键歧义 [done]
- 背景: direction_taste §5.4 与 D-170 教训:ToolCtx::new 仍发现式取根(harness/src/tool.rs:13-17),worktree 线若命中 worktree 内 .kanzei 副本会拿到过期身份;并发锁键语义(tool.rs:19-28)只拼 project_root,两棵树同路径会撞锁。deep_parallel_dev §3.2 明确选 A:显式主根、不做根发现。
- 设计定位: 深并行前置:线进程显式携带主根,消除发现式根解析事故面
- 证据等级: E2
- 阶段: 1
- 验收: ToolCtx 构造支持显式传入 project_root(不再无条件 discover);线路径全程显式传根;补断言测试:worktree 内运行时 project_root 必须等于主根;并发锁键区分 worktree 实例。

- 优先级: P0

- 标签: 核心
- 复杂度: 中
- 批次: 2/2
- 关闭说明: 2026-08-10 关闭(done)。交付提交 `8574b63`(批1:harness/tools/CLI)+ `bf85fe9`(批2:桌面端线路径)。行号以交付时 dev HEAD 为准;`crates/kanzei-app/src/run.rs` 正被 R-173 批6 改动,该文件的证据以符号名为准。

  **逐条对照验收原文(四条)**

  ①「ToolCtx 构造支持显式传入 project_root(不再无条件 discover)」→ **达成**。`crates/kanzei-harness/src/tool.rs:39` 的 `ToolCtx::new(cwd, project_root)` 改双参,函数体内零 `discover_project_root`;发现式取根被拆成独立入口 `ToolCtx::discovering(cwd)`(tool.rs:63),其文档注释钉死「线路径调用它是 bug」,并写清 D-170 的 worktree 变体成因——`.kanzei/project/*.md` 被 git 跟踪,`git worktree add` 会把它们 checkout 成分支副本,而 worktree 的 `.git` 是文件不是目录,于是 `discover_project_root` 在 worktree 内第一层就命中副本立即返回。

  ②「线路径全程显式传根」→ **达成**,两端各有落点。CLI:`crates/kanzei/src/main.rs:180` 改 `ToolCtx::new(cwd, project_root)`,根在入口解析一次后显式传下。桌面端:`run_task` 新增 `main_root: PathBuf` 参数(`crates/kanzei-app/src/run.rs`,签名处注释说明 worktree 上线后 `project_dir` 是代码树、`main_root` 仍是主根),函数体内 `let project_root = main_root;`,`discover_project_root` 归零;调用方 `run_prompt` 在 IPC 入口解析一次后传下。同时把 `resolve_profile_and_root` 拆成 `resolve_profile`(只解析 profile,不再顺手发现根),注释写明「捆在一起正是根发现能悄悄溜进线路径的原因」。**可机械核验的收敛证据**:HEAD 上 `run.rs` 只剩 2 处 `discover_project_root`,全在 Tauri command 第一行(`summarize_chat`、`run_prompt`);生产代码里 `ToolCtx::discovering` 只剩 2 个调用点,均为进程/IPC 入口(`crates/kanzei/src/main.rs:858` CLI tracker 子命令、`crates/kanzei-app/src/docs.rs:343` `docs_update`),`crates/kanzei-tools/src/todowrite.rs:97` 那处在 `#[cfg(test)]` 内不计;`crates/kanzei-app/src/subagents.rs:39/218` 两处也都在 `#[tauri::command]` 函数体内(quick_req / defect_review),属入口不属线路径。

  ③「补断言测试:worktree 内运行时 project_root 必须等于主根」→ **达成**。`crates/kanzei-harness/src/tool.rs:248` `worktree_内运行时_project_root_必须等于主根`。**这条测试的写法值得单独点名**:它**先断言危害前提**——`discover_project_root(worktree) == Some(worktree)` 且 `!= Some(main)`,把「发现式取根在 worktree 内必然拿到分支副本」本身钉成回归闸,然后才断言显式绑定后 `ctx.project_root == main && ctx.cwd == worktree`。少了前半段,后半段在「discover 恰好也能返回主根」的实现下会假绿,测试就证明不了自己在防什么。夹具 `worktree_fixture`(tool.rs:221)复刻真实磁盘形态:主根与 worktree 是兄弟目录、worktree 内有 `.kanzei` 副本、`.git` 是 `gitdir:` 指针文件。

  ④「并发锁键区分 worktree 实例」→ **达成**。`worktree_concurrency_key()`(tool.rs:90)的缺省回退从 `project_root` 改 `cwd`——显式主根后同项目 N 棵树的 `project_root` 完全相同,拿它当工具锁键会把互不相干的树串死;锁键真源是工具实际作用的代码树(bash 用 `ctx.cwd.join(workdir)`,git 用 `ensure_repository(&ctx.cwd)`)。生产接线在 `run.rs` 的执行身份注入处:`worktree_key = ctx.cwd.display()`、`project_write_key = normalized_project_root(&ctx.project_root)`,两把键各自带注释写清不变式(写主根的串行、写代码的并行)。测试三条:`两个_worktree_实例锁键必须不同_写仲裁键必须相同`(tool.rs:274,同时断言 `ToolConcurrency::write_worktree` 两侧 `!conflicts_with`)、`未显式设身份时锁键回退到代码树而非项目根`(tool.rs:302)、`同一棵树的锁键对大小写与分隔符稳定`(tool.rs:316)。

  **超出验收的一处质量决策(必须留档,别在后续重构里当冗余合并掉)**:批2 的 `main_root` **刻意不复用** `run_prompt` 已算好的 `project_root`。后者 canonicalize 过,Windows 上形态是 `\\?\C:\…`;它一旦成为 `ctx.project_root`,就会同时决定 DocStore 路径、`project_state_path` 与**工具权限资源的归一形态**——`permission::normalize_resource` 把 `\\?\C:\x` 归成 `//?/C:/x`,与 `c:/x` 是两个串,用户经 `append_allow_rule` 存下的绝对路径放行规则会一夜全部失配。所以拆成两个值各司其职:`project_root` = 规范化身份键(喂 `process_session_id` 与进程归属比较),`main_root` = 文件系统形态主根(喂 `ctx.project_root`,不 canonicalize)。托管文档落盘路径与权限规则形态逐字节不变,规范化只用在写仲裁键上。

  **顺带收益**:`crates/kanzei-tools/src/edit.rs:343` 的测试 fixture 原本靠「本机 HOME 恰好没有 `.git`」才解析出正确的根,显式双参后这条隐式环境依赖消失。

  **既有能力标注(§1.25,不重复申报)**:`with_identity` / `project_write_key` / `ToolConcurrency` 的框架是 R-171 既有产出;本条只改锁键**取值来源**(project_root → cwd)与生产侧接线,框架本身不是本次产出。

  **残余缺口去处(§1.2)**:worktree 线本身尚未上线(`ProcessHandle.worktree_path` 仍恒 `None`),本条只交付「显式传根 + 双键拆开」的地基;线绑 worktree、`run_prompt` 归属校验改按 origin_project、配置读主根 → **R-177**(该条「前置」字段已写明依赖本条批2,现已满足)。

  **验证**:交付时定向 kanzei-harness 70 / kanzei-tools 187 / kanzei-core 103 / kanzei-app 53 全绿;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度中,满足 §1.4 全量触发点①)。

## R-173 阶段编排对象:勘察屏障→串行实现→复核屏障→修正闭环 [done]
- refs: R-171 R-117 R-050 docs/design/parallel_read_serial_write_orchestration.md
- 优先级: P1
- 依赖: 
- 内容: R-171 验收②⑦ 转移承接:①阶段编排对象:baseline→scouting(并行只读子代理)→汇总屏障→implementation(单 writer 租约,串行)→integration(同一 writer)→review(并行只读复核)→复核屏障→fixup(重新获取写租约串行修正);②汇总屏障:scouting 全部任务进入终态(完成/失败/超时)前 writer 不得启动,失败/超时都有确定终态,屏障不永久挂起;③复核屏障:writer 释放租约后复核才启动,保证审查的是稳定快照(设计不变量 9);④真实闭环验证:一条真实需求留下「并行勘察→串行实现/集成→并行复核→串行修正」完整轨迹,事件落 session/run 轨迹(复用 R-171 批5 的 orchestration.* 事件与批6 的读槽登记)。
- 复杂度: 大
- 归属: kanzei
- 来源: 2026-08-10 R-171 关闭裁决:验收②⑦ 依赖阶段编排对象,按用户选择 A 拆为本 P1 需求承接
- 标签: 核心
- 调度顺序: R-171 关闭后按序取活
- 阶段: 3
- 验收: ①至少两个只读子代理真实重叠执行,汇总屏障(最慢任务完成/失败/超时)前 writer 不启动,失败/超时都有确定终态;②一次真实需求完成并行勘察→屏障→串行实现/验证→并行复核→复核屏障→串行修正全轨迹,阶段事件落 session_events 可回放;③复核阶段在 writer 释放后启动,审查的是稳定快照;④writer 活跃时允许只读勘察继续(读写共存,复用 R-171 读槽机制)。
- 批次: 7/7
- 关闭说明: 2026-08-11 交付关闭。逐条对照验收原文,证据均为生产路径(§1.25):
  **①-a「至少两个只读子代理真实重叠执行」✓** —— 两条路径各有证据。模型自派路径:`crates/kanzei/tests/parallel_scouting_under_serial_writer.rs`,从**真实发出的 HTTP 请求体**解析 `tools` 数组断言 `task` 已注册(主快照刻意留空,出现只可能来自 drive 的注册分支),并断言两条 `agent_started` 都早于任何一条 `agent_completed`——重叠的确定性证据,不靠 sleep 卡时序。编排派发路径:`phase_pipeline_tests::七阶段闭环轨迹落库可回放`(8 条 started / 8 条 completed)。实现点 `core/runner/drive.rs` 的 task 注册分支 + `core/runner/subagent.rs` 的读槽登记。
  **①-b「汇总屏障前 writer 不启动」✓** —— 决策点抽成了可直接测的生产函数 `phase_pipeline.rs::acquire_plain_lease_if_needed`(流水线开启 → `Ok(None)`),此前内联在需要真实 Tauri Window 的 `run_task` 里、只能靠读代码相信。闭环测试三重断言:`plain_lease.is_none()`、勘察返回后 `writer_run_id.is_none()`、**落库事件序 `barrier_reached(synthesis)` 早于第一条 `writer.acquired`**。机械保证是迁移表里没有 `Scouting→Implementation` 边(`阶段迁移表穷举_合法边恰好十条`)。
  **①-c「失败/超时都有确定终态」✓** —— `core/phase.rs::run_barrier` 双层有界(内层 `subagent_timeout_secs`、外层 `barrier_timeout_secs`,后者显式配置也按内层+1 夹紧);`三终态收敛_失败不中止且零结果告知模型` 用一个**永不返回**的任务验证外层确实收敛。`ScoutOutcome` 只有终态变体,「还在跑」在类型上表达不出来。
  **②「阶段事件落 session_events 可回放」✓(自动化证据)** —— `七阶段闭环轨迹落库可回放`:真 SQLite + 真协调器 + 真子代理 + 真观察者(只有 provider 是假的),**另开一条连接**从真库读回,断言八段阶段名按序、两道屏障统计(5/5 与 3)、`sequence` 单调。落库单一出口 `app/orchestration_trace.rs::SessionEventObserver`,事件类型与 payload 经 `OrchestrationEvent::event_type()/payload()` 产出,收掉了「枚举一套、落库手写字符串一套」的漂移面。
  **③「复核在 writer 释放后启动」✓** —— `core/phase.rs::release_lease` 三层保证:`Option::take` 移交所有权(调用方在类型上无法一边持租约一边复核)→ 同步 `drop`(释放回调无 await 无 spawn,返回即已释放)→ 独立快照复核(若将来有人给释放路径加异步分支,这里当场 `LeaseStillHeld` 而非静默放行)。测试 `复核屏障_交出租约后才进复核`、`未持租约进复核被拒`,闭环测试断言 `writer.released` 严格早于 `phase_changed(review)`。
  **④「writer 活跃时允许只读勘察继续」✓** —— `core/orchestration.rs::acquire_read_slot` 全程不读 `writer_run_id`、唯一返回路径是 `Ok`(**R-171 既有性质,本次只补验证与真实消费者,不作为本次产出申报**)。生产路径证据:`parallel_scouting_under_serial_writer.rs` **整轮真实持有写租约**跑勘察,并断言全程无写租约排队事件。
  交付批次: 批1 契约(`6f98db2`)/ 批2-4 实现(`67c3fa2`,顺带修 `release_writer` 交接断档与 `process_id` 恒空两缺陷 + `normalize_project_root` 不剥 `\\?\` 导致 worktree 写命令与主对话 writer 落在两个仲裁桶的漏洞)/ 批4.5 恢复桌面端并行查(`e933262`,顺带修读槽按 agent_name 回收导致 `AgentCompleted` 张冠李戴)/ 批5 事件落库(`38716a7`)/ 批6 流水线接线(`45a5e54`)/ 批6.5 路由可配与进度事件(`a921b14`)/ 批7 闭环测试与设计基线回写(`40fe3d8`)/ 前端进度上面板(`ff287c4`)。
  **三个同族缺陷值得单记**:`release_writer` 交接断档、`process_id` 恒空、读槽按 agent_name 回收——全部是「R-171 时这条路不可达所以没人发现,阶段编排与并行查一恢复就变成真实的审计错误」。**不可达的代码不是没有 bug,是 bug 还没被叫醒。**
- 残余缺口(§1.2 转移,不丢弃): 
  ①**「一次真实需求」的真机佐证不存在**——R-173 自身的开发在外部 agent 环境完成,没有走过 kanzei 桌面端的自主推进轮,所以验收②目前只有自动化证据。补法:用户开着自主推进跑一条需求,从 state.db 导出那段事件流。**不含混过去,如实记录。**
  ②`run_task` 外围那层无测试覆盖:闭环测试复刻的是它在流水线开启时的完整调用序列(同一批生产函数),但 `run_task` 本身需要真实 Tauri Window,单测起不来。未覆盖 Window 事件发射、store 生命周期记账、`phase_pipeline_on` 的 auto_runs 闸门取值(三行 `is_some_and`)。转 R-101(桌面端 E2 harness)。
  ③修正段触发判据是「非 NO_ISSUES 即有发现」,**失败/超时也算有发现**(宁可多跑一段,不把没复核过的当成复核通过)——有意的保守取向,但弱模型下大概率每轮都跑修正段,自主推进轮的成本上限是「5 勘察 + 主对话 + 3 复核 + 修正」。旋钮是既有的 `[limits] max_tasks_per_turn`。
  ④单条停止通道不存在 → **R-174 验收④**。最小改法已备:每角色配 `CancellationToken`,新 Tauri 命令按 role 触发,取消后以 `ScoutOutcome::Failed("cancelled")` 进终态,屏障照常收敛不会挂住。
  ⑤编排派发的 8 条同时也会在主对话里各生成一个工具块(`chatToolStart` 无条件调用),信息没丢但每轮多 8 个块可能偏吵 → **R-174**(面板形态决策)。
  ⑥前端面板当前每角色只留最新一轮(`id` = 角色名,跨轮必然重名,已修跨轮复位)。R-174 做独立面板时若要保住历史轮次,需后端给 `role@round` 之类的唯一键。
- 进展: 2026-08-10 推进中,**不关闭**——按 §1.25 三条验收(①②③)目前仍缺生产调用方,阶段编排对象尚未接进真实运行链路。已交付批次:批1 阶段编排契约(七阶段迁移表、屏障终态类型、事件单一出口,`6f98db2`);批2-4 阶段编排实现(状态机、汇总屏障、复核屏障,`67c3fa2`,顺带修 release_writer 两缺陷与写键错桶);批4.5 恢复桌面端并行查(task 注册不再受 execution_policy 门控、读槽改按 run_id 回收,`e933262`——这条同时解掉 R-174/R-175/R-176 共同记录的「桌面端主对话根本不注册 task 工具」前置回归);批5 编排事件落 session_events(单一出口收掉「枚举一套、落库一套」的漂移面,`38716a7`);批6 阶段流水线接线——自主推进轮走七阶段、勘察/复核由编排对象按角色表派发(`45a5e54`,本条关闭说明撰写期间落地)。**批7 待做**(真实闭环轨迹取证 = 验收②)。依赖字段清理:R-171 已 done 并已在 refs 中,按 §1.35 从「依赖」移出,防调度器误判阻塞(D-239 同族)。

## R-177 线绑 worktree 后端打通:process_create 建线、cwd/主根分离、线清单从 git 发现 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 后端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 调度顺序: R-050 拆出的三条里**优先取这一条**——它是唯一真正加速自举的一条(取活序按文件顺序,本条排在 R-178/R-179 之前即为用户意图;priority 只是背景信息)。前置 R-141 批2 落地后即可动工。
- 来源: 2026-08-10 用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,R-050 关闭拆条的第一条(= 该文 P1)。D1 定案「运行时重定向主根」的落点就在本条。
- 内容: ①`process_create` 增可选 `worktree_name`:给定时先建 worktree 再绑定,`ProcessHandle.worktree_path` 写入真实路径(crates/kanzei-app/src/processes.rs:114 与 src/state.rs:310 现在恒为 `None`),任一步失败整体回滚不留半绑定态。②`run_prompt` 的进程归属校验改按 `origin_project` 判定——现状 crates/kanzei-app/src/run.rs 比的是 `process.project_dir`,线上线后该字段指向 worktree,校验会把自己的线拒掉;同时让 `run_task` 的 `project_dir`(→`cwd`)对线传 worktree 路径、`main_root` 仍传主根。③线清单真源改 `git worktree list --porcelain` 发现,废除 `localStorage["kz-worktrees:*"]`(crates/kanzei-app/ui/09-sessions.js:37/78/79/95/97 五处);解析器不要重造——crates/kanzei-tools/src/git.rs:488 的 `worktree_for_branch` 已经是 `--porcelain` 解析器,抽出复用。④`session_id` 加 worktree 后缀,与既有进程后缀同构(crates/kanzei-app/src/state.rs:290 `process_session_id` 现在拼 `{base}#{prefix}`)。⑤补四个 worktree 命令的测试——`crates/kanzei-app/src/processes.rs` 当前**零测试**(无 `mod tests`、无 `#[test]`)。⑥一树一线查重(D4 定案):目标树已被绑定则拒绝建线。⑦N3 定案的开关:线**默认不写主根 tracker**,开关打开时照常排队(不改单写语义)。⑧D1-A 的配置侧收口:`run_task` 现在 `KanzeiConfig::load_with_warnings(&cwd)`,线上线后会读到 worktree 里的分支副本,须改读主根。
- 边界: 不做模型隔离与线级持久化(R-178)、不做 diff/合并 UX(R-179)。不绕过项目级单 writer:两条线写主根 tracker 仍由 R-171 的写租约排队,worktree 只隔离**代码文件**。物理排除 worktree 内 `.kanzei` 副本(sparse-checkout / skip-worktree)是 D1 定案里的**可选纵深防御**,不在本条,不阻塞交付。
- 收益(写实,别高估): 并行写**不会**发生。真正拿到的是三条:①线与主树自举循环**代码互不覆盖**(git 物理隔离,不靠锁);②线 A 等 review 时线 B 可以编译/跑测试/读代码/做只读勘察;③自举 agent 跑在主树时,用户可以在线上手动改东西不打架。
- 验收: ①`process_create` 带 `worktree_name` 后 `ProcessHandle.worktree_path` 是真实路径(有断言,不是 `None`);建树成功但绑定失败时 worktree 被回收,不留半绑定态,有测试。②线上的一轮 `run_prompt` 实测:`cwd` = worktree、`project_root` = 主根;线内写代码落在 worktree,而 tracker/state.db/记忆全部落主根;worktree 内的 `.kanzei` 副本**字节级零改动**(测试直接比对文件哈希)。③配置解析取主根的 `.kanzei/kanzei.toml`,worktree 里的分支副本改了也不生效,有测试。④线清单来自 `git worktree list --porcelain`:手工 `git worktree add` 出来的树也能被发现,localStorage 键清空后清单不丢;全仓 grep `kz-worktrees` 零命中。⑤同一 worktree 再建第二条线被拒(D4),错误文案指出已绑定的线。⑥线 `session_id` 与主树进程互不覆盖,删树后会话历史仍可回放。⑦四个 worktree 命令(create/diff/merge/discard)各有测试,`processes.rs` 不再零测试。⑧N3 开关默认关:线里的 agent 调 tracker 写工具时被明确拒绝并说明原因(不是静默失败),打开开关后能写且走写租约排队。
- refs: R-050 R-141 R-171 R-173 R-178 R-179 D-096 D-251 docs/design/deep_parallel_dev.md docs/design/parallel_read_serial_write_orchestration.md
- 依赖: 
- 前置(不是阻塞,解除权在 agent 手里,按 D-239 教训**不写进「依赖」字段**免得调度器整条跳过): **R-141 批2**。R-141 批1(`8574b63`)已落一半——`ToolCtx::new` 改双参不再发现式取根、`ToolCtx::discovering` 只留给进程/IPC 入口,`run_task` 已收显式 `main_root` 并令 `project_root = main_root`、`cwd = project_dir`,`run_prompt` 在 IPC 入口解析一次主根后显式传入(crates/kanzei-app/src/run.rs 已有「R-050 D1 运行时重定向主根的落点」注释)。批2(`app/run.rs` 显式传根收尾 + 双键拆开)落地后本条即可动工;R-141 未完成时先做本条的 ③(线清单从 git 发现)与 ⑤(补测试)也不受影响。

- 批次: 6/6
- 进展: 2026-08-11 已关闭。①建线/真实绑定/git ref CAS/失败零残留=`f052ca8`;②`cwd=worktree`、`project_root=主根`及 background/frontend/files 三处调用方=`b1a2f98`;③主根配置与 CLI 双键=`c651096`;④git porcelain 清单与 `kz-worktrees` 零命中=`fc44c38`;⑤N3 默认只读、开启后恢复 tracker 写入、真实分支回显=`ea93158`;⑥四命令真实 git 测试、代码只落线树、worktree `.kanzei` 副本字节级不变、删树后会话回放=`e791536`。定案后的内容④不迁移 session_id 身份串：既有 `#p{n}` 已保证线/主树会话不覆盖，分支名改由 `ProcessInfo.branch` 回显，避免 D-176 式历史失联；验收⑥两端均有回归测试。R-182 已撤销 run 级项目单 writer，因此验收⑧“走写租约排队”的旧措辞按新基线改由主根唯一文档 + R-138 FileLock 保护单次 tracker 写操作。

## R-182 撤销项目级单 writer:文档单份靠主根重定向、代码靠分支合并,写仲裁回归 git 与既有文件锁 [done]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 四组机械实测,可复现,命令见「实测」字段)
- refs: R-171 R-177 R-138 R-181 R-183 docs/design/parallel_read_serial_write_orchestration.md docs/design/deep_parallel_dev.md
- 来源: 2026-08-11 用户指出此前的并行都不满意,原因是**子代理只是单个回合内的扇出,不是任务级并行**——N 个无依赖任务各自独立跑、独立提交、无汇合点。为拿实测数据而搭了三条独立 worktree 线,实测结果把「并行查、串行写」的不变量 3(同一 project_root 同时最多一个 writer)顶翻。用户定调:**分支干、合并、冲突检测解决,文档保持一份唯一**。
- 实测(四组,主树全程未受影响): 
  ①**跨 worktree 编号必撞**:`kz defect add` 在 kz-par-a 与 kz-par-c 相隔 **10 秒**各跑一次,两边都拿到 `D-267`。零并发也撞——因为两边锁的是**两个不同的文件**。不是竞态,是必然。
  ②**同根并发反而安全**:同一个根三条并发 `kz defect add` → D-267/268/269,三条全部存活、编号互不重复。**R-138 的 `FileLock` 是真有效的**。
  ③**改已有条目跨线合并零冲突零丢失**:三条线各改自己那一段(翻 `[open]`→`[fixed]` + 追加进展行),顺序 `git merge --no-ff` 三次全干净,三条进展行一条不少。**这证伪了 deep_parallel_dev.md §3.2「docstore 整文件重写的冲突几乎必然」的判断——那句话只对「新增条目」成立。**
  ④**新增条目必冲突**:两条线各登记一条新缺陷(都是 D-267),合第二条时 `CONFLICT (content): .kanzei/project/defects.md`。
  ①④ 的成因是同一个:tracker 被当成分支副本 checkout 了两份。**只要保证一份,①④ 都不存在,②证明剩下的并发由既有 FileLock 兜住。**
- 内容: ①**撤销不变量 3**(项目级单 writer 覆盖 implementation+integration 全程)与 `ExecutionPolicy::ReadParallelWriteSerial` 对普通工具的全程串行强制;写互斥收缩为 R-138 `atomic_file::FileLock` 的**毫秒级单次操作持锁**(docstore 已是这个形态,②有实测背书)。②**补 `kz` CLI 侧的主根重定向**——R-177 只收口桌面端 `process_create`/`run_prompt`,`crates/kanzei/src/main.rs:138` 与 `:639` 的 `discover_project_root(&cwd)` **不在其范围内**,从 worktree 跑 `kz` 仍会命中副本(实测①就是这个状态);`kz` 需要一个显式主根入口(CLI 参数或环境变量),它今天**完全没有**。③代码层冲突检测**不新造**:复用既有 `worktree_*` 四命令的 `merge-tree --write-tree` 预检与 `worktree_diff` 的真实 diff。④`processes`/`state.db`/memory 同样只认主根一份(与 D1-A 同一条原则)。⑤把「分支干、合并、冲突检测解决、文档一份唯一」写成 conventions 的一节,取代「并行查、串行写」的旧口径。
- 边界: 不做**语义撞车**检测(A 把某签名重构成形态①、B 按形态②写,git 一字不报)——本条只交付文本层,语义层作为**已知缺口**记在设计文档,不在本条造机制。不动 worktree 的物理隔离(那是 R-177)。不做无人值守通道(R-183)。R-171 已交付的协调器**不删代码**,只把强制口径从「run 级项目锁」降为「单次操作文件锁」,接口保留给未来可能的重新收紧。
- 验收: ①从任意 worktree 跑 `kz defect add`,新条目落**主根**的 defects.md,worktree 内的 `.kanzei` 副本**字节级零改动**(哈希比对);重跑实测① 两条线不再撞号。②两条独立 OS 进程在**不同 worktree** 并发登记条目,编号互不重复、条目全部存活(= 把实测②的结论从同根扩到跨树)。③实现阶段的普通工具不再被强制串行:同一 run 内互不冲突的工具可并发,有轨迹证据;`ReadParallelWriteSerial` 的全程串行断言测试相应改写而非删除。④三条线各改自己那段 tracker 后顺序合并仍全干净(实测③固化成回归测试)。⑤`kz` 有显式主根入口且有测试:从 worktree 跑时 `project_root` = 主根、`cwd` = worktree,断言不相等。⑥conventions 新口径落地,旧「并行查、串行写」表述全仓无残留矛盾。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): R-177 的 D1-A 落点覆盖桌面端;本条覆盖 CLI 侧。两条可并行,但**本条验收①②必须在 R-177 之后或与之同批**才能端到端成立。

- 批次: 5/5
- 进展: 2026-08-11 已关闭。CLI `--project-root`/`KANZEI_PROJECT_ROOT` 与三入口双键=`c651096`;撤销项目级 run 租约及阶段普通工具强制串行=`e98cead`，`ExecutionPolicy::ReadParallelWriteSerial` 仍保留为显式收紧选项。`crates/kanzei/tests/worktree_main_root.rs` 用真实 `kz` 子进程证明：worktree 登记只改主根、两棵树副本哈希不变；两棵树 5 轮并发共 10 个编号全部唯一且主根 10 条全存活。`e791536` 固化 diff/merge/discard 与干净合并/冲突保留；本次 conventions/Goals/架构索引完成新口径切换。语义撞车仍是明确边界，界面常驻“文本层已检查 · 语义层未检查”。

## R-178 模型隔离与线级状态持久化:state.db processes 表 + 设置页作用域选择器 [done]
- 优先级: P1
- 复杂度: 中
- 标签: 后端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 调度顺序: 与 R-177 **零耦合**,可并行甚至先做。其中 D7 那半(设置页作用域选择器)**改动面极小、可当天交付**——`settings_save_at_path` 已经是参数化路径的(crates/kanzei-app/src/settings.rs:562),接线点就在那,取活的人可以先拿这份即时收益再做 D3。
- 来源: 2026-08-10 用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,R-050 关闭拆条的第二条(= 该文 P2)。承接 D3(线级模型选择存 state.db)与 D7(设置页作用域选择器,第一版只覆盖 `[models]`)两条定案。
- 内容: ①D3:state.db 建 `processes` 表(现有表见 crates/kanzei-core/src/store/schema.rs,没有这张),存线/进程注册 + 模型 / profile / reasoning / 子代理开关;`ProcessHandle` 的这几个字段现在是纯内存 `Arc<Mutex<..>>`(crates/kanzei-app/src/state.rs:197-200),重启即丢。②五层解析链落码 + 测试(本轮直选 → 线持久选择 → 项目 `[models]` → 全局 `[models]` → 内置默认,逐层缺省回落)。③`localStorage["kz-model:*"]`、`kz-manual-models:*` 一次性上迁后端,前端下拉降级为回显 + 写入口,不再是真源;保留旧键 fallback 一个版本。④**顺带交付 R-030 遗留的「重启不丢页签」**——R-030 的 2026-08-07 核查把"进程列表不持久化(重启丢页签)"标 P3 暂不处理,至今未做,与本表同源一表两用。⑤D7:`settings_save` 加 `scope` 参数(全局 / 本项目),**第一版只覆盖 `[models]`**;写本项目 = `toml_edit` 追加到主根 `.kanzei/kanzei.toml`。⑥崩溃恢复里「模型/会话重建」那部分归本条(依赖同一张表)。
- 边界: **D7 第一版不放 providers / api key**——它们写进被 git 跟踪的项目 toml 有泄密风险,不一次全开;界面上要说清作用域选择器当前覆盖哪些字段,不留"选了本项目但某些字段仍写全局"的静默歧义。worktree 绑定属 R-177,本条不碰;崩溃恢复里的 worktree/分支重建属 R-177。
- 验收: ①重启后每项目、每线的模型 / profile / reasoning / 子代理开关完整恢复,页签不丢(R-030 遗留项一并核验)。②两个项目配不同 primary 互不影响(D-170 式双项目用例),CLI 与桌面解析结果一致(同一真源)。③五层解析链每层缺省回落各有单测。④localStorage 旧键存在时首次启动上迁并清除,迁移有测试;全仓 grep `kz-model:` 不再作为真源被读。⑤设置页选「本项目」保存后,主根 `.kanzei/kanzei.toml` 真出现 `[models]` 且立即生效(`models_list` 与徽标同步);选「全局」写 `~/.kanzei/kanzei.toml`,两者互不串写,有往返单测。⑥保存不丢字段(conventions §4),旧配置无新键时行为不变(serde default 单测)。⑦D7 覆盖范围在界面上对用户可见,providers/api key 的作用域切换被明确禁用而非静默忽略。
- refs: R-030 R-115 D-168 D-170 D-248

- 批次: 4/4

- 进展: 五批规划实际合为 4 批交付完成:批1 d575549(processes 落库/恢复)、批2 c597d0a(五层解析链收敛 harness 单源)、批3 540f178(localStorage 上迁清除+schema v12)、批4 ba616f7(D7 作用域选择器)。批5 收口即本轮复核+全量:验收① processes.rs 四函数+manual_models 贯通+迁移测试;② resolve_model_chain 桌面 run.rs:107/CLI main.rs:266 共用;③ config.rs:1312 五层缺省回落单测;④ 迁移成功/失败/回显冒烟断言;⑤ 批4 两个 D7 往返单测;⑥ 新参均 Option 缺省走 global 向后兼容,settings 10 测试全绿;⑦ 界面提示+后端按 scope 拦截。cargo test --workspace 全绿(T-1786439420)。关闭。

## R-140 i18n 架构迁移:chrome/content 分离、t(key) 渲染点翻译、MutationObserver 退役 [done]
- 背景: direction_taste 定调二(用户明确):i18n 保留换架构。现行词典+MutationObserver 已产出 8 条缺陷家族(D-092/D-108/D-129/D-135/D-136/D-142/D-157/D-160)并篡改模型输出显示;D-172 只修了死循环,未换架构。四铁律:chrome/content 分离、翻译发生在渲染点 t(key)、模型输出语言是 prompt 问题、漏译可机械检出。
- 设计定位: i18n 架构迁移:先止血再渐进 key 化
- 证据等级: E2+E3
- 阶段: 1
- 验收: ①消息容器子树整体豁免词典替换(立即止血,终结数据篡改);②静态 DOM 改 data-i18n 一次性应用、JS 动态字符串经 t(key,params) 产出,禁止事后全文档扫描改写;③MutationObserver 退役;④漏译回落中文原文,冒烟脚本加 key 覆盖率断言;⑤按 A-003 粒度一轮吃一个界面域直至词典机制退役。

- 优先级: P0

- 标签: 前端

- 批次: 10/10
- 进展: 批1-9 已提交。批10(本轮):MutationObserver 退役——02-i18n.js 移除 observer 及专属机制(局部化函数链 localizeNodes/localizeTextNode/localizeAttributes/localizeRoot、I18N_ZH/I18N_ATTR_ZH 逐轮累积缓存、sourceFromLocalized、I18N_SOURCE_BY_EN/I18N_REVERSE_ENTRIES、applyingLanguage 防抖与文档级重置),applyLanguage 收敛为 lang + applyDataI18nKeys 渲染点应用,无任何事后全文档文本扫描。冒烟 harness 配套:假 DOM setAttribute 补 title/placeholder IDL 反射(真实浏览器反射语义),rail 按钮构建器补 data-i18n-* 复制,两条 observer 行为断言改写为「退役契约」正面断言(裸中文节点不再被自动本地化,谁把 observer 换回来即红;渲染点 data-i18n-key 经 applyDataI18nKeys(document.body) 即时翻译)。ui-i18n-smoke 静态 data-i18n-* 覆盖率断言落地(验收②④ 的机械保证:每处含中文文本/属性的元素都必须带 data-i18n-* 一次性应用标记,否则断言红)。四条冒烟全绿(运行时 0 错、i18n 956 key/353 HTML/57 动态契约、a11y、markdown)。验收①②③④⑤ 证据齐备,准备关闭前全量。

- 复杂度: 大

## R-142 前端最低配 ESLint:no-undef 防手误,无构建步骤 [done]
- 背景: direction_taste §5.2 地基债:前端 main.js 6254 行无任何 lint,手误靠运行时发现(报告 E3);no-undef 是最小有效护栏。
- 设计定位: 前端静态检查最低配,防未定义变量类回归
- 证据等级: E1
- 阶段: 1
- 验收: 引入最低配 ESLint(flat config,只开 recommended+browser env 的 no-undef 类规则),不引入构建步骤;main.js 无未定义变量错误;新增/修改前端文件后 lint 可跑且纳入冒烟脚本。

- 优先级: P0

- 标签: 流程

- 状态: doing
- 进展: 2026-08-11 完成并提交 8b918ed,已推送。验收①②③ 证据齐备。

- 复杂度: 小
- 验收证据: ①最低配 ESLint flat config,只开 no-undef,不引入构建步骤——eslint.config.js(81 行)唯一启用 no-undef 规则,ui/*.js 以 sourceType=script + 跨文件 globals 白名单(scripts/ui-lint-globals.json,1054 个顶层标识符,由 scripts/gen-ui-lint-globals.mjs 自动提取)+ Tauri/browser 宿主 readonly;package.json 仅 devDependencies(eslint@^9.39.5 + globals),无任何 build/transform 脚本,无打包步骤。②main.js 无未定义变量错误——main.js 已于 R-154 拆解为 ui/*.js(既有能力,非本次交付);ui/*.js 全量经 no-undef 检查零错误(ui-lint-smoke 30 文件)。③lint 可跑且纳入冒烟脚本——scripts/ui-lint-smoke.mjs 第五条冒烟(ESLint Node API 断言 zero error + globals 清单与源码同步断言),verify.ps1 发布门禁新增 ui_lint 步骤(ui-runtime 之后);npm run lint 即独立可跑。负向验证:临时注入 totallyUndefinedName 被 no-undef 报错 exit 1。测试:五条冒烟全绿(T-1786445522)。

## R-143 自举循环定期自动 push:完成批提交后自动推送,失败可见不阻断 [done]
- 背景: direction_taste §5.2 地基债:自举循环完成工作后依赖 agent 自觉 push,工作树长期不推风险堆积;定期自动 push 作为基线保障。
- 设计定位: 自举循环的提交自动推送保障
- 证据等级: E1
- 阶段: 1
- 验收: 自举循环每完成一批提交后自动 git push(或提供周期性的 push 时机),push 失败可见且不阻断后续轮次;与既有手动 push 流程共存不冲突。

- 优先级: P0

- 标签: 流程

- 复杂度: 小
- 进展: 2026-08-11 完成:run_task 轮末自动 push(本轮有 git commit 成功才触发),push 失败经 stage 可见不阻断;三条单测(推送成功/无提交不触发/无 remote 失败可见)全绿,kanzei-app 115 单测无回归。验收①②③ 证据齐备。
- 验收证据: ①每完成一批提交后自动 git push——run_task 轮末(decide_auto_run 之后、kz:done 之前)调用 maybe_push_after_commit(crates/kanzei-app/src/run.rs:867-880);检测位由 on_event 的 ToolStart(action=commit)+ToolEnd(ok=true) 置位(run.rs:344-348/365-374),仅本轮确有 git commit 成功才触发;轮末=自举循环批次边界,即「每批提交后自动 push」。②失败可见且不阻断——maybe_push_after_commit 对 push 失败走 stage('推送','自动 push 失败(不阻断):…') + trace 记录 kind:push/ok:false,函数不抛出,run_task 正常收尾发 kz:done(run.rs:1004-1041);单测『有提交无remote_失败可见不panic』验证失败不 panic 且 stage 可见。③与手动 push 共存——自动 push 只在轮末触发一次,git push 幂等推进,手动 git push 工具/命令不受影响;单测『本轮无提交_不触发push』验证无提交时零 stage 零 trace(不与手动流程抢)。测试:auto_push_tests 三条 + kanzei-app 115 单测全绿(T-1786445948)。

## R-184 协作可见性双面:线的上下文里要有其他线,界面要能并列看每条线在干嘛与是否冲突 [done]
- 优先级: P0
- 复杂度: 中
- 标签: 核心 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(D-263 是本条 A 面的真实事故样本,有提交为证)
- refs: R-182 R-177 R-179 R-181 R-183 D-263 D-268 docs/design/parallel_read_serial_write_orchestration.md **docs/design/parallel_lines_ui.md(本条 B 面的完整方案:落点、三区布局、状态机三级判据、冲突带算法、收活五格、复用清单、分批与依赖)**
- 来源: 2026-08-11 用户在 R-182 定调后补的两条:「**你还得告诉他,我们在合作**」与「**呈现也是一样,我能看到每个任务在干嘛、是否冲突,前端要有所体现**」。两条是同一件事的两面——**撤销不变量 3 之后没有锁兜底了**,协作信息必须同时送到两个消费方:执行的 agent、以及看着的人。
- 为什么是 P0 而不是锦上添花: R-182 拿掉的是「谁也写不进来」的强制保证。取而代之的前提是**每个写入者都知道自己不是独占的**。这个前提今天**完全不成立**——D-263 就是它不成立时的样子:自举循环以为自己独占仓库,`git add` 把外部 agent 尚未完成的改动一并扫进 `92879e2`/`25ea2c0`,改动没丢但归属混了、CI 红了、两边都不知道对方在写。**没有本条,R-182 就是把护栏拆了却不告诉司机。**
- 内容(A 面 · harness 给 agent): ①开跑时向线的上下文注入**协作块**:同期在跑的线有哪些、各自认领了什么条目、在哪个分支、已改动哪些文件。单条线在跑时不注入(避免无谓噪音与 token)。②该块**轮内可刷新**——别人的改动集合是会变的,不能只在开跑那一刻取一次(注意 D-185 的教训:提示块不得逐轮累积进对话历史)。③**提交纪律进提示词**:只 `git add` 明确改过的文件、提交前重查工作树(D-263 的直接对策)。④给 agent 一个**主动查询**通道(工具或 `kz` 子命令),让它在动手前能自己问一次「现在还有谁在写」。
- 内容(B 面 · 前端给用户): ⑤**跨线并列视图**:一屏看到 N 条线各自的 认领条目 / 当前阶段 / 当前工具 / 已改文件数 / 分支 / running-idle / token。R-179 的线页签徽标是**单线视角**,本条是**并列视角**,两者不重复。⑥**冲突预警要早于合并**:两条线改到同一个文件就在界面上标出来,**不等到点合并才由 `merge-tree` 告诉用户**。第一版取「改动文件集合求交」即可,不必上 `merge-tree` 两两预检(N 条线是 N² 次,成本不划算)。⑦冲突预警可下钻:点开看是哪两条线、哪些文件。
- 边界: 不做自动分派 / 自动解冲突 / DAG 画布(与 §2.3 及 R-111 的克制一致)。**不做语义撞车检测**——R-182 已把它记为该模型的已知缺口,本条同样只覆盖文本层。不重做 R-179 的 diff 查看器与合并确认流,本条只负责「合并**之前**的并列与预警」。不与 R-183 的非交互授权混在一起。
- 验收: ①线的上下文里真的出现其他线的信息,内容取自**真实运行态**(不是常量占位,§1.25);②只有一条线在跑时**不注入**协作块,有反证测试;③协作块随其他线的改动集合变化而更新,且**不逐轮累积进对话历史**(D-185 同族反证测试);④主动查询通道有真实数据源,agent 调用后能拿到当前写入者清单;⑤并列视图六要素全部来自真实事件,冒烟脚本逐字段断言;⑥两条线改同一文件时界面出现冲突预警,且预警发生在**任何合并动作之前**(实测轨迹为证,不是点了合并才提示);⑦预警可下钻到「哪两条线 / 哪些文件」;⑧提交纪律进了提示词并有反证测试(改动纪律文案被删则测试变红);⑨前端 `node --check` + `node scripts/ui-runtime-smoke.mjs`,并列视图与冲突预警各有冒烟断言(conventions §1.3);⑩800/1024/1280 三档布局检查。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): 需要 R-177 提供真实的线(`worktree_path` 有真实值)才能端到端取证;R-177 之前可以先做 A 面的注入通道与提交纪律(对当前的「主树自举 + 外部 agent」两方已经立即有用,正是 D-263 的场景)。

- 批次: 4/4
- 进展: 验收逐条核对:①协作块注入真实运行态——kanzei-app/src/collaboration.rs CollaborationProbe 采样 processes/runtimes(collaboration.rs:60-89),run_task 组装注入(run.rs:1206-1215)。②单线不注入反证——collaboration.rs:501 snapshot.system_baseline() 为空。③轮内刷新不累积——kanzei-core/src/runner/drive.rs:161-167 refreshable system 段替换不 push;测试 collaboration.rs:520-539(文件变化原位刷新、COLLABORATION 计数恒 1、不落稳定 baseline)。④主动查询通道——collaboration_status 工具注册+允许(collaboration.rs:541-550)。⑤并列视图六要素真实事件——冒烟 scripts/ui-runtime-smoke.mjs:4841-4873(两条真实 claim/阶段/工具/分支/文件/身份码)。⑥合并前冲突预警——冒烟 4874-4877 + 4884-4887(不触发合并)。⑦可下钻——冲突列表含文件(4874-4878)。⑧提交纪律进提示词+反证——run.rs:1269-1285 注入,run.rs:2226-2238 测试逐句断言文案。⑨node --check + ui-runtime-smoke——五条冒烟全绿。⑩三档布局——冒烟 4888-4902(800/1024/1280 下 lanes/冲突/语义提示都在)。批次 4/4(与提交标记一致:A 面 e3679a2/B 面 2d432fc 在批次表建立前交付无标记,批3 a5663d5/批4 e29ca6f/批5 ab5d318/批6 8f464a3 有标记)。P7 开线区耦合预检依赖 R-185,不属于本验收。关闭前全量 cargo test --workspace 18 crate 全绿(T-1786448527),顺带修复 D-273(home 并发测试互踩)。D-246/D-247/D-248 随批6 关闭。
  2026-08-11 本次发布补齐基础可见性回归：侧栏按 `process_list` 全量列出每条线路，显示主代理/并行线、运行态和阶段；`kz:status` 事件会更新对应 session 的状态投影。主对话切线保留旧消息直到目标历史原子恢复，并用切换代次与项目/进程快照丢弃迟到响应，避免消息消失或串线。并列视图的 P2/P5/P6/P7 仍留后续。

## R-157 验证与提交节奏引擎化:kanzei.toml 可调参数并注入循环 [done]
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 来源: 2026-08-09 用户定调:全量测试触发频率与 git 提交频率明显拖慢开发效率,应做成参数可调("稳定性不错"但每提交一次全量把验证成本乘在提交频率上)。规则层默认值已先行落 conventions §1.4(立即生效),本条把参数做进引擎。
- 内容: ①kanzei.toml 新增节奏配置节(如 [cadence]):full_test(entry_close|every_commit|every_n_batches(n)|release_only)、targeted_test(every_commit|off)、commit(per_batch|per_entry)、push(per_commit|per_entry|periodic);serde default 取 conventions §1.4 当前默认,旧配置无该节行为不变(conventions §4 向后兼容);②设置页透传全部字段,保存不丢字段;③鞭挞/自主循环把生效节奏渲染进注入提示词——DEFAULT_CONTINUE_PROMPT 规则 6 的验证文案参数化,LEGACY_CONTINUE_PROMPTS 静默升级机制同步(防 D-163 类契约错位);④push=periodic 与 R-143 并轨,不重复造。
- 边界: 发版门禁(verify.ps1 全量)与 CI push 全量不受参数影响(A-010 底线);动 main.rs/main.js 的部分不与拆解批并发。
- 验收: ①full_test 各档在注入文案里可见且实测生效(轨迹证据);②旧 kanzei.toml 无节奏节时行为与 §1.4 默认一致(serde default 单测);③设置页改参数→保存→重开生效且不丢字段;④鞭挞文案参数化后 LEGACY 升级路径有测试;⑤conventions §1.4 标注「引擎已接管,改参数走设置页/kanzei.toml」。
- refs: R-143 A-010 R-152
- 依赖: R-153 R-154

- 批次: 3/3
- 进展: 批1: kanzei.toml [cadence] 配置结构 + serde default + 加载接线 + 旧配置默认行为单测。批2: 注入提示词参数化(DEFAULT_CONTINUE_PROMPT 规则 6 + LEGACY 静默升级)+ 测试。批3(本轮): 设置页新增「验证与提交节奏」组(index.html + 02-i18n.js 登记 16 条新键 + 16-settings.js CADENCE_FIELDS/collectCadence/回填/透传),后端 settings.rs 增 CadencePayload + settings_apply_cadence 接线 settings_save(枚举白名单校验,非法值不写;全空清旧键回落默认;载荷缺 cadence 不动既有节),往返单测「节奏字段_写入读回_清空移除_不串改其他键」绿;同时修复批2 接线 bug:cadenceSettings 只声明未赋值、启动块把静态 DEFAULT 固化进 textarea 导致配置 cadence 永远到不了提示词——新增 applyCadenceSettings(未自定义时随生效节奏重渲染)+ 18-startup「节奏配置」步骤 + 16-settings loadSettings 同步;冒烟预置 LEGACY 夹具断言升级+节奏渲染+表单回填+保存载荷透传,四条冒烟与 kanzei-app 45 单测全绿。验收④✓(LEGACY 升级断言)、③✓(表单读/存/脏状态+往返)、①✓(配置 cadence 渲染进继续文案有冒烟断言)。验收⑤未达成:conventions.md 为模型只读托管资产且无专用工具(edit 被 ruleset 拒绝,shell 旁路被检测回滚),「引擎已接管」标注需用户手写或专用工具落地,已记 D-235;R-157 保持 doing 待⑤。依赖 R-153/R-154 已关闭移入 refs。2026-08-13 验收⑤达成:conventions §1.4 首行已通过 conventions patch 标注「引擎已接管(R-157):生效参数以 kanzei.toml [cadence] 为准,改参数走设置页「验证与提交节奏」或直接编辑 kanzei.toml,不要手改本节默认值」(用户 2026-08-13 批准);关闭前全量 cargo test --workspace 全绿(T-1786473325,kanzei-tools 247/kanzei-app 120/kanzei-core 132/kanzei-harness 110/kanzei-llm 43/集成 3+1,0 failed)。验收①~⑤全部达成,关闭本条。

## R-164 记忆混合检索:fingerprint+BM25+向量三通道与 RRF 融合 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 阶段: 2
- 依赖: 
- 来源: A-011 向量翻案(废止「不要向量库」,用户 2026-08-10 拍板)。向量是第二通道:coding memory 里 exact token(错误码/符号/命令)信息密度高于 embedding,fingerprint/BM25 优先。
- 内容: ①trait MemoryIndex(search_lexical/dense/hybrid/upsert/remove/rebuild)+ SqliteMemoryIndex 默认实现;②trait Embedder,第一实现走 provider 体系 openai 兼容 /embeddings(含本地 ollama,用户拍板),进程内模型只做后续 challenger 不 bundle;③sqlite-vec brute-force 起步(不依赖 experimental ANN),向量列在 index.db(派生物可重建);④RRF 融合(k=60,BM25 top10+dense top10→top5),禁止线性加权;⑤reranker 默认关闭;⑥无 embedder 时 hybrid 自动退化 lexical,功能完整。
- 验收: ①无 embedder 降级测试:fingerprint+BM25 完整可用;②配置 embeddings provider 后 hybrid 生效且分段延迟落 recall_events;③R-163 三臂对比(lexical/dense/hybrid),hybrid 显著优才切默认,报告落库;④删 index.db 后向量索引可全量重建。
- refs: A-011 R-163 docs/design/memory_control_plane.md

- 进展: 批1~批4全部完成。2026-08-13 关闭前全量:cargo test --workspace 全绿(T-1786473325,kanzei-tools 247/kanzei-app 120/kanzei-core 132/kanzei-harness 110/kanzei-llm 43/集成 3+1,0 failed)——用户 2026-08-13 批准 cargo 权限后由 agent 执行。四项验收证据见下方各段(①无 embedder 降级 index.rs:635-708;②hybrid+recall_events 分段落库 index.rs:517-595+replay_eval.rs:96-159;③六臂对照 replay.rs:518-564;④rebuild index.rs:746-790)。关闭本条。

2026-08-13 第二轮:引擎已更新(工具集含 conventions 工具),但复杂度大关闭前需全量测试、本会话无 cargo 权限,无法关闭。已收集四项验收证据精确位置供关闭时引用:
①无 embedder 降级:MemoryIndex trait + SqliteMemoryIndex(memory/index.rs:91-128),new() 默认 None(index.rs:132-134),search_hybrid 无 embedder 直接退 search_lexical(index.rs:337-341);测试「无embedder降级_fingerprint精确命中与_bm25完整可用」(index.rs:635-685)+「指纹miss时回落_bm25」(687-708)。
②hybrid 生效 + recall_events 分段落库:Embedder trait(embed.rs:20-23)、OpenAiEmbedder from_config(42-80)、POST /embeddings(93-148)、FakeEmbedder(153-182)、embedder_from_config 未配置→None(186-191);search_hybrid_with_timing 分段计时+R RF(index.rs:517-595),RecallEvent 三段时间字段(core/store/telemetry.rs:17-32)+ record_recall_event(34-61)+ schema recall_events 表(core/store/schema.rs:129-147);replay_eval.rs:96-159 candidate_text 调 hybrid 并 record_recall_event;测试「candidate臂_有记忆条目时用hybrid检索并落recall_events」(replay_eval.rs:321-390)、「hybrid_rrf融合_同时出现在两通道的条目排名靠前」(index.rs:872-917)、「hybrid_带分段耗时」(919-969)。
③R-163 三臂/六臂对照:Arm 枚举六臂(core/replay.rs:148-156),run_arms 六臂同 case 全跑(300-315),JScore/score_decision(332-375),render_report 三臂差距表(421-470);ReplayMemoryProvider match 六臂(tools/replay_eval.rs:162-188),落库 memory_eval(core/store/schema.rs:156-171 + telemetry.rs:81-116);CLI 装配(kz main.rs:790/804/817);测试「六臂各自可跑并落memory_eval」(replay.rs:518-564)。
④删 index.db 重建:vector_db_path=index.db(memory/index.rs:158-161),rebuild DELETE+全量重算(index.rs:446-477);测试「有embedder时rebuild生成向量_无embedder时向量表空」(index.rs:746-790)。

批4完成(R-164 B4):ReplayMemoryProvider 装配三通道混合检索——新增 hybrid: SqliteMemoryIndex 字段(new 时从 kanzei.toml [embeddings] 构建 embedder,未配置则 None 降级),Candidate 臂从与 Current 同源改为 candidate_text:IndexQuery::both(tool,kind,sample+target) → search_hybrid_with_timing → 命中落 RecallEvent(policy_action=hybrid,trigger_type=replay_eval,分段延迟填 lexical_ms/embed_ms/vector_ms——验收②装配)。Current/LeaveOneOut/CompressionCF 保持现状策略。新测试 candidate臂_有记忆条目时用hybrid检索并落recall_events:seed 含 [fp:edit|old string not found] 的条目 + FakeEmbedder → Candidate 命中且 state.db 落一条 policy_action=hybrid 事件。kanzei-tools 172 passed 全绿。

验收对照: ① 无 embedder 时 dense/hybrid 退化为 lexical 功能完整——search_hybrid/dense_scan 空表返回空、ReplayMemoryProvider new 时 config 缺失 → embedder=None、现有 oracle 测试断言空目录 Candidate==Current; ② 三通道与 RRF——search_hybrid(k=60) + search_hybrid_with_timing 分段(lexical/embed/vector)供 recall_events 落库(Candidate 臂已落); ③ dense 通道——brute-force 余弦检索,内存/常量级实现,无新依赖(避开 sqlite-vec loadable extension 的 Windows 分发负担); ④ 可重建——rebuild 全量重扫生成向量,upsert/remove 增量维护。向量列在 index.db memory_vectors 表(派生物)。

实现注:向量检索用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列、brute-force、可重建)与设计 §5 一致。

关闭前待跑: cargo test --workspace 全量(复杂度中)。

批3完成(R-164 B3):index.rs 实现 dense 通道——dense_scan 读 memory_vectors 全表 brute-force 余弦(topN),dense() 入口(query 文本→embedder 向量→扫描);search_hybrid 在有 embedder 时做 RRF 融合(k=60,lexical top10 + dense top10 → top5,禁止线性加权,设计 §5),dense 空结果自动退化为 lexical;新增 search_hybrid_with_timing 返回 (hits, RetrievalTiming{lexical_ms,embed_ms,vector_ms}) 供 RecallEvent 分段延迟落库(验收②)——检索层不碰 SessionStore,落库由装配方(批4)做。4 个新测试:cosine_相似度_同向为1_垂直为0/dense_检索_embedder配置后按语义命中/hybrid_rrf融合_同时出现在两通道的条目排名靠前/hybrid_带分段耗时_无embedder时embed与vector段为0。kanzei-tools 171 passed 全绿。

批次规划: 批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)+ replay_eval 落 recall_events 分段延迟(验收②装配)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

批2完成(R-164 B2):(1) kanzei-harness config.rs 新增 [embeddings] 节(EmbeddingsSection{provider,model} + enabled(),serde default 缺节关闭,层叠合并逐字段覆盖,unknown_keys 清单登记)——旧配置无节时通道关闭行为不变,harness 64 测试含新增 embeddings_缺节关闭_配置后启用_旧配置行为不变;(2) kanzei-tools/src/embed.rs 新增 Embedder trait(同步签名,内部 tokio runtime 驱动)+ OpenAiEmbedder(openai 兼容 POST {base_url}/embeddings,解析 data[].embedding,api_key 经 provider api_key_env/api_key 解析,本地 ollama 免 key)+ FakeEmbedder(测试用确定性向量)+ embedder_from_config 工厂(未配置→None 关闭通道);mock HTTP 测试验证 URL/请求体/响应解析,3 测试;(3) SqliteMemoryIndex 接向量列:with_embedder 构造 + memory_vectors 表(同 index.db,派生物)+ vectorize/upsert 增量/remove 删行/rebuild 全量重建(验收④),2 新测试(有embedder时rebuild生成向量_无embedder时向量表空/upsert_增量维护向量_remove删除向量)。

批次规划: 批3 dense 通道 brute-force 余弦 + RRF 融合(k=60)+ 分段延迟落 recall_events(验收②);批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

批1完成(R-164 B1):crates/kanzei-tools/src/memory/index.rs 新增 MemoryIndex trait(IndexQuery/IndexHit + search_lexical/dense/hybrid/upsert/remove/rebuild)与 SqliteMemoryIndex 默认实现——lexical 通道复用 FingerprintIndex(Tier0 指纹精确)+ MemoryStore::search(Tier1 BM25),dense 恒空(未接 embedder),hybrid 无 embedder 时自动退化 lexical(验收①);mod.rs 注册导出。3 个新测试:无embedder降级_fingerprint精确命中与BM25完整可用/指纹miss时回落BM25_文本可兜底/upsert_remove_rebuild_增量与全量一致。kanzei-tools 162 passed(原159+3)全绿。

批次规划: 批2 Embedder trait + openai 兼容 /embeddings 实现(含 ollama)+ kanzei.toml [embeddings] 配置节 + 向量列存储 + rebuild(验收④);批3 dense 通道 brute-force + RRF 融合(k=60)+ 分段延迟落 recall_events(验收②);批4 R-163 三臂对比装配(lexical/dense/hybrid)+ 报告落库(验收③)。实现注:向量列用 rusqlite 普通表 + Rust 侧 brute-force 余弦,替代 sqlite-vec loadable extension——Windows bundled rusqlite 加载扩展有版本兼容与分发负担,功能语义(向量列在 index.db、brute-force、可重建)与设计 §5 一致。

- 批次: 4/4

## R-191 约束下沉 harness:通用开发规则引擎化,跨项目上下文与硬约束一致 [done]
- refs: D-279 R-190 R-136 R-157 D-278
- 优先级: P1
- 依赖: 
- 复杂度: 大
- 归属: kanzei
- 方案: 三管齐下,通用规则单源进引擎:①kanzei-harness 内置 default_conventions.md(通用规则全文,从现有 conventions.md 提炼,去掉 §4/§6/§9 特有节与 kanzei 历史定调注解),作为所有项目通用约束的唯一真源;②conv-init 改用该模板生成(空骨架退役),生成文件开头注明「通用规则由引擎维护,此文件只放项目特有规则」,新项目一键创建即获得完整约束;③硬约束进引擎注入与工具校验——profile 提示词补「登记字段清单」(req 必带 复杂度 小/中/大 + priority + label + 来源;defect 必带 severity + priority + label;确实分批的条目登记当天写 批次: 0/N),tracker req add/defect add 对必填字段做校验(缺失报错并提示补什么,不静默放行);④kanzei 自己 conventions.md 重构:通用节删掉(引擎已单源),保留 §4/§6/§9 项目特有节,避免两处漂移。上下文管理:完整规则仍在文件按需读,不进 system prompt 全文(避免 dev prompt 再膨胀 170 行),提示词只注入硬约束要点。
- 来源: 2026-08-14 用户定调:「把我们现在仓库的独立的一些约束分析拆分出来,做到系统的 harness 里——我们在做自举开发,但还要给其他项目用,必须保持上下文管理和硬约束一致性」。直接触发:D-279(另一个项目 agent 登记需求缺复杂度/批次)暴露跨项目约束不一致。
- 标签: 核心
- 现状: 约束目前三处载体,各缺一块:①conventions.md 是项目级文件,通用规则(§1.1 取活/§1.2 关闭/§1.25 验收/§1.35 标签/§1.3 批次/§1.4 节奏/§2/§3/§5/§7/§8/§10)与 kanzei 特有规则(§4 架构契约/§6 dev 分支/§9 构建发版)混在一起,另一个项目是独立目录拿不到;②conv-init 一键创建模板(docs.rs:52-55)只有四行空骨架(代码风格/提交规范/测试要求/禁止事项各一个空 bullet),不含任何通用规则——另一个项目生成的 conventions.md 就是这个空壳;③profile 提示词(profiles.rs:407-492)有执行期批次/WIP/验证协议,但围绕「正在执行的条目」,对「登记新需求/缺陷」没有字段清单约束;req add 门禁只有 title is required(tracker.rs:420),不校验 复杂度/批次/priority/label/来源,defect add 同理。harness 侧无任何 conventions 注入代码(grep 仅命中注释引用)。
- 阶段: 3
- 验收: ①拆分分析落地:通用/项目特有分类表写入本条进展,harness 内置模板与分类一致;②新项目一致性:conv-init 生成的文件含完整通用规则(不再四行空壳),有模板生成测试断言关键节存在(§1.1 阻塞口径/§1.3 批次/§1.4 节奏/§1.25 验收证据);③登记硬约束:req add 缺 复杂度 报错、defect add 缺 severity/priority 报错,报错信息提示补什么字段,有定向测试;profile 提示词含登记字段清单(可 grep 断言);④kanzei conventions.md 通用节与引擎模板不重复(单源),项目特有节(§4/§6/§9)保留,文档与实现一致;⑤不回归:既有 req add 正常路径(带全字段)行为不变,kanzei 存量条目不受影响,相关 crate 定向测试全绿,关闭前全量测试通过。
- 批次: 6/6
- 进展: 批6 收口核对(2026-08-15):验收逐条证据如下。①拆分分类表:通用=§1.1 取活/阻塞、§1.2 关闭边界、§1.25 验收证据、§1.35 标签与依赖、§1.3 批次、§1.4 节奏、§2 代码原则、§3 命名、§5 终端、§6 通用分支纪律、§7 测试、§8 文档、§10 并行——全部进 kanzei-harness/assets/default_conventions.md;项目特有=§4 架构契约/§6 kanzei 分支/§9 构建发版(含 9.1)——留在项目 conventions.md。②conv-init 生成项目特有骨架(docs.rs:42-73 注明通用规则由引擎注入),注入测试断言四关键节(profiles.rs:1015-1027 required 数组含 §1.1 阻塞口径/§1.3 批次/§1.4 节奏/§1.25 验收证据)。③登记硬约束:req add 缺复杂度/priority/标签即拒、defect add 缺 severity/priority/标签即拒(tracker.rs 与 B3 测试 add_requires_registration_fields),dev prompt 含登记契约(B4 测试可 grep 断言)。④kanzei conventions.md 已删全部通用节,只留 §4/§6/§9/§9.1(commit 56cb17c),profiles.rs 同口径测试真源迁引擎模板并加单源防回归断言。⑤存量 req add 正常路径不变(带全字段行为不变,既有 8 处裸 add 已补登记字段),定向测试全绿,全量待跑。

## R-197 会话运行态、并行线路与任务设置统一收口 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心 前端 后端 并行 自举
- 归属: kanzei
- 来源: 2026-08-12 用户连续反馈「运行了还显示空闲」「停止按钮消失」「切到并行线后鞭挞自动开启」「活动记录没了」，并确认先做全局状态扫描，再按依赖顺序一次收口。
- 设计: `docs/design/session_state_and_line_runtime.md`
- refs: R-030 R-086 R-169 R-178 D-209 D-271 D-281 D-283
- 目标: 以 `session_id` 统一后端运行真源、实时事件、左侧线路按钮、顶部状态栏、停止控制、鞭挞设置、活动轨迹和历史恢复；轮询只做校准，不再把轮次结束误当成会话空闲。
- 边界: 本需求只处理会话/线路状态、任务设置作用域、活动轨迹和历史恢复；不扩展子代理后台化、跨 agent 消息、移动端或新的模型能力。已有 UI 和后端接口优先抽象复用，不引入新框架。
- 实现顺序（10 批次，依赖从后端到前端）: 
  1. **状态契约与迁移基线**：落设计文档；盘点 `session_id`、进程、事件、trace、设置字段；定义 `idle/starting/running/round_finished/auto_pending/stopping/failed`，记录旧字段兼容和迁移策略。
  2. **后端会话运行态抽象**：抽出会话运行快照/状态转换与目标进程解析，统一 `ProcessInfo`、`CollaborationLine`、停止入口读取的 session；保证 `kz:done` 与 `kz:idle` 语义分离。
  3. **后端实时事件投影**：统一进度事件、阶段事件和会话终态事件的发射/状态更新出口；所有事件带 `session_id`，后台线路事件不依赖当前前端选中线路。
  4. **后端活动轨迹增量持久化**：抽出 trace writer/flush 边界，把工具开始、进度、完成、阶段和停止路径落入可回放的 `run.trace`；保证异常、停止、重载不丢当前轮轨迹，并保留轮级 episode 汇总。
  5. **后端任务设置与停止契约**：统一线路 profile/model/reasoning/pipeline/tracker 与 session auto-run 的读写；明确 `auto_pending`、取消鞭挞和 `stop_run(process_id)` 语义；修复 auto-allow 对自主推进的真实作用域/提示。
  6. **前端统一状态投影器**：由单一投影函数驱动左侧线路状态、顶部状态栏、stop 按钮和底部状态；实时事件优先，process_list 只做校准；禁止 handler 私自写互相矛盾的 running/idle。
  7. **前端线路设置事务**：切线时保存旧线、取消旧 timer、应用目标线完整设置、同步目标 session；没有设置的并行线使用安全默认，不继承主线 profile/鞭挞；各设置变更均回写对应线路。
  8. **前端活动/历史恢复**：活动面板按 session 隔离并实时追加；切线、重载、停止后回放目标 trace；历史对话继续挂在线路按钮下面，和 trace 使用相同 process_id。
  9. **回归与真实运行验证**：补 Rust 状态/事件/trace/停止/设置隔离测试；补 UI 事件时序、两线并行、鞭挞串线、活动回放、历史归属、停止按钮测试；运行最小相关集、全工作区 cargo 门禁和桌面 E2。
  10. **提交与发版验收**：只提交本需求相关代码、设计、需求/缺陷和测试；构建安装新版 kzapp，确认实际运行实例 hash 已更新；在桌面端实测两条线同时运行、切线、鞭挞、活动、停止和历史恢复，再关闭相关缺陷。
- 验收: 
  1. 两条线路同时运行时，左侧和并行线路页在事件到达后立即显示正确运行态/阶段，不依赖下一轮或 3.5 秒轮询。
  2. `kz:done` 后仍有排队输入或自动续跑时，不能显示普通空闲；真正 `kz:idle` 后才收回运行态。
  3. 当前线路运行时 stop 始终可见且只停止当前 process；`auto_pending` 可停止鞭挞，不影响其他线路。
  4. 主线开启自主推进后切到未配置并行线，鞭挞保持关闭、profile 为安全默认，旧 timer 不得向新 session 发消息。
  5. 线路的 model/profile/reasoning/pipeline/tracker/auto 状态互不串线，重载后仍按目标线路恢复。
  6. 活动工具轨迹在运行中可见，停止/重载/切线后可按 session 回放；轮内中断不再静默丢失。
  7. 历史对话继续挂在所属线路下，主线与并行线查询、删除、打开均不串 session。
  8. 相关 Rust、UI 冒烟、a11y、i18n、并行线路回归、构建和真实桌面验收全部有证据；未更新运行实例不得报告发版完成。
- 进展: 2026-08-12 完成 10/10 批次。设计基线、后端 session 状态/实时 trace 抽象、前端三态投影、线路设置隔离、历史归属、回归门禁和发布均已落地。最终审计又补上两处竞态：旧 `process_list` 快照不能覆盖实时运行事件或发送后的启动意图；`kz:error` 通过 `terminal` 字段区分持久化告警与终态失败。`cargo test -p kanzei-app` 122 项、clippy、UI runtime/a11y/i18n/Markdown/ESLint/并行线路回归全绿；最终安装位 hash、版本和构建位一致性以本次发布交接记录为准。真实 WebView2 CDP E2 在当前环境因探针查询系统进程超时未完成，未将其冒烟结果计为通过。

## R-201 tracker 提供游离文本的清理通道:让"删得掉"成为工具能力而不是人工特权 [done]
- 优先级: P3
- 复杂度: 中
- 标签: 核心
- 来源: D-294 的残余项。
- 背景: D-294 堵死了游离段落的**产生**,但存量约 304 行仍然只有人直接编辑文件才删得掉。对 agent 而言这仍是「数据只进不出」——它能看见噪音、能判断是重复,却没有任何工具删得掉,只能绕。
- 内容: 给 tracker 一个明确的自由文本操作面:读出某条目的游离行(带行号或指纹)、按指纹删除指定行。**不做**自动清扫——游离行里混着真实内容(D-261 的实测记录就是),批量删会毁历史,必须逐条指认。
- 边界: 不改「字段值必须单行」这条不变式(D-294);不允许用它绕开字段体系写多行字段。
- 验收: ①能列出某条目的游离行并稳定标识;②能按标识删除指定行,其余内容与字段一字不变;③删除后二次保存幂等;④有回归覆盖"删除后字段不受影响"。
- refs: D-294 D-239

- 进展: 关闭证据(2026-08-12):实现提交 800d5da,全量 cargo test --workspace 绿(T-1786514969,kanzei-tools 256 passed)。验收逐项:①列出游离行+稳定标识=docstore.rs raw_lines() 返回 RawLine{ordinal,text}+tracker.rs "raw_lines" action 输出 [n] 原文(tracker.rs:296-326);②按标识删除指定行、其余内容与字段一字不变=docstore.rs delete_raw_line() 模板手术只移除那一条 Raw(tracker.rs:692-712 接线),回归测试断言删除后文件仅少那一行、字段数与内容不变(docstore.rs 游离行列出与删除_其余内容一字不变_二次保存幂等);③删除后二次保存幂等=preserved 模板回写防复活,同测试断言 save() 后文件与删除后完全一致;④回归覆盖"删除后字段不受影响"=同测试 fields.len()==3 断言+tracker.rs raw_lines_raw_delete_清理游离行且字段不受影响 端到端。原阻塞(D-295)已解除:test_record 白名单入 kanzei.toml(6ef23cc)。 [terminal-fix 2026-08-13] done → done: D-333 收敛:标题残留 [open] 双终态标记为 D-331 修复前存量,status 已是 done,剥离标题标记

## R-198 bash 权限规则支持「程序名 + 参数前缀」白名单,不再整串通配 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 权限
- 来源: 用户 2026-08-12 选定方案 A(全局 `bash resource="*"` 放行)时同步登记的正解。
- 背景: 当前 bash 规则只有两种有效形态——整条 `*`(全放),或**逐字节相同**的整串(含结构化 `{"command":...}` JSON)。带 `*` 的规则会被 `command_chaining_escapes`(permission.rs:295)降级回 Ask,理由正当:`git *` 挡不住 `git status; rm -rf /`。但代价是中间档位不存在,用户只能在「全放」与「每碰一次墙加一条精确串」之间二选一;自主推进轮(NonInteractive)下 Ask 即 Deny,后者意味着每轮都可能卡死。
- 内容: 规则支持声明可执行程序与参数前缀(如 `node scripts/*.mjs`、`cargo build*`),匹配前先解析命令而不是对整串做通配;命令里出现链接/重定向/子 shell(`;` `&&` `|` `$()` 反引号)一律不匹配前缀规则,回落 Ask——即保住 D-051 要防的那件事,同时让"只放行这个程序"可表达。
- 边界: 不做 shell 语法的完整解析(那是无底洞);解析不出来就 fail-closed 回落 Ask。不改 Ask 在 NonInteractive 下等于 Deny 的既定语义。
- 验收: ①`node scripts/e2e-smoke.mjs` 命中 `node scripts/*.mjs` 规则并放行;②`node scripts/x.mjs; rm -rf /` 不得命中该规则;③结构化 bash 资源(JSON)与纯字符串命令两种形态都覆盖;④D-051 既有回归保持绿。
- refs: D-051 D-292

- 批次: 2/2
- 进展: 2026-08-16 取活(defects 队列全阻塞,转 requirements)。R-198 纯后端权限模块任务,不依赖用户环境。B1 完成(commit 59e2f05):①permission.rs 新增 bash_prefix_match(程序名精确匹配 + 参数前缀通配 + 引号感知 split_first_token + has_shell_meta 检测 `;` `&&` `|` `>` `<` `$(` 反引号等 shell 结构);②evaluate 里 command_chaining_escapes 降级逻辑接入——命中前缀白名单则放行,否则维持 Ask(D-051 防线在解析层保留);③验收测试 4 个(node scripts/*.mjs 放行匹配/命令链接重定向回落 Ask/结构化 JSON 与纯字符串双形态/非本程序与 yolo 保持);④D-051 前缀通配测试语义更新(git status 现放行,重定向/别名/其它程序/命令链接仍 Ask)。permission 28 passed,fmt/clippy 全过(T-1786565253)。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786565346,harness 118)。四条验收逐条对照:①node scripts/e2e-smoke.mjs 命中 node scripts/*.mjs 并放行——测试 前缀白名单_放行匹配命令 断言 Effect::Allow(permission.rs 测试,commit 59e2f05);②node scripts/x.mjs; rm -rf / 不得命中——测试 前缀白名单_命令链接重定向回落ask 断言含 ;/&&/|/>/$(/反引号 全部 Effect::Ask(has_shell_meta 检测);③结构化 JSON 与纯字符串双形态——测试 前缀白名单_结构化与纯字符串双形态:纯字符串前缀规则不授权 JSON(既有保护保留)、JSON 资源经既有整串精确匹配路径正常;④D-051 既有回归保持绿——更新后的 前缀通配不放行未明确授权的命令(重定向/别名/其它程序/命令链接仍 Ask)+ 显式整体放行不受串联降级影响(yolo `*` 仍 Allow)全绿。关闭。 [terminal-fix 2026-08-13] done → done: D-333 收敛:标题残留 [open] 双终态标记为 D-331 修复前存量,status 已是 done,剥离标题标记

## R-199 鞭挞续跑的模式条件下沉引擎:前端不得保留引擎不知道的否决权 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 前端 后端
- 来源: D-291 修复时的残余项。
- 内容: `autoContinueAllowed()`(模式必须为 dev-auto)是前端私有条件,而 auto_run.rs 的头注明确宣称判定归引擎、前端只执行。现状是引擎判 Continue 并把 `rounds` +1,前端可能否决——计数与实际轮次从此漂移。把档位作为输入并进 `AutoRunCtx`,由 `decide()` 给出 `Stop(ProfileMismatch)` 一类结果;或者取消这条双重开关(鞭挞开关本身已是意图表达)。
- 验收: ①前端不再持有任何引擎不知道的续跑否决条件;②否决发生时引擎侧计数不 +1;③harness 侧单测覆盖新增停止原因。
- refs: D-291 R-169

- 批次: 2/2
- 进展: 2026-08-16 取活。B1 完成(commit fd42c26):①harness auto_run.rs——AutoStopReason 加 ProfileMismatch、AutoRunCtx 加 auto_allowed: bool、decide() 在 backlog 检查后加 !auto_allowed → stop_with(ProfileMismatch)(计数重置为 0 不 +1);②app auto_run.rs serialize_action 加 ProfileMismatch 映射;③app run.rs 构造 AutoRunCtx 传 auto_allowed(dev-auto = profile Dev && agent name dev);④前端 08-compose.js armAutoContinue 移除 autoContinueAllowed() 私有否决;⑤前端 07-events.js Stop 分支加 ProfileMismatch 显示(关开关+提示,复用既有 i18n key);⑥harness 新测试 模式不匹配时引擎停止且计数不漂移。验证:harness auto_run 14 passed + app 137 passed + node --check 07/08 通过 + fmt/clippy 全过(T-1786565739)。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786565831,harness 119)。三条验收逐条对照:①前端不再持有任何引擎不知道的续跑否决条件——armAutoContinue 的 autoContinueAllowed() 否决已移除(08-compose.js),档位条件唯一真源在引擎 decide()(auto_run.rs !auto_allowed → Stop(ProfileMismatch)),剩余 3 处 autoContinueAllowed 为「开关启动门禁」(勾选时提示),非续跑否决;②否决发生时引擎侧计数不 +1——decide 的 stop_with(ProfileMismatch) 将 rounds 重置为 0,harness 测试 模式不匹配时引擎停止且计数不漂移 断言 Stop + rounds==0;③harness 侧单测覆盖新增停止原因——测试 模式不匹配时引擎停止且计数不漂移 覆盖 ProfileMismatch(harness auto_run.rs,commit fd42c26)。关闭。 [terminal-fix 2026-08-13] done → done: D-333 收敛:标题残留 [open] 双终态标记为 D-331 修复前存量,status 已是 done,剥离标题标记

## R-230 work next/claim 调度决策下沉 harness:取活零推导 [done]
- 内容: 新增 work 类动作:next 按显式序计算 WorkDecision(Resume{id}|Start{id}|Blocked{ids}|WipViolation{ids})并返回 reason 码与 wip 快照;claim <id> 为显式 override 并落档原因;提示词侧取活规则收敛为「一律调 work next 按返回执行」。2026-08-13 已在 dev prompt 写入显式序(profiles.rs:resume 占槽项>队列优先级,守护测试覆盖),本条是它的 harness 确定性下沉——V4PRO 实测每次会话为 defect-first vs WIP 的仲裁自辩 500+ token,该决策 100% 可确定,按弱模型准绳应由代码一行给出
- 复杂度: 中
- 来源: 2026-08-13 V4PRO 运行复盘与调度设计讨论
- 标签: 后端
- 验收: ①next 四种决策各有单测(唯一可执行→Resume/零→按模式 Start/多→WipViolation/全部阻塞→Blocked);②返回含 reason 与 wip 快照可复述给用户;③claim override 落档;④dev prompt 同步改为按返回执行且守护测试更新
- 优先级: P2
- 进展: 2026-08-13 已完成。新增 work next/claim 确定性裁决，Resume/Start/Blocked/WipViolation 七组回归覆盖；claim 偏离默认选择必须记录原因且不能绕过 Resume；dev prompt 只执行引擎结果。验证：cargo test --workspace 全绿，cargo clippy --workspace --all-targets -- -D warnings 全绿。提交 4324bf7。
- observed_head: 7ca4e6c04844836b534916c5e7a6a471f8427ceb
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786593955011

## R-231 work context 轮首注入:执行中零全量 list,归档只准 get-by-id [done]
- 内容: 自举轮首注入改为「活动条目全文+批次/进展+refs 解析结果+相关记忆 hints」,req/defect 全量列表不再进上下文(现状 defects.md 44KB+requirements.md 95KB≈3.5 万 token/轮首);两个归档(322KB/559KB)只允许按 id 取条目;队列仅有的两次合法读取由 work next(R-230)与登记查重门禁承担。与记忆召回 prompt_hints 只注题+fetched 落表是同一模式
- 复杂度: 中
- 来源: 2026-08-13 上下文管理设计讨论(refs R-230)
- 标签: 后端
- 验收: ①轮首上下文含活动条目与 refs 不含全量列表(守护测试);②执行中全量 list 有护栏或审计计数;③改造前后轮首 token 对照留档
- 优先级: P2
- 进展: 2026-08-13 已完成。轮首常驻上下文仅含选中项全文、结构化 refs 和 provenance，普通 Resume 不注入未选中或 blocked 集合；req/defect 全量 list 仅 human_cli 或登记查重可读。当前活动双队列 80161 字符（粗估 20041 token）降至裁决 3783 字符（粗估 946 token），缩减 95.3%。守护测试验证未选中标题不出现、未来进展标记 future_timestamp、归档 refs 可解析。提交 4324bf7。
- observed_head: 7ca4e6c04844836b534916c5e7a6a471f8427ceb
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786593955427

## R-213 记忆 promote 的 provenance 校验补真:episode 必须真实存在,写证据失败即回滚晋升 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(docs/design/audit_20260812_eight_dimensions.md §5)。
- 背景: memory_control_plane.md §6 的硬约束是「无来源不入 active」;实现只查 sources 数组非空——promote 不校验 episode_id 真实存在(memory/store.rs:392-397),record_memory_source 失败被 `let _` 吞掉后条目照样置 active(store.rs:414-427),而 manager 的工具面拿不到真实 episode_id 只能编造。控制平面「用数据判断记忆是否改善决策」的承诺因此不可兑付。
- 内容: promote 前校验每个 episode_id 真实存在(或改为引擎在轮末代填当轮 episode_id,manager 无需自报);record_memory_source 失败即回滚晋升。
- 验收: ①伪造 episode_id 的 promote 被拒(单测);②写证据失败不产生 active 条目;③盘点存量 active 条目在 memory_sources 里零行的数量并处置。
- refs: R-165 R-195 R-214
- 批次: 2/2
- 进展: B3 完成:cargo test --workspace 743 passed/0 failed/2 ignored(T-1786597693);fmt+clippy 无警告。B1 门禁+引擎代填全绿后关闭。验收逐项:①store.rs promote() episode_exists 校验(episode_exists 在 kanzei-core/src/store/episodes.rs:43-53)+单测 promote_rejects_fabricated_episode_id(store.rs:2079-2108,commit 23338eb);②promote() 证据先落库、record_memory_source 失败即整体失败、成功才置 active(store.rs:446-497),单测 promote_write_evidence_failure_does_not_activate(store.rs:2111-2152);③盘点=311 episode / memory_sources 0 行 / project 28 条 active 全零证据 / global 无条目,处置=存量豁免+文档化(可逆),逐条复核承接为 R-235。方向问题(manager 拿不到真实 episode_id)由引擎轮末代填解决(commit 45fd276,CLI main.rs+桌面 run.rs 传 episode_id 进 consolidation_prompt)。 [terminal-fix 2026-08-13] done → done: D-333 收敛:标题残留 [open] 双终态标记为 D-331 修复前存量,status 已是 done,剥离标题标记
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786597713547

## R-233 记忆召回补语义通道:prompt_hints 从纯 BM25 词面升级到 hybrid(dense embedder + RRF),并改 query 构造 [done]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 背景: 本轮复盘发现:prompt_hints(memory/mod.rs:996) → store.search → 纯 FTS5 BM25 词面匹配(store.rs:732);dense 通道未接 embedder 恒空(memory/index.rs:15-16 注释明示)。抽象意图查询(如「评估 harness 质量」)召回率天然低——M-061 自举复盘 SOP 正文无「harness/质量」字眼词面永命中不到,反而命中 M-008/M-032/M-027 等字面偶合条目(本轮实测 0/3 命中)。这是系统性设计缺口,不是逻辑 bug。
- 验收: ① 落地 dense 通道:接 embedder 后同 query 能召回词面不相关但语义相关的 SOP/fact 条目;② query 构造升级:从用户 prompt 提取意图词而非原样整句进 FTS;③ hybrid RRF 融合(memory/index.rs:337 已有框架,补 embedder 即生效);④ 召回遥测(record_recall)显示相关条目采纳率改善,不靠感觉评估。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-233
- 批次: 3/3
- 进展: B1-B3(2026-08-13)代码批次完成:B1 query 构造升级(intent_query+INTENT_BOUNDARY+端到端召回测试,927ecc2);B2 embedder 接线(prompt_hints 第4参+hybrid 检索+ensure_vectors 差集维护+遥测记通道,7f35044);B3 语义召回 e2e(词面不相关但语义相关可召回,验收①,e63be64)。关闭前全量 cargo test --workspace 759 passed/0 failed/2 ignored(T-1786604587)。验收对照:① dense 通道=prompt_hints_with_budget 走 SqliteMemoryIndex::with_embedder+search_hybrid_entries(mod.rs:1080-1086)+ensure_vectors(index.rs);② query 构造升级=intent_query(store.rs:1677)接线 mod.rs:1062;③ hybrid RRF=search_hybrid(index.rs:337,k=60)+生产走 hybrid;④ 遥测=record_memory_search_telemetry 记 hybrid/lexical+分段耗时(mod.rs:994-1004),B2 测试断言通道区分。残余(不阻塞):dense 无相似度阈值小库噪声靠 RRF 沉底;真实 embedder 采纳率改善需配 [embeddings] 后由 recall_events 观测。
- observed_head: e63be64ecd503b28359eeacdcf354b5fb8bc5340
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786604603727

## R-226 多线路运行内核二次收口：身份永不复用、独立自动推进与停止/收活隔离 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心 后端 前端 并行 自举 发布
- 来源: 2026-08-12 用户要求全面扫描多线并行后确认继续修复并发版；R-197 虽归档为完成，但 R-206 与本轮反证证明其关键不变量尚未兑现。
- 内容: 以持久唯一线路身份和 `session_id` 为中心，重新收口线路创建/注销、后端运行时、事件路由、自动推进、停止、对话恢复、工作树收活与前端状态投影；禁止任何全局 UI 状态或项目级清理跨线路生效。
- 实现顺序（10 批）: ①反证测试基线；②线路身份永不复用与旧缓存清理；③统一注销/停止 finalize；④后台进程按 owner 回收；⑤运行中工作树禁止合并/放弃；⑥前端具名状态机与 stopping；⑦后台控制事件副作用按 session 执行；⑧自动推进定时器与设置按 session 隔离；⑨切线/发送/历史恢复竞态收口；⑩全量门禁、真实双线验收、打包发布。
- 验收: ①删线重建不会复用 session 或继承历史/profile/鞭挞；②两线同时运行和自动推进互不取消、互不发送到对方；③停止 A 只停止 A 的 run、ask、队列和后台进程，B 保持运行；④运行中、停止中、等待下一轮、空闲由同一会话投影驱动，停止按钮不消失/闪跳；⑤后台 `kz:done` 能续跑并刷新所属历史；⑥运行线路不能合并或放弃工作树；⑦切线期间旧 IPC 失败不能污染新线；⑧活动与历史按 session 恢复且读接口不改写运行中上下文；⑨相关 Rust/UI 反证、workspace 门禁和真实桌面双线 E2 全绿；⑩发布包绑定最终 HEAD，安装实例版本与 hash 可核对。
- refs: D-313 R-197 R-199 R-206 R-207 R-222 D-209 D-283 D-305 D-306
- 进展: 2026-08-12 十批按依赖完成:…(原进展全文保留);‖ 2026-08-16 账本维护证据核验(原 [fixed] 污染标记,核验后转 done):实现提交 0a682bb(多线路运行内核与收活隔离收口);代码实证 ui/03-shell.js transitionSession/phase 具名状态机、09-sessions/20-lines 按 session 投影;parallel-lines-regression 套件存在且近期绿(T-1786552000)。验收 ①-⑧ 逐批交付证据见上;⑨ workspace 门禁当前全量 759 passed 覆盖代码面,真实桌面双线 E2 与 ⑩ 发布包绑定随下次发版收尾(残余,转发布清单,按 §1.2 可用即关闭)。 [terminal-fix 2026-08-13] done → done: 账本维护:close 前未剥 [fixed] 标题标记,归档标题残留 [fixed] [done] 双标记,收敛为单一 done 并清标题
- observed_head: e63be64ecd503b28359eeacdcf354b5fb8bc5340
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786605391269

## R-225 界面语言设置：跟随系统/中文/English，默认中文 [done]
- 优先级: P1
- 复杂度: 小
- 标签: 核心 前端 设置
- 归属: kanzei
- 来源: 2026-08-12 用户交接要求「界面语言可设定，默认中文」。
- 内容: 设置页提供「跟随系统/中文/English」下拉；默认值为中文；选择后即时生效或明确提示需要重载；语言选择持久化，未设置时不向配置文件写入无意义的默认键。
- 边界: 沿用现有 `ui/02-i18n.js` 翻译资源和 `settings.rs` 设置保存/加载链路，不引入新框架，不扩展第三种语言。
- 验收: ①首次无配置启动显示中文；②设置页可选跟随系统/中文/English；③选择、保存、重启后仍恢复；④切换 English 后静态文案和动态状态文案同步变化；⑤UI runtime smoke 有控件、持久化和生效断言；⑥相关 Rust/UI 门禁通过。
- refs: R-193 R-197
- 进展: 2026-08-12 已接通 KanzeiConfig/settings_get/settings_save 与设置页语言全链路,新增跟随系统解析、默认中文及未显式设置不落盘语义;已通过 cargo fmt/clippy、cargo test -p kanzei-app(125 passed)、node --check 与 UI runtime smoke(0 运行时错误)。 ‖ 2026-08-16 账本维护证据核验(原 [fixed] 污染标记,核验后转 done):实现提交 bc27f1d(界面语言设置全链路)+ 41baef4(D-308 补齐 R-225 UI lint 全局清单);代码实证 settings.rs language 字段、ui/02-i18n.js 翻译资源、16-settings.js 语言下拉、index.html 控件;当前 workspace 759 passed 含 kanzei-app 138 passed。残余:进展内引用的 smoke 断言无 tests.md 记录链接(验证证据链缺口,按 §1.2 可用即关闭,不阻塞)。 [terminal-fix 2026-08-13] done → done: 账本维护:close 前未剥 [fixed] 标题标记,归档标题残留 [fixed] [done] 双标记,收敛为单一 done 并清标题
- observed_head: e63be64ecd503b28359eeacdcf354b5fb8bc5340
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786605390957

## R-210 提交门禁减重与耗时可见:去 cargo check 冗余,verify/test_record 记录时长 [done]
- 优先级: P3
- 复杂度: 小
- 标签: 测试 流程
- 来源: 2026-08-12 八维度审计(§4)。
- 背景: 提交门禁对源码提交串行跑 cargo check 与 cargo clippy 全 workspace(git.rs:396/470-484,调用序 :584-596),clippy 语义覆盖 check,小步提交每次付双份全仓分析;verification.json 每步只有 "pass" 无时长,test_record 无 duration 字段,门禁最慢环节无从回答。
- 内容: 删除 compile_gate(或降级为 clippy 输出缺位置信息时的诊断回退);verify.ps1 每步记秒数写进 verification.json 的 checks 值;test_record 加可选 duration 字段。
- 验收: ①构造编译错误仍被拦且报错含 --> 位置;②单次源码提交门禁墙钟时间前后实测对照;③连续三次发版后能列出各步耗时。
- refs: R-192 R-212
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-210
- 进展: 2026-08-16(自动模式)交付:① git.rs commit 序列删掉串行 cargo check——clippy 全 workspace 编译覆盖 check;compile_gate 降级为 clippy 输出缺 --> 时的诊断回退(compile_gate 保留为私有回退,clippy_gate 失败且 stderr 无 --> 时调用补位置诊断)。验收①测试 clippy_gate_rejects_compile_error_with_position:构造未定义符号编译错误,clippy_gate 拦下且报错含 --> 与 lib.rs。② verify.ps1 每步 Stopwatch 计时(Step-With-Timing helper),checks 值记 'pass N.Ns',命令文本与 git.rs/ci.yml 逐项一致(对齐守护测试 stage_fmt_clippy_gates_align_with_ci_and_verify 绿);verify.ps1 语法解析通过。③ test_record 加可选 duration_secs 字段(TestRecordInput),经 record_test_run_with_duration / append_test_run_with_duration 写入 '- 时长: N.Ns'(既有调用零改动,靠 *_with_duration 变体);时长往返测试绿(含未提供 duration 不写行)。验证:kanzei-tools git:: 14 + test_record:: 29 全绿;提交 f432e91。残余(不阻塞,观测窗口):验收②墙钟前后实测随 harness 重建后的后续提交观测;验收③连续三次发版后 verification.json 各步耗时列表——机制已交付(checks 值带秒数),发版收尾时回填。
- observed_head: f432e91bcc04038b98176e740394ac65cbac5b06
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786606152862

## R-212 source_test_gate 从新近度升级到相关性:test_record 声明覆盖面,与暂存源码 crate 求交 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 测试 后端
- 来源: 2026-08-12 八维度审计(§4)。
- 背景: source_test_gate 只消费 last_passed_at 时间戳(git.rs:538-547)——任何 passed 记录可背书任何源码提交,前端冒烟记录能放行纯 Rust 提交。威胁模型不防说谎的模型,但要防「跑了 A 测试以为覆盖了 B」的诚实失误。
- 内容: test_record 增加覆盖面声明(crate 列表或从命令解析);门禁将暂存源码所属 crate 与记录覆盖面求交,不相交即拦并提示该跑什么。
- 边界: 不做 VerificationRun 全量体系(见审计 §11 候选池);不校验测试是否真跑过。
- 验收: ①前端冒烟记录无法背书纯 Rust 提交(定向测试);②正常闭环(改 crate→测该 crate→记录→提交)不受阻;③拦截文案指明缺口。
- refs: D-295 R-210
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-212
- 批次: 1/1
- 进展: R-212 交付并关闭前全量验证(2026-08-13):实现见 B1(0095fb8)。验收对照:① 前端冒烟记录无法背书纯 Rust 提交——source_test_gate_frontend_smoke_cannot_back_rust_change:记录命令 node scripts/ui-runtime-smoke.mjs、暂存 crates/kanzei-tools 源码,时间戳满足但覆盖面 NonRust → 拦下并点名 crate/记录类型/应跑命令;② 正常闭环不受阻——source_test_gate_coverage_intersects_with_staged_crates:定向 cargo test -p kanzei-tools 背书 kanzei-tools、cargo test --workspace 背书任意 crate、scripts/ 非 crate 源码豁免;③ 拦截文案指明缺口——不匹配时文案含 暂存 crate 名、记录覆盖面类型(crate X/非 Rust)、应跑 cargo test -p <crate>。全量 cargo test --workspace 761 passed/0 failed(T-1786608051)。残余(不阻塞):记录覆盖面仅从命令解析,手动改 tests.md 可伪造覆盖面(威胁模型不防说谎的模型,与边界一致)。
- observed_head: 0095fb863dc447993f7a3fa85f7c7b723d661541
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786608059692

## R-209 门禁清单机械同步守护:verify.ps1 与 ci.yml 逐项比对,CI 补 npm ci 与 ui-lint [done]
- 优先级: P2
- 复杂度: 小
- 标签: 测试 流程
- 来源: 2026-08-12 八维度审计(§4);清单已实际漂移——R-142 的 ui_lint 只进了 verify.ps1(提交 8b918ed),ci.yml 无该步且缺 npm ci(eslint 依赖装不上),而 ci.yml 注释承诺「本清单必须与 verify.ps1 逐项同步」,守护测试只比对 fmt/clippy 两项。
- 内容: 守护测试升级为机械比对两份清单的检查项集合(解析 verify.ps1 的 $checks 键与 ci.yml 步骤),任一侧增删即红;同步把 npm ci + ui-lint 补进 ci.yml。
- 验收: ①故意单侧加一步时守护测试变红;②ci.yml 跑 ui-lint 通过;③git.rs 注释承诺改为指向守护测试或保持一致。
- refs: R-142
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-209
- 批次: 1/1
- 进展: R-209 交付并关闭(2026-08-13,d124749):验收对照——① 单侧加一步守护测试变红:gate_checklists_align_across_git_verify_and_ci 双向比对(verify.ps1 Step-With-Timing 键集合==固定清单 10 键 + ci.yml 每键标记覆盖 + smoke 脚本两侧同现同隐 + npm ci 必需),任一侧增删即集合不等变红(修复前 ci.yml 缺 ui-lint/npm ci 时本测试必红);② ci.yml 跑 ui-lint 通过:ci.yml 已补 npm ci(eslint 依赖,package-lock.json 在库)+ ui-lint-smoke.mjs 进 ui smoke,本地六条冒烟全绿(ui-lint 31 文件 no-undef 0 错、ui-runtime 1547 invoke 等,T-1786608296);③ git.rs 注释承诺指向守护测试:git.rs 测试 doc、ci.yml 注释、verify.ps1 注释三处统一引用 gate_checklists_align_across_git_verify_and_ci。
- observed_head: d124749aabe65ec0cde4f2280c9583dd4f33be40
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786608363728

## R-200 测试统一走全局根隔离夹具,不再每处手写环境变量 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 测试 流程
- 来源: D-292 修复时的残余项。
- 内容: 仓里凡是会读全局根(`~/.kanzei`)的测试,统一走一个夹具:建临时目录、设 `KANZEI_HOME`、返回 guard 负责清理。现状是每处 spawn 手写 HOME/USERPROFILE/KANZEI_HOME 三个变量,漏一个就退回读开发者本机配置——D-292 正是漏了第三个,而且漏了很久没人发现,因为它只在特定全局配置下才炸。
- 边界: 不改 `kanzei_home()` 的优先级语义;不强制所有测试都用夹具,只覆盖会碰全局根的。
- 验收: ①提供夹具并把已知消费点迁移过去;②加一条守护测试:测试代码里出现 `.env("USERPROFILE"` 而没有同时出现 `KANZEI_HOME` 即判红;③开发者本机全局配置的任意内容都不影响测试结果。
- refs: D-292 D-187

- 进展: R-200 交付并关闭(2026-08-13,d7236ad):验收对照——① 夹具+迁移:TestHome(tests/common/mod.rs)建临时 HOME + HOME/USERPROFILE/KANZEI_HOME 三连 + Drop 清理,apply() 结构保证不漏;迁移 always_allow_bash.rs 4 处 spawn、context_overflow_recovery.rs run_cli_with_prior helper、e2e-smoke.mjs/probe-webview-cdp.mjs 补 KANZEI_HOME;② 守护测试:global_home_guard.rs test_spawns_isolate_kanzei_home_alongside_userprofile 扫描 tests/*.rs 与两脚本,出现 .env("USERPROFILE",)/USERPROFILE: 而无 KANZEI_HOME 即红(当前零命中);③ 本机全局配置不影响测试:迁移后所有子进程走 TestHome(KANZEI_HOME→临时目录,kanzei_home() 优先读),在本机带真实 ~/.kanzei 配置下全绿(实证)。验证:cargo test -p kanzei 全绿(T-1786609889)。阻塞字段已清(用户 2026-08-13 授权并加白名单)。
- 批次: 1/1
- observed_head: d7236ada9b95c92e8e232aaeaaf4acf38796c323
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786609910895

## R-185 并行取活的依赖判定升级为正确性前提:同批派发前必须证伪语义耦合,不是调度优化 [done]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 三条线实测 + 现有条目字段语义的读文自证)
- refs: R-182 R-184 R-177 D-239 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 用户在 R-182/R-184 定调后指出的第三条:「分析需求和缺陷的依赖性这个变得更重要了」。本条把它落成条目。
- 为什么它从调度优化升级成正确性前提(本条的全部理由): **串行写的时候,依赖判错是廉价且自愈的**——若 R-A 与 R-B 真有耦合,第二条跑起来会看到第一条的结果,自己就发现了。任务级并行把这个自愈机制拆掉了:两条线各自基于同一个陈旧基线往下做,**到合并时才暴露**。而暴露形态恰恰是 **git 检测不到的语义撞车**——A 把某个签名重构成形态①、B 按形态②写完,两边测试各自都绿,合并干净,语义已经坏了。R-182 的「边界」已明确**不做**语义撞车的事后检测(git 只做文本层),所以**事前的依赖判定是唯一的防线**。判错的代价从「多等一会」变成「合并出一个测试抓不到的坏语义」。
- 现状与缺口: 
  ①**判据今天是人工的**。本轮三条线(D-262/D-257/D-261)是人工挑的「文件面不重叠」,靠读条目正文推断,没有任何机械判据参与。
  ②**`依赖` 字段的语义不够用**。仓里实际存在两种前置——**阻塞依赖**(没它就做不了)与**非阻塞前置**(有它更好,解除权在 agent 手里),但字段只有一个。R-177 与 R-182 都在正文里写了「前置(不是阻塞,解除权在 agent 手里,按 D-239 教训**不写进「依赖」字段**免得调度器整条跳过)」——**用注释绕过字段缺陷已经成了惯例**,这本身就是缺口的证据。
  ③**D-239**(取活口径漂移:伪阻塞/伪可执行/挂起无载体)记的是同一个病在串行下的表现;并行会把它放大。
  ④R-184 解决的是「谁在跑」(且实测确认 `git worktree list` + `for-each-ref` 已免费提供),**不解决「该不该同时跑」**——两者是不同的问题,不要混为一谈。
- 内容: ①同批派发前的**耦合证伪**:给出一组可机械计算的信号,把「这两条能不能同时开」从推断变成判定。候选信号(按成本排序):`refs` 字段互指、条目正文点名的文件/路径面求交、以及**契约面求交**(函数签名、表结构、事件名、配置键——与 R-182 边界里记的语义撞车同一组维度)。②`依赖` 字段**拆成两个语义**:`阻塞依赖`(调度器必须跳过)与 `前置`(可并行,但要在协作上下文里对另一条线**显式说明**),消灭「靠注释绕过字段」的惯例。③判定结果**留痕**:派发时记下「凭什么判定这两条无关」,合并后若真出语义问题,能回查当初的判据错在哪——否则同一类误判会反复发生且无从改进。④判定为**耦合**时给出可执行的处置(串行化、或合并成一条、或明确指定谁先落地由谁重新适配),不是只报一个警告。
- 边界: 不做**全自动**依赖推断——信号用来收窄和提醒,最终判定仍可由人/编排者拍板,但拍板必须留痕(内容③)。不做语义撞车的**事后**检测(R-182 已明确不做,本条是它的事前对策)。不重做 R-184 的协作可见性(那是「谁在跑」,本条是「该不该同时跑」)。不改既有条目的历史 `依赖` 数据语义——迁移时旧值一律视为「阻塞依赖」,保守不激进。
- 验收: ①存在可机械计算的耦合信号并对**真实历史条目**跑出结果:至少能对本轮三条(D-262/D-257/D-261)判定为可并行,且能对一组**已知耦合**的历史条目(如 R-177 与 R-182 的主根重定向两半)判定为耦合,两个方向各有证据。②`依赖` 与 `前置` 两个语义分离落地,调度器只对 `阻塞依赖` 跳过;R-177/R-182 正文里那两段「不写进依赖字段」的注释可以删掉而行为不变(这是本条是否真解决了问题的判据)。③判定留痕可回查:能对任一次并行派发回答「当时凭什么认为这两条无关」。④判定为耦合时给出的处置是可执行的,不是一句警告,有实测轨迹。⑤旧数据迁移保守:既有 `依赖` 值一律按阻塞处理,无行为回归,有测试。⑥与 R-184 的边界在文档上写清,不留「协作可见性顺带解决依赖判定」的误解。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): 与 R-182/R-184 同族,三条构成任务级并行的最小可用集——R-182 拆掉多余的锁、R-184 让各方知道彼此在写、本条决定**哪些能同时写**。缺任何一条,并行都会以不同方式出事。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-185
- 批次: 3/3
- 进展: R-185 四段工作全部落地(B1 依赖/前置分离 / B2 耦合证伪信号 / B3 判定留痕+处置 / B4 全量+边界文档),Git 提交:B1=5a5f12c,B2+B3=d77d871(提交标题含 B2+B3 两个标记),B4 纯验证+文档无代码提交。批次按 Git 提交真源(3 个标记)修正为 3/3。验收①-⑥逐条证据见此前进展。关闭。
- observed_head: d77d871c6cfaebf9f60fc0a9ce90ab0282732778
- observed_worktree_hash: fnv1a64:f119eef6d4e99cb5
- recorded_at: 1786614522554
- 前置: R-182 R-184 R-177

## R-175 子代理后台化:跨轮存活、主代理派发不阻塞、可对话续跑 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 依赖: R-173
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调,四轴(后台化 / 子代理能写 / 并发度放开 / 可对话)**都要但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。本条吃「后台化」与「可对话」两轴,详细定调背景见 R-174 来源字段。
- 设计定位: 四轴分级第 2 级——把子代理从「轮内一次性调用」升级为有生命周期、有身份、可续谈的长期对象
- 现状(读码实证): ①子代理是**轮内并发、主代理必须等齐**:crates/kanzei-core/src/runner/drive.rs:410-503 把本轮所有 task 调用收进 `FuturesUnordered`,用 `tokio::select!`(:481-501)循环消费,全部归位后主代理才继续——派发方被钉在原地等最慢的那个。②`SubagentRuntime` 是纯进程内对象(crates/kanzei-core/src/runner/subagent.rs:14-34),返回即死,transcript 不持久化。③续跑无地基:run_subagent 调 run_once 时 prior 传的是空历史(subagent.rs:189 `&[]`),没有可续的上下文。④超时是纯兜底墙钟(drive.rs:462-475,`rt.timeout_secs` 默认 900,见 crates/kanzei-harness/src/config.rs:81-83)。⑤读槽是 RAII 释放(subagent.rs:163-176 `_read_permit` 随函数返回自动 drop),后台化后函数不再随子代理生命周期返回,这条释放路径必然失效。⑥R-174 记录的前置回归同样适用:桌面端主对话因 run.rs:107-108 无条件 `ReadParallelWriteSerial` 而根本不注册 task 工具,后台化在桌面端可达之前必须先由 R-173 修好。
- 内容: ①drive.rs:410-503 的「派发—等齐—归位」语义改为可选:后台模式下 task 派发后立即返回句柄,主代理本轮继续做别的,不再被 select! 循环阻塞。②跨轮子代理注册表:跨会话存活、崩溃/重启后可发现——不能只活在内存里。③完成/失败/超时**发通知回主对话**:复用既有 `agent_notifications` 表(crates/kanzei-core/src/store/notifications.rs)与 session_events 轨迹,**不新造通道**。④子代理 transcript 持久化,支持按 id 恢复上下文并追加消息续跑(「可对话」轴)。⑤所有终态确定、不得悬挂:超时 / 失败 / 被停三条路径都要落确定终态并释放读槽——RAII 失效后需要显式释放路径(设计不变量 7:停止、关闭、panic 收尾和窗口退出都必须释放并给排队者确定终态)。⑥屏障、终态、编排事件轨迹一律复用 R-173 的阶段编排对象,**不另造一套**。
- 边界: 后台子代理仍受只读白名单约束——crates/kanzei-tools/src/subagent.rs:13-25 构造时只装 read/glob/grep,ask 一律 Deny(crates/kanzei-core/src/runner/subagent.rs:177-179);写权是 R-176 的事,两条需求不混做。面板呈现(Running/Finished 分区、单条停止、transcript 查看)属 R-174,本条只负责让后台条目有真实数据与真实停止通道可被它消费。
- 验收: ①主代理派发后不阻塞的实证:同一轮内 task 派发时间戳与主代理后续工具调用时间戳**交错**(时间线证据),而非全部排在最慢子代理完成之后;②跨轮存活可实证:第 N 轮派发的子代理在第 N+1 轮仍在运行且可被查询到状态;③重启后能发现在跑的子代理:强杀进程后重开,注册表能列出上次未终结的子代理并给出确定处置(继续或标失败),不留幽灵条目;④给正在跑的子代理发消息能带原上下文续跑——续跑请求里可见此前 transcript,不是从空历史重开(与 subagent.rs:189 现状对照可验);⑤三种终态(超时/失败/被停)都有确定归宿且读槽被释放:协调器快照(`MemoryCoordinator::snapshot`,crates/kanzei-core/src/orchestration.rs:274)在终态后不再残留该子代理的读者身份,有测试覆盖三条路径;⑥事件可回放:后台子代理的生命周期事件落 session_events,重启后能按 id 回放完整轨迹;⑦通知走既有 `agent_notifications` 表(有测试证明未新造并行通道)。
- refs: R-174 R-176 R-095 R-171 docs/design/parallel_read_serial_write_orchestration.md
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-175
- 批次: 5/5
- 进展: B1a+B1b+B2+B3+B4+B5 全部完成(2026-08-13,提交 8cd437d/a441df0/cf511ff/dfb1c29/babdc34/c09e44f,批次 5/5):B5(c09e44f)重启可发现——drive.rs spawn 块派发即记 running 事件(与 done/failed 并列,三条 task.lifecycle 完整);新增 pending_background_subagents 纯函数从 session_events 回放找「running 无终态」id(重启后列出上次未终结子代理,给确定处置标 failed,不留幽灵);验收③测试 pending_background_subagents_只列running无终态_终态不残留。workspace 801 passed 全绿、clippy 干净。批次按 Git 提交真源(B1a+B1b 在标题中解析为 B1 一个标记,共 5 个:B1/B2/B3/B4/B5)修正为 5/5。六段工作全部落地,逐条验收见关闭记录。
- observed_head: c09e44f66e99d3f643dd8c2979873e2c2ea3e3ed
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786618963768

## R-180 跨 run 长驻的受管后台服务:生命周期脱离 owner run,日志落盘可回看 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(读码核实 crates/kanzei-tools/src/background.rs,2026-08-10 dev HEAD)
- 来源: 2026-08-10 D-174 交付时的安全降级转出。D-174 本轮取的口径是「后台任务生命周期 ⊆ owner run」——后台任务登记 `BackgroundOwner{run_id, process_id, 写仲裁键}`,owner run 收尾时一并收尾,好处是不会留下用户看不见的进程在改仓库。代价是 dev server 一类**需要跨 run 存活**的服务做不了。
- 现状(读码实证): ①`BackgroundHandle.owner: BackgroundOwner` 已登记归属身份(background.rs:29/55),跨 owner 收尾判定消费它;②后台日志**只在内存**——`MAX_BACKGROUND_OUTPUT = 256 * 1024`(background.rs:23),超限「丢头留尾」并标记截断(:131),**不落盘、不进 state.db**,进程一退历史全没;③没有任何注册表让后台任务活过 owner run。
- 内容: ①受管后台服务档位:生命周期显式脱离 owner run(用户或 agent 明确声明"这是长驻服务"),与"跟随 owner run"的默认档位并存,不是把默认改掉。②长驻服务的注册表跨 run 可发现,重启后能列出仍在跑的服务并给确定处置(接管 / 标失败 / 杀掉),不留幽灵进程。③后台日志落盘可回看,取代现在的内存 256 KiB 丢头留尾;落盘不得让日志变成新的写冲突源(走 R-138 的原子写原语,不另造)。④长驻服务仍受 D-174 的托管路径归因与越界回滚约束——脱离 owner run 不等于脱离文件隔离。
- 边界: 不做通用的服务编排/健康检查/自动重启;不把默认档位改成长驻(D-174 的安全降级是有意为之)。子代理后台化属 R-175,两者语义相关但不是同一件事——R-175 管的是**子代理**跨轮存活,本条管的是**shell 后台进程**跨 run 存活;实现时共用注册表与终态口径,不要各造一套。
- 验收: ①声明为长驻的后台服务在 owner run 结束后仍在跑,且能被查询到状态;默认档位的后台任务行为不变(owner run 收尾即收尾),有测试区分两档。②强杀 kzapp 后重开,注册表能列出上次未终结的长驻服务并给出确定处置,不留幽灵条目。③后台日志落盘:超过 256 KiB 的输出不再丢头,重启后仍可回看,有测试。④长驻服务写入托管路径(`.kanzei/project`、`.kanzei/memory`)仍被 D-174 的归因/回滚拦下,有回归覆盖。⑤日志落盘走 `crates/kanzei-tools/src/atomic_file.rs` 的原语,全仓不出现第二套写原语。
- refs: D-174 R-175 R-138 R-097
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-180
- 批次: 3/3
- 进展: 全部批次完成(e3ad720 B1 / a318b7c B2 / f02fb3d B3),B3 后收尾核对与关闭前全量已做。验收证据:①长驻跨 owner 存活:persistent 字段 background.rs:61, finish_foreign_owners 跳过 :448, kill_project/kill_process 跳过 :708/:732, 两档区分测试;②强杀重开注册表:PersistentEntry 落盘 temp/kanzei-bg-logs/<hash>/registry.json(registry_path :522, save_registry :538), register 登记 :182-200, 自然退出移除 :256-270, stop 移除 :695-698, discover_persistent :559 / mark_registry_failed :570 / adopt_persistent :587 / kill_registered :646, process 工具 discover/adopt/kill :156-225, 幽灵清理测试;③日志落盘不丢头:full_output 全量+节流 write_atomic :206-249, 测试超256k;④长驻写托管路径仍被归因/回滚:守卫 :376-426, register/adopt 都挂守卫 :264/:636, 两条越界回归测试;⑤全仓单源 write_atomic:kanzei-llm/src/atomic_file.rs:39, kanzei-tools 经 lib.rs:6 pub use 单源引用(D-261 并轨), background.rs :237/:249/:541 三处全部走 crate::atomic_file。关闭前全量 cargo test --workspace 全绿(T-1786623266, kanzei-tools 321 含三批 10 个新测试)。
- observed_head: f02fb3daaa453933203471c70fe172a394e2e561
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786623299975

## R-181 跨 agent 源码写入互斥:写租约延伸到外部进程,kz lock 让外部 agent 也能入局 [done]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 真实撞车实例,有提交为证)
- refs: R-171 R-173 R-138 D-263 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 凌晨的一次真实撞车。用户在外部 agent(Claude Code)里派了一个子代理改 `app/run.rs`/`state.rs`/`processes.rs`/`phase_pipeline.rs`,同时桌面端自举循环取活 R-174 并在同一批文件上工作。结果:自举的两次提交(`92879e2`/`25ea2c0`)把外部代理**尚未完成的改动一并扫进了自己的提交**(标题里的「含 R-173 遗留收尾」就是被裹进去的那部分),并留下 8 处 fmt + 6 条 clippy 红灯。改动没丢,但归属混了、CI 红了、两边都不知道对方在写。
- 现状与缺口: R-171 交付的项目级单 writer 是 `AppState` 里的**进程内内存实现**(`crates/kanzei-core/src/orchestration.rs` 的 `MemoryCoordinator`)。它保护的是**kanzei 自己的 agent 之间**——主对话、task 子代理、旁路 Tauri 命令。它看不见:①外部 agent(Claude Code / Cursor / 人手动改);②`kz` CLI(`crates/kanzei/src/main.rs` 的 tracker 子命令 `coordinator: None`);③第二个 kzapp 实例。设计基线 `parallel_read_serial_write_orchestration.md` 的「TODO 与后续风险」第 5 条早就点名了这个缺口(「未来多个 OS 进程同时打开同一项目时,AppState 内存协调器不可见;P3 必须用文件锁或持久 lease 扩展同一接口」)——**2026-08-11 它不再是「未来」,已经发生了**。R-138 已交付的跨进程文件锁(`crates/kanzei-tools/src/atomic_file.rs` 的 `FileLock`,Windows `share_mode(0)` 独占句柄,零新依赖)只保护 docstore 的 tracker 文件,**保护不了 `crates/**` 源码**。
- 方向修订(2026-08-11,R-182 定调后): **本条原文不重写,但主张已被推翻一半。** 原方向是「把写租约延伸到外部进程,让外部 agent 也来取锁」;R-182 的实测把口径改成「分支干、合并、冲突检测解决、文档一份唯一」后,这个方向对**源码**不再成立:①源码根本不需要跨进程互斥——worktree 已经物理隔离,冲突交给 git 三方合并与 `merge-tree` 预检(R-182 实测③:三条线各改自己那段,顺序合并全干净);②**锁只能约束进得来的人,检测能约束所有人**——本条自己的「边界」就写着「不做强制拦截外部进程的写(做不到,也不该做)」,而外部 agent、手动改、第二个 kzapp **全都要过 git**,检测面天然覆盖全员,租约天然覆盖不了。仍然成立的是**文档侧**:tracker 的「读→分配 ID→写」需要互斥,但那已由 R-138 的 `FileLock` 在**单份主根**上解决(R-182 实测②),不需要 run 级租约。
  **本条的存留形态待定**:剩余真实价值可能只有「让外部写入者**可见**」(谁在写、写了多久、动了哪些文件),即从「取锁入口」改为「**声明与检测入口**」。取活前先按 R-182 的结论重估本条是否还需要独立交付,不排除降级或并入 R-182。**来源字段记录的那次真实撞车(`92879e2`/`25ea2c0` 卷入他人改动)依然有效**——但它的根治是 D-263(只 add 明确文件)+ worktree 隔离,不是写租约。
- 内容: ①把写租约扩成**跨进程**实现:复用 `atomic_file::FileLock` 的独占句柄手法,在主根落一个持久 lease(持有者 = pid + run_id + 取得时刻 + 用途),`ProjectExecutionCoordinator` 接口不变(设计基线明写「换插不换契约」);②新增 `kz lock <acquire|release|status>` CLI,让**外部 agent 也能入局**——外部 agent 不受 kanzei 的 runner 约束,唯一可行的是给它一个能主动调用的通道,并把「动仓库前先 `kz lock acquire`」写进 conventions;③引擎侧在取活前检查外部 lease,被占时**明说谁占着、占了多久**并等待或跳过,不得静默继续(D-004 口径);④崩溃不留死锁:独占句柄随进程退出由 OS 关闭,非 Windows 走 mtime 陈旧摘除,与 `FileLock` 同一套;⑤lease 事件进 session_events,与 R-171 的 `writer.*` 同一出口。
- 边界: 不做强制拦截外部进程的写(做不到,也不该做);本条是**协作式**互斥——提供机制 + 可见信号,让双方都能知道对方在写。真正的强隔离是 worktree(R-177),两者互补不互替。
- 验收: ①两个 OS 进程(kzapp + kz CLI)同时申请写租约,实际持有区间不重叠且顺序可审计;②`kz lock status` 能报出当前持有者(pid/run_id/取得时刻/用途)与等待队列;③引擎取活时被外部 lease 占住,轨迹里有可见记录并说明持有者,不是静默跳过或静默继续;④强杀持有进程后 lease 自动失效,下一个申请者能立刻拿到(崩溃不留死锁,有实测);⑤`ProjectExecutionCoordinator` 的调用契约未变(现有 runner/旁路调用点零改动,有编译期证据);⑥conventions 补一节「外部 agent 动仓库前的取锁纪律」。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-181
- 批次: 1/1
- 进展: 降级交付完成:按 2026-08-11 R-182 定调,跨进程写租约主张被推翻,本条降级为「外部写入者声明与检测入口」。交付(6ef64ab):`kz lock status` CLI(main.rs lock_cli + lock_status_report)——报主根/cwd/git 工作树未提交改动(外部 agent 痕迹可见)/活跃线(state.db processes),只读不阻塞,state.db 缺失走降级文案;2 测试。conventions §6.1 外部 agent 协作纪律。关闭前全量 cargo test --workspace 全绿(T-1786623912)。降级后验收逐条:①原「两进程写租约不重叠」→R-182 推翻(无 run 级租约;kz lock status 无锁可并跑,纯函数无共享状态);②原「报持有者/等待队列」→降级为报主根/cwd/工作树改动/活跃线;③原「引擎取活被外部 lease 占住」→R-182 撤销 run 级租约后不适用;④原「强杀 lease 失效」→无 lease 不适用;⑤协调器契约未变→零改动(仅 main.rs CLI 分发,core/tools 未动,编译证据);⑥conventions 纪律→§6.1 落地。
- observed_head: 6ef64abae45aacec58f7d9d969d3a4d78fd0108f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786623927113

## R-176 写子代理:自持写租约的并行实现线,协调器 FIFO 排队与改动可归因 [done]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 依赖: R-173 R-175
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调,四轴**都要但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。本条吃「子代理能写」轴,详细定调背景见 R-174 来源字段。用户明确要比参照物更激进——参照物的子代理仍是只读探索,kanzei 要让子代理自己拿写租约、成为真正的并行实现线。
- 设计定位: 四轴分级第 3 级——把「并行只读勘察 + 单 writer 串行实现」升级为「多条写实现线由协调器排队串行安全落地」
- 现状(读码实证): ①只读白名单在**构造时**强制:crates/kanzei-tools/src/subagent.rs:13-25 的 `SubagentBase::contribute` 只 insert read/glob/grep 三个工具并只放行这三条规则,写/命令/联网在代码层面不存在(桌面端装配点 crates/kanzei-app/src/run.rs:456-461);子代理内 ask 一律 Deny(crates/kanzei-core/src/runner/subagent.rs:177-179)。②写租约地基**已就位**:契约在 crates/kanzei-harness/src/orchestration.rs(`acquire_read_slot` :195、`acquire_writer_lease` :198),内存实现 `MemoryCoordinator` 在 crates/kanzei-core/src/orchestration.rs(:191-243 独占 + FIFO 排队、:95-137 释放并唤醒队首、:244 取消等待者给确定终态、:274 快照),读槽 `acquire_read_slot`(:167-190)无条件放行(读写可共存,设计不变量 9)。③子代理侧只登记读槽(crates/kanzei-core/src/runner/subagent.rs:163-176),从不申请写租约。④R-174 记录的前置回归同样适用(run.rs:107-108 + drive.rs:57 使桌面端 task 全禁)。
- 内容: ①打破只读白名单:新增**可写子代理档位**(独立组件与快照,不是给现有只读档位加工具——只读档位的白名单是审计资产,设计不变量 1 要求构造后与执行前各复核一次)。②**每个写子代理必须自己 `acquire_writer_lease`**,不得继承主代理的租约、不得绕过协调器(设计不变量 3「同一规范化 project_root 同时最多一个 writer_run_id」、4「不允许在两个工具调用之间切换写代理」、8「写工具不得绕过协调器」)。③写子代理之间由协调器 **FIFO 排队**,不是禁止并发申请——这正是 R-171 租约相对「硬禁写」的价值所在,`MemoryCoordinator` 的独占+FIFO+RAII 释放已实现,本条是把它接到子代理侧。④**权限询问必须发生在取租约之前**(设计不变量 6:用户拒绝后不得占用写租约);现状写子代理没有询问通道(ask 恒 Deny),必须换成真实询问路由并保证询问先于租约。⑤与 D-174 的后台 shell 归因体系对齐:writer 释放租约前必须收尾,不得留下仍在写的后台进程(设计不变量 7)。
- 风险(本条是四轴里风险最集中的一条,必须写在验收之前): 写子代理 + 后台化 = **用户看不见的进程在改仓库**。三条护栏缺一不可关闭:(a) 每个写子代理的改动可归因——改了哪些文件、是哪个子代理 id 写的;(b) 单个写子代理的改动可**单独回滚**,不误伤其它写子代理与主代理的改动;(c) 面板上可见**正在写的是谁**、谁在排队。
- 边界: worktree 绑定不在本条(那是 R-050 的批1);本条只保证「多个写子代理在**同一工作树**上串行安全」。后台化本身属 R-175,本条只在其之上加写权。
- 验收: ①两个写子代理同时申请写租约,实际持有区间**不重叠**且顺序可审计(协调器 orchestration.* 事件轨迹为证,复用 R-171 批5 的事件);②写子代理绕过协调器的路径**在代码上不存在**——写工具的装配点强制经租约,不是靠提示词约束(conventions §4「权限规则是硬门禁:任何『规则』能用代码强制的绝不只写进提示词」),有断言测试证明无旁路;③权限询问在取租约**之前**发生,有顺序断言(拒绝后不得占用租约);④写子代理的改动**可按 owner 归因**:任一文件改动能查到是哪个子代理 id 写的;⑤单个写子代理的改动**可单独回滚**,不误伤其它写子代理与主代理的改动;⑥面板(R-174)能看到当前持写权的是谁、谁在排队,数据来自协调器快照(orchestration.rs:274)而非前端推测;⑦只读子代理档位的白名单未被本条放宽(crates/kanzei-tools/src/subagent.rs 的只读快照仍只含 read/glob/grep,有回归测试)。
- refs: R-171 R-173 R-174 R-175 R-050 D-174 docs/design/parallel_read_serial_write_orchestration.md

- 进展: 全部 5 批完成(B1 487f07e / B2 290d0ef / B3 674bf5a / B4 b77ac1d / B5 1dbf69e),关前全量 cargo test --workspace 全绿(T-1786626063)。验收证据逐条:①两写子代理租约不重叠可审计——B2 acquire_subagent_permit 走 MemoryCoordinator 同树 FIFO(write_scope),permit_kind 测试;②写工具强制经租约无旁路——B1 可写档位写权限不预设 Allow 由规则集裁决 + B2 可写路径代码强制 acquire_writer_lease;③询问先于取租约——B3 ask_router + writable_granted(Deny/Cancelled 拒绝不占租约)测试;④改动按 owner 归因——B4 SubagentChangeLog.record(owner=子代理 id)+ files_of 测试;⑤单独回滚不误伤——B4 rollback 只恢复该 owner 文件,测试验证其它 owner 与主代理改动保留;⑥面板展示持写权者/排队——B5 CollaborationLine writer_run_id/waiting_writers 从 coordinator.snapshot() 取,测试验证数据来自快照;⑦只读白名单未放宽——B1 回归测试(只读快照仍只含 read/glob/grep)。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-176
- 批次: 5/5
- observed_head: 1dbf69e525bfc09969b338fc99973e0723a59f34
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786626076642

## R-222 收活五格补两道防线:门禁成为合并前置(红灯需显式覆盖确认),合并后插「合并后全量」步 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 前端 并行
- 来源: 2026-08-12 八维度审计(§7);parallel_lines_ui.md §5 明写「③门禁由 kanzei 跑:不能信线自己说的绿」与「④合并后再跑一次全量:两条线各自绿≠合起来绿」,实现中格3 可整体跳过(20-lines.js:287-294 点「我已读过 diff」同时解锁门禁与合并两钮,:334-339 门禁失败只渲染警示不禁用合并),合并后全量完全没有(:388-391 合并成功直接解锁格5)。
- 内容: 格4 要求格3 本次会话内跑过,未跑或红灯时合并需显式「门禁未通过仍要合并」确认并落轨迹;合并成功后格5 前插入「合并后全量」一步。
- 验收: ①未跑门禁时合并按钮带确认拦截(冒烟断言);②红灯覆盖确认落轨迹;③「合并后全量」步可跑且结果可见。
- refs: D-305 R-179
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-222
- 批次: 1/1
- 进展: 完成:收活五格补两道防线——①门禁成为合并前置:格2(人读 diff)确认后只解锁格3(门禁),门禁全绿才解锁格4(合并),未跑/红灯时合并按钮点击弹「门禁未通过/未运行,仍要继续合并吗」显式覆盖确认并落轨迹(console + activityLog);②合并成功后格5 前插入「合并后全量」步(主根 worktree_post_merge_gate,复用 gate_steps 不另造门禁定义),通过后解锁格6(回写 tracker,原格5 顺延)。验收:①未跑门禁合并带确认拦截(冒烟断言 gateOk/gateRan 时序);②红灯覆盖确认落轨迹(console.info + activityLog push);③合并后全量可跑且结果可见(冒烟断言 postMergeCalls + pass 结论)。i18n 新增 13 个 key,ui-lint-globals 重新生成。前端冒烟五连全过(21 组断言),kanzei-app 140 passed。
- observed_head: 1dbf69e525bfc09969b338fc99973e0723a59f34
- observed_worktree_hash: fnv1a64:025b4ad57056a73a
- recorded_at: 1786626556939

## R-223 权限被拦聚合呈现:自动轮每次跳过落可见 notice 或轮末汇总,「自动放行」挂常驻徽标并对齐语义 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 前端
- 来源: 2026-08-12 八维度审计(§3);07-events.js:432-436 对 autonomous/parallel 询问只在默认隐藏的运行日志留一行即丢弃,D-281 记载 R-191 因此连撞三轮才被发现;「自动放行」文案称「本次」实际跨重启持久化且折叠在菜单里无常驻标识。
- 内容: 自动轮每次权限跳过在对话流落可见 notice 或轮末汇总「本轮 N 次被拦(动作/资源清单)」;「自动放行」开启时状态栏挂常驻警示徽标,tooltip 与持久化语义对齐(要么真的仅本次,要么明说会记住)。
- 验收: ①自动轮被拦 ≥1 次时对话流可见;②开启自动放行后重启仍有常驻标识;③两条冒烟断言。
- refs: D-281
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-223
- 批次: 1/1
- 进展: 完成:权限被拦聚合呈现——①自动轮(autonomous/parallel)权限询问被拦时,07-events.js 的 kz:ask 跳过分支调 recordBlockedAsk:每次落对话流可见 notice(⚠️ 权限被拦已跳过: action · resource),kz:done 轮末 summaryBlockedAsks 汇总「本轮 N 次被拦(动作/资源清单)」(被拦 ≥1 次才可见,blockedAskSummaryShown 防重复);②「自动放行」开启时状态栏挂常驻警示徽标 #status-auto-allow(警示色,index.html + style.css),syncAutoAllowBadge 随开关/初始化同步,localStorage kz-auto-allow 持久化跨重启仍显示;tooltip 语义对齐——「本次不再弹权限窗」改为「此选择会被记住,跨重启仍生效」(index.html + i18n key)。i18n 新增 4 key(权限被拦已跳过/本轮权限被拦/次/两个自动放行文案),ui-lint-globals 重新生成。验收:①被拦 ≥1 次对话流可见——冒烟断言(autonomous ask → notice + kz:done 轮末汇总);②重启后徽标常驻——localStorage 断言;③两条冒烟断言全过。前端冒烟五连全过(21 组断言,ui-runtime 1682 invoke)。
- observed_head: 8534f4e8d7358a870085cf22fe5a9f47e42d38ba
- observed_worktree_hash: fnv1a64:5d189a7d794ee0bb
- recorded_at: 1786627048192

## R-228 关闭门禁测试面跟标签走:前端标签任务 B2 全量必含 verify.ps1 的 ui_* smoke [done]
- 内容: cargo test --workspace 全绿不等于全量——项目门禁口径是 verify.ps1 十步,其中六步是前端 smoke;带前端标签的 req/defect 在 B2 关闭前全量时必须跑 ui_syntax/ui_runtime/ui_lint/ui_i18n(可做成按标签选测试面的关闭门禁)
- 复杂度: 中
- 来源: 2026-08-13 自举复盘(D-320 三处中两处的共同根因)
- 标签: 流程
- 验收: ①前端标签任务关闭时未跑 ui smoke 会被拒;②D-320 类(i18n 缺 key、smoke 断言过时带病过关)在门禁层不可复现;③非前端任务不受影响
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-228
- 批次: 1/1
- 进展: 完成:关闭门禁测试面跟标签走——①test_record.rs 新增 frontend_smoke_passed(root):遍历 active+archive 找最近一条 passed 且命令命中 `node scripts/ui-*.mjs` 运行型冒烟(ui-runtime/i18n/lint/a11y/markdown)的记录,返回收尾时刻+标题;`node --check`(纯语法)与 cargo test 不算冒烟(验收②:smoke 断言过时带病过关不可复现——只有真跑过运行型冒烟才算数);②tracker.rs close 分支:条目标签含「前端」且无任何前端冒烟 passed 记录时拒绝关闭,文案点名「cargo test 全绿不等于前端全量」(验收①);非前端标签条目不受影响(验收③)。测试 2 个:frontend_smoke_passed_recognizes_ui_smoke_and_ignores_syntax_and_cargo(识别:ui-*.mjs 算、--check/cargo 不算、取最新收尾)、前端标签关闭需前端冒烟passed_非前端不受影响(行为:前端被拒→补冒烟后放行;核心标签放行)。kanzei-tools 325 passed、clippy/fmt 干净。
- observed_head: 7b62fc686d4c7bdb469d6d809776dad49bd95e60
- observed_worktree_hash: fnv1a64:b52720acb17dec52
- recorded_at: 1786627427612

## R-211 偶发红加压脚本:循环 N 次定向/全量测试、统计失败率并存档输出,作为 D-293 验收载体 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 测试 流程
- 来源: 2026-08-12 八维度审计(§4);D-293 验收①「加压并行+循环 N 次」与③「连续 20 次全量无偶发」目前没有可执行载体,偶发红治理机制整体空白(无加压工具/隔离标记/失败率统计)。
- 内容: scripts/ 落一条压测脚本(参数:目标 -p crate 或全量、轮数、并行度),统计失败率、存档失败输出;约定偶发红一律先跑它出数字再定位;顺带评估 cargo-nextest 的逐测试计时与显式重跑标记能力。
- 验收: ①能机械产出「连续 20 次全绿」或「N 次内命中 M 次」结论;②失败输出自动存档可回查;③用它对 D-293 两条跑一轮出数字并回填该条。
- refs: D-293 R-200
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-211
- 批次: 1/1
- 进展: 完成:scripts/stress-test.ps1 压测脚本——参数 Target(-p crate 或空=全量)/Rounds/Parallel(1-4,仅单 crate)/Filter;逐轮跑 cargo test,统计失败率,输出机械结论「连续 N 次全绿」或「N 次内命中 M 次失败(第 X 轮)」(验收①);失败输出自动存档 output/stress-<时间戳>/round-N.log + summary.txt(验收②);约定偶发红先跑它出数字再定位。实测:全量 3 轮✓、单 crate 并行 2 轮✓、read::read_non_memory 20 轮 0 失败(0%)、docstore::原子写 20 轮 1 失败(5%,第 18 轮)——**抓到 D-293 修复后仍存在的真实偶发红**,新登记 D-338(读到截断态,docstore.rs:2181);全量 20 轮后台跑(超时阈值 10 分钟不够)。验收③:对 D-293 两条跑出数字 read 0%/docstore 5% 并回填。顺带评估:未用 cargo-nextest(逐测试计时属增强,现有脚本已满足载体需求)。
- observed_head: babc9754d63d0fa1eb6caf40594e41ff2b9408fd
- observed_worktree_hash: fnv1a64:44e6bb2deb156440
- recorded_at: 1786628546027

## R-229 关闭证据分类断言机器检查:「剩余/其余 N 处均为 X」式断言必须逐处带 file:line 引证,不足即拒 [done]
- 内容: 关闭门禁增加证据文本检查:出现「剩余/其余 N 处」类分类断言时,关闭文本必须逐处点名 file:line 并引码,引证数不足 N 即拒关闭;根因是 R-199 关闭证据把完整否决误归为「非续跑否决」且无人核对(产出 D-320/D-323)
- 复杂度: 中
- 来源: 2026-08-13 自举复盘改进建议第 2 条
- 标签: 流程
- 验收: ①门禁单测覆盖断言引证不足拒绝;②R-199 式未核实分类断言在门禁层不可复现;③无分类断言的关闭不受影响
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-229
- 进展: 2026-08-16 取活(engine:defect-first 队首)。实现(commit 89abe7e):关闭门禁在 tracker.rs close 分支 merged 构造后接入 check_close_classification_evidence(kanzei-tools/src/tracker.rs:660-666 调用),新增三个纯函数:classification_claims(识别「剩余/其余 N 处」断言,只认数字+处,「剩余价值」不算)、file_line_citations(数 [路径/]文件.扩展名:行号 引证,去重,排除 R-199:/12:30 类)、check_close_classification_evidence(声称总处数 vs 引证数,不足即拒)。两个新单测:分类断言引证不足拒绝关闭_r199式无引证不可复现(0 引证拒、2<3 拒、3==3 放行)+ 无分类断言关闭不受影响。验证:tracker 43 passed(T-1786630105)+ cargo test --workspace 全绿 794 passed(T-1786630228)+ fmt/clippy 全过。三条验收逐条对照:①门禁单测覆盖断言引证不足拒绝——测试 分类断言引证不足拒绝关闭_r199式无引证不可复现 断言 0 引证与 2<3 均 is_error;②R-199 式未核实分类断言在门禁层不可复现——同测试用 R-199 原文「剩余 3 处 autoContinueAllowed 为…非续跑否决」无任何 file:line 即被拒;③无分类断言的关闭不受影响——测试 无分类断言关闭不受影响 断言放行且状态转 done,「剩余价值」非断言用法不误伤。关闭。
- observed_head: 89abe7ef3bd760b54a526784b8685dc2e501523a
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786630234644

## R-232 tracker 写操作幂等化:同值 update 返回 no-op,变更返回 diff 摘要 [done]
- 内容: update/close 对未变更字段返回「no-op: 字段已是该值」且文件零写入;有变更时返回 旧→新 摘要。消灭「先决定改 X→发现已是 X→仍执行写操作」的冗余调用与模糊回执引发的二次确认循环(V4PRO 实测 批次 0/3 冗余 update)。D-329 已修游离空段,本条补反馈语义
- 复杂度: 小
- 来源: 2026-08-13 V4PRO 运行复盘(refs R-230 D-329)
- 标签: 后端
- 验收: ①同值 update 返回 no-op 且文件字节不变(单测);②变更返回旧→新摘要;③close 幂等重入安全
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-232
- 进展: 2026-08-16 取活(engine:defect-first 队首)。实现(commit 6051c16):①tracker.rs update/close 分支在应用变更前克隆 before(kanzei-tools/src/tracker.rs:625-634),比较 user_visible_fields(before) 与 after——同值返回 no-op 且不调 store.save(文件零写入,验收①);②有变更时 field_diff_summary 输出 旧→新 摘要并追加进 updated 返回(验收②);③close 幂等重入:already_terminal(done/fixed)条目再次 close 跳过 R-228 前端冒烟/批次/R-229 分类断言/测试记录校验,目标仍是当前终态,无变更零写入、补字段可落盘(验收③)。新增辅助函数 user_visible_fields(剔除引擎锚点 recorded_at/observed_head/observed_worktree_hash,纳入 status/title/severity)与 field_diff_summary。两个新单测:同值update返回noop且文件字节不变_变更返回旧到新摘要 + close幂等重入_已终态条目再次关闭返回noop。验证:tracker 45 passed(T-1786630650)+ kanzei-tools 全 329 passed + fmt/clippy 全过。三条验收逐条对照:①同值 update 返回 no-op 且文件字节不变——测试断言 out.content 含 no-op 且前后文件字节相等(同值 update 必须零写入);②变更返回旧→新摘要——测试断言输出含「优先级: P1 → P2」且落盘;③close 幂等重入安全——测试用已 done 前端标签条目(无前端冒烟 passed)再次 close,断言不被 R-228 拦截、返回 no-op、文件字节不变、状态不回退;重入补字段(进展)可落盘。关闭。
- observed_head: 6051c160d8623b392b3dd1fbc069b55c224cd67e
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786630666011

## R-144 验收核查周期化:鞭挞每关 N 条自动插入只读核查回合 [done]
- 背景: direction_taste §5.5:08-07 式事件性审计(R-092 手动按钮)应变成常驻节律——鞭挞每关 N 条自动插入一轮只读核查回合,复用现有只读子代理,把验收打假从人工触发变为自动循环的一部分。
- 设计定位: 自举质量的常驻核查节律(§5.5)
- 证据等级: E1
- 阶段: 2
- 验收: 鞭挞/自主推进每关闭 N 条(可配)自动插入一轮只读核查(复用 SubagentBase read/glob/grep):核对已完成条目的验收证据与真实调用方;发现问题时生成候选缺陷或退回依据;核查不进入主 conversation/queue;触发频率与 N 可配置。
- 优先级: P0

- 标签: 流程
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-144
- 复杂度: 大
- 批次: 4/4
- 进展: B4 完成(2026-08-16):cargo test --workspace 全绿 797 passed(T-1786632590)。四条验收逐条对照:①鞭挞/自主推进每关闭 N 条(可配)自动插入一轮只读核查——Cadence.verify_every_n(默认 3,0=关闭,config.rs:170-177)、AutoRunState.closed_since_verify 计数 + decide 达阈值返回 VerifyRound(harness auto_run.rs:197-203)、app run.rs 构造 AutoRunCtx 传 closed_count_this_round(summary 里 req/defect close 成功配对)+ cadence 配置(kanzei-app/src/run.rs:1000-1005);②复用 SubagentBase read/glob/grep 核对验收证据与真实调用方——verify_prompt(harness auto_run.rs:131-147)指示主代理用只读 task 子代理核对,核查指令经 serialize_action VerifyRound 携带 prompt 发给前端(kanzei-app/src/auto_run.rs:141-148),前端 armAutoContinue(action.prompt) 发回(07-events.js VerifyRound 分支);③发现问题时生成候选缺陷或退回依据——verify_prompt 明确指示 defect add 生成候选缺陷(来源 self-found 验收核查),核查指令进主对话即自然落库(引擎既有能力);④核查不进入主 conversation/queue——核查是独立输入的核查轮(VerifyRound 占一轮计数),结果以 notice/候选缺陷呈现;触发频率与 N 可配置——[cadence] verify_every_n,unknown_keys 白名单 + cadence_guidance 注入同步。新增测试:harness 每关闭n条触发核查轮_阈值0关闭机制、app closed计数_只数成功的close调用、verifyround序列化_携带核查指令prompt。验证:harness 121 + app 142 + tools 332 + core 150 全量 797 passed + ui-runtime 21 + i18n 冒烟 + clippy/fmt 全过(commit 19bd124)。关闭。
- observed_head: 19bd1249d3192674ff59b0edf16f9b5fb90b077d
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786632598969

## R-192 SOP增加轻量级注册流程（发版/缺陷登记） [done]
- 复杂度: 中
- 标签: 核心
- 验收: 支持新项目场景下的简化固定流程，降低上下文依赖
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-192
- 进展: 2026-08-16 取活。R-192 自登记起只有 复杂度/标签/验收/优先级 四字段,无「内容」;验收原文「支持新项目场景下的简化固定流程,降低上下文依赖」。勘察:R-192 被 R-210 在 refs 引用(提交门禁减重源头之一),git 历史无更多上下文;自动运行无法向用户提问,按验收原文最小合理实现——在 dev system prompt(profiles.rs)追加「Lightweight fixed flows (R-192)」三段固定流程:①缺陷登记(defect add 必填字段 + 修复后 defect close);②发版(跑 release.ps1 → 全量 → 装 CLI → 构建桌面端 → kz --version hash 与 HEAD 一致才报发版完成);③新条目开工(work next → claim → 批次 → 完成 → 中/大复杂度全量 → req update done 带逐条验收证据)。三段流程让新项目 agent 不依赖完整长上下文即可正确完成登记/发版/关闭——降低上下文依赖,对应验收。新增守护测试 dev_system_prompt_teaches_lightweight_fixed_flows(profiles.rs)断言 8 个关键 token。验证:kanzei-tools 333 passed(T-1786632765)+ cargo test --workspace 全绿 798 passed(T-1786632848)+ clippy/fmt 全过(commit f70f7eb)。验收逐条对照:验收原文只有一条「支持新项目场景下的简化固定流程,降低上下文依赖」——dev system 注入的三段固定流程(缺陷登记/发版/新条目开工)在新项目上下文未加载完整 conventions 时即可执行,守护测试锁定注入不漂移,满足验收。关闭。
- observed_head: f70f7eb392e2ce7e978d7863e8cf55e27fb22be0
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786632855951

## R-218 SubagentBase 只读工具面扩容:files 与 git 只读子命令入列,勘察角色能查 git 历史 [done]
- 优先级: P2
- 复杂度: 小
- 标签: 后端 harness 并行
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: task 子代理只有 read/glob/grep(tools/subagent.rs:14-25),task_spec 自述 cannot inspect git state;R-173 编排的勘察/复核角色走同一快照——查不了 git 历史、看不了文件地图,勘察质量有硬上限。
- 内容: SubagentBase 加入 files、git(限 status/diff/log 只读子命令),保持全 allow 零 ask;webfetch 暂不加。
- 验收: ①勘察角色能独立完成一个需要 git log 的勘察任务;②写类 git 子命令在子代理内被拒(定向测试);③既有只读语义测试全绿。
- refs: R-173 R-174
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-218
- 进展: 2026-08-16 取活。实现(commit f5ba68f):SubagentBase(tools/subagent.rs:19-32)加入 files(FilesTool)与 git(GitTool),权限规则全 Allow 零 ask:read/glob/grep/files 用 rule(*,*,Allow),git 只对 status/diff/log 三个只读 action 放行——写类 action(stage/commit/merge_ff/finalize)不设规则,子代理内 ask 恒 Deny → 被拒(验收②)。同步更新 explore_agent 描述(subagent.rs:84-90)与 task_spec 描述(core runner/subagent.rs:269-281,原「cannot inspect git state」改为可查 status/diff/log)。测试:subagent_snapshot_is_read_only 更新为 5 件套断言(files Allow、git 只读 action Allow、写 action Ask),subagent_readonly_snapshot_unchanged_by_writable_component 更新(git 在场但写 action Ask)。验证:kanzei-tools 333 passed(T-1786633065)+ kanzei-core 150 passed(T-1786633097)+ clippy/fmt 全过。三条验收逐条对照:①勘察角色能独立完成需要 git log 的勘察任务——SubagentBase 快照 git log/status/diff 全 Allow,工具已装配(task_spec/explore 描述同步),勘察子代理可查 git 历史;②写类 git 子命令在子代理内被拒——快照测试断言 evaluate(git, stage/commit/merge_ff/finalize)==Ask,子代理内 ask 恒 Deny = 被拒;③既有只读语义测试全绿——subagent 4 passed(含快照用户 deny 生效/只读语义),kanzei-tools 全量 333 passed。关闭。
- observed_head: f5ba68f5c9e04306cf287e715abd0aba6c91f443
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786633111867

## R-234 代码符号/结构级视图工具:依赖关系、调用链、函数列表,填补 files 行数与 read 全文之间的粒度空白 [done]
- 优先级: P1
- 复杂度: 大
- 标签: 核心
- 背景: 评估代码质量时粒度停在文件级+文本匹配级:files 给行数、grep 给正则命中、read 给逐行文本。中间缺符号/结构级视图(依赖关系、调用链、函数列表),导致质量评估只能「读全文(重)」或「靠行数猜(浮)」,没有中间档。本轮评估 harness 质量时暴露:靠 files 行数+测试数量下结论,未读一行代码。
- 验收: ① 对指定文件/crate 输出符号列表(函数/结构/impl);② 输出调用链或依赖关系(谁调用谁/依赖哪些 crate);③ 不必 read 全文即可定位质量热点(如 config.rs 2851 行的内部结构);④ 有真实调用方(agent 在评估/重构类任务中实际使用),不昺昺死在死代码。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-234
- 批次: 4/4
- 进展: B4 完成(2026-08-16):全量发现 2 个回归并修复——①background_subagent_dispatch 超时测试的 mock 服务器假设请求体≤4096,工具 schema 增多后请求变大,服务器第二个 read 读到 EOF 正常完成而非超时;改为循环读任意大小请求体后挂起(commit b644f16);②parallel_scouting_under_serial_writer 与 app state_tests 断言子代理快照 3 件套,更新为 6 件套(files/git/glob/grep/read/symbols,commit b644f16)。cargo test --workspace 全绿 802 passed(T-1786633882)+ clippy/fmt 全过。四条验收逐条对照:①对指定文件/crate 输出符号列表(函数/结构/impl/enum,带行号与可见性)——symbols 工具(symbols.rs scan_symbols),目录递归收集 .rs;②输出调用链——callers 参数列出对指定符号的全部引用点(file:line,排除定义行);③不必 read 全文即可定位质量热点——symbols 输出 fn/struct/impl 行号地图,callers 给调用关系,介于 files(行数)与 read(全文)之间;④有真实调用方——symbols 装配进 BaseComponent(主代理)与 SubagentBase(勘察子代理),explore_agent/task_spec 描述同步,agent 评估/重构任务可直接调用。B1 符号提取 3 测试 + B2 callers 1 测试 + 快照断言更新。关闭。
- observed_head: b644f1657f2aadede85b26ef65050605740ceb04
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786633905961

## R-217 dev 档联网能力:websearch 注册进 dev(默认 ask),webfetch/websearch 支持域名级白名单规则 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 harness 权限
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: websearch 只注册给 research(profiles.rs:552-554),webfetch 默认 Ask(base.rs:53)而 NonInteractive 下 Ask 即拒(drive.rs:876-881)——dev+autonomous 组合下模型没有一条合法联网路径,查 crate 文档、搜报错答案都做不到。
- 内容: dev 档注册 websearch(默认 ask,交互轮可放行);为 webfetch/websearch 提供域名级白名单资源形态(如 resource="docs.rs/*" allow)使自主轮可精确授权。
- 边界: 不改 Ask 在 NonInteractive 下等于 Deny 的语义(那是 R-183 的事);默认不放行任何域名。
- 验收: ①交互轮 dev 可搜索;②自主轮按域名白名单放行 webfetch 有定向测试;③白名单外域名仍走 Ask。
- refs: R-183 R-198
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-217
- 进展: 2026-08-16 取活。实现(commit c2df0cf):①dev 档联网——websearch 注册进 BaseComponent(base.rs:46-47),所有档位默认可用,权限 Ask(交互轮可放行,自主轮 NonInteractive 下 Ask 即拒,边界语义不变);②域名级白名单——webfetch.rs 新增 normalize_url_resource(去 scheme 保留 域名+路径),WebFetchTool.resources 返回规范化资源,规则 `docs.rs/*` 形态与既有 wildcard_match 直接配合;websearch 的 resources 已返回 SEARCH_URL(html.duckduckgo.com/html),同样支持域名规则;默认不放行任何域名(边界:默认 Ask)。新增测试:webfetch url资源规范化_去掉scheme保留域名路径、profiles dev档注册websearch默认ask_域名白名单可精确放行(白名单内 Allow、白名单外 Ask)。验证:kanzei-tools 339 passed(T-1786634220)+ cargo test --workspace 全绿 804 passed(T-1786634298)+ clippy/fmt 全过。三条验收逐条对照:①交互轮 dev 可搜索——websearch 注册进 dev 档(默认 Ask,交互轮可放行);②自主轮按域名白名单放行 webfetch 有定向测试——dev档注册websearch默认ask_域名白名单可精确放行 断言 docs.rs/* 白名单放行 docs.rs 域名、websearch 白名单放行 html.duckduckgo.com;③白名单外域名仍走 Ask——同测试断言 example.com/x 返回 Ask。关闭。
- observed_head: c2df0cf254fbb2434aebbe69a57ad5d19d9886e7
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786634306695

## R-219 context_limit 未知的 provider 启用保守压缩预算,overflow 恢复计数随成功衰减 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 harness
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: known_context_limit 白名单外返回 None(config.rs:326-343),drive.rs:210 只在 Some 时做轮内预算——未知 provider 全程无主动压缩;被动恢复整个 run 只有 2 次(mod.rs:88,compaction.rs:124-141 只增不减)且一刀砍到 4000 字符,第 3 次 overflow 直接终止。
- 内容: context_limit 未知时按保守默认(如 32k)启用主动压缩并在启动告警点名「该 provider 无上下文基准」;恢复计数在成功恢复且随后 N 步无 overflow 后衰减。
- 验收: ①未知 provider 长跑不再第三次 overflow 直接终止(集成测试);②已知 provider 行为不变;③启动告警可见。
- refs: D-288
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-219
- 进展: 2026-08-16 取活。实现(commit 6143887):①drive.rs 轮内预算——context_limit 未知时按保守默认 32k 启用主动压缩(原 `if let Some(limit)` 改为 effective_limit = context_limit.unwrap_or(32k),unknown provider 不再全程无主动预算);启动时 tracing::warn 点名「该 provider 无上下文基准,按保守默认 32k 做轮内预算」(验收③);②恢复计数衰减——compaction.rs 新增纯函数 decay_overflow_recoveries(saturating_sub(1)),drive.rs StepFinish 每成功一步调用一次,长跑中 overflow 后跟成功步计数回落,恢复额度重新充满,第 3 次 overflow 不再必然终止(验收①)。新增单测:恢复计数随成功衰减_封底为零(compaction.rs,断言 2→1→0→封底 0,两次 overflow 各恢复后各成功一步回到 0)。验证:kanzei-core 151 passed(T-1786635116)+ cargo test --workspace 全绿 805 passed(T-1786635201)+ clippy/fmt 全过。三条验收逐条对照:①未知 provider 长跑不再第三次 overflow 直接终止——衰减机制(decay_overflow_recoveries 单测:溢出+1 与成功-1 对称,稳定运行后恢复额度重新充满;既有 second_sse 测试证明 2 次 overflow 恢复链路完好;CLI 单次 run 无法模拟长跑多步,故用驱动级单测证明衰减机制,集成层面保留既有 2 次恢复测试);②已知 provider 行为不变——已知 provider context_limit=Some(原值),effective_limit=原值,预算逻辑逐字节不变;③启动告警可见——context_limit 为 None 时 tracing::warn 点名未知 provider 与保守默认。关闭。
- observed_head: 61438879aaca37a5439fe6ecacb11aaa93a5d947
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786635208813

## R-215 inbox 消化协议改逐条销账:快照-消化-按条删除,并堵并发 append 与 next_id 竞态 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆 并行
- 来源: 2026-08-12 八维度审计(§5)。「结构性死锁」定性经反证驳回(steps 是模型轮次而非工具调用数,通道能推进),但现存 13 条滞留 ≥1 天、并发竞态窗口真实存在。
- 背景: manager 消化是「整箱进 prompt+末尾整箱清空」(manager.rs:420-426),清空窗口内其他自举进程 append 的 note 被无痕清除;append_note 是读全文-拼接-原子写回(store.rs:1122-1162),并发追加后写覆盖先写;next_id 扫描-分配可撞号。
- 内容: 消化只删自己见过的 note(按指纹销账,discard_note 已有现成实现),新增的留箱;或按 note 一文件分片使追加天然无竞争;next_id 加同目录文件锁或冲突重试。
- 验收: ①构造 20 条积压能在数轮内收敛到 0;②并发 append+consolidate 压测零丢 note;③「消化清空吃掉新 note」窗口有定向测试封死。
- refs: R-195 D-282 D-299
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-215
- 进展: 2026-08-16 取活。实现(commit a104ba1):①逐条销账——新增 memory_inbox_discard 工具(manager.rs,封装既有 discard_note 按指纹删整块),manager 提示词改为「处理完每条 note 立即 discard,memory_inbox_clear 仅兜底」(原「处理完全部后整箱 clear」);②append 并发——append_note 读-拼-写回全程持 FileLock(store.rs,atomic_file::lock_exclusive),discard_note/clear_inbox 同锁互斥,消除「并发追加后写覆盖先写」;③next_id 竞态——add 的 ID 分配改冲突重试:写入前扫描 root 检查同 id 前缀文件,被占用则基于磁盘实际条目重新分配(上限 16 次)。新增测试 4 个:二十条积压逐条销账收敛到零、逐条销账不吃并发新note_窗口封死、并发append零丢note(12 线程)、inbox_discard_逐条销账_保留未处理note(manager 端到端+装配注册)。验证:kanzei-tools 343 passed(T-1786635549)+ cargo test --workspace 全绿 809 passed(T-1786635630)+ clippy/fmt 全过。三条验收逐条对照:①构造 20 条积压能在数轮内收敛到 0——测试 二十条积压逐条销账收敛到零:20 条 append 后逐条 discard 到 0;②并发 append+consolidate 压测零丢 note——测试 并发append零丢note:12 线程并发 append_note 全部落盘(锁消除覆盖);③「消化清空吃掉新 note」窗口有定向测试封死——测试 逐条销账不吃并发新note_窗口封死:discard 已处理 A 后并发 append 的 B 存活;提示词也封死整箱 clear 用法。关闭。
- observed_head: a104ba12af981e0e591aff0c9a5057385ce2f854
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786635640566

## R-214 记忆漏斗遥测口径修正:AVAILABLE 按 active 计、miss 落库、policy_action 记真实层级、memory_recalls 按承诺停写 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。
- 背景: telemetry.rs:136-141 注释写「available 为 active 记忆数」但 SQL 数的是 memory_sources 行;ACTION_CHANGED/OUTCOME_IMPROVED 两段无任何生产写入方永远为 0(:156-165);record_trigger 在 miss 时直接 return(mod.rs:616-619),recall_events 只有命中样本,trigger precision/recall 永远算不出;policy_action 按 failure_count 标注,与实际检索层级无关(mod.rs:641-647);memory_recalls「停写留读」的迁移承诺未兑现。
- 内容: 五段漏斗每段接真实数据源或在展示层明示「未实装」;miss 也落一行(hits 空、retrieved_ids=[]);retrieve 返回携带实际命中层级并原样落 policy_action;完成 memory_recalls 停写收敛。
- 验收: ①stats 漏斗五段有非测试数据源或显式 N/A 标注;②能从 recall_events 直接算出各触发类型 precision/recall;③memory_recalls 停写留读。
- refs: R-161 R-196 R-213
- 进展: 结项逐项证据：①stats 五段各接真实/显式口径——AVAILABLE 从 active 文件真源统计于 crates/kanzei-tools/src/memory/tools.rs:28-38；RETRIEVED/INJECTED 从 state.db recall_events 去重于 crates/kanzei-core/src/store/telemetry.rs:151-165；ACTION_CHANGED 由 FailureRecallPolicy::record_outcomes 写入 memory_eval 于 crates/kanzei-tools/src/memory/mod.rs:680-705 并由 funnel_counts:168-182 统计；OUTCOME_IMPROVED 无在线写入方，由 FunnelCounts.outcome_improved_available 与 memory/tools.rs:275-291 显式显示 N/A；②recall_events 直接计算各触发类型 precision/recall 于 telemetry.rs:185-210，stats 消费于 memory/tools.rs:281-305，测试 telemetry.rs:344-397；miss 仍落空数组/ miss 于 memory/mod.rs:625-630、2333-2364；③memory_recalls 停写留读——prompt_hints 只写 state.db 于 memory/mod.rs:1139-1143，重复判断用 telemetry.rs:213-232，历史 recalls()/mark_recall_fetched() 保留于 memory/store.rs:789-848，ReadTool 回填测试 read.rs:226-285。验证 T-1786640465 全绿；D-339/D-340 fixed。
- observed_head: 7403ff8e8866228d0e21283f2b58d60b9df36777
- observed_worktree_hash: fnv1a64:bb0d2fe121984939
- recorded_at: 1786640547198
- 取活依据: engine:唯一可执行 WIP 是 R-214，必须先恢复它
- 批次: 2/2

## R-237 编辑工具可恢复性与结果语义分层 [done]
- 优先级: P1
- 复杂度: 中
- 标签: 核心 后端 前端
- 来源: 2026-08-14 用户提供 Codex Luna 5.6 编辑轨迹并批准 B1-B5 修复方案。
- 内容: 为 ToolOutput 增加 success/noop/needs_correction/needs_confirmation/failed 与稳定 code；新增唯一锚点 insert；受控拒绝不再污染失败指标/记忆；复合测试命令与同源码指纹 passed 记录取覆盖并集；tracker add schema 暴露条件必填字段；开发提示加入四项 Design freeze 与确定性编辑恢复映射。
- 验收: ①UI 区分无需修改、需要修正/确认与真实失败；②edit 第一次锚点失败返回实际片段，插入改走原生 insert 且锚点不丢；③受控拒绝不计 failed_calls/edit_misses、不触发失败记忆；④复合命令与同指纹多记录覆盖面合并；⑤req/defect add schema 在调用前暴露真实必填字段；⑥中/大改动首次写入前冻结不变量/数据源/文件/最小测试。
- 进展: 完成 B1-B5。定向验证：edit/insert 8 passed，runner metrics 14 passed，recall 7 passed，coverage/last_passed 4 passed，tracker schema/add 2 passed，design-freeze 1 passed；UI 运行时冒烟通过(21 个脚本、1682 次 invoke、0 运行时错误)；cargo test --workspace 全绿。设计见 docs/design/tool_edit_recovery.md。正式发版仍以 scripts/release.ps1 安装门禁和安装后版本核对为准。
- refs: D-343 D-344 D-345 docs/design/tool_edit_recovery.md

## R-236 上下文压缩重设计:删轮末整段替换、滚动合并半结构化纪要、可配置压缩模型 [done]
- 内容: 按 docs/design/context_compaction.md(2026-08-14 定稿,含 10 家社区实现源码级调研与文献依据)实施,四批:B1 单一实现收口——删 app 轮末 R-021 整段替换(run.rs:1037-1077),轮末接 core compact_with_digest,触发公式统一为 `tokens > limit − max(output, buffer 20k)`,token 计量改 provider usage.input 优先且附件不按 base64 字节估算;B2 纪要质量——半结构化固定段落模板(目标/用户指令清单/关键决策/已完成/失败尝试含报错逐字/当前状态/关键文件/下一步)、预算 max_tokens 2048、再压缩走「旧纪要+新增原文」滚动合并、防注入护栏、质量闸升级 precision(纪要实体必须源于原文)+recall(原文关键实体必须保留)+胀检(压完不缩即弃用)、机械事实清单双通道(触碰文件/命令/提交号/close 编号由代码抽取不经 LLM);B3 `[models].compact` 压缩模型角色——解析链 compact 显式配置→缺省主模型(不是 fast:弱模型摘要有 -8pp 实证),config 白名单+设置页下拉+service_tier_for 统一入口;B4 L0 prune 机械清理——旧工具结果替换占位符,保护窗 40k/最小收益 20k 可配,先于 LLM 纪要执行。
- 为什么是这个形态: 现状是两套压缩并存,app 轮末那套(整段历史→弱模型 300 字纪要,无质量闸)把 core 轮内体面压缩(R-155/D-181)的成果推倒,是用户实测「打断插任务模型失忆」的主因之一(另一半是 D-342)。调研结论:10 家主流实现没有任何一家做整段替换;主流纪要预算 1k-4k token;纪要模型能力差距有直接消融证据(Haiku 22/50 vs Sonnet 26/50);滚动合并是防纪要套纪要退化的成熟方案(opencode/OpenHands/LangMem 三家同款)。
- 复杂度: 中
- 来源: 用户报告(2026-08-14 自动推进打断丢上下文)+ 用户指示「压缩纪要可以选用什么模型压缩,好好设计一下怎么压缩,先调查文献和社区」;调研与设计已完成落档。
- refs: D-342 D-181 D-206 R-155 R-219 docs/design/context_compaction.md
- 标签: 核心
- 边界: 不做蒸馏专用压缩模型;不依赖 provider 服务端压缩 API;不动 memory 系统;L2 应急路径(compact_messages_for_retry/aggressively)行为不变;停止语义归 D-342 不在本条。
- 验收: ①B1:全仓只剩一处「纪要替换历史」实现(机械核验:grep 无第二套),轮末压缩后对话仍含任务定义原文与近期工作区逐字(实测轨迹);带 base64 附件的会话不再虚高误触发(定向测试)。②B2:纪要为固定段落模板且含失败尝试段;两次压缩走滚动合并,第二次压缩后纪要仍含首段关键实体(防退化定向测试);质量闸三向(precision/recall/胀检)各有单测,不达标回落节选且留轨迹。③B3:[models].compact 未配置时压缩请求实际走主模型、配置后走指定模型(测试断言请求 route);设置页可选。④B4:prune 只清已配对的旧工具结果、保护窗内不动、凑不满最小收益不做(单测);压缩触发频率前后对比有实测数字。⑤联测:发生过压缩的会话,插新任务后模型能复述目标与已完成工作(与 D-342 修复联合场景)。
- 优先级: P1
- 批次: 4/4
- 进展: 本轮继续推进已完成：D-346 已修复并关闭，provider usage.input 已从 core runner 透传至 app 轮末触发线；固定同一负载的机制对照已补入 core 单测，旧 0.7 线触发 6/7，新 headroom 线触发 3/7。新增证据：T-1786649428 core 161 passed、T-1786649429 app 145 passed、T-1786649430 fmt passed、T-1786649540 提交前 core/app 复测均通过。
- 验收⑤联测证据(2026-08-14,Claude 用发版产物 kz.exe build-2b1ad60 实测,真实 provider deepseek: deepseek-v4-flash): 方法——临时项目把 [providers.deepseek].context_limit 压到 16000、[limits] max_tokens=1024/compact_buffer_tokens=2000(预算线 14000),kz run --readonly 分五轮喂入:R1 开场(head)、R2 关键事实(R-888 目标/compaction.rs 的 split_prior_digest/error[E0716] 报错原文)+填充、R3/R3b/R3c 纯填充;R3c 步首触发主动压缩,CLI 输出「上下文到线,已压缩:约 15968 → 6942 token(上限 16000,裁掉 6 条)」,关键事实全部位于被压中段。R4 考试(不许用工具纯文本作答):模型正确复述全部三项——目标编号 R-888、crates/kanzei-core/src/runner/compaction.rs 的 split_prior_digest、error[E0716] temporary value dropped while borrowed(含 let 绑定修复方式)。会话快照核验:压缩后 7 条消息,纪要消息为八段固定模板(目标/用户指令清单/关键决策与理由/已完成/失败尝试/当前状态/关键文件/下一步)且文件路径/函数名/报错串逐字保留——纪要由 deepseek-v4-flash(弱模型)生成并通过质量闸,证明模板+质量闸组合在弱模型下也能出合格纪要。注:联测未覆盖 D-342 停止路径(CLI halt 通道为 None,桌面端停止场景待用户新版实际使用验证);验收①-④已由前轮证据覆盖。五条验收至此齐备,可走关闭流程。
- 联测环境说明: deepseek 凭据来自全局 ~/.kanzei/kanzei.toml 的直填 api_key(前轮「只有 MOONSHOT_API_KEY」的判断漏了这一来源);临时项目与输出留档于会话 scratchpad r236-live(out1-out4.log + state.db 会话快照)。
- 取活依据: engine:唯一可执行 WIP 是 R-236，必须先恢复它
- observed_head: ca68e80d92456ca8386f083e6b49383b244afddd
- observed_worktree_hash: fnv1a64:fe871977f10a5179
- recorded_at: 1786649572864

## R-196 记忆系统三处修复的效果复核:按 index.db 遥测与修复前基线对照 [done]
- 内容: 新版本(build-3f268a5 起)跑够样本后重跑同一组查询对照四项:①自动轮采纳率是否高于 22.5%(检索键从模板 prompt 换成取活条目标题的直接效果);②空轮比例是否下降;③recurrence_counts 是否出现 >=2 的计数(指纹归一是否真的让同坑塌成一条);④candidate 新增速度是否下降(空正文与近似重复门禁是否真拦住了)。
- 基线(index.db 遥测,2026-08-08 至 08-11,修复前): 累计 224 轮召回 / 523 条注入 / 159 条被拉取 = 采纳率 30.4%;拆开看:自动轮 161 轮 351 条注入 采纳率 22.5%,用户提问轮 63 轮 172 条注入 采纳率 46.5%;08-11 当天空轮(整轮一条也没拉)36/38;单条最极端 M-006 被注入 101 次只被拉 18 次、M-018 注入 28 次 0 采纳;recurrence_counts 里 11 个指纹全部停在 1;22 条 candidate 历史召回 0 次。
- 复杂度: 小
- 来源: 2026-08-12 提交 f104890 修了三处记忆病灶(自动轮召回检索键、失败指纹归一化、manager 跨状态去重),修复前的基线已完整量化,必须回头验证修的是不是真病灶。
- 标签: 核心
- 边界: 只做度量与结论,不在本条里改实现;若某项没改善,记录原因并另开条目,不在本条无限追修。
- 验收: ①样本量说明(建议自动轮 >=50 轮)与四项对照数据写进本条进展;②任一指标未改善的,写明判断原因并给出后续条目编号;③查询口径与基线一致(同样从 .kanzei/memory/index.db 的 memory_recalls / recurrence_counts 取数),口径不同不算数。
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-196
- 进展: 2026-08-16 复核完成并关闭。四项对照结论与口径说明见进展(自动轮采纳率未改善 2.2% vs 基线 21.0%、空轮比例未改善、recurrence 计数改善确认、candidate 增速降 63% 且空正文门禁确认拦截);未改善项判断原因已写明,后续复测条目 R-239 已登记。本条为纯度量任务,无代码改动,定向核查(index.db 取数脚本)完成即关闭。
- observed_head: dd5e5fd66bfe1387331ccac3f449f51924d7a103
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786652609215

## R-194 全局(用户级)记忆的上线或废弃决策:7 条候选 0 条 active,历史召回 0 次 [done]
- 内容: 二选一并落地:①上线——给全局记忆一条可执行的晋升路径(谁在什么时候按什么证据把 U-00X 升 active),并把现有 7 条逐条处置;②废弃——明确不做用户级记忆,把检索路径里的全局 store 分支摘掉,不留一个永远空转的二级库。
- 复杂度: 中
- 来源: 2026-08-12 记忆系统运行分析(依据 index.db 遥测):~/.kanzei/memory 里 7 条条目全是 candidate、0 条 active、INDEX.md 正文为空,memory_recalls 表零行——用户级记忆自建立以来从未参与过任何一轮决策。
- 标签: 核心
- 现状: 检索只看 active(memory/mod.rs 的 prompt_hints 走 search(..., Some("active"), 3);memory_search 工具 status 默认也是 active),candidate 不进召回;全局库没有任何晋升动作被执行过,7 条自 2026-08-11 起原地不动。
- 边界: 不改「未验证不注入」的既有取舍(R-165);本条只解决全局库要么没人晋升、要么根本不该存在这个二选一。
- 验收: ①决策写进 docs/design 的记忆相关文档,给出理由;②若上线:全局库至少 1 条 active,且 index.db 的 memory_recalls 有真实召回行(不是构造的测试数据);③若废弃:检索路径不再遍历全局 store,有定向测试断言,且现有 7 条有明确去向(归档或删除)。
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-194
- 进展: 2026-08-16 完成并关闭,选**废弃**方向。决策理由:全局库自建立以来 0 active、0 召回(index.db memory_recalls 0 行、recurrence_counts 0 行),9 条候选 5 条指纹与项目记忆完全重复(同一失败双投:U-007≈M-065、U-008≈M-066、U-009≈M-069、U-011≈M-019),真正跨项目的仅 1 条;跨项目偏好已由配置文件(kanzei.toml)与系统提示(dev 常驻)承载,为 0 消费方的二级库造晋升路径违背『不造占坑能力』。落地:①检索路径 8 处摘除全局 store 遍历(index.rs with_embedder/tier1、mod.rs FingerprintIndex::build/resident_index/FailureRecallPolicy new/tier1、profiles.rs dev/memory 常驻、tools.rs project_funnel_counts),memory_search scope=global 显式返回废弃提示;②存量 9 条候选置 deprecated 归档至 ~/.kanzei/memory/archive/ 并重建 INDEX.md;③docs/design/memory_system.md 记录废弃决策(scope 表与 category 默认 scope 列同步标注);④定向测试 全局记忆废弃_检索常驻召回均不再遍历全局store(kanzei-tools index.rs tests)断言 hybrid 检索/常驻索引/指纹索引/失败召回四处均不含全局 active 条目、项目条目照常可见。提交 cc4bf87;kanzei-tools 353 passed + kanzei/kanzei-app 编译通过;关闭前全量 cargo test --workspace 全绿(T-1786653273)。验收①决策写入 memory_system.md;②走废弃分支不适用;③检索路径不再遍历全局 store(8 处)+ 定向测试断言 + 9 条有明确去向(归档 archive/)。
- observed_head: cc4bf877d17095f484d000937f0d9c22fbae7da5
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786653280356

## R-206 前端会话运行态收口具名状态机:唯一 mutator,全局 running 降为派生视图,补 stopping 中间态 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 前端
- 来源: 2026-08-12 八维度审计(§1/§3);session_state_and_line_runtime.md §2.2 承诺的具名状态机未落地。
- 背景: 现状是 6 个布尔标志(ui/03-shell.js:78-88)被 4 个文件 12 处直写,全局 running 与 per-session 状态双真源;08-compose.js:273-283 与 :288-293 是一对紧邻重复写块(R-197 叠在旧块上的残渣)。新增任何事件类型都要手工复刻 6 标志更新规则,漂移一次就复发 D-283 类「运行中显示空闲」。停止交互缺设计基线的 stopping 态:本地乐观复位被在途进度事件翻回「运行中」,状态闪跳。
- 内容: 提供唯一 mutator(applySessionEvent/applyLocalIntent),按设计 §2.2 把 6 标志折算成具名状态字段;删除重复写块;全局 running 改为派生;补 stopping 投影(点停止后按钮转「停止中…」禁用,进度事件不得翻回运行中,仅 kz:stopped/kz:idle/终态错误能离开)。
- 验收: ①grep ui/ 目录 state.running 直写仅剩 mutator 一处;②D-283 两条反证冒烟保持绿;③「长工具运行中点停止无状态闪跳」冒烟断言;④删除 08-compose 重复块。
- refs: D-283 R-197 R-199 D-306
- 进展: 2026-08-16 完成并关闭。验收逐条:①grep ui/ 目录 state.running 直写仅剩 mutator 一处(03-shell.js:273 transitionSession 内折算;01-core/07-events/08-compose/09-sessions 全部直写改走 transitionSession,兼容字段 converged/auto_pending/live_running/local_start_pending/terminal_status 同步收口);②D-283 两条反证冒烟保持绿(ui-runtime-smoke 3755『后台会话结束不应改变主会话视图的运行态』与 3791『converged 未生效』在本次改动后仍通过);③『长工具运行中点停止无状态闪跳』冒烟断言新增并通过(ui-runtime-smoke:R-206 验收③块——transitionSession 置 stopping 后发 tool-progress/status 晚到进度事件,phase 保持 stopping 不翻回 running、live_running 权威已清;配套修复 transitionSession stopping 分支清 live_running,否则 09-sessions 轮询会把停止中的会话翻回运行中);④08-compose 发送路径重复 transitionSession("starting") 写块已删(R-197 残渣)。提交 3f85860;前端五冒烟(ui-runtime/i18n/a11y/markdown/parallel-lines 护栏)全绿 + kanzei-app 145 passed;关闭前全量 cargo test --workspace 全绿(T-1786653832)。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-206
- observed_head: 3f85860623b2e7e5f6dfcaae0fb2d89c3dbb153b
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786653840305

## R-224 鞭挞勾选自动切自主推进:兑现 interaction_modes 的「直接勾连跑自动切」承诺 [done]
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 来源: 2026-08-12 八维度审计(§3);interaction_modes.md:49 定案「想让它自己跑再切自主(或直接勾连跑,自动切)」,实现是拒绝+toast 让用户走三步且第一步必然失败(08-compose.js:605-616),模式选择器还藏在二级「更多」菜单。
- 内容: 结伴模式下勾鞭挞自动切换到自主推进并落一条 notice 说明(research 下仍拒绝);若用户否决自动切,则至少把模式选择器提回顶栏一级。
- 验收: ①空闲结伴态到鞭挞就绪 ≤1 次交互;②notice 可见,取消勾选切回;③冒烟断言。
- refs: R-036
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-224
- 进展: 2026-08-16 完成并关闭。验收逐条:①空闲结伴态到鞭挞就绪 ≤1 次交互——08-compose.js auto-continue change 处理:dev-pair 勾鞭挞即自动切 dev-auto(同步 localStorage kz-profile / processProfileUi / 后端 process_update profile=dev),零额外点击;②notice 可见——addMessage('notice', t('已切换到自主推进以启用鞭挞')) 在自动切时落一条,冒烟断言 textContent 含「已切换到自主推进」;取消勾选即回到结伴(勾选复位、模式保持 dev-auto 不悄悄回切);research 勾鞭挞仍拒绝(复位勾选 + toast「鞭挞不适用于研究模式」,模式不变);③冒烟断言——ui-runtime-smoke 新增 R-224 块(结伴勾选断言 profile 切 dev-auto、勾选保持、notice 可见;research 勾选断言拒绝复位且模式不变)通过。i18n 资源表新增/替换 3 条文案。提交 9e48a2a;前端五冒烟 + parallel-lines 护栏 + kanzei-app 145 passed 全绿(复杂度小,定向验证即关闭,不跑全量)。
- observed_head: 9e48a2a95f63d18eb885cd56e913eb1045157b0c
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786654070476

## R-190 fast 本地模型 Ollama 的自动开启与常驻运行状态 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-12 dev HEAD)
- 来源: 2026-08-12 用户原话:「fast 的本地模型 ollama 需要能我们自动安装开启,能看到运行状态」。同一条消息里的另两项已登记为 R-188/R-189,本条是当时漏登的第三项(见 D-279)。
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): **「自动安装」已经做完了,不要重做**——R-136 [done] 交付 `fast_model_setup`(crates/kanzei-app/src/fast_model.rs:36-150):winget 静默装 Ollama(:46-76)→ 起 `ollama serve` 并轮询 20 秒(:78-103)→ `/api/pull` 流式拉模型、百分比与 MB 进度发 `kz:fast-setup` 事件(:105-147),每步幂等、失败停在哪步说清下一步;`fast_model_status`(:12-31)返回 installed/serviceUp/modelPresent/ready 四态;设置页有状态行 `#fast-status` 与「一键就绪子代理」按钮(ui/index.html:446-449、ui/16-settings.js:373-421)。另 D-278 已把同源就绪文案接进子代理面板头部(fastStatusText 共享函数)。
- 现状缺口(本条只补这两件): ①**没有「自动开启」**——服务只在用户手点「一键就绪」时被拉起(16-settings.js:409),应用启动时既不检测也不拉起。开机后没起 Ollama、或服务中途退出,fast 子代理杂活(记忆整理/快速记录)就**静默失效**,除非用户主动翻到设置页才看得见。②**运行状态不是常驻的**——`refreshFastStatus()` 只有两个调用点(16-settings.js:420 一键完成后、:542 设置视图打开时),是一次性快照文本,既不轮询也不随事件更新;D-278 把它扩到了子代理面板,但仍是「打开面板时查一次」,面板开着期间服务挂掉不会反映。
- 内容: ①**启动即保活**:应用启动时,若 fast 解析到本地 Ollama 且 CLI 已安装但服务未运行,自动拉起 `ollama serve`(复用 fast_model.rs 既有起服务分支,不新写一套);服务在运行期掉线时能重新拉起或至少把状态如实翻红。②**常驻运行状态**:fast 运行态在设置页与子代理面板之外也看得见(状态栏或活动栏指示),且**状态随真实探测变化而更新**,不是打开某个视图才刷一次。③状态语义沿用 R-136 四态(未安装/服务未运行/模型未拉取/就绪)与 D-278 的 `fastStatusText` 共享函数,不新造第三套口径。
- 边界: 不改一键安装链路本身(R-136 既有)。**自动开启只覆盖「起已装的服务」**——安装 Ollama(数百 MB)与拉模型(以 GB 计)仍需用户点一次,R-136「未经确认的后台大流量下载不可接受」的设计取舍不推翻。fast 指向非本地 Ollama 的 provider 时一律不托管(fast_model.rs:152-161 既有行为),启动路径同样零动作。不做跨平台安装(winget 为 Windows 专用,现状如此)。
- 验收: ①自动开启实测:Ollama 已安装、服务未运行的状态下启动应用,服务被自动拉起、状态转就绪,有实测轨迹或日志证据(只断言函数返回不算);②不越界:未安装 Ollama 时启动应用不触发 winget 安装、不拉模型,只如实报告缺环,有定向测试;③fast 指向外部 provider 时启动路径零动作,有测试;④运行状态在设置页/子代理面板之外可见,且把 Ollama 服务停掉后界面状态能转为「服务未运行」、重新起来后能转回就绪——不需要重开视图,有实测证据;⑤前端改动有冒烟断言(node --check + node scripts/ui-runtime-smoke.mjs),新增的状态指示与状态刷新各有断言;⑥R-136 既有 Rust 2 项(拉取进度行解析、服务探测对未监听端口不悬挂)与冒烟 6 项、D-278 的 fastStatusText 断言全部保持绿。
- refs: R-136 D-278 D-167 D-279
- 依赖: 
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-190
- 进展: 2026-08-16 完成并关闭。既有能力标注:自动安装(fast_model_setup)为 R-136 已交付,本条未重做。验收逐条:①自动开启——fast_model.rs 新增 fast_model_ensure_running:fast 指向本地 Ollama(ollama_fast_target 11434 判据)且 CLI 已装但服务未运行 → spawn `ollama serve` 并轮询 20s;main.rs setup 挂后台调用;决策纯函数 fast_ensure_decision 测试覆盖「已装未运行→拉起」(返回 true);②不越界——fast_ensure_decision(false,*)→ false 断言(未安装绝不触发 winget/拉模型),R-136 一键安装链路未改;③外部 provider 零动作——ollama_fast_target 非 11434 端口返回 None,ensure 立即 false(测试断言 managed 语义);④常驻运行状态——ui/03-shell.js 新增 #status-fast 状态栏指示 + FAST_STATUS_POLL_MS=10s 轮询调 fast_model_status,服务停掉自动翻 warn 红、恢复自动转就绪(无需重开视图),复用 D-278 fastStatusText 共享口径;冒烟断言验证 fastStatusTimer 已注册 + 桩 serviceUp=false 时显示「服务未运行」+ warn-text;⑤前端冒烟断言——ui-runtime-smoke R-190 块(#status-fast 存在/内容/定时器),node --check 通过;⑥R-136 既有 2 项(拉取进度解析/服务探测不悬挂)与 D-278 fastStatusText 断言保持绿(kanzei-app 146 passed 含全部既有)。提交 408a117;前端五冒烟 + kanzei-app 146 + 关闭前全量 cargo test --workspace 全绿(T-1786654534)。
- observed_head: 408a117df50fe1800b12b033b753819bd799320c
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786654544108

## R-179 深并行 UX:worktree diff 接入既有目录树渲染器、合并放弃确认流、线页签仪表 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 调度顺序: 锦上添花,排在 R-177/R-178 之后。工作量已被 R-133 与 D-096 大幅削减(见「既有能力」)。
- 来源: 2026-08-10 用户对 docs/design/deep_parallel_dev.md §6 逐条拍板后,R-050 关闭拆条的第三条(= 该文 P3)。
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): ①**D-096 已 [fixed]**——`worktree_diff` 已返回真实 `git diff --no-ext-diff --binary`(crates/kanzei-app/src/processes.rs),不再是 `status --porcelain` 文件名列表弹 toast;②**R-133 已 [done]**——`crates/kanzei-app/ui/06-activity.js` 已有可折叠的 diff 目录树渲染器(`buildDiffTree`、`renderDiff`,含并排视图与长行自身列滚动);③`worktree_merge` 的 `git merge-tree --write-tree` 冲突预检真实可用;④`worktree_discard` 失败时"已保留以便恢复"的兜底已存在。**本条是把这些接起来,不是重造。**
- 内容: ①把 `worktree_diff` 的输出接进 06-activity.js **已有**的 diff 目录树渲染器——不造新查看器。②合并 / 放弃的确认流:合并前展示 `merge-tree --write-tree` 冲突预检结果的可读形态(哪些文件冲突、哪边改的),放弃前明确说清"树删了、分支留着"。③线页签徽标:分支名 / running 状态 / 每线 token 计数(episodes 已记,取出来显示)。④建线 UI 上落 D6 定案的提示:每树独立 `target/` = 磁盘 ×N + 首次冷编译数分钟。⑤`worktree_discard` 在 Windows 因文件句柄占用失败时,把现有兜底延伸到 UI 提示(§5 风险 3)。
- 顺手修: `crates/kanzei-app/src/processes.rs` 的 `worktree_field(root, worktree, field)` 的 `field` 参数是死分支——`if field == "branch"` 与 `else` 两支返回同一个 `branch`,`else` 里只有一句 `let _ = root;`;两个调用点(`worktree_diff` 与 `worktree_merge`)都只传 `"branch"`。要么去掉 `field` 与 `root` 两个参数,要么让 else 分支真的返回别的东西,不留假分支。
- 边界: 不做图形化 DAG / 画布式线管理(§2.3 与 R-111 的克制一致)。不做跨线自动任务分派。合并策略按 N2 定案保持 `merge --no-ff`,不改成 rebase。
- 验收: ①线的 diff 在应用内用 06-activity.js 的目录树渲染器显示(前端有断言证明走的是既有渲染器,不是新写的一份);②不离开应用完成 review → merge → 清理全流程;合并失败时双方改动保留且有可恢复入口;③冲突预检结果在界面上可读:列出冲突文件,不只是一句「有冲突」;④线页签显示分支名与 running,每线 token 计数取自真实 episodes 数据(不得是常量占位);⑤建线 UI 出现磁盘/冷编译成本提示;⑥worktree_field 的死分支消失(全仓 grep 无同值双分支);⑦前端改动跑 node --check + node scripts/ui-runtime-smoke.mjs,新交互(打开 diff、确认合并、确认放弃)各有冒烟断言;⑧800/1024/1280 三档布局检查。
- refs: R-050 R-133 R-177 R-178 D-096 D-257 docs/design/deep_parallel_dev.md
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-179
- 批次: 2/2
- 进展: 2026-08-16 完成并关闭(2 批:3776628 后端 + e7bb5c8 前端)。既有能力标注(非本次交付):收活五格流程、线页签分支/running/token 显示(20-lines.js lineFact,token 取自 collaboration_snapshot 真实运行期 usage,与 episodes 同源非常量)、discard 失败「已保留以便恢复」toast(09-sessions.js:110)。验收逐条:①线的 diff 接入 06-activity.js 既有 buildDiffTree 目录树渲染器(20-lines.js diffLoad:porcelain 行解析为 {path,additions,deletions},原始差异收进可折叠 details;冒烟断言 typeof buildDiffTree === 'function' ? buildDiffTree(treeFiles));②不离开应用完成 review→merge→清理全流程——收活五格既有链路完整,合并失败双方改动保留且可恢复(merge_worktree 错误文案含「双方改动已保留」,discard 失败 toastError 带 retry);③冲突预检可读——worktree_merge_preview 命令 + parse_merge_tree_conflicts 解析 CONFLICT 行成文件列表,confirmWorktreeMerge 确认文案列出冲突文件(单测覆盖 content/modify-delete 两种格式);④线页签分支/running/token 为既有能力,已核实 token 来自真实运行期 usage;⑤建线 UI 落磁盘/冷编译成本提示(09-sessions.js createWorktreeLine confirm:每线独立 target/ + 首次冷编译数分钟);⑥worktree_field 死分支消失——收敛为 worktree_current_branch(worktree),全仓 grep 无同值双分支;⑦前端冒烟——ui-runtime-smoke R-179 块(buildDiffTree 接入/merge_preview 调用/建线提示/三档 lines-list)通过,node --check 通过;⑧800/1024/1280 三档——冒烟循环改 innerWidth 断言 lines-list 存在。前端五冒烟 + kanzei-app 147 + 关闭前全量 cargo test --workspace 全绿(T-1786655288)。
- observed_head: e7bb5c80bf05331957d51dc2392efd492e7d6d42
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786655299573

## R-187 面板与提示音管理功能设置 [done]
- priority: P2
- 原始描述: 设置面板+各类提示音管理
- 复杂度: 中
- 归属: kanzei
- 标签: 核心
- 验收: 用户可配置界面面板及各类通知音效的设置与管理
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-187
- 进展: 2026-08-16 完成并关闭。验收「用户可配置界面面板及各类通知音效的设置与管理」:提示音部分落地——设置页新增「提示音」区块(index.html settings-group):总开关 + 音量滑杆 + 试听按钮 + 运行完成/失败/停止三事件开关;03-shell.js 新增 readSoundSettings/saveSoundSettings/soundEnabledFor(localStorage kz-sound-settings,默认全开音量 0.12 与原固定值一致),playRunNotice 按配置决定是否播放 + 音量可调;16-settings.js loadSoundSettingsControls 回填控件、change 即存、试听按钮用当前音量播「完成」音;i18n 登记 4 条文案;ui-runtime-smoke R-187 断言(控件存在/默认全开音量 0.12/总开关关闭后 soundEnabledFor 全 false)。**范围说明**:原始描述「设置面板+各类提示音管理」中「设置面板」指设置页本身(既有,含提示音新区块),「各类提示音」即运行完成/失败/停止三种事件音;若用户对「面板管理」另有具体诉求(如面板布局/显隐自定义),需另开条目。提交 85907fc;前端五冒烟 + kanzei-app 147 + 关闭前全量 cargo test --workspace 全绿(T-1786655610)。
- observed_head: 85907fc021ee8e902c9cadbd474f9af0e82cfd4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786655624130

## R-188 架构浏览直观化:代码生成的架构图渲染工具(harness/skills,非文生图) [done]
- acceptance: ①工具从真实数据源(代码结构/依赖/设计文档)生成架构图,纯代码渲染,禁止文生图与预置图片;②架构浏览页显示生成的架构图,并随数据刷新;③图不可用时降级为现有文字视图,不空白;④图上节点可点击/定位到对应文档或代码;⑤生成链路有可运行的自动化验证。
- complexity: 中
- content: 架构浏览(architecture 索引/设计文档)当前只有文字树与索引列表,读起来很不直观。需要一个代码生成的架构图渲染工具(harness 或 skills 形态),自动从真实代码/文档数据生成架构图并嵌入浏览界面。硬约束:架构图必须是代码生成的(如 mermaid/graphviz/SVG),不是文生图,也不是预置的静态图片。
- label: 核心
- priority: P2
- status: todo
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): 架构浏览页本身已存在(R-122)——后端 `architecture_snapshot` 只读命令供数据(架构索引正文 + docs/design 文档清单),前端 crates/kanzei-app/ui/19-arch.js:8-104 渲染「索引 + 设计文档树」,按索引状态分层(已入册的按索引章节分组、未入册的单列,让「有文档没入册」的缺口在界面上直接可见),点击条目走既有 Markdown 查看器 openDocViewer。本条是在这份既有数据源与视图之上**加图**,不是另起一个架构页;验收③的降级文字视图就是这棵既有的树,不要重写。
- 现状(2026-08-12 读码核实,dev HEAD): 19-arch.js 全文 129 行,**无任何图形渲染**——零 svg/canvas/mermaid/graphviz 依赖,输出是纯 DOM 文本树;`architecture_snapshot` 也只回「索引文本 + 文档名/标题列表」,**不含依赖边、调用关系、模块归属等成图所需的结构化数据**。所以本条的第一道工作量在**数据侧**(从 crates 依赖、模块引用或设计文档里抽出可成图的节点与边),渲染选型(mermaid/graphviz/自绘 SVG)是第二道;验收①的「真实数据源」指的就是这层抽取,不能拿手写的图字面量顶替。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-188
- 批次: 2/2
- 进展: 2026-08-16 完成并关闭(2 批:4d9fb34 数据侧 + de5a346 前端)。验收逐条:①工具从真实数据源生成架构图,纯代码渲染,禁止文生图/预置图——architecture_snapshot 新增 graph 字段,build_workspace_graph 从 workspace Cargo.toml members + 各 crate 的 kanzei-* 依赖(workspace/path 两形态)抽取依赖边,单测覆盖;前端 renderArchGraph 自绘 SVG(零外部依赖,桌面端离线可用),非文生图/预置图;②架构浏览页显示生成的架构图并随 refreshArch 刷新——renderArch 调 renderArchGraph,index.html 加 #arch-graph 容器,刷新即重渲染;③图不可用时降级文字树不空白——图数据空或无 createElementNS 时 arch-graph 隐藏、renderArch 文字树照常渲染(renderArchGraph 调用包 try-catch,异常不中断),冒烟断言文字树保留;④节点可点击定位——openArchCrate 优先打开同名设计文档,否则 crate Cargo.toml(均经 docs_read_custom 只读),冒烟断言点击触发 docs_read_custom;⑤生成链路自动化验证——build_workspace_graph 单测 + ui-runtime-smoke R-188 断言(SVG 渲染/节点边计数/点击定位/降级)。既有能力标注:架构浏览页文字树与 architecture_snapshot 为 R-122 交付,未重写,作为降级视图复用。前端五冒烟 + kanzei-app 148 + 关闭前全量 cargo test --workspace 全绿(T-1786656195)。
- observed_head: de5a3466eb6c52b9b0be37e2e59610f3923e89cb
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786656204141

## R-189 亮色主题:前端渲染器换色结构化评估与第二套配色 [done]
- acceptance: ①前端渲染器颜色来源结构评估:颜色集中在可换色层(变量/类)还是散落硬编码,评估结论写入需求进展或设计文档;②亮色主题完整可用:全局一键切换暗/亮并持久化;③亮/暗两套主题在 800/1024/1280 与纯键盘下均可达可用对比度;④换色改动不引入新框架,沿用现有渲染器结构。
- complexity: 中
- content: 当前桌面端只有暗色主题。需要先评估现有前端渲染器代码的颜色来源是否结构化(颜色是否集中在可换色层如 CSS 变量/主题类,还是散落硬编码),再设计并落地一套亮色主题。
- label: 前端
- priority: P2
- status: todo
- 现状评估(2026-08-12 读码核实,dev HEAD;直接对应验收①): 结构上适合换色,工作量在收口不在重构。颜色集中在 style.css :root 语义 token,ui/*.js 与 index.html 零颜色字面量,换色纯 CSS;剩余字面量分四类(a 半透明叠加保留/b 徽章底色成对给亮色值/c 框架面提升 token/d diff语法着色与 on-accent)。两处非 CSS 换色面:color-scheme 与 Monaco setTheme。落地:拆 [data-theme] 两组 token + 字面量提升 + 非 CSS 面同一开关。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-189
- 批次: 3/3
- 进展: 2026-08-16 完成并关闭(提交 1fb30eb)。验收①评估结论写入「现状评估」字段:颜色集中在 style.css :root 语义 token,ui/*.js 与 index.html 零颜色字面量;剩余字面量四类处置,手动 token 化引用点逐处引证:滚动条 style.css:77-78→--scrollbar/--scrollbar-hover;活动栏 style.css:83→--activitybar;workspace 状态 style.css:297-299→--badge-ok-soft/--badge-warn-soft/--badge-soft;优先级徽章 style.css:400-402→--badge-err/--badge-warn/--badge-info;状态胶囊 style.css:408-410→--badge-alert/--badge-info/--badge-ok;danger 按钮 style.css:649-650→--danger-btn+--on-accent;代码块/工具输出底 style.css:1047/1184/1205/1208→--code-bg/--code-output-bg;warn 徽章底 style.css:1166→--badge-warn;diff 着色 style.css:1214-1215/1239-1240/1242-1243→--diff-add/--diff-del;warn/err fallback style.css:904/1321/1444-1445/1456-1457→--warn/--err;线路主题色 style.css:441-444→--line-1..4;状态点 style.css:673-674→--dot-idle/--dot-run;diff 行号 style.css:1212→--diff-line-number;语法着色 style.css:1223-1226→--syntax-comment/--syntax-string/--syntax-number/--syntax-keyword;日志金字 style.css:1250→--log-gold;未入册头 style.css:1547→--arch-unindexed;新 token 定义 style.css:21-29(dark)/style.css:42-48(light),成对给亮色值。验收②[data-theme=light] style.css:32-48 + 03-shell.js:466-497 currentTheme/applyTheme/initTheme + localStorage kz-theme 持久化(默认 dark 零回归)+ index.html:667 theme-toggle + 17-files.js:222-228 Monaco setTheme(vs/vs-dark);color-scheme style.css:19/41 随主题(D-154 原生控件)。验收③ui-runtime-smoke.mjs:6084-6117 R-189 断言(切换/持久化/Monaco 联动)+ 既有 a11y 冒烟(纯键盘语义)+ R-179 三档循环(800/1024/1280)。验收④换色纯 CSS + Monaco setTheme,未引入新框架,沿用 :root token 结构。前端五冒烟 + kanzei-app 148 + 关闭前全量 cargo test --workspace 全绿(T-1786656576)。
- observed_head: 1fb30ebff4553181f4de5344479c2dd991093cc1
- observed_worktree_hash: fnv1a64:8c41ba37517e4824
- recorded_at: 1786656720597
- 现状评估: **结构上适合换色,工作量在收口不在重构(2026-08-12 读码核实)。** ①颜色只有一处——`crates/kanzei-app/ui/style.css` 之外零颜色:20 个 ui/*.js 与 index.html 里没有任何 JS 计算或写入颜色,换色是纯 CSS 的事,不碰渲染逻辑。②可换色层已存在——`:root` 定义 22 个语义命名 token(--bg/--panel/--fg/--accent/--ok/--err 等),全文绝大多数颜色引用走 var(),命名是语义而非色值,亮色版可直接换值。③剩余字面量分四类处置:(a) 由调色板派生的半透明叠加与阴影(rgba(208,104,78,.16) 等,天然跨主题保留);(b) 暗色专属的徽章/状态胶囊底色(亮色下不能复用,必须成对给值,是本条真正的设计工作量);(c) 未 token 化的框架面(活动栏/滚动条/代码块底/danger 按钮,机械提升为新 token);(d) diff/语法着色与强调按钮前景(后者需 --on-accent)。④两个 CSS 够不着的换色面:`color-scheme: dark`(style.css:19,决定原生控件深浅变体,D-154 教训)与 Monaco 编辑器 `theme: "vs-dark"`(17-files.js:223,须同步 setTheme)。⑤落地路径:`:root` 拆成 [data-theme=dark]/[data-theme=light] 两组 token(默认 dark 零回归)+ 字面量提升 token + 两处非 CSS 面挂同一开关;不引入新框架。

## R-147 增加使用手册与作者话内容板块 [done]
- 复杂度: 中
- 归属: kanzei
- 验收: 页面顶部新增独立区块，展示项目使用手册和来自作者的说明文字
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 R-147，必须先恢复它
- 进展: 2026-08-16 交付:①index.html:166-172 #chat-area 顶部新增 <details id="manual-panel">(summary「使用手册」+ #manual-body),位于 #messages 之前,默认 hidden+收起不遮挡对话;②15-views-misc.js 新增 async refreshManual():file_preview 读 docs/目录.md(真实数据源,非展示壳)→ renderMarkdown 渲染进 #manual-body,读取失败保持隐藏不显示空壳;③09-sessions.js renderProjects(currentProject 更新处)调用 refreshManual——启动首次确定项目、切换项目、移除项目三条路径都刷新,是唯一真实调用方;④02-i18n.js 登记「使用手册/点击展开/手册文件不是文本」3 词条(M-014 静态中文登记);⑤style.css 新增 .manual-panel 折叠样式(参照 settings-group,内容 max-height 34vh 内部滚动);⑥docs/目录.md 为手册内容源(项目心智模型/目录结构/推荐阅读顺序+作者说明文字)。验证:ui-runtime-smoke 新增 file_preview 桩与两条断言(有内容渲染可见/读取失败隐藏、恢复重新显示),i18n/lint/a11y/markdown 冒烟全绿,cargo test --workspace 全绿(T-1786657249)。
- 验收证据: 验收原文「页面顶部新增独立区块,展示项目使用手册和来自作者的说明文字」——①区块:index.html:169 <details id="manual-panel" class="manual-panel hidden"> 在 #chat-area 顶部(#messages 之前),折叠样式 style.css:584-600;②展示内容:refreshManual(15-views-misc.js)以 file_preview 读 docs/目录.md 为真实数据源,renderMarkdown 渲染进 #manual-body;docs/目录.md 全文为项目使用手册(心智模型/根目录/目录结构/前端文件/推荐阅读顺序)+作者说明文字(开头「我已经按当前工作区做完只读梳理…」);③调用方:09-sessions.js:635 renderProjects 每次 currentProject 确定/切换时调用 refreshManual,启动链(18-startup 的 renderProjects)与切项目/移除项目均经过它;④自动化验证:ui-runtime-smoke.mjs 新增 file_preview 桩与断言(手册渲染可见、读取失败隐藏、恢复重新显示),ui-i18n/ui-lint/ui-a11y/ui-markdown 冒烟全绿,cargo test --workspace 全绿(T-1786657249)。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:3cea6831be337da9
- recorded_at: 1786657366690

## R-160 README添加项目设计目标说明 [done]
- priority: P2
- 原始描述: readme里加一些设计目标，比如专为永久工作设计等等
- 复杂度: 中
- 归属: kanzei
- 验收: README中包含明确的设计目标和开发指南，如永久工作支持等核心特性说明
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-160
- 进展: 2026-08-16 交付:README.md 新增两节(中英双语同步)——①「## 设计目标」(中文)与「### Design goals」(英文):首条即「永久工作优先/Permanent work first」(外部记忆跨会话保留、会话可恢复、轨迹可回放、agent 自举),后续「好用压倒一切/Usability over everything」「真正的任务级并行/Real task-level parallelism」「受控合并/Controlled integration」「规则写在代码里/Rules in code」「复刻优先,创新只投护城河/Replicate first, innovate only in the moat」「工单就是文件/Tickets are files」「中英文并重/Chinese and English both matter」共 8 条,末尾链接方向基线 docs/design/direction_taste.md;②「## 开发指南」与「### Development guide」:分支与发布(dev 开发/main 只收 ff 合并/发布树 package.ps1/唯一安装位)、测试(定向+冒烟+全量触发点)、提交门禁(fmt/clippy 强制)、规范单源、外部 agent 协作(kz lock status、禁止 git add .)。验证:cargo test --workspace 全绿(T-1786658480)。
- 验收证据: 验收原文「README中包含明确的设计目标和开发指南,如永久工作支持等核心特性说明」——①设计目标:README.md「## 设计目标」节(约 28-44 行)首条「永久工作优先:外部记忆是独立控制面,跨会话保留;会话可恢复、轨迹可回放;agent 用自己维护的 backlog 开发自己」,共 8 条明确目标并链接 docs/design/direction_taste.md;英文同构「### Design goals」(约 137-152 行);②开发指南:README.md「## 开发指南」节(约 107-115 行,分支/测试/门禁/规范/协作 5 条),英文「### Development guide」(约 154-162 行);③核心特性说明:永久工作(①首条)、任务级并行、受控合并、规则写在代码里、复刻优先等均成节;④验证:cargo test --workspace 全绿(T-1786658480)。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:07011c83ec0b20ac
- recorded_at: 1786658592507

## R-172 新建配置文件的注释模板补齐各节骨架示例 [done]
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 归属: kanzei
- 来源: 2026-08-10 设置页全字段走查。settings_open 原先在新建配置时把 `codex_fast_mode = false` 合成进载荷写死(已作为缺陷修掉),现改为写纯注释模板。用户定调:**保留注释模板**(不回退成 0 字节空文件),但当前模板只有三行注释,全新环境下打开「配置原文」看不到有哪些节可写,第一次上手缺线索。
- 内容: 把新建配置的注释模板补成带各节骨架的注释示例(至少覆盖 [models]、[providers.X]、[limits]、[proxy]、[cadence] 的键名与取值范围),全部以注释形式给出——**不得写成生效的显式值**,否则会被当成用户设定、绕过 fill_defaults 的默认(这正是被修掉的那个 bug 的形态)。
- 边界: 只动模板文本;不改 settings_open 的写入时机与「留空即默认」语义;模板内容写进文件、不是界面文案,不受 ui-i18n-smoke 约束。
- 验收: ①全新环境下 settings_open 产出的文件含各节骨架注释;②解析后配置仍等价于全默认(有单测:模板文件 load 后与 KanzeiConfig::default() 一致);③不引入任何生效的显式值。
- 进展: 2026-08-16 交付:settings.rs:657 settings_bootstrap_file 把三行注释模板补成各节骨架注释——[models](primary/fast/scout/compact/reasoning/codex_fast_mode 键名+取值范围)、[providers.<名字>](protocol 四枚举/base_url/api_key_env/api_key/auth/context_limit)、[limits](14 个键+取值)、[proxy](env 或 URL)、[cadence](full_test 四档/full_test_batches/targeted_test/commit/push/verify_every_n)、[profile]、[permissions] 全部以 # 注释给出,零生效值;模板仍合法 TOML(settings_write_document 解析通过)。新增单测「配置模板_骨架注释齐全_且解析后等价于全默认」:①断言五节骨架注释都在;②toml 解析后与 KanzeiConfig::default() 逐字段一致;③断言无非注释的 key = value 行(不引入生效显式值)。修正旧测试「打开配置原文_新建文件不把_codex_fast_mode_写死成_false」:原断言 !text.contains("codex_fast_mode") 过严(注释键名也算命中),改为只查非注释的 codex_fast_mode 赋值行,保留「新文件必须未表态」语义。验证:cargo test -p kanzei-app settings:: 14 passed,fmt/clippy 全绿。
- 验收证据: 验收①「全新环境下 settings_open 产出的文件含各节骨架注释」——settings.rs:657-704 settings_bootstrap_file 模板含 [models]/[providers.X]/[limits]/[proxy]/[cadence] 各节键名与取值范围注释;settings_open(settings.rs:669)文件不存在时调用该函数;新增单测断言五节骨架注释(needle 列表)全部出现。验收②「解析后配置仍等价于全默认(有单测:模板文件 load 后与 KanzeiConfig::default() 一致)」——新增单测 settings.rs 配置模板_骨架注释齐全_且解析后等价于全默认:toml::from_str 后与 KanzeiConfig::default() 逐字段对比(models 六字段/providers 空/proxy/profile/permissions/limits 五字段/cadence 五字段)。验收③「不引入任何生效的显式值」——同测试断言模板无非注释的 key = value 行(live_assignments 为空);旧测试改后只拦截生效赋值,config.models.codex_fast_mode == None 语义保留。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:d70610e294d8b51b
- recorded_at: 1786658839331

## R-220 kanzei.toml 用户面配置参考:由 unknown_keys 已知键名单驱动生成,测试锁定一致 [done]
- 优先级: P3
- 复杂度: 小
- 标签: 文档 harness
- 来源: 2026-08-12 八维度审计(§6)。
- 背景: harness_m1.md:16-53 的配置样例停在 M1(缺 limits/cadence/embeddings/permissions.non_interactive 全部新节,profile 取值没提 readonly);用户只能读 config.rs 源码猜键名。
- 内容: 生成配置参考(文档或 kz config schema 命令),覆盖全部可调键、一句话说明与默认值;加测试断言文档键表与 unknown_keys 已知键名单一致,防两处漂移。
- 验收: ①全部已知键有说明与默认值;②单侧增删键时一致性测试变红;③D-300 修复后的 barrier_timeout_secs 在参考里可见。
- refs: D-300 R-172
- 进展: 2026-08-16 交付:①config.rs 把 unknown_keys 手写名单提取为各节已知键常量(TOP_LEVEL_KEYS/MODELS_KEYS/EMBEDDINGS_KEYS/LIMITS_KEYS/PROVIDER_KEYS/PROFILE_KEYS/CADENCE_KEYS/PERMISSIONS_KEYS/PERMISSION_RULE_KEYS),unknown_keys 改为引用常量——名单单源,增删键只改一处;②新增 pub fn config_reference() 由常量驱动生成用户面配置参考(纯注释 TOML,每节每键一行 `# 键 = 取值范围/默认值 说明`),覆盖全部已知键(含 language 顶层标量、providers.<名字> 动态节、permissions.rules 数组表);③kz main.rs 新增 `kz config schema` 子命令(usage 同步),打印 config_reference();④新增守护测试 config_reference_covers_all_known_keys:参考包含每个已知键(顶层节/动态节/规则键按各自形态),反向断言参考不出现名单外键;另加 barrier_timeout_secs 显式断言(D-300 修复键可见)。验证:cargo test -p kanzei-harness config:: 48 passed(含新测试)、cargo test -p kanzei 17 passed、fmt/clippy 全绿。
- 验收证据: 验收①「全部已知键有说明与默认值」——config_reference(config.rs)遍历九个已知键常量,每键输出 `# 键 = 取值范围/默认值 说明` 行,覆盖 models 6 键/providers 6 键/limits 14 键(含 barrier_timeout_secs)/cadence 6 键/profile/embeddings/permissions 及 rules 3 键/proxy/language;`kz config schema`(main.rs config_cli)为真实消费者。验收②「单侧增删键时一致性测试变红」——config_reference_covers_all_known_keys 测试:参考缺失任一已知键即红(正向),参考出现名单外键即红(反向两条:子键形态 + 顶层节形态);名单增删后参考由常量自动跟随,若改名单不改参考生成则正向断言红,若参考手工多写键则反向断言红。验收③「D-300 修复后的 barrier_timeout_secs 在参考里可见」——测试显式断言 reference.contains("barrier_timeout_secs"),且 LIMITS_KEYS 含该键、emit_section 输出 limits 节。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:bd331b4adc8345ef
- recorded_at: 1786659418707

## R-208 新建 kanzei-base 零依赖底层 crate:承接 atomic_file 与 FileLock,解开 llm 寄居 [done]
- 优先级: P3
- 复杂度: 小
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);atomic_file.rs:11-14 自述因 llm 是依赖图最底层只能放这里(D-261 决策),消费方横跨 tools 与 llm;kanzei-harness 不依赖 llm,其 orchestration.rs:34/41 只能在注释里引用 FileLock 行为——R-181 的跨进程 lease 契约在 harness、原语在 llm,照单实施会撞依赖方向墙。
- 内容: 新建 kanzei-base(或 kanzei-fs)零依赖 crate 承接 atomic_file/FileLock;llm/tools 改从它取;harness 增加对它的依赖。
- 边界: 纯搬迁零行为变更;过渡期保留 re-export 避免大面积改 use。
- 验收: ①kanzei-llm 不再导出文件系统原语;②kanzei-harness 可直接依赖该 crate;③全仓测试绿。
- refs: R-181 R-203
- 进展: 2026-08-16 交付:新建 kanzei-base 零依赖 crate(Cargo.toml [dependencies] 为空,lib.rs 声明 pub mod atomic_file)。atomic_file.rs(657 行,纯 std)从 kanzei-llm/src 整体迁入 kanzei-base/src,头部注释改写(D-261 决策 + R-208 迁出说明),逻辑零变更;原 kanzei-llm/src/atomic_file.rs 删除、lib.rs 移除 pub mod atomic_file(验收①:llm 不再导出文件系统原语);auth/store.rs use 改 kanzei_base::atomic_file;llm/tools/harness 三个 Cargo.toml 增加 kanzei-base.workspace = true;kanzei-tools lib.rs re-export 改 kanzei_base::atomic_file(保留 pub use,工具内部 crate::atomic_file::* 全部零改动);architecture.rs 注释引用更新。workspace Cargo.toml members + workspace.dependencies 注册 kanzei-base。验收③全仓测试:cargo test --workspace 全绿(无 FAILED/error),fmt/clippy 全绿。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:c97180595f4232f8
- recorded_at: 1786659778448
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-208
- 验收证据: 验收①「kanzei-llm 不再导出文件系统原语」——crates/kanzei-llm/src/lib.rs 已删 `pub mod atomic_file;` 行,原 atomic_file.rs 已物理删除(Remove-Item 后 Test-Path False);llm 内部唯一消费方 auth/store.rs:18 改为 `use kanzei_base::atomic_file;`。验收②「kanzei-harness 可直接依赖该 crate」——crates/kanzei-harness/Cargo.toml 新增 `kanzei-base.workspace = true` 且 cargo check --workspace 编译通过(harness 是依赖方之一)。验收③「全仓测试绿」——cargo test --workspace 全绿(T-1786659557),fmt/clippy 全绿。边界「纯搬迁零行为变更;过渡期保留 re-export」——atomic_file.rs 内容除头部注释外逐字搬迁;kanzei-tools/src/lib.rs:6 保留 `pub use kanzei_base::atomic_file;`,工具内部 20+ 处 crate::atomic_file::* 调用零改动。消费方链:kanzei-base ← llm/tools/harness 均编译通过。

## R-241 Session 事件真源与 Shadow Projection：typed event、流式草稿恢复、legacy 迁移 [done]
- refs: D-209 D-342 R-236 docs/design/deepseek_harness_upgrade.md
- 内容: 冻结最小 typed event 词表与 format_version；为每会话分配原子 sequence；user message、assistant draft chunk/commit、tool call/result、turn stop/complete/fail 按发生顺序双写；从最新 legacy conversation.updated 快照生成带 provenance 的 seed；新增只读 shadow projector，与现有 messages 快照逐轮比较，第一批不切换 UI 和模型 prior。
- 复杂度: 大
- 批次: 4/4
- 来源: 2026-08-14 DeepSeek Harness 对照评审与用户边界确认；参考 docs/reference/deepseek_harness_reference_20260814.md。
- 标签: 核心
- 边界: SQLite 是运行时会话、事件、线路、运行状态真源；Markdown 只用于需求/缺陷/设计/长期记忆及会话导出，不作为高频事件真源。不逐 token 落库，assistant 可见增量按有界时间/字节批次持久化为 draft；draft 只有 committed 后才成为正式 assistant message，中断则保留为 interrupted 诊断记录。第一批不停止 conversation.updated、不切 UI、不改 Compaction 存储语义。
- 迁移与回滚: 新增 schema/version 与索引必须提供 Alembic 等价的 Rust SQLite 迁移、升级前备份和旧数据兼容；legacy 导入幂等且不伪造历史细节。Shadow 阶段保留旧读写路径，关闭新双写即可回滚；投影缓存可删除重建，原事件不得依赖缓存。
- 验收: ①并发追加 sequence 不重号不丢号；②user/assistant draft/tool/终态每类事件均有 round-trip 测试；③生成中强杀后可回放有界的 interrupted assistant 草稿，且模型 prior 不把它当完整回答；④legacy 导入重复执行幂等并保留 provenance；⑤projector 从同一日志重复回放逐字节一致；⑥shadow comparison 对正常、停止、权限拒绝、工具错误、多工具部分完成路径给出差异报告；⑦SessionInvariant 在提交前拒绝重复 result、跨 step 配对和非法终态。
- 优先级: P0
- 取活依据: override:用户确认 D-351 实机验收通过并明确授权取活 R-241，2026-08-14。
- 进展: B1-B4 已完成。冻结 format_version=1 typed facts，提交前 invariant + SQLite 原子 batch；runner 在 assistant commit/tool results 进入 history 的语义边界发事件，CLI/桌面双写；草稿按 2048字符或750ms 批次持久化，stream restart/进程重启/停止/失败闭合为 interrupted/terminal 且不重放工具；legacy seed 幂等保留 provenance；确定性 surface/transcript projector 与 conversation_shadow_get/逐轮 session.shadow_compared 已交付，旧 conversation_get 和模型 prior 未切换；复用 session_events，无 schema 迁移。T-1786672324：D-342 定向停止、全 workspace 和 clippy all-targets 全绿。
- observed_head: 1550b9ceb9229ef1512b89d8f1e05543bdf38af9
- observed_worktree_hash: fnv1a64:aa29e71147a914cf
- recorded_at: 1786672415435

## R-247 开线即绑定:选条目起线、「被取得」标记接取得线真源、线级取活写权限口径 [done]
- 内容: D-354 已落引擎层线级 claim(「取得线」字段)。本需求补 UI 与流程面:①开线区选条目→起线时即以该线身份 claim(设计 parallel_lines_ui P7 的绑定动作);②backlog「被取得」标记与泳道 claim 显示改读「取得线」字段,替代 claim_from_prompt 的 prompt 头猜测(D-304 口径:协作快照是唯一事实源);③线级取活写权限口径:work claim 属于取活动作,评估对分支线默认放行 write:claim(或开线绑定时由主进程代 claim),不再要求先手动开「允许写主根追踪器」才能取活;④线停机/关闭/合并收活时「取得线」的释放流程
- 前置: D-354
- 复杂度: 中
- 标签: 流程
- 验收: ①从并行视图选一个未被持有的条目起线,该条目立即带该线「取得线」标记;②backlog 与泳道显示的持有关系与 tracker 字段一致,prompt 猜测路径删除;③新开的线不需要手动开 tracker 写开关即可完成取活绑定;④线关闭或收活合并后条目持有释放或转终态,有断言
- 优先级: P1
- 取活依据: override:override:用户已认可推进 R-247 到适合自举的阶段；先完成开线绑定、取得线真源与收线释放闭环。
- 批次: 3/3
- 进展: 2026-08-14 B1 完成:主进程在 process_create 建树注册后以新分支身份复用 WorkTool 原子 claim，失败整体回滚；分支 tracker_writes 仍默认关闭；关闭/放弃在统一注销出口 release，合并成功后立即 release 且保留线路供后置门禁；协作快照改读 tracker 取得线并删除 prompt 猜测。反证:kanzei-tools release 测试 1 passed；kanzei-app 开线绑定/失败回滚/合并释放 3 passed；协作快照真源测试 1 passed。 | 2026-08-14 B2 完成:并行页新增未持有条目选择器与按条目开线，process_create 携带 work_item_id；backlog 直接读 docs_snapshot.claimed_by，空闲持有仍显示，prompt 解析静态反证锁死；关闭/放弃/合并后立即刷新文档投影。验证:UI runtime 1830 invokes/0 error、parallel-lines-regression、lint 1282 globals、i18n 1100 keys、a11y、markdown 全绿。 | 2026-08-14 B3 完成:设计文档同步取得线真源、原子开线与显式释放语义；浏览器验收覆盖 1280 桌面深色/亮色和 620 窄屏，选择器与按钮无重叠、无横向溢出，亮色下拉文字 rgb(31,41,51)/背景 rgb(232,231,227)。预览宿主桩与本地服务均已撤销。
- observed_head: 3c7824c1493b11e66ed628beab1ca8286c8fca7b
- observed_worktree_hash: fnv1a64:53a0da8f0141c57e
- recorded_at: 1786677413822

## R-203 kanzei-tools 解体第一步:memory/ 子树拆成独立 crate,tools 不再依赖 kanzei-core [done]
- 优先级: P3
- 复杂度: 大
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);kanzei-tools 已 25,430 行成全仓最大 crate(超 app 的 15,994 与 core 的 11,781),memory/ 7,314 行寄居其中且 manager.rs:304 直开 kanzei_core::SessionStore、mod.rs:595 实现 kanzei_core::RecallPolicy——「工具层」坐在依赖图顶端,与 lib.rs 自述「内置工具+双模式 profile 组件」脱节,记忆控制平面(R-161~R-167 主战场)没有独立编译/测试边界。
- 内容: memory/ 拆成 kanzei-memory crate(依赖 core+harness);kanzei-tools 回落到纯工具实现。
- 边界: 纯搬迁行为零变更;pub API 经再导出保持调用点零改动;不与 R-204 同批。
- 验收: ①kanzei-tools 不再依赖 kanzei-core;②memory 子系统独立编译与测试;③全仓测试绿。
- refs: R-204 R-208
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-203
- 批次: 3/3
- 进展: B1~B3 完成。验收逐项证据:①kanzei-tools 不再依赖 kanzei-core——crates/kanzei-tools/Cargo.toml [dependencies] 已无 kanzei-core(dev-dependencies 保留并注释:write.rs 的 runner 集成测试需驱动完整 run_once 且 write 为私有模块无法外迁);`cargo tree -p kanzei-tools --edges normal -i kanzei-core` 显示 core 仅经 kanzei-memory 传递;源码 kanzei_core:: 引用仅剩 write.rs #[cfg(test)] 区(189/386/402 行)。②memory 子系统独立编译与测试——新建 crates/kanzei-memory/(workspace member),src/{lib.rs,docstore.rs,embed.rs,replay_eval.rs,scheduling.rs,memory/*};cargo build -p kanzei-memory 独立编译通过(T-1786692709 前),cargo test -p kanzei-memory 128 passed 0 failed(T-1786692709)。③全仓测试绿——cargo test --workspace 全部 crate 0 failed(T-1786693157;memory 128/tools 230/app 154/core 172/harness 124/llm 44/base 9/kanzei 3)。边界:纯搬迁零变更(仅破环必需 4 处引用改写:store.rs content_hash→kanzei_base 3 处、mod.rs workable_titles→crate::scheduling 1 处;parse_input 复制、atomic_file re-export);pub API 经 tools lib.rs `pub use kanzei_memory::{memory,docstore,embed,replay_eval}` 再导出,kanzei/kanzei-app 零源码改动编译并测试绿;workable_titles 调度链复制进 kanzei-memory/src/scheduling.rs(与原 tracker 版逐字一致,同源注释,R-204 统一),tracker.rs 结构未动、不与 R-204 同批。提交:R-203 B1 ae4a9ea、R-203 B2 03a74e4。遗留:工作树非本次改动(memory 归档/phase_pipeline.rs)未动。
- observed_head: 03a74e4086f2463db1e8c9eb1f5827a417199d53
- observed_worktree_hash: fnv1a64:53a0da8f0141c57e
- recorded_at: 1786693171066

## R-207 worktree 生命周期下沉 kanzei-tools:建线/回执/回滚/合并预检桌面与 CLI 共用 [done]
- 优先级: P3
- 复杂度: 大
- 标签: 后端 并行
- 来源: 2026-08-12 八维度审计(§1);app/processes.rs 1,786 行四域混杂,worktree 业务(:777-1355)桌面独占并自带 git plumbing,与 tools/git.rs 双轨(全仓非测试 spawn git 35 处);kanzei/src/main.rs:702 注释自认「桌面端独占能力架构债」;R-183(kz 无人值守)与 R-181(外部 agent 入局)都需要 CLI 侧线管理能力。
- 内容: worktree 生命周期(create/receipt/rollback/merge 预检/状态)下沉到 kanzei-tools 的 git 域或新 worktree 模块,桌面与 CLI 共用同一实现;processes.rs 只剩 Tauri 接线与 AppState 交互。
- 验收: ①kz CLI 能调用同一实现完成建线/合并预检;②processes.rs 收敛;③既有 worktree 测试(含跨进程并发建树)全绿。
- refs: R-183 R-181 R-179
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-207
- 批次: 5/5
- 批次说明: 见进展
- 进展: B1~B5 完成。验收逐项证据:①kz CLI 调用同一实现完成建线/合并预检——crates/kanzei/src/main.rs worktree_cli:create 调 kanzei_tools::worktree::create_worktree_with_receipt,merge-preview 调 validate_worktree_path/worktree_current_branch/worktree_command(merge-tree --write-tree)/parse_merge_tree_conflicts;命令分发冒烟(kz worktree 无参 → usage+明确用法);kanzei 编译+17 测试绿;内核测试 tools worktree 11(建树回滚闭环/目录残留零回滚/冲突提取)。②processes.rs 收敛——2104→约1600 行(提交 58d4d0e 减 507 行),worktree 核心全部迁走,只剩转发壳+AppState 交互(bound_thread_for_worktree/with_idle_bound_process/acquire_project_write_lease/reclaim/discard_worktree_and_unregister/merge_worktree_and_release);state.rs WorktreeInfo 改 re-export;桌面与 CLI 单源。③既有 worktree 测试(含跨进程并发建树)全绿——cargo test -p kanzei-app --bin kzapp processes:: 44 passed;关闭前 cargo test --workspace 全部 crate 0 failed。提交:R-207 B1 cb4e458、B2 d5a4d1d、B3 58d4d0e、B4 bce293c。CLI 真机建树冒烟被环境拦截(bash 禁 git 突变,临时仓无法用结构化工具),改以命令分发验证+内核测试背书,如实记录。
- observed_head: bce293caea4d7d062d8ae380f15aa8766a0dda0b
- observed_worktree_hash: fnv1a64:53a0da8f0141c57e
- recorded_at: 1786694754072

## R-205 config.rs 拆出 project_root.rs 与 permission_persist.rs:D-270 修复的结构落点 [done]
- 优先级: P3
- 复杂度: 中
- 标签: 后端 harness
- 来源: 2026-08-12 八维度审计(§1);config.rs 2,684 行混装配置 schema/TOML 合并/权限规则持久化(:1067-1120)/项目根发现与文件系统身份判定(HOME 守卫全部实现,:1121-1357)四域,改权限形态(R-198)、改根发现(D-270)、改 schema 三类互不相干的工作在同一文件冲突。
- 内容: 拆出 project_root.rs(根发现+文件系统身份,D-270 四缺口的修复落这里)与 permission_persist.rs(append_allow_rule/generalize_resource/digest);config.rs 收敛到 schema+merge+resolve。
- 边界: pub API 经 lib.rs 再导出零变更;D-300 是两行修不必等本条,先行。
- 验收: ①三文件职责如上;②API 面零变更;③全仓测试绿。
- refs: D-270 D-300 R-198
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-205
- 批次: 3/3
- 进展: B1~B3 完成。验收逐项证据:①三文件职责——crates/kanzei-harness/src/project_root.rs(根发现 discover_project_root/discover_project_config/resolve_project_root + HOME 守卫 is_home_root/is_same_dir/卷元数据身份判定,D-270 修复落点),permission_persist.rs(append_allow_rule/generalize_resource/rule_digest/is_wildcard_resource),config.rs 收敛到 schema+merge+resolve(3192→约 2750 行)。②API 面零变更——config.rs re-export pub use crate::{project_root::{discover_project_config,discover_project_root,is_home_root,resolve_project_root}, permission_persist::{append_allow_rule,generalize_resource}},全仓编译绿(kanzei-tools/kanzei/kanzei-app 零源码改动);生产 use crate::permission_persist::{is_wildcard_resource,rule_digest} 改道;config 测试经 use crate::project_root::* 继续跑不丢(130 全绿)。③全仓测试绿——关闭前 cargo test --workspace 全部 crate 0 failed。提交:R-205 B1 f702185、B2 ed06b96。测试位置说明:project_root/permission_persist 域既有测试仍驻留 config.rs tests(经 glob 导入 pub(crate) 判定函数),实现已单源,测试归属整理留待后续。
- observed_head: ed06b969f419b779c1c17dea7c0e81a65fb45397
- observed_worktree_hash: fnv1a64:53a0da8f0141c57e
- recorded_at: 1786695614019

## R-250 子代理结构化返回:task 支持 schema,主代理零解析 [done]
- refs: R-004 R-012 R-176 R-218 R-246 docs/design/subagent_management.md
- 内容: 现状 explore/writer 子代理只返回自由文本(subagent.rs:94、112 的系统提示都是「reply with ONLY the requested information」),主代理必须自己从散文里解析结论——弱模型在这一步的读错是自举质量的直接损耗。本条给 `task` 工具加可选 `schema` 字段:传入 JSON Schema 时子代理被强制以该结构返回,校验在工具层完成,不合规即让子代理重试,主代理拿到的是已验证对象。
- 复杂度: 中
- 批次: 0/2
- 来源: 2026-08-14 三系统工具面对照:DeepSeek 与 Claude Code 的子代理均支持结构化返回,kanzei 缺。对照结论——kanzei 子代理的**安全模型**(只读白名单代码层隔离、写租约走协调器、权限规则原样生效)比两者都扎实,缺的是**效率与可控性**。
- 标签: 核心
- 边界: 只做返回侧的 schema 约束,不做 fork(继承主对话历史)、不做运行中查看与追加指令、不做嵌套派生——这三条各自独立评估,其中 fork 与 R-246 的 child agents owner 语义相关,不在本条抢跑。schema 为可选字段,不传时行为与现状逐字节一致。
- 验收: ①传 schema 时返回值经校验,不合规触发子代理重试且重试次数有上限;②不传 schema 时既有 explore/writer 行为无回归(机械核验:现有子代理测试全绿);③同轮多个并行 task 各自独立校验,互不影响;④校验失败的诊断指出是哪个字段不合规,不是笼统报错;⑤只读子代理白名单不因本条放宽(沿用 R-176 验收⑦的复核手法)。
- 验收逐条对照(2026-08-14 交付 12098eb): ①schema_check::validate 校验通过才回喂,不合规续上已有历史补纠错指令重跑,MAX_SCHEMA_RETRIES=1 为上限,用尽后把最后原文一并交回主代理;②不传 schema 时 `let Some(schema) = schema.as_ref() else { break ok(text) }` 早退,后续分支全不执行——现有 7 条子代理测试与 kanzei-app 154 条全绿;③校验发生在 run_subagent 内,每个 task 调用各跑一份,同轮并行互不共享状态;④诊断带 JSON 指针路径与期望/实际类型(如 `$.findings[0]: missing required field \`line\``),3 条定向测试锁住 required/type/enum 三类诊断内容;⑤本条未碰 SubagentBase,只读白名单一字未动,R-176 验收⑦的复核测试保持通过。额外:取消 token 注册移出重试循环——每轮重注册会在两轮之间留未注册窗口,那里的 stop_task 会静默落空。
- 收尾: 2026-08-14;测试记录 T-1786703740
- 优先级: P2

## R-204 tracker.rs 拆分:action 分发、取活调度、测试三域分离,调度成为独立可审计模块 [done]
- 优先级: P3
- 复杂度: 中
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);tracker.rs 2,988 行为全仓第一大文件:execute 的 match 从 :257 到 :787 十余臂内联,取活调度(schedule_entries/dependency_states/block_reasons/backlog_status/workable_titles,:956-1370)被 auto_run/CLI/docs/memory 四方消费,:1372 起 1,616 行测试同文件——恰是自举最高频改动面,取活语义(D-207 抱怨的源头)散落在工具文件里无人能单独审计。
- 内容: 拆成 actions/(每 action 一函数)+ scheduling 独立模块(供四方统一消费)+ 测试分域下沉;execute 只剩路由。
- 边界: 四个既有消费方调用点零改动;行为零变更。
- 验收: ①调度逻辑有独立测试文件;②execute 只剩路由;③全仓测试绿。
- refs: D-207 R-203
- 批次: 2/2
- 进展: 批1 完成(2026-08-14):scheduling 模块拆出——新建 crates/kanzei-tools/src/tracker/scheduling.rs(调度域全部:append_progress/schedule_for_display(_with_states)/workable_titles/backlog_status/DependencyStates/dependency_states(_from_documents)/dependents_map(_with_states)/schedule_entries/block_reasons/deadlock_banner/structured_entry/is_*键判断/tracker_ids/coupling_signals/dispatch_verdict/entry_paths);tracker.rs 收窄:删搬走代码(~500 行),加 pub mod scheduling + pub use 再导出(消费方 kanzei_tools::tracker::xxx 路径零改动)+ pub(crate) use is_prerequisite_key/tracker_ids(work.rs 主逻辑)+ cfg(test) block_reasons(work.rs 测试);execute 的 list 分支经 use scheduling::{dependency_states,schedule_entries,structured_entry,deadlock_banner} 继续工作。验证:cargo check -p kanzei-tools 零警告零错误;cargo test -p kanzei-tools --lib 241 passed/0 failed(原 46 tracker:: 含 coupling/dispatch/backlog 测试经 re-export 全绿)。途中误删 render_line 签名行已当场补回。批2:调度测试下沉 scheduling_tests.rs(验收①)+ actions.rs 拆出 execute 路由化(验收②)。 || 批2a 完成(2026-08-14):调度测试下沉 scheduling_tests.rs(coupling_signals/dispatch_verdict/dependents_map/backlog_status 三态与读取失败 5 测试独立文件,直接 use scheduling 模块),tracker.rs tests 仅留 Tool 行为测试;kanzei-tools lib 241 passed。 || 批2b 完成(2026-08-14):actions.rs 拆出——execute 十余臂路由化(每个 action 一函数:list/get/raw_lines/repair_reused_id/repair_missing_id/void_id/archive/add/update_close/reorder/raw_delete/reopen/fix_terminal/archive_fill/normalize),execute 只剩解析/归一化/list 拒绝/锁/load/完整性门禁 + match 路由 + D-112 尾门禁(验收②);辅助函数(classification_claims/file_line_citations/check_close_classification_evidence/user_visible_fields/field_diff_summary/archived_or_unknown)下沉 actions.rs,render_line/unknown_id 留 tracker.rs(check_refs 用);校验方法仍挂 TrackerTool。clippy ptr_arg 修复(&mut [Entry]),cargo clippy --workspace -D warnings 全绿;kanzei-tools lib 241 passed/0 failed(行为零变更,提交 e57e6f5)。批3:全量 cargo test --workspace(复杂度中关闭前)+ 关闭。
- observed_head: e57e6f59fd106a379b44dea5f8c8d680b7148f3a
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786707865718
- 证据: 验收①调度逻辑有独立测试文件:crates/kanzei-tools/src/tracker/scheduling_tests.rs(coupling_signals/dispatch_verdict/dependents_map/backlog_status 三态与读取失败 5 测试直接 use scheduling 模块),cargo test -p kanzei-tools --lib scheduling 5 passed;tracker.rs tests 仅留 Tool 行为测试。验收②execute 只剩路由:tracker.rs execute = 解析 input→顶层字段归一化→list 拒绝→work_selection 锁→docstore 锁→load→完整性写门禁→match 一行调用 actions::xxx(self,input,ctx,&store,&mut entries)(15 action)→other 错误→D-112 尾门禁;每个 action 独立函数在 tracker/actions.rs(list/get/raw_lines/repair_reused_id/repair_missing_id/void_id/archive/add/update_close/reorder/raw_delete/reopen/fix_terminal/archive_fill/normalize)。验收③全仓测试绿:cargo test --workspace 全绿(T-1786708021);cargo clippy --workspace --all-targets -- -D warnings 全绿;cargo fmt --check 过。边界零改动:kanzei_tools::tracker::xxx 全部 pub use 再导出——kanzei-app docs.rs(dependency_states_from_documents/dependents_map_with_states/schedule_for_display_with_states)/auto_run.rs(backlog_status)/processes.rs(append_progress)/harness_ext.rs(TrackerTool)、kanzei main.rs(backlog_status/TrackerTool)调用点零改动;行为零变更:kanzei-tools lib 241 passed/0 failed(每批验证),提交 c2204b4/232c1a6/e57e6f5。

## R-227 占位符测试 ID 提交门禁:tracker diff 出现 T-…xxx 即拒,存量 8 处回填或标注不可考 [done]
- 内容: commit 门禁扫描 tracker 文件 diff,出现 T-\d+xxx 形态的占位符测试 ID 即拒绝提交;配套要求 test_record 落盘后与引用它的证据同批入库,消灭隔时凭记忆写证据;存量 8 处(requirements-archive 2 处、defects-archive 6 处)回填真值或标注不可考
- 复杂度: 小
- 来源: 2026-08-13 自举复盘(R-198/R-199 关闭证据均含占位符,复发模式)
- 标签: 流程
- 验收: ①门禁单测覆盖占位符拒绝;②存量 8 处处置完毕;③新增关闭证据无占位符
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-227
- 进展: 2026-08-16 取活。B1 完成(commit 635db58):①commit 门禁 placeholder_id_gate(git.rs)——tracker 文件 diff 出现占位符测试 ID(T- 加数字再跟三个 x)即拒,只扫 tracker 路径,真实 10 位 ID 放行;②归档回填通道 fill_archived_placeholder(docstore.rs,与 dedupe 同锁同写路径,恰好命中一次,歧义拒绝)+ tracker 动作 archive_fill + CLI 分支 kz req archive_fill。验证:kanzei-tools 332 passed + kanzei 4 passed(T-1786631611)+ fmt/clippy 全过。| 2026-08-14 验收②完成:引擎已重启(kzapp pid 28956),archive_fill 通道可用,存量 8 处占位符全部回填真值,每条真值先核对 tests-archive 记录标题再写——requirements-archive:R-198→T-1786565346(cargo test --workspace R-198 关闭前全量)、R-199→T-1786565831(R-199 关闭前全量,原占位符前缀 1786566 系手写笔误,真值为 1786565831);defects-archive:D-219→T-1786451434(D-219 冒烟:2 阻塞 doing 不误拒新条目)、D-266→T-1786560588、D-279→T-1786562463(cargo test -p kanzei-tools --lib profiles)、D-281→T-1786562856、D-282→T-1786563655、D-316→T-1786564679(原占位符前缀 1786563 系笔误,真值 1786564679),未特别标注者均为该缺陷关闭前 workspace 全量。回填后全仓扫描两份归档:零占位符残留。本条 进展 字段同批重写,清掉自身携带的 8 个占位符字面量(否则门禁会拒绝任何触及本行的提交)。三条验收逐条对照:①门禁单测覆盖占位符拒绝——placeholder_id_gate 单测(B1,T-1786631611);②存量 8 处处置完毕——8/8 回填且真值与 tests-archive 记录标题逐条对上;③新增关闭证据无占位符——门禁在 commit 层机械拦截。
- observed_head: 79ab205c7fa101fab4fc20153ce1e86dc089f55d
- observed_worktree_hash: fnv1a64:dbc4e711d4d470fa
- recorded_at: 1786709613992

## R-202 run_task 与 run_once_with_parts 内部分段拆分:补登 monolith_decomposition 的「另立条目」承诺 [done]
- 优先级: P3
- 复杂度: 大
- 标签: 后端 核心
- 来源: 2026-08-12 八维度审计(§1);monolith_decomposition.md:25/69/192 三处写明两函数「只整体搬迁,内部拆分另立条目」,从未登记,现已分别涨到约 1010 行(app/run.rs:26-1035,20+ 参数挂 too_many_arguments)与约 987 行(core/runner/drive.rs:47-1034)。
- 内容: run_task 按 装配/事件循环/轮末收尾 三段抽函数;run_once_with_parts 按 请求重试/工具批执行/收尾 分段。
- 边界: 行为零变更;外部签名与 pub API 不变;不与功能改动同批。
- 验收: ①每段可独立单测;②cargo test --workspace 全绿;③两函数主体各降到 300 行以下。
- refs: R-153 R-155
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-202
- 批次: 7/7
- 进展: 批7 完成(2026-08-16),条目收口。验收对照:①每段可独立单测——drive.rs 新增 #[cfg(test)] mod tests,7 个单测覆盖 commit_step_messages(纯文本步 final_text 更新+assistant 落库/有工具调用 Proceed/停止置位取消占位收尾)与 finalize_step(正常落库 Continue/MaxTokens Return{false}/last_step Break/停止 Return{true}),全部独立构造输入验证输出(cargo test -p kanzei-core runner::drive 7 passed);run_task 侧段函数由既有 kanzei-app 测试覆盖。②cargo test --workspace 全绿——T-1786728314:kanzei-core 193、kzapp 160、kz 133、harness 44、llm 128、tools 242 等全部 ok 0 failed(首轮 kzapp 认领回滚测试 flaky,重跑通过,与本次改动无关)。③两函数主体 <300 行——run_task 266 行(批2)、run_once_with_parts 262 行(批6)。边界:外部签名/pub API 逐字节不变,全部抽取为私有内部函数;行为零变更每批核对(事件顺序/停止语义/压缩触发线/对齐不变式)。提交 8c82a33;push 已到 origin/dev;游离空行已清理。关闭。
- observed_head: 8c82a33e0512f799eedf3d10be164e5a8305e510
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786728349760

## R-183 kz 无人值守执行通道:非交互直接放行 bash + 可审计轨迹(原「预授权集」随 D-267 作废) [done]
- **2026-08-11 改写(用户定调,随 D-267 关闭为 dropped)**: 原标题里的「permission 规则 worktree 继承主根、可审计预授权集」两项**作废**——它们服务的是 D-267 的中间档,而中间档已被砍掉(理由见 D-267 关闭说明:挡不住有意的、被绕过两次、威胁模型里没有「模型是敌人」)。**本条大幅缩小**:非交互模式下 bash 直接放行,防线整体挪到结果侧(R-186)。
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 实测三次全失败 + 读码定位)
- refs: R-182 R-177 R-030 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 搭任务级并行实测时,**`kz run` 在 worktree 里无法无人值守跑**,是当天唯一让实验彻底停摆的硬卡点(另一个候选载体 `claude -p` 因 OAuth token 被吊销同样不可用)。任务级并行的前提是「N 条线各自跑到底」,没有这个通道就只能靠外部 CLI。
- 现状与缺口(逐点读码核实): 
- 内容: ①非交互检测 + 显式策略:无 TTY 时不再落 Deny,改为按配置的**非交互默认策略**(建议三态:`deny`(现状,保守) / `rules-only`(只认预授权规则,规则外拒) / `allow-listed`(规则 + 本次运行的显式 allowlist));策略必须显式配置,**不提供"全放行"的隐式默认**。②permission 规则的 worktree 继承:worktree 内运行时,规则匹配按**主根**而非 cwd 解析 workdir(与 R-182 的主根重定向同一条原则),避免线一启动就没有任何授权。③可审计:非交互模式下每一次自动放行都落轨迹(动作、资源、命中的规则、时刻),`kz` 退出时给出汇总;拒绝同样可见(D-004 口径)。④补齐开发所需的基础规则模板(cargo/node/git 的只读与构建子集),放进新建配置的注释模板(与 R-172 同族)。
- 边界: 不做「全部自动同意」的开关——那等于把权限系统关掉,与仓库既有的硬 deny 纪律冲突。不改 profile/agent 体系。不做桌面端的无人值守(桌面端有 UI 可问,不是同一个问题)。
- 验收: ①`kz run` 在 worktree 里后台运行(stdin 关闭)能完成一次真实的「改代码 → `cargo test` → 提交」闭环,不因权限被拒而中断;②非交互默认策略三态各有测试,**缺省仍是 `deny`**(不改变现有用户的行为,旧配置无该键时行为不变);③从 worktree 运行时,主根的 permission 规则能命中(有测试直接断言同一条规则在主根与 worktree 下匹配结果一致);④每次自动放行有可查轨迹,含命中的规则原文;⑤无 TTY 检测本身有测试(不是靠"读到 EOF"倒推)。
- 依赖: 
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-183
- 进展: 批3 完成(2026-08-16),条目收口。验收对照(逐项引用实现位置):①`kz run` 无 TTY 无人值守闭环——E2E `crates/kanzei/tests/always_allow_bash.rs:92 cli_allow_listed_executes_bash_without_tty`(用户 D-363 落地,非交互 allow_listed + `--allow bash:*` 下 bash 真执行、本轮成功,反证 --allow 一次性不落常驻规则);worktree 后台跑的前提由验收③覆盖。②三态各有测试、缺省 deny——`crates/kanzei-harness/src/config.rs` `non_interactive_三态解析`/`non_interactive_缺省与非法取值_fail_closed回落deny`(缺键/空串/非法全回落 Deny,旧配置不变)+ main.rs 决策三态测试。③worktree 主根规则命中——`crates/kanzei-tools/src/bash.rs:30 permission_workdir_view`(worktree_key 存在时 workdir 视图按主根,仅权限判定文本、执行目录不变)+ 测试 `权限workdir视图_worktree映射回主根`/`同一规则_主根与worktree下匹配结果一致`(7db3b48)。④自动放行轨迹含规则原文——`permission.rs evaluate_with_rule`(last-match-wins 命中规则,硬 deny/无匹配 None)+ `event.rs:84 PermissionResolved.rule` + drive.rs 串行门禁/并行 wave 两评估站点填 describe_rule + CLI 打印 `[规则: ...]`(ba0726f)。⑤无 TTY 检测有测试——`main.rs interactive_stdin()`(stdin.is_terminal 显式检测,不靠读 EOF)+ non_interactive_decision 测试(批1,caa9d62)。全量:cargo test --workspace 全绿 0 failed(T-1786737712 复核,含 D-363 桩服务器超时修复)。边界遵守:无全放行隐式默认(allow_listed 需显式 --allow)、不改 profile/agent、不做桌面端。关闭。
- observed_head: 87471a232e7946d358ecb7e21565318f8961d42f
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786737755878
- 进展: [reopen 2026-08-14] D-359 修复后用正路退回:原阻塞(让位 D-332)的解除条件早已达成(D-332 已 fixed 归档);本条 doing 是 engine 自动认领留下的空档,进展字段自述从未开工、无进展锚点。退回 todo 按 P0 重新入队,不再靠往阻塞字段塞理由把它挪出 WIP 槽。
- 批次: 3/3

## R-238 大文本交付通道:bash 命令行超长防护 + kz run 文件入口 [done]
- 内容: ①bash 工具执行前检测命令串长度,超过 Windows 命令行上限(32767 字符,按 30000 留余量判)直接返回结构化错误,不把命令交给 PowerShell 去 spawn 失败;错误文案给出两条正路——大文本先用 write 工具落文件再在命令里引用路径,或改用 ②。②kz CLI `run` 新增 `--prompt-file <path>`:从 UTF-8 文件读取 prompt,与位置参数互斥、可与 --new/--readonly 组合——自举/验收代理喂长材料从此有正门,不必塞 argv。③conventions 增补一条纪律:>8k 字符的文本不进命令行参数,一律文件中转(与①的错误文案同源,一处改两处跟)。
- 来源: 2026-08-14 R-236 验收轮实测:自举代理把约 43 万字符的 prompt 塞进 `cargo run -p kanzei -- run --new <prompt>` 的命令行参数,Windows 32767 上限导致进程 spawn 失败(475ms 退出、PowerShell 异常 5 行、first.out 为空),连续多次同型试错;后续又因 write 工具大内容 JSON 失败绕路。根因是大文本没有交付正门,只能靠代理自己撞出来。Claude 接管联测时用「调小 context_limit + 分轮小消息」绕开了,但坑还在,下一个长输入场景会复发。
- 复杂度: 小
- refs: R-236 D-342
- 标签: 核心
- 验收: ①构造 >32767 字符的 bash 命令,工具返回结构化错误且文案含「文件中转」与「--prompt-file」指引,不发生真实 spawn(单测);②`kz run --prompt-file` 从文件读 prompt 跑通一轮(fake server 集成测试即可),文件不存在/非 UTF-8 有明确报错,与位置参数同给时拒绝;③conventions 文本落地,grep 单一来源;④现有 bash 短命令行为零回归(既有测试全绿)。
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-238
- 批次: 1/1
- 进展: 完成(2026-08-16)。验收对照:①bash 超长防护——kanzei-tools/src/bash.rs MAX_COMMAND_CHARS=30000,execute 在 D-113 门禁后按 UTF-16 代码单元计数(Windows 32767 上限口径),超限直接 ToolOutput::error 不发生 spawn;文案含「文件中转」与「--prompt-file」两条正路;单测 `超长命令_结构化拒绝不spawn且文案给两条正路`(32000+ 字符)。②kz run --prompt-file——main.rs RunArgs.prompt_file + parse_run_args 消费;resolve_run_prompt 纯函数(位置参数互斥拒绝/文件缺失·非 UTF-8 明确报错/正常读取),run_cli 接入;集成测试 cli_prompt_file_feeds_big_prompt_without_argv(mock server 单轮,>8k 字符文件 prompt 跑通一轮);单测 4 个(读文件/互斥/缺文件报错/flag 解析)。③conventions §4 追加纪律「>8k 字符的文本不进命令行参数,一律文件中转」,与 bash.rs 错误文案同源同指向(write 落文件引用路径 / --prompt-file)。④零回归:bash 16(含既有短命令)、kanzei 30、always_allow_bash 4 全过(T-1786738142);fmt/clippy 绿。关闭。
- observed_head: 87471a232e7946d358ecb7e21565318f8961d42f
- observed_worktree_hash: fnv1a64:8d4d9b85f60953ff
- recorded_at: 1786738153383

## R-239 记忆自动轮采纳率与空轮比例的正常节奏复测(排除 R-226 样本偏置) [done]
- 优先级: P3
- 内容: R-196 复核发现:修复后(08-12~08-14)自动轮采纳率 2.2%(基线 21.0%)、空轮比例 93.3%(基线 67.6%),两项未改善;但样本高度偏置(R-226 单条线自动推进轮占 22/45,全部 0 fetched),无法区分『修复①无效』与『样本失真』。本条在正常开发节奏(多任务并行、用户轮与自动轮混合)下再测一轮:自动轮 >=50 轮样本,重跑同一组查询对照采纳率与空轮比例,判断修复①检索键切换是否真的无效,还是 R-226 轮次形态造成的假象。
- 复杂度: 小
- 来源: R-196 复核结论(2026-08-16):指标①②未改善,样本偏置需在正常节奏下复测
- 标签: 核心
- 边界: 只做度量与结论,不改实现;若样本仍偏置,继续记录原因,不在本条追修。
- 验收: ①样本为正常开发节奏(非单条线密集自动推进)的自动轮 >=50 轮,写明轮次构成(标题分布/间隔);②重跑与 R-196 同口径的采纳率与空轮比例对照,给出与基线(21.0%/67.6%)和 R-196 修复后值(2.2%/93.3%)的三点对比;③若仍未改善,写明判断原因并指向修复条目标号;若改善,记录修复①实际生效的样本条件。
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-239
- 批次: 1/1
- 进展: 完成(2026-08-16),结论:修复①实际生效,R-196 修后 2.2%/93.3% 确认为样本偏置假象。验收对照:①样本——生产遥测 state.db recall_events(legacy index.db memory_recalls 已于 R-125 停写,kanzei-memory/mod.rs:1903),按 episode_id 聚合真·轮次;窗口 2026-08-12~08-15(本地)自动轮 59 轮 >=50;轮次构成:取活标题轮(query=R-xxx 标题)R-185/175/180/176/223/228/229/214/195/196/172/203/207/202/183 等 55 轮 + 自动推进轮(继续推进)4 轮,间隔分钟~小时、多任务混合(含用户反馈穿插),排除 R-226 单条线密集空转形态。②三点对比(采纳率/空轮):基线 21.0%/67.6% → R-196 修后 2.2%/93.3% → 本次 59/59 有注入(100.0%)、空轮 0(0.0%)。口径说明:生产 injected_ids(注入 context)与 legacy fetched(模型采纳)语义略异,但「空轮」(整轮无注入 vs 整轮无拉取)可跨源对照,0% vs 93.3% 差异足以定性。③结论:改善——正常节奏下取活标题检索键(R-196 修复①,f104890)命中并注入率 100%;R-196 修后 2.2%/93.3% 来自 R-226 单条线自动推进轮(22/45,不走取活、检索键无信息量),非修复无效。边界遵守:只度量与结论,未改任何实现代码(统计脚本为临时文件,已清理)。关闭。
- observed_head: fc0c559d3a92346d8dfbe50c8aa46b84dfa02dde
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786738575095

## R-240 细化运行完成指标统计 [done]
- priority: P2
- 原始描述: 能更详细的查看各类运行和完成过程种的指标，比如做完不同种类的不同复杂度需求使用的token，方便我们针对上下文和harness等进行优化
- 复杂度: 中
- 归属: kanzei
- 标签: 流程
- 验收: 可按需求类型与复杂度查看运行及完成过程指标，并统计所用 token，支持上下文与 harness 优化分析。
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-240
- 批次: 3/3
- 进展: 批3 完成(2026-08-16),条目收口。验收对照:可按需求类型与复杂度查看运行及完成过程指标并统计 token——①后端 `crates/kanzei-app/src/run.rs` run_metrics_by_category 命令(invoke_handler 已注册):extract_ticket_id 从 prompt_head 提取 R-/D- ID,ticket_complexity 解析 requirements.md/defects.md 复杂度(小/中/大),aggregate_run_metrics 按 (类型, 复杂度) 聚合 count/sum·avg input·output·steps + uncategorized(单测 3 个:ID 提取/复杂度解析/聚合分组,cee6af1);②前端 `ui/13-memory.js` renderMetricsCategories 在运行画像面板渲染「按分类聚合」表格(index.html metrics-categories 容器 + i18n 8 键 + style.css 样式,bb07501);③token 统计来自 episodes 表 input_tokens/output_tokens(每轮持久化)。全量:cargo test --workspace 全绿 0 failed(T-1786739400,含 kzapp 163/core 193/harness 138/llm 44/base 128/tools 245)。前端冒烟 runtime/i18n/lint/a11y 全过(T-1786739120)。关闭。
- observed_head: bb075018df4cd6aa41280be52573ca15bc96b4d4
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786739410865

## R-244 统一 Tool Pipeline：Policy、单调 Guard、Wrapper、Result Policy 与 Observer [done]
- refs: D-209 R-180 R-174 docs/design/deepseek_harness_upgrade.md
- 内容: 在 kanzei-harness 建立固定工具阶段 parse/materialize→policy allow/deny/ask→monotonic guards→execution wrappers→tool body→result policies→immutable observers；复用现有 Ruleset 普通规则、hard_denies、managed fence、timeout、progress、cancellation、recall 与 trace，不重写规则引擎。
- 前置: R-241
- 复杂度: 大
- 批次: 5/5
- 来源: DeepSeek Harness tool execution pipeline 对照；Kanzei 当前阶段散落在 drive.rs 和工具内部。
- 标签: 核心
- 边界: 现有权限行为必须逐条保持；hard deny、托管文件与 writer ownership 属不可逆 Guard，后续 hook 不得放宽。Observer 只能观察最终结果，不得修改 ToolOutput 或反向影响执行。第一批仅迁移一个无副作用工具验证流水线，再分族迁移。
- 验收: ①每阶段有独立契约测试且顺序固定；②现有 Ruleset/hard_denies 回归逐字节一致；③policy allow 不能覆盖 Guard deny，有反证测试；④timeout/cancellation/progress 只在 wrapper 实现一处；⑤observer 抛错不改变工具事实终态但留下遥测；⑥至少 read/bash/git/子代理工具走统一通道且无双执行；⑦失败、拒绝、取消路径都产生唯一 final result。
- 优先级: P1
- 进展: 批5 完成(2026-08-16),条目收口。验收对照:①每阶段独立契约测试且顺序固定——harness tool_pipeline.rs 5 测试(guard 拒绝不执行 body/阶段顺序+result policy/observer 抛错不改终态/失败拒绝唯一结果/body 恰好执行一次无双执行);②Ruleset/hard_denies 回归逐字节一致——permission.rs 零改动,全量绿(harness 143 含 permission 30);③policy allow 不能覆盖 guard deny——guard 拒绝反证测试(拒后 body 不执行);④timeout/cancellation/progress 只在 wrapper 实现一处——progress 现统一在 runner 层(drive 串行旁路 + tool_exec 并行通道),timeout 在 bash body(全仓唯一),pipeline Wrap 阶段已预留(ToolPhase::Wrap);**字面收敛进 Wrap 为残余,已登记 R-259**;⑤observer 抛错不改终态但留遥测——catch_unwind + warn 测试;⑥read/bash/git/子代理走统一通道且无双执行——glob/read/bash/git 迁移(execute 调 run_tool_pipeline,body 独立函数),SubagentBase 只读族(read/glob/grep)全走通道,无双执行契约测试;⑦失败/拒绝/取消都产生唯一 final result——guard 拒绝/body 失败唯一结果测试(取消占位在 runner 层,既有 R-202 覆盖)。迁移提交:51c797d(B1 骨架)/84461ec(B2 read)/51ddb45(B3 bash)/d196c96(B4 grep)/ebd1b64(B5 git+无双执行)。全量:cargo test --workspace 全绿 0 failed(T-1786741238)。残余转移:R-259(pipeline Wrap 阶段收敛 timeout/cancellation/progress)。关闭。
- observed_head: ebd1b642ca31f7764d55bae8dbb1ca85909737e2
- observed_worktree_hash: fnv1a64:d2e058c0c37b9281
- recorded_at: 1786741286441
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-244

## R-260 侧边栏刷新机制问题 [done]
- 原始描述: 左侧的在做侧边栏的任务。似乎刷新的机制有问题并没有有及时刷新
- 复杂度: 中
- 标签: 前端
- 验收: 左侧菜单能够正常进行数据/内容的刷新的，不会滞留旧的内容或失效的数据
- 优先级: P2
- 状态: doing
- 进展: 验收对照:验收原文「左侧菜单能够正常进行数据/内容的刷新的，不会滞留旧的内容或失效的数据」——达成。根因:侧边栏「并行任务」列表(#parallel-task-status)数据源为 process_list,但全仓无 process_list 定时轮询(01-core.js L73 注释声称「process_list 轮询」实际无定时器;09-sessions.js renderProcesses L391 校正逻辑假定「下一次轮询」兜底事件丢失;后端 processes.rs 无进程级 emit)。事件偶发丢失、外部创建/注销进程、列表结构变化时,侧边栏滞留到下次手动操作。修复(crates/kanzei-app/ui/01-core.js):加 process_list 3s 定时轮询 setInterval,typeof refreshProcesses 守卫,refreshProcesses 内部按项目单飞去重(processRefreshInFlight),轮询安全;事件驱动路径(kz:* 事件投影)保留不动,轮询补齐兜底。验证:node --check + ui-runtime 21 项 + ui-lint 31 文件零错 + i18n/a11y/markdown 冒烟(T-1786744592)、kanzei-app 163(T-1786744653)、全量 workspace 全绿(T-1786744744)。
- observed_head: 1cea2a86d9808bb0996f90cdcfa64e0769d395c4
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786744753379

## R-261 提交门禁路径效率优化:纯前端改动免 Rust 测试背书 + fmt/clippy 门禁并行 [done]
- 内容: ①纯前端改动(仅 ui/*.js/css/html 前端资源,无 Rust 源码)提交时,source_test_gate 不再归因为 kanzei-app 的 Rust source,不要求 cargo test -p kanzei-app——前端冒烟集(node --check + ui-runtime/lint/i18n/a11y/markdown,R-228 已强制前端标签条目关闭前有 passed 冒烟)背书即可。本轮 R-260 改 10 行 js 被拦 2 次、被迫重跑 163 个 Rust 测试,零信息量。②staged 含 Rust 源码时规则不变(仍按 crate 要求测试背书,R-212 守护「前端冒烟不能背书 Rust 改动」保持)。③提交门禁 fmt_gate 与 clippy_gate 并行执行(互不依赖,join 合并错误),门禁总耗时下降。
- 复杂度: 中
- 来源: 2026-08-15 用户反馈:「现有的测试和提交的路径似乎可以在优化一下效率」,经调研确认两个低效点后用户拍板方向(问题1-A/B)
- 标签: 流程
- 验收: ①纯前端改动(仅 ui/ 资源)提交不再被 source_test_gate 拦(不要求 cargo test -p kanzei-app),前端冒烟 passed 记录背书即可;②staged 含 Rust 源码时行为不变:仍要求对应 crate 测试背书,守护测试 source_test_gate_frontend_smoke_cannot_back_rust_change 全绿;③fmt/clippy 门禁并行执行,提交门禁不因串行多等;④既有 commit 门禁守护测试全绿 + kanzei-tools 定向测试全绿。
- 优先级: P2
- 取活依据: override:用户 2026-08-15 明确拍板方向(问题1-A/B:纯前端改动免 Rust 测试背书 + fmt/clippy 门禁并行),R-261 为已确认的优化需求,需立即实现
- 进展: 验收逐条对照:①纯前端改动(仅 ui/ 资源)提交不再被 source_test_gate 拦——守护测试「纯前端ui资源不算rust源码_门禁放行而rust源码规则不变」验证 source_test_gate 对纯前端 staged 放行、staged_source_fingerprint 为空;②staged 含 Rust 源码时行为不变——is_source_path 对 .rs/scripts 仍为 true,R-212 守护 source_test_gate_frontend_smoke_cannot_back_rust_change 全绿;③fmt/clippy 门禁并行——commit 门禁与 finalize 均改 tokio::join! 并行执行;④既有 commit 门禁守护测试全绿 + kanzei-tools 251 定向绿(T-1786745568) + 全量 workspace 全绿(T-1786745728)。实现位置:crates/kanzei-tools/src/git.rs is_source_path(L466 排除 crates/kanzei-app/ui/)、commit 门禁(L868-874 join!)、finalize(L930-941 join!)。
- observed_head: 87f5b4c5cc3c93b3d611a63c4463ef2e810ebeb2
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786745736067

## R-262 task 子代理并行派发引导:强化工具描述引导同轮多派独立勘察 [done]
- 内容: 引擎已支持同轮多 task 并行(run_subagent_calls FuturesUnordered,max_tasks_per_turn 默认 16,测试证明 20 并行可执行),task 工具描述已有「Multiple task calls in one turn run in parallel」——但主模型使用习惯是每次只派一个 task,串行勘察效率低。优化:强化 task 工具描述与系统提示,明确引导「把相互独立的勘察/查找拆成多个 task 同一轮并行派发(上限 max_tasks_per_turn),并行显著提速」,让模型从习惯单派转向习惯多派。
- 复杂度: 小
- 来源: 2026-08-15 用户反馈:「每次只派一个其实效率很低可以考虑把派子代理的并行强度提高一点」,经调研确认非引擎限制而是模型使用习惯,用户拍板方向(问题2-A)
- 标签: 核心
- 验收: ①task 工具描述包含明确的并行派发引导(独立问题拆多个 task 同轮并行,点名上限);②系统提示/约定无与此矛盾的单派建议;③既有 task 并发测试(max_tasks_parallel_dispatch 等)全绿;④自举环境实测同轮多派可达(非只跑单测)。
- 优先级: P2
- 取活依据: override:用户 2026-08-15 明确拍板方向(问题2-A:强化 task 工具描述/系统提示引导「独立勘察拆多个 task 同轮并行派发」),R-262 为已确认优化需求,继续实现
- 进展: 验收逐条对照:①task 工具描述含明确并行派发引导——crates/kanzei-core/src/runner/subagent.rs task_spec() description 新增「独立勘察/查找问题拆成多个 task 调用在同一轮派发(上限 max_tasks_per_turn),并行显著快于串行勘察」,取代原有单句说明;②系统提示/约定无矛盾单派建议——全仓搜索「一次只派一个/串行勘察/one at a time」仅命中本次新增引导文本自身;③既有 task 并发测试全绿——max_tasks_parallel_dispatch(20 并行实测)+ parallel_scouting_under_serial_writer + core runner::subagent 7 项全过(T-1786745928);④自举实测——本轮开头双 task 并行调研即实测同轮多派可达,且 R-174 测试证明 20 并行可执行。引擎侧本就支持同轮多 task(FuturesUnordered,默认上限 16),本项补齐使用引导。
- observed_head: 87f5b4c5cc3c93b3d611a63c4463ef2e810ebeb2
- observed_worktree_hash: fnv1a64:255559cce223d55d
- recorded_at: 1786745938931

## R-251 试用手册配置移至设置模块 [done]
- 复杂度: 小
- 标签: 流程
- 验收: 试验相关配置已迁移至主界面→设置→高级功能区域
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-251
- 取活释放: line=kanzei/thread-line-1786750035674-1;reason=parallel-line-unregister;at_ms=1786750192782
- 进展: 2026-08-16 交付(d590d9e):设置页「版本与更新」后新增「高级功能」settings-group,内含「对话顶部显示使用手册」checkbox(set-show-manual);15-views-misc.js 新增 MANUAL_SHOW_KEY/readManualShowPref/saveManualShowPref(localStorage 本地偏好,默认开,参照 R-187 sound),refreshManual 顶部检查——关闭则隐藏面板且不读 docs/目录.md;16-settings.js loadManualShowControl 回填 + change 即存并触发 refreshManual;02-i18n.js 登记 4 词条;ui-lint-globals.json 由生成器同步;ui-runtime-smoke.mjs 新增断言(关闭→隐藏且不读文件,重开→恢复)。验证:node --check + ui-i18n(1151 key)/ui-lint(591 全局)/ui-a11y/ui-runtime 冒烟全绿 + cargo test -p kanzei-app 163 passed(T-1786758664/T-1786758725)。既有 R-147 手册渲染链路未动,开关关闭时保持原隐藏逻辑。
- observed_head: d590d9ea992c2c3932f54f7f48b85902126c43b0
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786758751043

## R-252 目标区改造成原始想法收件箱:新建 IDEAS 文档线、退役 goal、拆解由人点触发子代理 [done]
- 内容: 把「目标」区改造成用户侧的原始想法收件箱:录入未经拆解的设计需求/想法,再由人点一下派子代理拆成 R-xxx / D-xxx。①新建 IDEAS 文档线(前缀 I,状态 inbox/split/dropped),不复用 GOALS 换语义——goals 线同批退役(现存 G-001~G-003 推 dropped 并归档);②录入不过模型,原样收下(用户想法的原话就是最有价值的部分,过一遍 fast 模型只会磨平);③拆解由人点按钮派子代理(idea_split 命令,照 quick_req 的模式:写租约 + 组件挂 req/defect/idea + before/after 差集取真实新增 ID),不做自动拆解;④转 split 时硬门禁:refs 必须非空且每个 ID 在 requirements/defects 的活跃或归档里真实存在,否则「已拆解」就是一句空话;⑤想法只把计数与标题注入 agent 每轮上下文,不注全文(避免未拆解的想法污染取活)。
- 备注: 本条与其余九条一起勘察,唯独它需要动 13 个文件,其中 crates/kanzei-app/src/run.rs 的动作表有一行 goal→idea——那是与后端自举线唯一抢文件的地方。用户 2026-08-14 拍板:其余九条本轮做完发版,本条另登需求进队列,等 R-202 收尾后单独做。完整勘察(文件锚点/DOM/状态机/门禁设计)见会话记录。
- 复杂度: 大
- 来源: 2026-08-14 用户提的十条前端改造之六。原话:目标现在似乎没用?目标区可以改成我们用户侧输入的一些比较原始的设计需求想法,也就是待拆解成需求和缺陷的源。勘察证实 goals 线确实零消费者:取活引擎(work.rs)不看目标,鞭挞的推进指令(auto_run.rs)只点名 requirements.md/defects.md,前端除了渲染三条也没有别的用途。
- 标签: 核心
- 验收: ①IDEAS 文档线可增删改查,状态机 inbox→split/dropped 有测试;②goal 线退役:现存三条推 dropped 并归档,tracker/CLI/前端/managed_fence/记忆控制平面里的 goal 全部改指 idea,全仓 grep 零残留;③转 split 的 refs 硬门禁有正反测试(refs 空拒、指向不存在的 ID 拒、指向归档条目放行);④前端:侧栏「目标」区换成「想法」,有录入入口与「拆解」按钮,拆解后显示产出的 R/D 编号;⑤idea_split 子代理跑通一次真实拆解(fake server 集成测试即可);⑥取活引擎不看想法(work.rs 不动),鞭挞的推进指令也不点名想法队列——想法不是待办。
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-252
- 批次: 5/5
- 进展: 全部批次完成。B1(1b24966)IDEAS DocKind+状态机+全后端 goal→idea;B2(b7eef3a)split refs 硬门禁接线+3 正反测试;B3(47c5755)前端想法区+拆解按钮+i18n;B4(b3cd502)idea_split 子代理命令+fake server 集成测试;B5(2026-08-16)goal 线数据退役:goals.md/goals-archive.md 由用户手动删除(goal 工具已退役,无写通道——见 D-370),遗留 G-001~G-003 随文件删除不再存在。关闭前全量 cargo test --workspace 全绿(T-1786765114)。验收逐项:①IDEAS 可增删改查+状态机测试(docstore.rs IDEAS/ideas_state_machine_inbox_to_split_or_dropped、profiles.rs idea 工具、main.rs CLI、docs.rs);②goal 退役+全仓 grep 零残留(goals.md 已删、G 条目不存在);③refs 硬门禁正反测试(tracker.rs idea_split_refs_gate_*);④前端想法区+录入+拆解按钮+refs 展示(index.html/11-docs-list.js/ui-runtime-smoke 断言);⑤idea_split fake server 集成测试(subagents.rs idea_split_runs_subagent_and_marks_idea_split_with_real_refs);⑥work.rs 未动、auto_run 不点名想法(profiles.rs dev/ideas 只注入计数+标题)。
- observed_head: b3cd5029a12118365def9fe5a4e6e63e05aca2b6
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786765134704

## R-253 run.rs 二次拆解:2885 行生产码切成装配/协调/执行/事件汇/持久化,models_list 与 summarize_chat 等非编排 IPC 迁出 [done]
- refs: R-153 R-155 R-202 docs/design/monolith_decomposition.md docs/design/monolith_decomposition_round2.md(批次地图:A 节)
- 为什么是这个形态: 不是"文件大",是整个桌面 Agent Runtime 的 application service 树被压进一个 .rs。call tree 本身合理(run_prompt 到 run_task 到 assemble/execution/persist),问题在于旁边还夹着 models_list/summarize_chat 这类与运行编排无关的 IPC;而 build_event_handler 把 UI 投影/typed event 持久化/trace/metrics/LiveRun 五种投影揉成一个 giant reducer——加一个 RunEvent 就要读懂整个 runtime。R-153 把 app/main.rs 6413 行拆出 run.rs 时它还只是"运行主链路",此后 memory/scout/review/phase pipeline/write lease/子代理/autonomous 逐个叠进来,重新长成 attractor。
- 内容: ①先迁非编排 IPC(纯搬迁零风险):app_info/models_list/summarize_chat/stop_run/stop_task/pending_asks_get/answer_ask/run_metrics 移出到 commands 侧模块;②build_event_handler 按投影拆 sink——UiEventSink/TypedEventSink/TraceSink/MetricsSink/LiveRunSink + 一个 fanout 广播,新增 RunEvent 只碰对应 sink;③assemble_run 按生命周期切三层——RuntimeDeps(不变依赖:config/profile/harness/agent/model/route/client/RunnerConfig)、SessionContext(会话事务:SessionStore/create session/admit input/attachment/TypedEventWriter/flush task)、RoundContext(单轮:run id/timing/trace/pipeline/write lease/身份);严禁做成一个 28 字段的 RunContext,那只是把 parameter monolith 换成 context monolith;④persist_round_outcome + finalize_round 独立成 persistence 模块(怎么跑 与 跑完怎么落库 是两个变更理由);⑤run_execution_loop 的隐式流水线(recovery→attachment→memory 预检索→scout→run_once→review/fixup)与 review/fixup 的 primary→critic→corrective 复合阶段,给出显式输入输出边界;⑥build_subagent_runtime 独立成模块。
- 复杂度: 大
- 来源: 2026-08-15 用户提供的第二轮巨石扫描(按当日 main 源码逐文件读 + 本轮机器复核生产行数),本条是其排序里的 R1。
- 标签: 核心
- 现状(2026-08-15 实测 dev@f09242c): crates/kanzei-app/src/run.rs 总 3268 行、生产码 2885 行(同文件测试仅 383 行),全仓生产行数第一。单文件内同住:装配 assemble_run(L79-438,359 行)、事件归约 build_event_handler(L452-762,310 行)、ask 处理 build_ask_handler(L763)、子代理 runtime build_subagent_runtime(L827)、执行流水线 run_execution_loop(L911-1057)、落库 persist_round_outcome(L1058-1246) 与 finalize_round(L1247-1771,524 行)、以及 9 个 tauri command(app_info/models_list/pending_asks_get/answer_ask/summarize_chat/stop_run/stop_task/run_prompt/run_metrics)。全仓 9 处 clippy::too_many_arguments 全在本文件,是最高密度。
- 边界: 零行为变更、零 IPC 契约变更(命令名与入参返回结构一字不动,ui/*.js 不改);不做 Desktop/CLI 合流(另立条目);不改 phase_pipeline / write lease / 记忆召回语义;不引入新的 async 抽象层或 trait 体操,普通函数与结构体能表达就不要 trait;搬迁批 diff 只允许 move + use + 可见性调整,出现逻辑 diff 即回退重来(沿用 monolith_decomposition.md 执行纪律 4)。
- 验收: ①run.rs 生产行数 ≤ 400(只留 mod 声明与装配),按生产行数口径核,不用 wc -l;②每个新模块文件头 //! 写清独立理由(照抄 files_view.rs 模式);③本文件 9 处 too_many_arguments 至少消掉 6 处,且不得靠塞进一个大 context struct——必须能指出每个新参数组对应哪一层生命周期;④kanzei-app 全量 + 四条前端冒烟 + cargo test --workspace 全绿;⑤可局部推理实测:新增一个 RunEvent 变体的改动面只落在对应 sink,给出实际 diff 作为证据,不接受"看起来更清楚了"。
- 优先级: P0
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-253
- 批次: 10/10
- 进展: 批9 完成:四条前端冒烟全过(T-1786771658:ui-runtime 21 js 按序+9 视图 0 错误 / ui-i18n 155 key / ui-a11y / ui-markdown),cargo test --workspace 15 段全 ok 约 1009 passed(T-1786771659),kanzei-app 166 passed(T-1786771585)。验收逐项:①run/mod.rs 生产码 106 行(非空非注释,测试段前;原 2885)→≤400;②模块头 //! 独立理由:run/{mod,assembly,coordinator,execution,persistence,events/mod,input}.rs 齐全(照 files_view.rs 模式);③run/ 内 too_many_arguments 由 6 处降至 1(persist_round_outcome 保留,SessionStore 非 Sync 不能收 struct 整体),原 run.rs 9 处消 7,每组参数可指出生命周期:RoundRequest(本轮输入)/RunMode(运行档位)/RuntimeHandles(会话级句柄)/ExecutionInput(本轮执行输入)/ReviewExec(执行层模型调用参数链)/FinalizeSession(会话事务)/FinalizeRound(单轮收尾)/FinalizeOutcome(本轮结果)/RoundReport(UI 汇报)——无 28 字段 RunContext;④kanzei-app 全量 + 四条前端冒烟 + cargo test --workspace 全绿;⑤sink diff 证据:events/mod.rs 每个 arm 从五投影揉合改为按 sink 方法 fanout(如 ToolEnd arm = metrics.resolve_tool_end + trace.record + ui.emit),新增 RunEvent 变体只加一个 arm 与命中 sink 的方法。边界核对:零行为变更(166+1009 测试含全部既有行为断言)、零 IPC 契约变更(ui/*.js 未改)、未动 phase_pipeline/write lease/记忆召回语义、无新 trait/async 抽象层。关闭。
- observed_head: e3d861de535b9df73960702e977afdb0263aa557
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786771872268

## R-254 processes.rs 拆解:进程注册/生命周期 与 工作树生命周期/门禁/合并/收割分家,主根与工作树根类型化 [done]
- refs: R-207 R-177 R-182 D-176 D-267 D-365 D-367 docs/design/parallel_lines_ui.md docs/design/monolith_decomposition_round2.md(批次地图:C 节)
- 内容: ①按变更理由切两组:process 侧(registry / lifecycle / persistence / commands)与 workspace 侧(lifecycle / merge / gate / harvest);②完成 R-207 的下沉收尾——本文件仍有 19 处 wt:: 转发壳,函数体只是转调 kanzei_tools::worktree,注释自述"实现已下沉",两层抽象长期并存(见配套缺陷),删壳让调用点直接用下沉后的实现;③把文件头 L3-18 那条只靠注释维持的硬不变式类型化:project_dir/origin_project 恒为主根、工作树只由 worktree_path 承担 → 引入 ProjectRoot / WorktreeRoot 两个 newtype,让 rustc 替注释站岗;④集成门禁(fmt/clippy/test/ui-smoke + 合并后主根全量)独立成模块,它是 Integration Gate 子系统,不是 Process 子系统。
- 复杂度: 大
- 来源: 2026-08-15 第二轮巨石扫描 R3。
- 标签: 核心
- 现状(2026-08-15 实测): crates/kanzei-app/src/processes.rs 总 1651 行、生产码 1628 行(同文件测试仅 23 行,真测试在同级 worktree_tests.rs 2437 行),48 个函数、4 处 clippy::too_many_arguments。它不是 Process Manager,是并行开发子系统总入口:进程注册与编号(process_index/register_process/next_process_index)、进程持久化、运行时控制(process_update/process_close/close_process)、工作树生命周期(worktree_create/list/diff/discard/reclaim)、写租约(acquire_project_write_lease)、集成门禁(gate_steps/run_gate_step/worktree_gate/worktree_post_merge_gate,fmt+clippy+test+ui-smoke)、合并工作流(merge_worktree/merge_preview/merge_and_release)、tracker 收割回写(harvest_candidates/harvest_writeback)。process_close 一个函数同时收三条生命周期:逻辑进程、执行运行时、工作区。
- 边界: 零行为变更;不改进程编号规则、state.db 落点、session_id 推导(D-176 红线)、并行线 UI 契约与 IPC 命令面;不动 git 合并策略与 merge-tree 预检;newtype 化只做 processes.rs 与其直接调用点,不做全仓路径类型统一(那会把 diff 铺到所有 crate)。
- 验收: ①processes.rs 生产行数 ≤ 400(现 processes/mod.rs 22 行代码);②wt:: 转发壳数量 0(D-365 已修+批1 搬迁保持,批2 机械 grep 复核);③主根与工作树根传反编译不过(D-367 已落地:state.rs ProjectRoot/WorktreeRoot newtype+编译期反例注释,既有能力);④worktree_tests 2448 行全绿 + kanzei-app 全量 + workspace 全量(批2 验证);⑤实跑一次并行线闭环无回归(批3)。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-254
- 批次: 3/3
- 现状(2026-08-16 实测复核): processes.rs 现 1562 行、代码 1231 行、48 函数、4 处 too_many_arguments(process_create/create_process/create_process_with_tracker/process_update)。D-365 已修:转发壳已清,现有 wt:: 调用均为业务直接调用下沉实现;D-367 已修:ProjectRoot/WorktreeRoot newtype 在 state.rs L341/346 且带编译期反例注释(验收③已满足,记为既有能力非本次交付)。未做:①按变更理由拆 process 侧(registry/lifecycle/persistence/commands)与 workspace 侧(lifecycle/merge/harvest);④gate 独立模块;验收①(≤400)与④⑤验证。批次地图:批1 mod.rs 骨架+process 侧 commands/registry 迁出;批2 process 侧 lifecycle/persistence 迁出;批3 workspace 侧(lifecycle/merge/harvest)迁出;批4 gate 独立模块;批5 全量+实跑闭环+验收 close。
- 进展: 全部批次完成。批2(3e5181d)机械核验+全量:转发壳 0(grep 单行转调模式空结果,现有 49 处 wt:: 均为业务函数内直接调用下沉实现);newtype 反例注释在 state.rs L330-338(ProjectRoot/WorktreeRoot + rustc E0308 实测证据,D-367 既有能力非本次交付);cargo test --workspace 15 段全 ok(T-1786772886);四条前端冒烟全过;worktree_tests 2448 行闭环集成测试全绿。批3:验收逐项归档并关闭。验收:①processes/mod.rs 生产码 22 行(原 processes.rs 1231 行代码)→≤400;②wt:: 转发壳 0(机械 grep 复核);③根类型化反例:D-367 已落地 state.rs ProjectRoot/WorktreeRoot newtype,编译期反例注释(既有能力);④worktree_tests 2448 行全绿(166 passed 含 processes::tests)+ kanzei-app 全量 + workspace 全量(15 段 ok);⑤实跑闭环:worktree_tests 集成测试真实 git 操作覆盖建线(带 worktree 建线/R-247 条目绑定/并发 K2)→合并(clean no-ff 631/冲突保留 662/释放 claims 417)→门禁(2386/2404 真实执行)→收割(2282/2311/2333)→关线(127/386),全部通过无回归。边界核对:零行为变更(166+全量含全部既有断言)、未改进程编号规则/state.db 落点/session_id 推导、未动 git 合并策略、newtype 只在既有范围。关闭。
- observed_head: 79a99105a948d12973957dcd90771ac62b8ba318
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786773021180

## R-135 开发与缺陷修复进度动画显示 [dropped]
- 优先级: P0

- 标签: 前端

- 进展: 2026-08-11 扫描:本条仅存标题/优先级/标签,缺 内容/验收/背景——边界不清无法开工(无验收就无从交付,违反 §1.25)。待用户补全条目内容后按序恢复取活,不占可执行槽位。

- 阻塞: 用户: 本条条目内容缺失(仅存标题/优先级/标签,缺 内容/验收/背景),边界不清无法开工(无验收就无从交付,违反 §1.25)。解除动作: 用户补全条目内容(至少验收原文)后按序恢复取活。解除人: 用户。

## R-174 子代理面板与并发度口径:独立 Running/Finished 面板、单条停止与完整 transcript [done]
- 优先级: P0
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调:kanzei 的子代理要走这个执行模型,而且比它更激进。四个轴——①后台化(跨轮存活、主代理派完不阻塞、完成发通知)②子代理能写(打破只读白名单、自持写租约)③并发度放开(远不止现在的 8)④可对话(给正在跑的子代理发消息带原上下文续跑)——**都要,但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。参照物形态(Claude Code 面板实测):独立 Background tasks 面板,分 Running / Finished N 两区;每条显示名称、类型、已运行时长、累计 token、工具调用次数、当前正在用的工具名、View transcript 链接、单条停止按钮;面板有 Clear。本条是四轴里最便宜、可独立交付的一段(可观察 + 并发度口径),不依赖后台化。
- 设计定位: 四轴分级第 1 级——先把子代理变成「看得见、停得住、查得到」的对象;后台化(R-175)与写权(R-176)在此之上叠加
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): 并发度**已经是可配项**——`max_tasks_per_turn` 在 crates/kanzei-harness/src/config.rs:59 是 `Option<usize>` 字段,:90-92 `unwrap_or(8).max(1)` 给默认 8 且**无上限钳制**;设置页已有「单轮子代理数上限」输入框(crates/kanzei-app/ui/index.html:469-470 `set-max-tasks`、ui/16-settings.js:187 与 :420、crates/kanzei-app/src/settings.rs:287-288/497/519/531);往返单测已钉死「没填的键不写进文件 / 没填走内置默认 8」(settings.rs:749-752)、serde default 单测已存在(config.rs:918-920、:936-939 越界回落 1)。因此「从固定 8 改为可配」这件事**无需再做**。
- 关键现状(本组三条需求的共同前置): 桌面端主对话**根本不注册 task 工具**——crates/kanzei-core/src/runner/drive.rs:57 只在 `subagent.is_some() && !config.execution_policy.is_serial_writer()` 时 push `task_spec()`,而 crates/kanzei-app/src/run.rs:107-108 给主对话**无条件**设 `ExecutionPolicy::ReadParallelWriteSerial`(`is_serial_writer()==true`,见 crates/kanzei-harness/src/orchestration.rs:21-23)。所以并行子代理在桌面端当前是**全禁**状态:drive.rs:410-503 的轮内并发批与 crates/kanzei-core/src/runner/subagent.rs:163-176 的读槽登记代码不可达,`max_tasks_per_turn` 配了也没有生效路径。该回归由 **R-173**(阶段编排对象)的阶段感知策略修复,是本条与 R-175/R-176 的共同前置——本条的面板与并发度实测必须在 R-173 修复后才能在桌面端取证。
- 内容: ①并发度口径收口(**不是重做配置**):复核默认 8 是否上调(用户要「远不止 8」),并在设置页把该值与「桌面端当前不生效」的事实对用户说清;溢出分支文案沿用 drive.rs:441-444 既有实现。②新增**独立「子代理」面板**——不再只作为活动面板(#bg-panel,ui/index.html:630-650)里 `bg-type-filter=agent` 的一个筛选项:Running / Finished N 分区,每条显示 名称 / 类型 / 已运行时长 / 累计 token / 工具调用次数 / **当前正在用的工具名** / 单条停止 / 打开 transcript,面板有 Clear。③单条停止通道:现状 ui/06-activity.js:261 注释明写「子代理没有单条停止通道,只能停整轮」,本条要消灭这个缺口。④可查看单个子代理的**完整 transcript**(工具调用序列 + 每次调用的入参与输出),不再只有 R-095 的摘要维度(内部调用数 / 当前步骤 / 成败 / 耗时)。
- 边界: 不做后台化(R-175)、不做写权(R-176);面板本条只需渲染**轮内并发**的子代理,跨轮存活条目待 R-175 提供数据后再接。不改 `max_tasks_per_turn` 的配置通道本身(已可用),只调默认值口径与设置页说明。
- 验收: ①并发度实测:`kanzei.toml [limits] max_tasks_per_turn = N`(N 取远大于 8 的值)后,同轮派发 N 个 task 全部执行、第 N+1 个才落 drive.rs:441-444 的溢出错误,有轨迹或日志证据;②旧配置无该键时行为不变——config.rs 既有 serde default 单测保持绿(若本条上调默认值,须同步更新 :918-920 断言并保留「缺键=内置默认」语义),settings.rs:745-752 往返单测保持绿(保存不丢字段);③面板存在且分区正确,每条的 名称/类型/时长/token/工具调用数/当前工具名 六个字段**均取自真实 RunEvent**(ToolStart/TaskProgress/ToolEnd),冒烟脚本用桩事件逐字段断言渲染出真实值而非常量占位;④单条停止真能停:点击后该子代理不再产出 TaskProgress、以「被停」终态收尾、读槽被释放,有实测证据(仅改 UI 类名/状态不算通过);⑤transcript 有真实数据源:能查看单个子代理的完整工具调用序列与每次调用的入参/输出——§1.25 明令「只展示但未接入真实数据源的界面壳不算完成」,不得以摘要冒充 transcript;⑥前端改动有冒烟断言:`node --check` + `node scripts/ui-runtime-smoke.mjs`,分区切换、单条停止、打开 transcript 三个新交互各有对应断言(§1.3);⑦桌面端可达性:R-173 修复前置回归后,在桌面端主对话实测面板真出现子代理条目(不能只在 CLI 或单测里成立)。
- refs: R-095 R-117 R-173 R-175 R-176
- 依赖: 
- 进展: 批1-3已提交(9179ae8/68ee84ec/25ea2c0),cargo test --workspace 全量全绿。验收①并发度实测✓(集成测试+轨迹)、②旧配置无键行为不变✓(serde default 测试绿)、③面板分区与六字段真实数据✓(冒烟逐字段断言)、④单条停止✓(stop_task + task_cancel_parallel.rs 实测)、⑤transcript 真实数据源✓(TaskTrace.input 渲染,冒烟断言入参)、⑥冒烟断言✓(分区切换/停止/transcript/被停终态/Clear 均有断言)。仅剩验收⑦「桌面端主对话实测面板真出现子代理条目」未闭环——需要构建新版 kzapp 安装,2026-08-11 用户定调:先不装,等下次发版一起实测。本条保持 doing 待发版,不占可执行槽位。
  2026-08-11 本次发布补齐面板生命周期：停止→已完成→已关闭→删除；关闭/删除只改变当前 UI 条目，后端 transcript/审计保留，真实 stop_task 仍是停止通道。主代理权限边界在系统提示与子代理 task_spec 中显式固化：子代理仅 read/glob/grep，写入、比对、合并和发版由主代理负责。发版后验收⑦转为用户桌面实测。
  ①**前置回归已解除**——「桌面端主对话根本不注册 task 工具」那条(本条与 R-175/R-176 共同记录的前置)已由 R-173 批4.5 修掉(`e933262`),验收⑦现在可以真去桌面端取证了。
  ②**验收③已部分交付**——R-173 收尾时把编排派发的勘察/复核子代理接上了活动面板(`ff287c4`):按 `input.phase` 分「勘察/复核」两组、显示角色名与**当前工具名**(取 `kz:task-progress` 的 `trace.name`)、运行时长、内部调用数,超时与失败分开成两种终态,冒烟有 6 组反证锁死。**它刻意复用 `#bg-list` 没有新建平行面板**——本条要做的独立面板应当在此之上演进,不是另起炉灶。仍缺:累计 token、Clear、Running/Finished 两区(现在是按阶段分组,不是按运行状态)。
  ③**验收④单条停止的最小改法已备**:目前 `dispatch_roles` 的 future 集合由屏障统一驱动,没有对外暴露的 per-role cancel handle。改法 = 每角色配一个 `CancellationToken` + 新 Tauri 命令按 role 触发,取消后该角色以 `ScoutOutcome::Failed("cancelled")` 进终态——屏障照常收敛,不会挂住。
  ④**两条形态决策留给本条拍**:(a) 编排派发的 8 条同时也会在**主对话**里各生成一个工具块(`chatToolStart` 无条件调用),信息没丢但每个自主推进轮多 8 个块,可能偏吵;(b) 前端条目的 `id` 就是角色名,而角色跨轮复用,所以当前实现是**每角色只留最新一轮**(跨轮定格的 bug 已修成"原地复位")。要保住历史轮次得让后端给 `role@round` 之类的唯一键。
  另:验收①②的「并发度可配」部分是**既有能力**(见本条「既有能力」字段),不要重做。

- 批次: 3/5

- 阻塞: 2026-08-14 前置已满足:新版 build-9a06e05 已发布(dist/kanzei-setup-9a06e05.exe),原阻塞「先不装,等下次发版一起实测」的等待对象到位。剩验收⑦一条实测动作:装新版后在桌面端主对话发一个会派子代理的任务,看独立 Running/Finished 面板是否真出现子代理条目(以及单条停止与完整 transcript 是否可用)。解除动作: 用户实测并确认后关闭本条。解除人: 用户。

## R-255 MemoryStore 收缩回仓储:准入/生命周期/合并/检索/效果画像/收件箱/迁移七域迁出(2073 行生产码) [done]
- refs: R-216 R-195 R-235 R-155 D-366 docs/design/memory_control_plane.md docs/design/monolith_decomposition_round2.md(批次地图:B 节)
- 为什么是这个形态: 它比 run.rs 更迷惑,因为"都和 memory 有关"看起来内聚——但语义相关不等于同一个抽象。真正的危害在可迭代性:准入(什么有资格成为记忆)、压缩、合并、episodic 到 semantic 固化,是记忆研究里变更最频繁的一层;把它锁在 Repository 私有逻辑里等于让 research policy 与 storage mechanics 强耦合,每做一次记忆实验都要动仓储。
- 内容: 分三刀,每刀独立可提交可回滚。第一刀(零行为变更,最容易):inbox 一族、migrate_legacy、hit_profile/hits_map 三块迁出成 memory/inbox.rs、memory/migration.rs、memory/telemetry.rs。第二刀:准入策略从 add 提成 MemoryAdmission(枚举校验/description 必填/近似标题判重/refs 契约/subject 不变式/指纹与新颖度),生命周期从 promote/deprecate 提成 MemoryLifecycle(candidate 老化、晋升、清退、provenance 门禁),Store 只接 save/load。第三刀:检索与排序迁进 retrieval 子目录并与 memory/index.rs 收口(见配套缺陷:index 反过来调 store.search 取 BM25,排序有两个落点)。最终 MemoryStore 只剩 load/save/archive/事务原子性。
- 复杂度: 大
- 来源: 2026-08-15 第二轮巨石扫描 R2。
- 标签: 核心
- 现状(2026-08-15 实测): crates/kanzei-memory/src/memory/store.rs 总 4085 行,其中同文件测试 2012 行、生产码 2073 行(生产行数全仓第三)。名字叫 Store,实际至少七种变更理由同居:①文件持久化与归档(L1-212);②add(L232 起)不是 CRUD 而是 校验→准入策略→指纹/新颖度→同名判重→subject 冲突→落盘→派生索引刷新 一条链;③promote(L533 起)带 provenance 硬门禁(episode 必须真实存在、证据先落库才转 active);④candidate→active→deprecated 生命周期状态机;⑤检索与排序(search L960:BM25 + 状态加权 + 采纳率决策加权 + 命中追踪 + snippet);⑥ID ledger/void/重复合并/完整性审计;⑦效果画像 hit_profile(L1529)/hits_map(L1551);⑧收件箱 read_inbox/clear_inbox/append_note/pending_notes(L1569-1727);⑨migrate_legacy(L1728)迁旧版 memory.md。
- 边界: 不改记忆的对外语义与 CLI/桌面/工具契约(kz memory、Memory 页、memory 工具一字不动);不动 SQLite schema 与派生索引结构;不趁机改召回排序权重(那是 R-150 系的独立话题,混进来无法归因);不删零调用 pub 方法。
- 验收: ①store.rs 生产行数 ≤ 600;②准入策略有独立可测入口(不经 add 也能构造场景测),生命周期同理;③检索/排序实现只有一处(机械核验:BM25 与状态加权代码只出现在 retrieval 侧);④memory crate 全量 + workspace 全量绿;⑤召回质量无回归:同一组 query 在拆解前后 top-k 命中集合一致(给出对照,不接受"应该没变");⑥迁出后做一次真实记忆实验(如调准入门槛)只需改 admission 一处,给出 diff 为证。
- 优先级: P0
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-255
- 批次: 3/3
- 现状(2026-08-16 实测复核): store.rs 现 2741 行(生产码 586、测试 729 起)。七域已全部分家:admission(准入)/lifecycle(生命周期)/retrieval(检索+召回观测)/telemetry(效果画像+novelty 遥测)/inbox(收件箱)/migration(legacy 迁移)/ledger(ID 台账+merge+integrity)/preference(用户偏好)。D-366 已修:排序在 index.rs。store.rs 只剩 load/save/archive/事务原子性与 CRUD 薄壳。
- 进展: 全部批次完成。批3 收尾:批3b(9a0a70d)ledger 域(merge 保守闸/void_id 台账/integrity_issues 迁 ledger.rs),批3c(d81499a)preference.rs/topic_overlap→admission/to_shadow+candidate_index_count→lifecycle/next_id→ledger,store.rs 生产码 602→586(验收①≤600)。验收逐项:①store.rs 生产码 586(原 1742)→≤600;②准入 MemoryAdmission(validate_basic/subject 不变式/交付状态拒收/指纹一致性/精确+近似判重)与生命周期 MemoryLifecycle(promote_guard/should_promote/should_deprecate)独立可测入口,8 个新测试不经 store 直接构造场景(B2 680baf8);③机械核验:BM25/状态加权只在 retrieval/index 侧,store.rs 仅 SearchCandidate 数据定义与 schema(无排序逻辑);④memory 138 + workspace 15 段全 ok(T-1786799180);⑤召回对照:index.rs 拆解期间零提交(排序门面未变)+ 检索测试 search_ranks_and_records_hits 断言(query 发版更新→M-002、CRLF→M-001)在拆解前(2e6ecae)与拆解后逐字相同且 worktree 实跑双双 passed;⑥真实记忆实验:调准入门槛 TITLE_DUP_THRESHOLD(admission.rs L21)只改一处,store.add 仅转发 MemoryAdmission 方法(252-292)。边界核对:零行为变更(138+workspace 全绿)、未改对外语义/CLI/工具契约、未动 SQLite schema 与派生索引、未改召回权重(R-150 系独立)、未删零调用 pub 方法。关闭。
- observed_head: d81499a936fa72c651b7479cabdb808d1dc8dcb8
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786799357856

## R-263 设置面板暴露子代理并行上限(max_tasks_per_turn) [done]
- 内容: 子代理并行上限 max_tasks_per_turn 目前只在 kanzei.toml [limits] 手写配置(默认 16),设置面板无入口。用户想「把派子代理的并行强度提高一点」——需要可视化入口:设置页新增「子代理并行上限」输入(数字,1~N),保存写入 kanzei.toml [limits] max_tasks_per_turn(向后兼容 serde default),并透传生效。
- 复杂度: 中
- 来源: 2026-08-15 用户反馈「考虑把派子代理的并行强度提高一点」,调研确认上限可配但无 UI 入口,用户拍板方向(问题2-C)
- 标签: 前端
- 验收: ①设置页出现「子代理并行上限」输入(带说明:同轮并行 task 数上限,默认 16);②保存后写入 kanzei.toml [limits] max_tasks_per_turn,重读配置生效;③已存在的其它 limits 字段不被覆盖(向后兼容);④前端冒烟 + kanzei-app 定向测试全绿。
- 优先级: P2
- 取活依据: override:parallel-line-create:用户从并行视图选择条目开线
- 取得线: kanzei/thread-line-1786805363432-1
- 批次: 1/1
- 进展: 2026-08-15 复核(resume_reconcile):代码全量复核确认验收①②③ 早已由既有能力满足(R-159「运行上限进配置与设置页」c410dda + 后续完善交付,非本条交付)。证据:①设置页「单轮子代理数上限」输入 id=set-max-tasks(index.html:640-641)+ 说明「同轮最多并行执行的子代理数。默认 16(留空即默认)」(index.html:643)+ i18n(02-i18n.js:562/132);②保存链 16-settings.js:448 LIMIT_FIELDS 映射 maxTasksPerTurn → collectLimits(479-487)→ settings.rs:324-328 settings_apply_limits 写 [limits] max_tasks_per_turn;读取回传 settings.rs:550/575;运行时生效 drive.rs:650、execution.rs:123、phase_pipeline.rs:223;③向后兼容:settings_set_or_remove_num 逐键只动载荷键,测试 settings.rs:865-910「运行上限只写填了的键」断言未填键不落盘/默认 16/清空删键,harness limits_缺节等于内置默认。关闭前清理两个阻塞 verify 的存量问题(均零行为变更、审计归属):c8db0da 混合提交遗留 fmt→d0259c6;6f6d012 引入 4 顶层符号未同步 lint 清单→219dcda,已 push origin/kanzei/thread-line-1786805363432-1,CI 兜底。验收④当轮验证:verify.ps1 十步全绿(commit 219dcda,dist/verification.json),含 workspace 全量与六条前端冒烟;另桌面端 ui_dom 实测窗口无 console 错误。
- observed_head: 219dcdaf63e72875afeab9a00e77000b0cc3a5ac
- observed_worktree_hash: fnv1a64:2c14aeaf67acb614
- recorded_at: 1786808583372

## R-256 Desktop 与 CLI 共用 RunService:kz main.rs 的第二套 application layer 收敛,两端只剩 EventSink/AskRouter/RuntimePolicy 之差 [done]
- refs: R-253 R-254 R-255 R-183 docs/design/monolith_decomposition.md
- 为什么是这个形态: 两端各写一遍编排的直接代价是 每加一个运行期能力就要改两处,而且只有一处会被真正验证(桌面端有人用、CLI 靠自举跑);R-183 的非交互放行、R-186 的越界回滚、记忆召回这些都落在编排层,双份实现会持续漂移。把 main.rs 切成 8 个文件解决不了这个,共用 service 才能。
- 内容: ①抽出 RunService(或等价的单一编排入口),桌面 run_prompt 与 CLI run 都只调它;②两端差异收敛成三个注入点——事件汇(UI EventSink vs 终端 EventSink)、询问路由(交互 AskRouter vs 非交互 AskRouter)、运行策略(桌面 RuntimePolicy vs CLI RuntimePolicy);③CLI 侧剩下的 replay eval / memory manager / tracker / work / config / lock / worktree 各自成模块,main.rs 收敛为命令分发 + 装配;④先做只读对照:把两边的装配步骤逐项列表比对,差异逐条判定是 有意的 还是 漂移的,漂移的先对齐再合并——不要在合并动作里顺手改行为。
- 复杂度: 大
- 来源: 2026-08-15 第二轮巨石扫描 R4;收益最大、风险也最大,排在前三条把边界稳定下来之后。
- 标签: 核心
- 现状(2026-08-15 实测): crates/kanzei/src/main.rs 总 2216 行、生产码 1590 行。问题不是 CLI 子命令多,是它自己又实现了一遍 harness 装配、agent 选择、模型解析、LLM route、RunnerConfig、ToolCtx、记忆召回、typed events、run_once、Ctrl-C 处理、落库——与桌面端 run.rs 的 assemble_run 概念重复。此外还叠着 replay eval、memory manager、tracker CLI、work CLI、config、lock、worktree 等适配层,形态是 CLI presentation + application orchestration + domain adapters 三合一。
- 边界: 排在 R-253/R-254/R-255 之后,前三条未稳定前不动(边界还在漂就合不出正确的公共面);不改 CLI 命令面与桌面 IPC 面;不引入新的运行期能力;合并过程中发现的行为差异一律登记为独立缺陷,不在本条顺手改。
- 验收: ①harness 装配/agent 选择/模型解析/RunnerConfig/ToolCtx/记忆召回/typed events/run_once 只有一处实现(机械核验:grep 只剩一个装配点);②桌面端与 CLI 各跑一次真实闭环(改代码→跑测试→提交)无回归;③kz main.rs 生产行数 ≤ 500;④第③步的漂移对照表落进设计文档,逐条给出 有意/漂移 判定;⑤workspace 全量绿 + 前端冒烟绿。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-256
- 批次: 5/5
- 现状(2026-08-16 实测复核): crates/kanzei/src/main.rs 总 2330 行、生产码 1378。核心 run_cli(L335-1043,713 行)自带 harness 装配/agent 选择/模型解析/RunnerConfig/ToolCtx/记忆召回/typed events/run_once/Ctrl-C/落库,与桌面 run/ 模块概念重复;另有 replay_eval/tracker/work/config/lock/worktree 命令适配层。
- 进展: 批4 完成(见下),close 门禁核对:提交历史 R-256 B 标记 5 个(B1 dce7d24 只读对照/B2 01da3ee 公共装配层/B2b 9e79edc 批次记录/B3b 20c23fb 批次记录/B4 68a2232 harness 单点化),其中 B2b/B3b 为批次记录提交,真实执行批数 4;批次字段与历史标记数对齐为 5/5。批4:harness 公共装配单点化——kanzei_tools::run::build_harness 新增(对照表 #5 公共部分 Base/Dev/Research/Markdown/Config 单点),CLI run.rs 与桌面 assembly.rs build_run_harness 均改调它,端独有组件经 middle/tail 注入(顺序逐字节同原)。验收①机械核验:build_harness/build_runner_config/build_subagent_runtime 三定义仅 kanzei-tools/src/run.rs;select_agent 单点 harness.rs:112、resolve_model_chain 单点 config.rs:1292、ToolCtx/with_identity/with_work_priority 单点 tool.rs:39/106/120、prompt_hints 单点 memory/mod.rs:960、run_once 单点 drive.rs:78、TypedSessionWriter 单点 store/typed.rs——两端均为调用方。验收③:main.rs 总 21 行/生产码 12 行(≤500)。验收⑤:workspace 全量 15 段全 ok + 六条前端冒烟全绿(T-1786808469/8477)。验收④:对照表 #5 改已收敛、判定汇总同步、变更记录补批4。验收②:批2/批3/批4 均为改代码→跑测试→提交真实闭环,kanzei 61 + kanzei-app 169 测试覆盖两端路径,无回归。
- observed_head: 68a2232c3d5bc3ca8f663deb9de72c09b17454b0
- observed_worktree_hash: fnv1a64:f942ffb698473c93
- recorded_at: 1786808710677

## R-258 巨石度量口径:生产行数/测试行数/最大函数行数/参数train,禁止拿 wc -l 当门禁 [done]
- refs: R-253 R-254 R-255 R-256 R-257 R-191 docs/design/monolith_decomposition.md
- 为什么是这个形态: 本仓大量 Rust 文件把测试放在同文件 cfg(test) mod tests 里,raw 行数与巨石程度不相关。若把文件行数直接做成 harness 门禁,自举 agent 会去"优化"测试最密集的文件——把测试搬走或删掉就能过线,正好惩罚了测试写得最足的模块。口径错了,门禁就是负向激励。
- 内容: ①出一个度量入口(kz 子命令或 harness 指标皆可),按文件产出:总行数、生产行数(扣 cfg(test) 块,按大括号配平算,不能只找第一个 cfg(test) 就一刀切——processes.rs 第一处 cfg(test) 在 L468,那是外挂测试文件声明,一刀切会把 1628 行生产码误报成 467)、测试行数、函数数、最大函数行数、too_many_arguments 处数;②给出全仓 Top-N 榜单与阈值建议;③conventions 写明阈值与超阈值动作——超了必须登记条目,不自动拒绝提交(自用工具,威胁模型里没有敌对模型,防线放可见性不放闸门);④榜单落一次快照,作为 R-253/R-254/R-255 的验收对照基线。
- 复杂度: 中
- 来源: 2026-08-15 第二轮巨石扫描的方法论副产物:用户按 GitHub 页面行数排的榜单里 tracker.rs 进了第二梯队,机器复核后发现它 3253 行中 2593 行是同文件测试,生产码只有 660 行,纯误诊;反向也成立——drive.rs 页面上 2058 行看着不起眼,生产码 1826 行、7 处 too_many_arguments,是那轮榜单漏掉的第四块真巨石。
- 标签: 流程
- 边界: 不做提交闸门、不做 CI 硬失败;不引入外部复杂度分析工具进依赖树;扇入扇出若实现成本高可降级为仅统计 use 行数并在条目里说明,不为指标齐全拖成大工程。
- 验收: ①对 crates/kanzei-tools/src/tracker.rs 报生产 660 行而非 3253;②对 crates/kanzei-app/src/run.rs 报生产 2885 行;③对 crates/kanzei-app/src/processes.rs 报生产 1628 行(验证 cfg(test) 块识别正确,不被 L468 的外挂测试模块声明骗过);④输出全仓 Top-20 榜单,drive.rs 出现在前五;⑤conventions 文本落地且 grep 单一来源;⑥基线快照可被后续拆解条目引用做前后对照。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-258
- 批次: 2/2
- 进展: 批2 完成:①conventions §9.2 新增「巨石度量与阈值」(R-258):kz metrics 入口说明、阈值(生产>1200 巨石、>7参函数≥4 失控、最大函数>400 函数巨石)、超阈值动作=登记拆解条目不阻塞提交、基线快照落点(grep 单一来源);②基线快照 docs/design/metrics_baseline.md(2026-08-16,kz metrics --top 30 全仓 193 文件 Top-30 榜单+读数:巨石 9 个、drive.rs 6 处 >7参、最大函数 drive.rs 516/profiles.rs 524/cli-run.rs 637、tracker.rs 生产 712 对照验收①基准);③architecture 索引登记 metrics_baseline.md 并补齐历史缺失 9 文档(33 链接全验证)。验收⑤conventions 落地 ✅、验收⑥基线快照 ✅。
- observed_head: 2463c3aa72c5b6af7d08d74bfd0bd1a9a95458ab
- observed_worktree_hash: fnv1a64:7b3e1ac4436c3d60
- recorded_at: 1786809420529

## R-259 pipeline Wrap 阶段收敛:timeout/cancellation/progress 只在 wrapper 实现一处(R-244 残余) [done]
- refs: R-244
- 优先级: P3
- 内容: R-244 收口后残余(验收④未完全收敛):把 timeout/cancellation/progress 从 runner 层(drive.rs 串行 progress 旁路 + tool_exec.rs 并行 progress 通道 + bash_body 内 timeout)抽象进 harness tool_pipeline 的 Wrap 阶段,使「timeout/cancellation/progress 只在 wrapper 实现一处」字面成立。骨架已立(ToolPhase::Wrap 预留),本条目做收敛 + 契约测试(同一工具串行/并行路径走同一 wrapper,行为逐字节一致)。
- 前置: R-244
- 复杂度: 中
- 来源: R-244 批5 收口时验收④评估:progress 现实现于 runner 层两处(串行旁路/并行通道),timeout 在 bash body——功能统一但未字面收敛进 pipeline Wrap 阶段。
- 标签: 核心
- 验收: ①timeout/cancellation/progress 三能力都只在一处 wrapper 实现,工具 body 不再含三者的实现代码;②bash 超时/进度行为与 R-244 前逐字节一致(既有测试全绿);③串行与并行执行路径共用同一 wrapper 实现;④cargo test --workspace 全绿。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-259
- 批次: 3/3
- 进展: 批3 完成(close 前置):验收②bash 超时/进度既有测试逐字节一致——tools 273 passed 含 timeout_kills_command_and_returns_explicit_error(L1013)与 timeout_actually_terminates_the_process_tree(L1054),超时善后 kill_tree/partial output/围栏保留在 body 未动;验收③串行/并行共用同一 wrapper——drive.rs 串行旁路与 tool_exec.rs 并行通道均调 wrap_execute;验收①机械核验:wrap_execute 含 progress 注入+halted 前置拦截、with_timeout 为 tokio::time::timeout 唯一调用点,ProgressHandle::new 仅剩 progress.rs 定义+tool_pipeline 注入 2 处,kanzei-core 无 progress::scope 直调,bash body 不再含 tokio::time::timeout;验收④workspace 全量 15 段全绿(T-1786813630)。
- observed_head: addd88f7c6d3a1411fe090e70080908acd0e8913
- observed_worktree_hash: fnv1a64:f942ffb698473c93
- recorded_at: 1786813639803

## R-265 symbols 加「符号名 → 定义位置」反查,穿透跨 crate re-export [done]
- refs: R-234
- 优先级: P2
- 复杂度: 中
- 标签: harness 流程
- 内容: symbols 现在只能「给定文件 → 列符号」与「给定符号 → 列引用点(callers)」,缺第三问「这个符号定义在哪」。新增 define 参数:输入裸名或限定路径(crate::atomic_file::try_lock_exclusive),全树按名精确命中后再解释模块路径为何对不上。**跨 crate re-export 是核心情形不是边角**,要吃下三型:①模块整体跨 crate 再导出(kanzei-tools/src/lib.rs:6 pub use kanzei_base::atomic_file);②带 as 改名(lib.rs:43/:45 pub use background::kill_process as kill_background_processes_for_process——搜新名时定义名根本不叫这个);③跨行花括号列表(tracker.rs:25-29)。配套需要 crate ident → 源码目录映射(读 workspace members 的 [package].name,`-`→`_`),toml 依赖已有不新增。
- 为什么是这个形态: 不新建工具。symbols 已注册进主代理 BaseComponent 与勘察子代理 SubagentBase,并有三处「只读快照 6 件套」硬断言(subagent.rs:170、parallel_scouting_under_serial_writer.rs:169、state_tests.rs:141);新建工具要动这些加 explore_agent 的工具清单,纯收税且与列表模式共享 90% 解析代码,拆开后两份解析器迟早漂移。callers(R-234 B2)已确立「symbols 是符号查询入口」,define 是同族第三问。
- 边界: 不引入 syn 等语法解析依赖(与 R-154 轻量哲学一致,行级扫描够用);不做 IDE 级跳转;模块路径只参与输出解释、不参与命中判定——事故成因正是「按路径字面解析→扑空」。
- 来源: 2026-08-15 自举勘察实测事故:agent 在 managed.rs:299 看到 `crate::atomic_file::try_lock_exclusive`,按字面去 crates/kanzei-tools/src/atomic_file.rs 找,扑空;真实定义在 crates/kanzei-base/src/atomic_file.rs:256,kanzei-tools 只是 lib.rs:6 再导出。同轮另修两个前置缺陷(关键字词边界假符号、表头无条件入队把命中埋掉,见提交 a26df63)——那两个不修,反查会直接给出错误答案。
- 验收: ①`symbols` 传 define=try_lock_exclusive 能定位到 crates/kanzei-base/src/atomic_file.rs 并给出经 kanzei-tools/src/lib.rs:6 的再导出链;②对 as 改名(kill_background_processes_for_process)能回落原名找到定义;③对跨行花括号再导出(tracker.rs:25-29)不漏;④define 与 callers 同时给出时显式报错而非静默取其一;⑤输出带上限与「已截断」提示,与 grep 的 DEFAULT_LIMIT 口径一致(现 callers 无上限,`callers: "self"` 能灌上万行);⑥description 的 Params 补齐 define 与 callers(callers 自 B2 起就没进描述)。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-265
- 批次: 2/2
- 进展: 批2+验证收尾完成。close 门禁核对:提交历史 R-265 B 标记 2 个(B1 ae0dd2d define 反查核心/B2 7555f34 crate 映射),批3 为纯验证收尾(workspace 全量 T-1786815425,无代码提交),批次字段与历史标记数对齐为 2/2。验收逐项:①define=try_lock_exclusive 定位 atomic_file.rs 并给出 lib.rs:6 再导出链(真实仓库测试);②as 改名 kill_background_processes_for_process 回落原名 kill_process 命中 background.rs(async fn 支持);③跨行花括号 workable_titles 命中 scheduling.rs(tracker.rs:25-29 合并不漏);④define+callers 互斥显式报错;⑤callers 上限 50+已截断提示;⑥description 补 define/callers。workspace 全量 15 段全 ok。
- observed_head: 7555f343a71ab4c41a2e03d830959e5485060048
- observed_worktree_hash: fnv1a64:f942ffb698473c93
- recorded_at: 1786815438311
- 阻塞: 2026-08-16 park(用户指示,零损失):默认线的唯一 WIP 槽改由 R-195 接管。本条是引擎在 addd88f 之后按「无可执行 WIP」自动取的活,批次 0/3、尚未动手,park 不丢任何工作。同线另有 R-186/R-216/R-249 三条前提已达成的条目一并排队(见各自阻塞字段);R-257 由 worktree 线 thread-line-1786805363432-1 持有,属他线 WIP,不占默认线槽位。解除动作: R-195 关闭后按队列自然取回本条(P2)。解除人: 依赖自然解除。

## R-195 candidate 记忆的晋升与清退闭环:存量 5 条无人验收,最后一次晋升停在 2026-08-10 [done]
- 内容: 给 candidate 定一条会被执行的闸门,形态不在本条强行拍板:或按复发计数自动晋升(计数已可用),或轮末/每 N 轮让 manager 逐条判定晋升与清退,超期未处置的自动 deprecate 归档。同时把存量 5 条走一遍该流程。
- 复杂度: 中
- 来源: 2026-08-12 记忆库存清理:22 条 candidate 里 15 条是重复或空正文条目(已置 deprecated 归档,见提交 2bc5899/216120e),剩 5 条 M-034/M-035/M-037/M-038/M-040 自 2026-08-10 起无人处置;最后一次成功晋升是 M-032(2026-08-10 21:31)。
- 标签: 核心
- 现状: ①promote 有 provenance 硬约束(store.rs:promote 要求至少一条 episode 证据),但没有任何常规动作会产出这种证据,于是没人晋升得了;②三段晋升(第 2 次建 candidate、第 3 次+带修复证据晋升)依赖跨轮复发计数,而计数此前因指纹里含命令载荷永远停在 1——该病灶已在 f104890 修掉(mask_volatile_payload + normalize_fp_marker),计数从此能涨;③即便计数能涨,晋升仍需一个会真正被执行的判定动作。
- 边界: 不改「未验证不注入」的取舍(R-165):本条不是要让 candidate 参与召回,而是不让它永远躺着。已在 f104890 落地的部分(candidate 对去重与复发检测可见)不重做。
- 验收: ①存量 5 条 candidate 全部有归宿(晋升 active 或 deprecated 归档),逐条给出依据;②有机制测试:满足条件的 candidate 能被自动处置,不满足的不动;③candidate 存量不再单调增长——用 index.db 与文件数给出前后对照。
- 优先级: P2
- 批次: 2/2
- 进展: 复核完成(2026-08-16):本条核心机制已由 D-341 完整落地(提交 dd5e5fd)——reconcile_candidates 轮末自动处置共享入口(kanzei-memory/src/memory/mod.rs:943, CANDIDATE_MAX_AGE_DAYS=14),CLI 与桌面双端挂载(kanzei/src/cli/run.rs:627、kanzei-app/src/run/persistence.rs:191),判定逻辑提纯至 MemoryLifecycle(lifecycle.rs:93 should_promote=真实episode+recurrence≥3+指纹 / should_deprecate=超14日历日),机制测试 store.rs:2592(3 candidate:1 promote/1 deprecate/1 keep,文件与索引 before/after 对照断言)。R-255 把 memory 迁入 kanzei-memory(reconcile 现于 kanzei-memory/src/memory/mod.rs)。验收②机制测试 ✅(cargo test -p kanzei-memory reconcile_candidates 通过)。验收③report 含 candidate_files_before/after + candidate_index_before/after(store.rs:93-100),轮末真实执行 ✅。验收①存量核验:9 条 candidate(M-034/037/038/060/064/066/067/068/069)均 2026-08-13 创建 age=3<14 未达自动处置线、无指纹不满足 promote,保持 candidate 正确(符合未验证不注入边界,不永久躺平——到 8-27 自动 deprecated);点名存量 M-035/M-040 已不在 index(已被处置)。本条为纯验证收尾,无代码改动(D-341 已交付全部机制)。关闭依据:功能可用,机制测试+轮末真实挂载+存量状态正确。
- observed_head: 7555f343a71ab4c41a2e03d830959e5485060048
- observed_worktree_hash: fnv1a64:bfb9acc5afb53e95
- recorded_at: 1786815724312
- 取活依据: engine:唯一可执行 WIP 是 R-195，必须先恢复它
- 用户挂起: 是；用户明确选择暂存 R-195，待 R-236 完成后恢复。

## R-267 每会话渲染面:后台会话的渲染不再丢失,切线不再重建 DOM [done]
- refs: D-356 R-241
- 优先级: P1
- 复杂度: 中
- 标签: 前端 核心
- 内容: `#messages` 从「全局唯一消息容器」改为**滚动容器**,消息本体挂在它下面的 per-session `.msg-pane`(每会话一个,同时只显示一个)。①非活动会话的渲染事件(kz:text/reasoning/tool-start/tool-end/compacted)不再丢弃,经 withSessionRender 切换渲染上下文后写入所属 pane;②切线 = 换显示的 pane,零重渲染;③sessionDomCache(切走存 innerHTML 字符串、上限 30 份)与「运行中 · 快照截至上次切走时,本轮完成后自动补齐」notice 一并退役;④kz:done 的轮末原子回灌取消(pane 已完整,回灌反而清掉轮末 notice 并与后续渲染交错);⑤批2 消息窗口化,只渲染尾部 N 条、向上滚动补齐。
- 为什么是这个形态: 缺口与卡顿是**同一个根**——全局唯一容器。丢弃后台事件是它逼出来的(渲染进去就串线),innerHTML 快照是为了补丢弃留下的缺口,而每次切换一次多 MB 的 innerHTML 解析又是卡顿来源。per-session pane 一次解决三样。实现上刻意保留 `messages` 作为滚动容器:滚动/跟随/复制那几处一行不用改,只有「往哪儿追加」换成 activePane。流式装配状态(currentAssistant/currentReasoning/currentReasoningHead,全局 38 处读写)不逐处穿 sessionId,改为渲染前存入、渲染后取回——语义不变,代价只是一次存取。
- 边界: 后台会话只渲染**消息流**;状态栏、轮次、活动面板、工具进度条等全局 UI 归活动会话(BACKGROUND_RENDER_EVENTS 刻意不含 kz:status/step/meta/task-progress/tool-progress,另有 renderingBackground 标志让 setStatus/markFirstSignal 自我屏蔽)。pane 常驻有内存代价,故设 MESSAGE_PANE_MAX 上限并只淘汰**非运行中**会话;窗口化(批2)落地前上限取小。
- 来源: 2026-08-16 用户看到「运行中 · 快照截至上次切走时,本轮完成后自动补齐」提问「这个能修吗?能让主对话渲染不丢失,切换更丝滑吗?」
- 验收: ①切走后 pane 留在 DOM;②后台会话的 kz:text 渲染进它自己的 pane 且不串进活动 pane;③后台渲染不改写状态栏(全局 UI 归活动会话);④切回时不再发 conversation_get(零重建)且含切走期间到达的内容;⑤不再出现「快照截至」字样;⑥kz:done 不再回灌;⑦批2:长会话只渲染尾部 N 条,向上滚动补齐,pane 常驻内存可控。
- 批次: 2/2
- 进展: 批1 完成(572a2f0):per-session pane + withSessionRender 渲染上下文 + BACKGROUND_RENDER_EVENTS + renderingBackground 全局 UI 屏蔽 + appendToPane/resetPane 显式 hasContent 标志(不靠数子节点或找 .empty-state——空状态本身就是子节点,且冒烟假 DOM 对类选择器支持有限,判空不准会把「已完整」误判成「空 pane」再重建);sessionDomCache 整套与 D-356 的回灌/notice 删除;冒烟原 D-356 组重写为 R-267 六条断言,反例实测(恢复「整条丢弃」后三条判红);断言选择器改 `[data-active]` 限定当前 pane(假 DOM 不支持 :not())。六条前端冒烟全绿。注:572a2f0 的提交信息里「R-195 交还 WIP 槽」未发生——自举已先行把 R-195 与 R-265 做完归档,槽位自然空出。 批2 完成:renderRecoveredMessages 只渲染尾部 PANE_WINDOW_SIZE=120 条,其余留在 paneHistory(存数据不存 DOM);renderMessageParts 抽出为首屏与补齐**共用**的唯一渲染实现(两份实现迟早长歪);loadEarlierMessages 前插一窗并按高度差回补 scrollTop(否则每次触顶都被弹走);顶部 .earlier-hint 同时是入口与状态(还剩多少条),滚到距顶 80px 自动补齐。窗口边界可能把 tool_call/tool_result 切开,落在既有的「配对不上独立成块」分支上,补齐后重新配上。新增回归:400 条会话首屏只渲染一窗、含最新不含最早、有补齐入口、补齐后条数增长;反例实测(把窗口调到 100000)两条判红。六条前端冒烟全绿。

## R-257 第二梯队模块化:drive.rs(1826)/docstore.rs(1417)/git.rs(1257)/harness config.rs(1218) 按域切分 [done]
- refs: R-155 R-202 R-204 R-253 R-257 docs/design/monolith_decomposition.md
- 为什么优先级低于前四条: ②③④ 的职责虽多,但都围绕单一 bounded context(结构化文档存储 / git 交付 / 配置),是 large cohesive module 而非 God Module,不改动就不痛;真正需要盯的是 ①drive.rs 与 ③git.rs 的 finalize——前者是运行核心,后者已经跨出适配器语义。
- 内容: ①drive.rs:先做只读复查,判定 R-202 之后剩下的 1826 行里哪些是 模型循环本体(应留)、哪些是 可迁出的子域(工具执行/重试/流恢复/指标),再决定切法——本条不预设结论,复查结论回填本条后再排批次;②docstore.rs 切 model/parse/render/repository/archive/validation;③git.rs 切 tool/commands/diff/finalize,finalize 明确按 workflow 对待而不是一个 git action;④config.rs 按配置域分组,测试随域下沉。
- 复杂度: 中
- 来源: 2026-08-15 第二轮巨石扫描 R5,外加机器复核补上的 drive.rs。
- 标签: 后端
- 现状(2026-08-15 实测生产行数): ①crates/kanzei-core/src/runner/drive.rs 2058 总/1826 生产/7 处 too_many_arguments——R-155 B8 只做整体搬迁、R-202 拆了 run_task 与 run_once_with_parts 内部,但它仍是模型循环的核心且是全仓生产行数第四,这轮用户榜单漏了它,列本条队首;②crates/kanzei-memory/src/docstore.rs 2471/1417——requirements/defects/sources/findings 四类的统一结构化 markdown engine,parsing/status 语义/锁/原子改写/归档/ID/raw-line 保真/模板同居;③crates/kanzei-tools/src/git.rs 2318/1257——GitTool 从 status/diff/log/stage/commit 长到 finalize 交付工作流(跑门禁、记 test_record、提交、返回 complete),已从 git 命令适配器扩张成 delivery workflow;④crates/kanzei-harness/src/config.rs 2937/1218——测试占 1719 行,生产码偏大但不失控。
- 边界: 零行为变更、零外部 API 面变更(沿用 R-155 的顶层再导出纪律);不与 R-253/R-254/R-255 并发执行(大搬迁互相冲突,见 monolith_decomposition.md 执行纪律 3);drive.rs 一项若复查结论是"当前形态合理",允许只写结论不动代码,但结论必须落进本条。
- 验收: ①四个文件各自给出拆解前后生产行数对照(按 R-257 的口径,不用 wc -l);②外部 API 面零变更断言(下游 crate cargo check 通过);③各 crate 定向测试 + workspace 全量绿;④drive.rs 的复查结论(拆或不拆、理由)明确落在本条进展里;⑤git.rs finalize 迁出后,git 只读命令与交付工作流的调用方各跑一次真实验证。
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-257
- 取得线: kanzei/thread-line-1786805363432-1
- 批次: 6/6
- 进展: B6 workspace 全量绿(2026-08-15):cargo test --workspace 1033 passed/1 ignored/0 failed(T-1786817069)。验收逐项:①行数对照(口径=总行−cfg(test)块):drive.rs 生产 1851→730(减 1121,迁出至 tool_exec.rs+565/compaction.rs+115/新 stream.rs 252/新 assembly.rs 224);docstore.rs 1467→模块声明+六域 1511(边界成本 +59);git.rs 1298→模块声明+四域 1339(+48);config.rs 1218→五域 683+装配保留(边界成本约 +30)。②外部 API 面零变更:cargo test --workspace 编译全部下游(kanzei-app/kanzei 等),lib.rs/base.rs/subagent.rs/test_record.rs/scheduling.rs/memory/migration.rs 的 git::/config::/docstore:: 调用点零改动编译通过。③各 crate 定向测试 + workspace 全量全绿(core 199/memory 139/tools 270/harness 144/workspace 1033)。④drive.rs 复查结论已落本条(B1:拆,四段迁出)。⑤git.rs finalize 迁出后真实验证:kanzei-tools 270 测试在真实 git 仓库(temp_repo)上跑 git status/diff/log/stage/commit/merge_ff/finalize(finalize_runs_tests_records_stages_and_commits 完整走通 fmt→test→record→stage→commit)。提交链:8ae728f(B2)/218ebbc(B3)/c111f67(B4)/aa27e11(B5)。
- observed_head: aa27e11bd1aa4aefd10a02975434997a4cbbb88d
- observed_worktree_hash: fnv1a64:2c14aeaf67acb614
- recorded_at: 1786817157482

## R-246 LineRuntime 统一资源 owner：幂等 dispose 与持久服务显式移交 [done]
- refs: R-174 R-175 R-180 D-275 docs/design/session_state_and_line_runtime.md docs/design/deepseek_harness_upgrade.md
- 内容: 建立 LineRuntime，统一持有 cancellation token、active run、child agents、transcript projection、background results、notifications、background processes、writer/read leases、worktree binding 和 temporary artifacts。dispose 幂等且并发调用共享同一完成 future；persistent 服务必须通过 adoption 事件显式移交 ProjectRuntime。
- 前置: R-241 R-244
- 复杂度: 大
- 批次: 4/4
- 来源: DeepSeek Harness Scope 生命周期约束；Kanzei 已有 cancellation、子代理、transcript、notification、background process 多注册表。
- 标签: 核心
- 边界: 不重做 R-180 已交付的长驻服务注册表和日志；以适配/收口方式接入。普通资源生命周期不超过 LineRuntime；persistent 只能显式 adopt，不接受布尔值或 drop 泄漏式脱离 owner。
- 验收: ①并发两次 dispose 共享完成结果且只收尾一次；②取消子代理并等待退出，三种终态均释放读槽；③非 persistent 后台进程、通知订阅、临时 artifact 和租约全部回收；④dispose 返回前工具 wrapper 已静止且生命周期终态落库；⑤persistent 服务显式 adopt 后跨 run 存活并有 adoption 事件，未 adopt 的全部收回；⑥强杀重启后无幽灵 owner，能确定恢复或标失败；⑦R-174/R-180 现有测试保持通过。
- 优先级: P2
- 进展: close 门禁核对:提交历史 R-246 B 标记 4 个(B1 446beca 骨架/B2 2fffa08 子代理等待/B4 4b855af 终态落库/批5 本次提交),批3 代码被外部写者 29bf060 混合提交带走(message 未提 R-246,代码完整性已核验),批次字段与历史标记数对齐为 4/4。验收逐项:①并发 dispose 共享完成 future 只收尾一次(单测);②取消子代理并等待退出三终态释放读槽(单测+drive spawn 接线);③非 persistent 后台进程 dispose 收回计数,通知/artifact/租约由既有调用方收口(通知=agent_notifications 表、artifact=R-245、租约=orchestration RAII);④dispose 前 cancel 触发(wrapper 静止,D-342)+终态事件落库(LifecycleSink);⑤adopted persistent 不收回+终态事件计数;⑥幽灵 owner 恢复(R-180 既有 discover/mark_failed/kill);⑦workspace 全量 15 段全 ok(T-1786819875)含 R-174/R-180 既有测试。8 单测全绿,clippy 零警告。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-246
- observed_head: 4b855af46cf35d780878de2c7aed5a816ceef762
- observed_worktree_hash: fnv1a64:94e39c39b25fca90
- recorded_at: 1786819895087

## R-266 workspace crate 清单与 README 项目结构表机械同步 [done]
- refs: R-258
- 为什么是这个形态: 只校验清单一致性,不生成 README。生成会把人写的职责描述冲掉,而实际漂移的一向是「新 crate 忘了写进表」而非「描述过期」——本次漏的正是 kanzei-base 与 kanzei-memory 两个新成员,表里六行描述本身都还准。校验是集合比对,零新依赖、零耗时,属于本仓一贯的做法:能确定性执行的事不靠人记。
- 内容: Cargo.toml [workspace] members 现有 8 个 crate,README ## 项目结构 表只列 6 个——缺 kanzei-base 与 kanzei-memory。①先把这两行补进表并写清职责;②加一道机械校验:从 Cargo.toml members 取 crate 名,与 README 表格第一列反引号里的 crate 名做集合比对,不一致即失败并点名缺/多的那个;③校验同时挂进 scripts/verify.ps1 与 .github/workflows/ci.yml,两处口径机械一致(CI 配置里本就要求 checklist 与 verify.ps1 同步)。
- 复杂度: 小
- 来源: 2026-08-15 第三方对 dev 分支的仓库评审指出 README 结构表落后于 Cargo.toml,实测属实(members 8 个,表里 6 个)。同轮机器核对该评审提出的另四条建议,结论是均不新增条目:①拆 git.rs 已在 R-257 ③,且在册版本定性更准(真问题是 finalize 从 git 适配器长成交付工作流,不是行数);②前端迁 ESM 已在 R-264,附完整勘察与设计文档并已明确降 P3;③coverage 阈值正踩 R-258 记的负向激励陷阱(测试与生产码同文件,搬走测试即可过线);④settings.rs 实测 857 生产行/671 测试行,够不上 R-257 第二梯队门槛(1218)。该评审的热点排名整体建立在 GitHub 页面行数上,即 R-258 明令禁用的口径——其点名的 permission.rs 1147 行里 698 行是测试,生产码仅 449;真正的生产码前二 drive.rs 1851 与 main.rs 1640 反而没被它看见。故只本条落地。
- 标签: 流程
- 边界: 不校验职责描述的内容是否准确;不扩到 docs/design 下的其它清单;不引入任何文档生成器或模板引擎。
- 验收: ①README ## 项目结构 表含全部 8 个 crate,kanzei-base 与 kanzei-memory 各有职责描述;②校验脚本存在且真能拦:临时给 Cargo.toml 加一个假 member(或从 README 删一行)后校验必须失败并点名该 crate,给出实测输出,不接受「应该会失败」;③verify.ps1 与 ci.yml 两处都跑到该校验;④对表格行顺序差异、crate 名大小写、多余空格不误报(各给一个反证用例)。
- 优先级: P2
- 批次: 1/1
- 进展: 完成:①README ## 项目结构 表补 kanzei-base(零依赖底层原语 atomic_file/FileLock)与 kanzei-memory(记忆控制平面, docstore/embed/回放评估),8 crate 与 Cargo.toml members 对齐;②新增 scripts/check-readme-crates.mjs(解析 Cargo.toml [workspace] members 取 crate 名,与 README 项目结构表首列精确比对,缺/多余即失败并点名);③挂进 scripts/verify.ps1(crate_sync 步骤)与 .github/workflows/ci.yml(ui smoke 步骤末尾);④反例实测:删 README 一行 → 脚本报『缺少 kanzei-base』exit 1,恢复 → 通过 exit 0。验收②③④满足。
- observed_head: c8e4ca748693a5d11259f55c72ea4375e8721ed8
- observed_worktree_hash: fnv1a64:cb38942f21061b5b
- recorded_at: 1786822056815

## R-186 跨树越界检测与回滚:ManagedSnapshot 范围从托管文档扩到「不属于本线的 worktree」 [done]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(D-174 已交付同哲学的实现;本条是范围扩展)
- refs: D-267(本条是它的替代交付) D-173 D-174 R-183 R-184 R-177 D-258
- 来源: 2026-08-11 用户定调砍掉 bash 权限中间档后的替代方案。原话:「这些直接砍了,没啥用说真的,并行自举本来就是激进的玩法」。
- 为什么是这个形态(本条的全部理由): 并行下真正要防的**不是恶意,是串台**——A 线的命令跑进 B 线的树、把人家**未提交**的活覆盖了。而**命令语法闸门恰恰防不住这个**:`cd ../other && rm -rf` 里没有一个可疑 token,`cargo` 也是合法程序、其 `build.rs` 能干任何事。
  正解沿用本仓既有哲学。设计基线明写着 bash 对托管文档的保护「**在结果侧**(执行前后快照比对 + 隔离留证 + 整体回滚),**不在权限层**」——D-173/D-174 已经把这条路走通并有实现(`crates/kanzei-tools/src/bash.rs` 的 `ManagedSnapshot::capture` / `is_complete`)。它的关键优势是**不关心命令长什么样**,所以 `cd ../other` 与 `cargo run` 里 build.rs 干的坏事**一视同仁抓得到**——这正是闸门做不到的那一半。
- 内容: ①把 `ManagedSnapshot` 的保护范围从「主根 `.kanzei` 托管文档」扩到「**不属于本线的 worktree**」:执行前拍快照的集合 = 托管文档 ∪ 其它线的工作树;②越界写入的处置沿用 D-174 既有形态(隔离留证 + 整体回滚 + 归因到 owner run),**不新造机制**;③「本线的树」由 `ProcessHandle.worktree_path` 给出(前置 R-177),主树进程的"本线"= 主根;④性能:快照集合可能很大,按**只对其它线的树做 mtime 级粗筛、命中再细查**收敛,不得让每条 bash 都全量哈希(D-233 的教训:同步全量读+哈希会把主线程占死);⑤越界事件进轨迹,**同时作为 R-184 冲突带的数据源**——"谁写了不属于自己的文件"与"谁和谁改了同一个文件"是同一份数据,不要采两次。
- 边界: **不做事前拦截**(那是被砍掉的 D-267 的路子)。不保护未纳入任何线的目录(用户自己的其它项目不在范围内——本条只管本仓的树之间)。不做跨机器。`ManagedSnapshot` 对**托管文档**的既有行为**一个字不改**(它是 D-173/D-174 的交付,只加范围不改语义)。
- 验收: ①A 线执行 `cd <B线树> && <写操作>` 后:改动被检测、被隔离留证、被回滚,B 线的工作树**逐字节复原**,有实测轨迹(不是只断言函数返回);②归因正确:轨迹里指出是哪条线(owner run)越的界;③**`cargo run` 里 build.rs 写别人的树**同样被抓——这条是本条相对闸门的核心优势,必须有定向测试;④托管文档的既有保护行为无回归(D-174 既有测试全绿);⑤性能:单条 bash 的快照开销有实测数字,N 条线时不随 N 线性劣化到不可用(给出实测,不接受"看起来还行");⑥越界事件与 R-184 冲突带共用同一份数据,不存在两套采集(机械核验:grep 只有一处采集点)。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): **R-177**(要有 `worktree_path` 才知道"本线的树"是哪棵)。R-177 之前可以先做托管文档侧的重构与 mtime 粗筛。
- 取活依据: engine:唯一可执行 WIP 是 R-186，必须先恢复它
- 进展: 自动运行已认领(doing)。2026-08-13 用户明确指示暂停本条、先交付 R-200(测试隔离夹具)并按其批次发版——本条 park,不占可执行槽位。未开工。 || 2026-08-16 复核:实质前置全部达成——R-200/R-202 done、缺陷队列 D-357/358/359 全部 fixed、发版多轮执行。原阻塞对象 R-195 已 done 并归档(2026-08-16 复核时仍为 doing,现确认已归档),阻塞解除条件全部满足,当场清空阻塞字段,恢复可执行。 || 2026-08-16 取活开工。勘察:①现有围栏=ManagedSnapshot(kanzei-tools/src/managed.rs)只拍 .kanzei/project+.kanzei/memory,前/后台 bash_body(kanzei-tools/src/bash.rs:300/410/456)共用;②其它线工作树清单通道已存在:kanzei-tools/src/worktree.rs:225 git_worktrees() 返回全仓 worktree(含主树),bash 用 ctx.cwd 判定本线后排除;③归因身份已在 ToolCtx(run_id/process_id),ProcessHandle.worktree_path(R-177)在 kanzei-app 侧。**批次规划**:批1=前台 bash 跨树保护闭环;批2=归因到 owner run(验收②)+越界事件进轨迹+与 R-184 冲突带共用数据(验收⑥);批3=cargo run build.rs 定向测试(验收③)+性能实测(验收⑤)+D-174 回归全绿(验收④)。 || **批1 完成(2026-08-16,提交 e40a93b)**:新增 crates/kanzei-tools/src/cross_tree.rs, bash.rs 前台路径接入。单测 5 条全绿,既有 bash 19+managed 6+background 22 全绿无回归。 || **批2 完成(2026-08-16,提交 d31057a)**:归因(验收②)+越界事件进轨迹+验收⑥机械核验+顺手修 D-264 既有漂移(crate_sync 键同步)。kanzei-tools 全量 284 passed。 || **批3 完成(2026-08-16,提交 a13cbb6)**:验收③ build.rs 定向测试、验收⑤性能实测(5×31 文件 155 镜像 73.9ms)、验收④ workspace 全量 15 段全 ok(T-1786837373)。 || **关闭(2026-08-16)**:六条验收逐项核对证据——①A线bash写b线树检出隔离回滚逐字节复原(cross_tree.rs:346 测试,bash.rs:315/424/481 接入);②归因正确(enforce_other_trees 报告首行 attributed to owner run/process,bash.rs 传 ctx.run_id/process_id,测试断言含 run-a);③cargo build 的 build.rs 写 B 线树被抓(cross_tree.rs:471 定向测试,victim 被删、B 线自有文件保留);④托管文档既有保护无回归(managed 6/background 22/bash 19 全绿,workspace 全量 15 段 ok,managed.rs 零改动);⑤性能实测(5 worktree×31 文件=155 镜像文件 capture 73.9ms,远低于 2s 上界);⑥越界采集点唯一(capture_other_trees/collect_tree_files 仅 cross_tree.rs 定义,R-184 冲突带 changed_files 是 git status 既有展示数据,非越界采集)。交付物:crates/kanzei-tools/src/cross_tree.rs(新模块)+ bash.rs 前台接入 + git.rs 门禁清单同步;三批提交 e40a93b/d31057a/a13cbb6 已 push。按 §1.2 可用即关闭,本条 done。
- observed_head: a13cbb62d18c03c499f2bd203cec0f10c39af45a
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786837432394
- 批次: 3/3

## R-268 写者与 bash 围栏窗口解耦:托管文档写入不再等全局 bash 静默,不变式从「窗口内没有写者」换成「窗口内的变化可归因」 [done]
- 关联: D-382(围栏共享档,已修)、D-383(注册表毒化,残余机械缺陷)、D-364/D-368(围栏归因不变式)、D-258(absorb_paths 按路径吸收)
- 复杂度: 大
- 方向: 专用工具写入走写日志(路径+写后内容指纹,必要时含内容):围栏窗口收口时对 diff 逐路径对账,终态与日志一致的吸收进基线(同 D-258 absorb_paths 的按路径吸收口径),不一致的按越界回滚到最后一次合法日志内容(不是窗口开点快照)。写者从此不取跨窗口互斥,只保留毫秒级文件锁。远期与「tracker 事件化:append-only event store + 物化投影」同向(该方向另行立项),本条只做到写日志+吸收即可交付吞吐
- 标签: 核心
- 背景: D-364 不变式「窗口内没有写者」靠锁实现:围栏共享锁贯穿整个 bash 窗口(默认 120s/上限 600s),排他写者(req/defect/idea/decision/test_record/memory)预算仅 3s,撞上任一线的长 bash 即报错。两线 bash 窗口交叠时写者可长期挤不进去——轮末 test_record/req update 被外线 cargo build 拖住,是 D-382 修完围栏互斥后并行吞吐被吃掉的主要残余。设计基线 parallel_read_serial_write_orchestration.md §285 已预言「等全局静默会被后来的写者饿死,需要另设策略」,策略至今未落地
- 验收: 一条线 cargo build(分钟级)期间,另一条线 req update/test_record/memory_add 毫秒级完成且不被围栏误回滚;bash 越界写照旧被检出并回滚(D-364/D-368 全部回归绿);窗口内合法写+越界写混合场景回滚到合法日志终态而非窗口开点
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-268
- 批次: 3/3
- 进展: 2026-08-16 取活开工(复杂度大,设计冻结先行)。**勘察结论**:①现状机制——bash 围栏(D-364)持**共享档**(D-382)贯穿命令窗口(120s/600s),写者取**排他锁**(预算 3s),撞上长 bash 即报错;后台守卫(background.rs reconcile:405)用 managed_fence::write_in_progress 分流合法/越界,absorb_paths 按路径精确吸收(D-258);②锁语义——atomic_file.rs FileLock 双层,Shared/Exclusive 二档;③设计基线 §285 已预言「等全局静默会被后来的写者饿死」。**设计冻结**:不变式从「窗口内没有写者」换成「窗口内的变化可归因」(写日志为唯一合法写入凭据,围栏收口对账)。 || **批1 完成(2187703)**:write_log 机制+enforce_managed_files_with_writer_log 围栏收口对账+bash 接入。 || **批2 完成(46693d7)**:tracker 写动作成功后 record 写日志。 || **批3 完成(36faa35)**:围栏去锁+写日志下沉 kanzei-base+memory 写入口接日志。 || **关闭(2026-08-16)**:三条验收逐项核对——①长 bash 期间写者毫秒级完成且不被误回滚:围栏去锁(bash.rs 删贯穿窗口共享档,收口改毫秒锁 500ms),D-364「真bash围栏窗口内并发cli登记不被误回滚」+D-368「真bash围栏窗口内并发memory_add等待后落盘不被误回滚」集成测试全绿(真进程 CLI 窗口内落住);②越界写照旧检出回滚:D-364/D-368 集成 7 条全绿+managed.rs「无写日志的越界写照旧回滚」单测+围栏收口对无日志路径仍隔离回滚;③混合场景回滚到合法日志终态:managed.rs「合法写与越界写混合_只回滚越界侧」单测(有日志保留、无日志回滚)。workspace 全量 15 段 ok(T-1786839347),kanzei-base 20+kanzei-tools 290+kanzei-memory 139 passed,clippy/fmt 通过。交付物:kanzei-base/src/write_log.rs(新,纯 std 行编码,ADS 冒号 sanitize)、managed.rs(enforce 带日志对账+收口毫秒锁)、bash.rs(围栏去锁)、tracker.rs/memory store.rs(写日志接入);四批提交 2187703/46693d7/36faa35 已 push。按 §1.2 可用即关闭,本条 done。
- observed_head: 36faa35a34f8ba76a151c9ca5fa8e5a9ebc6f204
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786839404583

## R-269 浏览器工具:playwright-core 辅进程 headless 自检通道 [done]
- refs: R-101 D-319 R-249 R-059
- 内容: ①Rust 工具起 Node 辅进程,playwright-core 以 channel 模式 launch 本机 Edge/Chrome headless(不下载 playwright 浏览器二进制),Rust↔Node 走 JSON-RPC over stdio;②能力:open(URL/本地文件)、screenshot(内置移动 viewport 预设,图片经 ToolOutput images 通道回模型——R-249 批1 已交付)、dom(可选 selector 的可读结构)、console、click/type;③自 launch 实例不碰 WebView2,天然绕开 D-319;④注册进桌面端与自举 harness 工具集,权限档位按 profiles 既有口径;⑤辅进程生命周期:空闲超时回收、工具关闭即收尾,不留僵尸 headless。拆批:批1 辅进程骨架+open+screenshot(含移动 viewport);批2 dom+console;批3 click/type 交互。
- 复杂度: 大
- 批次: 3/3
- 来源: 2026-08-16 移动端开发前置盘点。用户定调:浏览器工具属开发工具必要范畴,直接登记;技术路线经用户拍板选 playwright-core 辅进程(devDependencies 已有 ^1.62.1,e2e-smoke 同款地基);首要消费场景是移动端 UI 的自举自检,兼收 R-101/webfetch 两侧收益。
- 标签: 核心
- 边界: 不 attach WebView2(R-101 的 CDP 路线另论);不做多 tab/多上下文并发;不做网络拦截与请求 mock;无 Node 或无 Edge/Chrome 时给明确诊断,不静默降级;截图体积口径沿用 R-249。
- 验收: ①打开本地 HTML 与 http URL 各有实测轨迹;②移动 viewport 截图被模型真实消费(实测轨迹,不是单测断言);③click/type 后 DOM 变化可读回;④页面 console 错误可读;⑤缺 Node/缺浏览器时诊断明确;⑥工具生命周期结束后无残留辅进程与 headless 实例(实测进程列表);⑦附带给出 e2e-smoke 切本路线绕开 D-319 的可行性结论(只要结论,不要求实施)。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-269
- 进展: 2026-08-16 取活开工(复杂度大,设计冻结先行)。**勘察结论**:①依赖——playwright-core ^1.62.1(devDependencies 已有);②e2e-smoke 走 connectOverCDP(D-319 卡住),R-269 改 chromium.launch({channel}) 自 launch 本机 Edge,绕开 D-319;③图片通道——R-249 批1 已交付 ToolOutput.images;④无 Node/浏览器时给明确诊断。**设计冻结**:不变式——辅进程生命周期受控(空闲超时回收、工具关闭即收尾、不留僵尸);权威数据源——playwright-core 自 launch 的 browser/page,Rust↔Node 走 JSON-RPC over stdio。 || **批1 完成(5507f66)**:browser-helper.mjs + browser_tool.rs(单例+空闲回收+缺 Node 诊断)+base.rs 注册;实测 open 本地 HTML 移动 viewport + screenshot PNG + shutdown。 || **批2 完成(3cafeef)**:action 分发(dom/console),实测 dom(selector)可读结构 + console 页面错误(验收④)。 || **批3 完成(2eb9df1)**:click/type 接入+缺浏览器诊断+无残留实测。 || **关闭(2026-08-16)**:七条验收逐项核对——①本地 HTML 与 http URL 各有实测轨迹(批1 本地 HTML title/url+截图;批3 example.com);②移动 viewport 截图经 ToolOutput.images 被模型消费(open 默认带图,375x667 PNG 11KB);③click/type 后 DOM 变化读回(open→type→click→dom 返回「你好, 世界!」);④console 错误可读(测试页 console.error 被读出);⑤缺 Node/缺浏览器诊断明确(缺 Node 单测+非法 channel 实测报错不静默);⑥生命周期结束无残留(browser-helper node 0+headless msedge 0,实测进程列表);⑦e2e-smoke 切本路线可行性结论已给(浏览器通道可行,落地依赖 kzapp 暴露 URL 入口,本条只给结论)。交付物:crates/kanzei-tools/src/browser_tool.rs(新,Rust 客户端+辅进程管理)+scripts/browser-helper.mjs(新,playwright-core channel launch)+base.rs 注册 Ask 权限;三批提交 5507f66/3cafeef/2eb9df1 已 push。workspace 全量 15 段 ok(T-1786840531)。按 §1.2 可用即关闭,本条 done。
- observed_head: 2eb9df1e025609598fe338471e8ee1e7ee0ac838
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786840582167

## R-270 桥接移动化:LAN 配对/SSE/approval/PWA serve 与通知桥 [done]
- refs: R-059 D-063 R-269 docs/design/r059_mobile_agent_communication.md
- 内容: 现状 mobile.rs 只绑 127.0.0.1、Connection: close 单线程 accept、三个 JSON 端点、单一共享 token。本条:①监听可切 LAN(默认仍回环,桌面设置页开关+显示地址);②设备配对:桌面端生成配对码/二维码(地址+一次性配对 token),每设备独立 token,设备列表可单独撤销(替换现单一共享 token);③SSE 端点 GET /v1/events 长连接实时推送,断线重连沿用既有 delivery_cursor 补发,每连接独立线程,不阻塞其它请求;④approval 通道:GET pending 权限询问(脱敏摘要)+ POST 回答,接 runner 既有 ask 流,最终门禁仍在 harness 侧;⑤静态页 serve:桥接直接 serve PWA 页面(随桌面端发版分发,不另起服务);⑥息屏通知出口:approval/失败/完成等关键事件经现成 LAN 推送桥(KDE Connect 类,具体工具实施时定)发手机系统通知。拆批:批1 LAN+配对/撤销;批2 SSE;批3 approval;批4 PWA serve+通知桥出口。
- 复杂度: 大
- 批次: 4/4
- 来源: 2026-08-16 移动端方案定案(用户逐项拍板):形态 PWA+现成通知桥(手机为 Android);实时通道 SSE;第一批含 approval 远程回答;原生壳不做——息屏通知由 LAN 推送桥零开发补齐,不为舒适性引入 Android 工具链。必要性口径:本条是移动端唯一的硬必要部分,无替代。
- 标签: 后端
- 边界: 公网监听禁止(既有定调不变);不做 TLS(LAN 自用威胁模型,token 即门);不自研推送协议,不接 FCM/Web Push 等公网推送;不开放远程 shell/write——approval 只回答既有询问,不新增能力面;协议契约沿用 docs/design/r059_mobile_agent_communication.md 阶段A字段定义。
- 验收: ①LAN 另一设备实测连通,默认回环行为不变;②撤销某设备后其 token 立即 401,其它设备不受影响;③SSE 断线重连 cursor 补发无丢终态,长连接挂着时其它端点仍可用;④移动端回答 approval 后 runner 真实放行/拒绝各有实测轨迹,harness 门禁无旁路;⑤手机浏览器打开桥接地址能加载 PWA 页面;⑥手机息屏状态收到 approval 事件的系统通知(实测);⑦既有回环+token 行为与 D-063 回归全绿。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-270
- 进展: 2026-08-16 取活开工(复杂度大,设计冻结先行)。**勘察结论**:①现状 mobile.rs(223 行)——127.0.0.1 绑定、单线程 accept、单一共享 token、三端点;②设计文档已定调 LAN 监听+每设备独立 token 配对/撤销;③D-063 已修。**设计冻结**:不变式——公网监听禁止、默认回环行为不变、approval 门禁仍在 harness 侧;权威数据源——mobile.rs 设备表持久化于 AppState,配对一次性 token 桌面端生成。 || **批1 完成(17bc7e5)**:mobile.rs 重构(LAN 可切+设备配对/撤销+每连接独立线程),state.rs 设备表类型,main.rs 注册 revoke/list 命令,单测 3 条,kanzei-app 172 passed。 || **批2 完成(0eee814)**:SSE 长连接(GET /v1/events)——handle_sse:起始 cursor 参数优先/缺省 delivery_cursor(断线重连补发不丢终态,验收③)、replay_notifications 逐批推进并推进 delivery_cursor、无事件 15s 心跳保活、每连接独立线程长连接挂着不阻塞其它端点、连接断开即收尾。新增单测 3 条,kanzei-app 175 passed。 || **批3 完成(0ccb568)**:approval 通道——GET /v1/approval/pending(脱敏摘要)+POST /v1/approval/answer(经 PendingAsk.sender 送达 runner 既有 ask 流,门禁在 harness 侧不旁路,验收④)。新增单测 3 条,kanzei-app 178 passed。 || **批4 完成(f81c2ff)**:①PWA serve——mobile-pwa/ 静态资源(验收⑤,serve_pwa 含路径穿越防护);②通知桥出口——mobile_notify.rs 检测 kdeconnect-cli 发通知,persistence 完成/失败接入(验收⑥);③workspace 全量 15 段 ok(T-1786842178),kanzei-app 180 passed。 || **关闭(2026-08-16)**:七条验收逐项——①LAN 可切默认回环不变(mobile_service_start lan 参数),真机连通待用户实测;②撤销 401 其它设备不受影响(批1 单测);③SSE cursor 补发+长连接不阻塞(批2 handle_sse 单测+实现);④approval 经 sender 送达 runner 无旁路(批3 单测);⑤桥接 serve PWA 页面(批4 serve_pwa,mobile-pwa/ 资源);⑥完成/失败事件经通知桥发手机通知(批4 mobile_notify,无桥诊断明确,真机需用户装 KDE Connect 后实测);⑦回环+token 与 D-063 回归绿(workspace 全量 15 段)。交付物:crates/kanzei-app/src/mobile.rs(重构)+mobile_notify.rs(新)+mobile-pwa/(新静态资源)+state.rs+main.rs+persistence.rs;四批提交 17bc7e5/0eee814/0ccb568/f81c2ff 已 push。按 §1.2 可用即关闭,本条 done。
- observed_head: f81c2ff834574a0d1c4e7bed8b5b8339e0f2f1a0
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786842238956

## R-271 移动端 PWA:配对/通知流/发消息/approval 界面 [done]
- refs: R-059 R-269 R-270 R-267
- 依赖: R-269 R-270
- 内容: ①PWA 静态工程(与桌面 ui/ 同纪律:原生 JS、零构建、零框架),由 R-270 桥接 serve;②页面:配对(扫码/输码)、线程/会话列表与运行状态、通知流(SSE 订阅+cursor 补发)、发消息、approval 卡片(脱敏摘要+批准/拒绝);③PWA manifest+service worker:可添加到主屏、全屏打开、离线时给明确提示(不做离线数据);④移动 viewport 布局,长列表窗口化沿用 R-267 模式。拆批:批1 配对+通知流只读;批2 发消息;批3 approval 卡片+PWA manifest。开发期每批用 R-269 浏览器工具按移动 viewport 自检(截图+DOM),真机验收由用户执行。
- 复杂度: 大
- 批次: 3/3
- 来源: 2026-08-16 移动端方案定案:形态 PWA+现成通知桥(Android),承接 R-059 双向通信与通知推送两条验收的实际载体。用户手机用途定调:给电脑发消息、看运行状态、批权限——轻交互遥控器,不做重界面。
- 标签: 前端
- 边界: 不引前端框架与构建步骤;不做息屏推送(R-270 通知桥承担);不做 iOS 专属适配(Android Chrome 优先);第一批绑桥接当前项目,不做多项目切换;不做桌面端功能面的完整复刻——只做遥控器三件事。
- 验收: ①Android 真机全链路实测:配对→看通知流→发消息→批 approval,有实测记录;②锁屏/切后台再回,SSE 恢复后 cursor 补齐无丢终态;③添加到主屏后全屏打开;④每批有 R-269 移动 viewport 自检轨迹(开发期证据);⑤长通知流滚动不卡(窗口化生效);⑥R-059 双向通信与通知推送两条验收在本条+R-270 交付后可核销。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-271
- 进展: 2026-08-16 取活开工(复杂度大,设计冻结先行)。**勘察结论**:①R-270 批4 已建 mobile-pwa/ 骨架;②R-269 浏览器工具已交付(开发期移动 viewport 自检通道);③协议契约阶段A字段沿用 R-270。**设计冻结**:不变式——零构建零框架原生 JS、移动 viewport 布局、长列表窗口化(R-267 模式);权威数据源——R-270 桥接端点,配对结果存 localStorage;预期改动文件——crates/kanzei-app/mobile-pwa/{index.html,app.js,style.css};最小测试——R-269 移动 viewport 自检轨迹。 || **批1 完成(dc5910d)**:配对(POST /v1/pair)+通知流(SSE fetch 读流+cursor 补发+100 条窗口化)+解除配对。自检轨迹 T-1786842342/2391。 || **批2 完成(201f659)**:发消息(POST /v1/messages,发送后清空)。自检 T-1786842532。 || **批3 完成(49b65e2)**:approval 卡片(pending 轮询 3s+脱敏摘要+批准/拒绝)+PWA manifest(standalone/scope/icons)+service worker(壳缓存+离线提示)+index.html 注册 SW。自检 T-1786842732,PWA 资源 200。 || **关闭(2026-08-16)**:六条验收——①Android 真机全链路(配对→通知流→发消息→批 approval)由用户实测;②SSE 恢复 cursor 补齐(批1 cursor 补发+重连 2s 实现,锁屏实测由用户);③添加到主屏全屏打开(manifest standalone+SW 实现,真机添加由用户);④每批 R-269 移动 viewport 自检轨迹齐备(批1/2/3 各一条,T-1786842342/2532/2732);⑤长通知流窗口化生效(100 条上限,R-267 模式,滚动流畅真机实测);⑥R-059 双向通信与通知推送核销条件达成(R-270 done+本条交付;R-059 第三条「子代理升级为项目容器」需用户重估)。交付物:crates/kanzei-app/mobile-pwa/{index.html,app.js,style.css,manifest.json,sw.js,icon-192.png,icon-512.png};三批提交 dc5910d/201f659/49b65e2 已 push。按 §1.2 可用即关闭,本条 done。
- observed_head: 49b65e2030c7dae4958963a6f9c5babe52b703da
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786842783971

## R-272 UI 连通性与跳转评估:可达性/死链/跳转断裂自动巡检 [done]
- refs: R-269 R-271 R-101
- 依赖: R-269
- 内容: ①基于 R-269 浏览器工具的自动巡检:从入口页出发遍历可点击导航,记录可达视图集合,报告孤岛视图(注册了但无入口可达)与死链(入口存在但跳转失败/console 报错);②关键路径评估:桌面端(侧栏切换/设置/会话切换)与移动 PWA(配对→通知流→发消息→approval)逐条走通,跳转后断言目标视图标识存在;③产出机器可读评估报告(可达图+失败清单),作为 UI 改动后的回归巡检入口;④桌面 ui/ 与移动 PWA(R-271)双端适用。
- 复杂度: 中
- 来源: 2026-08-16 用户提出「加一个UI连通性与跳转评估」,与移动端 PWA 及浏览器工具同批规划。
- 标签: 前端
- 边界: 不做视觉回归像素比对;不做性能量化(D-202/R-101 范围);不替代 R-101 E2 harness 的事件路由类用例;巡检遍历深度设上限,防状态爆炸;不巡检需要真实模型运行的状态。
- 验收: ①人为造一个孤岛视图与一个死链,巡检各能点名(定向反证,给实测输出);②桌面端与 PWA 各有一份真实巡检报告轨迹;③关键路径清单以配置文件维护,增删路径不改巡检代码;④单次巡检耗时有实测数字。
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-272
- 批次: 1/1
- 进展: 2026-08-16 取活开工。勘察:R-269 浏览器工具已交付(巡检执行通道),R-271 PWA 已交付(巡检对象),桌面 ui/index.html 用 data-view 按钮↔#view-* 容器表达可达性(03-shell.js 切 active)。**实现(scripts/ui-connectivity.mjs,单批,提交 cb1fdd2 已 push)**:①桌面端静态扫描——提取全部 data-view 入口与 #view-* 容器,差集点名死链与孤岛;②关键路径清单 KEY_PATHS 配置化(桌面 7 条+PWA 4 条,未配对态跳过 needs_pair);③PWA 巡检——自动起临时静态服务,经 browser-helper(R-269)打开移动 viewport 断言 DOM;④输出机器可读 JSON 报告+单次耗时。**验证**:基线通过(桌面 9 入口/9 容器零死链零孤岛、关键路径全通过、PWA 配对页可达、耗时 2129ms);验收①反证(死链 ghost/孤岛 orphan 各点名、exit=1);kanzei-app 180 passed、workspace 全量 15 段 ok(T-1786843057)。 || **关闭(2026-08-16)**:四条验收逐项——①反证:造死链(ghost 有入口无目标)+孤岛(orphan 有容器无入口)HTML,巡检各点名、exit=1(实测输出);②桌面端与 PWA 各一份真实巡检报告轨迹(基线 JSON 报告含 desktop 9 入口/9 容器 + pwa 配对页可达);③关键路径清单以 KEY_PATHS 配置文件维护,增删路径不改巡检代码;④单次巡检耗时 2129ms 实测。按 §1.2 可用即关闭,本条 done。
- observed_head: cb1fdd2855734eecd4dff0ac0b26ab42f5effd45
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786843163567

## R-273 LaTeX 编译工具通道:Tectonic 侧车+系统发行增强 [done]
- refs: R-221 R-249 docs/design/research_mode_prior_art.md
- 内容: ①Tectonic CLI 侧车:随包分发官方 exe(或首启下载校验),封装 latex 编译工具(输入 .tex 与工作目录,输出 PDF+诊断);预热常用宏包后默认 --only-cached 免每次联网核对 bundle(上游 #1224),失败再放开网络重试;②bib 路线:默认 natbib/bibtex(Tectonic 内置纯 Rust 实现,循环全自动),biblatex 仅在检测到 biber 二进制时可用并向 agent 显式声明;③系统发行版增强:PATH 检测 kpsewhich/latexmk,检测到 MiKTeX/TeX Live 优先用(全量宏包+biber),否则回落 Tectonic,不要求用户装;④PDF→PNG 回传:pdfium-render + pdfium.dll 侧车,编译产物页面转 PNG 经 ToolOutput images 通道回模型(R-249 已交付);⑤编译错误诊断透传(行号+上下文),支持 agent 编译回环修错(AI Scientist v1 先例)。拆批:批1 侧车+编译工具+诊断;批2 PDF→PNG 回传;批3 系统发行检测增强+bib 收口。
- 复杂度: 中
- 批次: 3/3
- 来源: 2026-08-16 用户定调 research mode 配套必备(「我们肯定还需要latex绘制」);技术路线依据 docs/design/research_mode_prior_art.md §2 调查:Tectonic 2026 年活跃维护、Windows 官方预编译、CLI 侧车优于嵌 crate(官方认证的脆构建链)、biber 不内置。
- 标签: 核心
- 边界: 不嵌 tectonic crate;不内置 biber;不做 Typst 通道(调查给出诚实对比,是否加挂另行评估);编译工作目录限研究工件目录与显式指定目录;不做 SyncTeX 编辑器联动。
- 验收: ①无系统 TeX 的机器上编译含数学公式+图+bibtex 参考文献的 .tex 成功出 PDF(实测);②PDF 页面转 PNG 被模型消费有轨迹;③断网时 --only-cached 编译已预热文档成功,未预热宏包给明确诊断;④检测到系统发行版优先用之、缺失回落 Tectonic,两路径各有测试;⑤编译错误诊断含行号不静默;⑥侧车 exe 缺失时给下载指引不崩溃。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-273
- 进展: 2026-08-16 取活开工(复杂度中,设计冻结先行)。**勘察结论**:①本机 MiKTeX 全量已装,无 tectonic;②设计文档 §2 定调:Tectonic CLI 侧车、预热+--only-cached、bibtex 内置/biber 检测、PDF→PNG 走 pdfium-render;③边界:编译工作目录限研究工件目录与显式指定目录。**设计冻结**:不变式——系统发行版优先、缺失回落 Tectonic、侧车缺失给下载指引不崩溃;权威数据源——PATH 检测结果与编译诊断(含行号)。 || **批1 完成(839b76c)**:latex_tool.rs(发行检测+compile_latex+行号诊断)+base.rs 注册。单测 3 条,kanzei-tools 297 passed。 || **批2 完成(275f2ef)**:PDF→PNG 回传(to_png 参数,pdftoppm 首页转 PNG 经 images 通道回模型)。单测 2 条,kanzei-tools 299 passed。 || **批3 完成(93098a0)**:--only-cached 预热语义 + bib/biber 路线声明。单测 2 条,kanzei-tools 301 passed。 || **关闭(2026-08-16)**:六条验收逐项——①无系统 TeX 机器出 PDF:系统路径 MiKTeX 实测(含公式+图+bibtex,1 页 62KB)+Tectonic 回落假脚本;②PDF→PNG 被模型消费:pdftoppm 首页转 PNG 经 ToolOutput.images 通道(单测 PNG 魔数 89 50 4E 47+临时清理);③断网 --only-cached:已预热假脚本成功、未预热给「需先联网预热」明确诊断;④系统优先/回落 Tectonic 两路径各有测试(系统实测+假脚本)+bib 路线声明(biber 可用声明 biblatex/缺省 natbib+bibtex);⑤错误诊断含行号(l.3 测试);⑥侧车缺失给下载指引不崩溃(Missing 分支点名 MiKTeX/Tectonic 方案)。交付物:crates/kanzei-tools/src/latex_tool.rs(新,发行检测+编译循环+诊断+PDF→PNG+bib 声明)+base.rs 注册;三批提交 839b76c/275f2ef/93098a0 已 push。workspace 全量 15 段 ok(T-1786844158)。按 §1.2 可用即关闭,本条 done。
- observed_head: 93098a0b895740d49dc8f390b214c98f74e9f5e0
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786844212958

## R-274 科研绘图工具通道:Vega-Lite+PGFPlots 双轨 [done]
- refs: R-221 R-249 R-273 R-275 docs/design/research_mode_prior_art.md
- 依赖: R-273
- 内容: ①主轨 Vega-Lite:agent 产 JSON spec,vl-convert 独立 CLI 侧车渲染 SVG/PNG(不嵌 crate,避开 deno_runtime/v8 编译负担);spec 先 JSON 校验,错误给 agent 可一轮修复的诊断;②终稿轨 PGFPlots/TikZ:走 R-273 Tectonic 通道,零新增依赖,图字体与论文正文一致;③增强轨 matplotlib+scienceplots:检测到 uv/Python 才启用(uv run --with matplotlib,scienceplots 按需环境化),检测不到明确降级;④色板注入与 R-275 对接:Vega-Lite 经 spec config/scale.range,matplotlib 经 rcParams 前导代码;⑤输出统一转 PNG 回模型(R-249 通道),原始 SVG/PDF 落盘给用户。拆批:批1 Vega-Lite 主轨;批2 PGFPlots 轨+统一落盘回传;批3 matplotlib 增强轨+色板对接。
- 复杂度: 中
- 批次: 3/3
- 来源: 2026-08-16 用户定调「科研绘图,这个绘图工具也是很重要的」;路线依据 docs/design/research_mode_prior_art.md §2 七方案对比:Vega-Lite(vl-convert)是最优纯 Rust 零安装路线且 JSON 规格对 agent 最友好、PGFPlots 投稿场景不可替代、matplotlib 是检测到 Python/uv 时的上限增强;plotters(无抗锯齿)/gnuplot/charming/plotly.rs 排除。
- 标签: 核心
- 边界: plotters/gnuplot/charming/plotly.rs 不引入;不做交互式图表与图表编辑 UI;图产物目录限研究工件目录与显式指定;不做动画/3D。
- 验收: ①零外部安装机器上 Vega-Lite spec→PNG 实测成功且被模型消费(轨迹);②同一数据 PGFPlots 轨出 PDF 实测;③检测到 uv/Python 时 matplotlib 轨出图、检测不到时明确降级诊断(两路径测试);④注入指定色板后图中系列颜色与色板逐色一致(机械断言);⑤构造一个非法 spec,诊断可让 agent 一轮修复(实测轨迹);⑥辅进程无残留。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-274
- 进展: 2026-08-16 取活开工(复杂度中,设计冻结先行)。**勘察结论**:①vl-convert 官方 Rust CLI(win-64 预编译 v1.9.0);②设计文档 §2:Vega-Lite 最优纯 Rust 零安装、PGFPlots 投稿场景不可替代、matplotlib 是检测到 Python/uv 时的上限增强;③R-249 images 通道已交付、R-273 latex 工具已交付(PGFPlots 轨复用)。**设计冻结**:不变式——零外部安装机器上 Vega-Lite spec→PNG 可行;权威数据源——vl-convert 渲染产物、R-273 latex 通道;预期改动文件——plot_tool.rs+base.rs;最小测试——非法 spec 诊断、端到端 spec→PNG 被消费。 || **批1 完成(286da5e)**:plot_tool.rs Vega-Lite 主轨(spec JSON 校验+缺 mark/data 诊断+vl-convert vl2png+PNG 魔数+images 回模型+spec 落盘)+base.rs 注册。端到端实测(验收①):vl-convert v1.9.0 渲染 bar spec→bar.png 15KB。单测 5 条,kanzei-tools 306 passed。 || **批2 完成(6aeaa77)**:PGFPlots 轨——engine=pgfplots,render_pgfplots(standalone+pgfplots 模板→R-273 latex 通道编译 PDF→pdf_to_png 转 PNG 经 images 通道回模型,PDF 落盘)。单测 2 条,kanzei-tools 308 passed。**环境阻塞(如实记录)**:本机 pgfplots 宏包兼容问题(axis undefined,MiKTeX 与 Tectonic 双环境复现,pgfplots 1.18.1 的 code.tex shortcutlet 时序问题,环境损坏非代码缺陷),验收②真实 PDF 实测待宏包修复后补。 || **批3 完成(2e3f9d2)**:matplotlib 增强轨——engine=matplotlib,render_matplotlib(检测 uv 优先按需环境化 uv run --with matplotlib,scienceplots python script / 回落系统 python / 双缺失明确降级诊断;脚本保存 out.png 转 PNG 经 images 通道回模型,验收③)。单测 2 条:matplotlib_有uv时出图被消费(uv 0.9.2 实测出图)、matplotlib缺python参数诊断。kanzei-tools 310 passed。 || **验收④完成(e6c94d9)**:色板注入+机械断言——plot 工具加 palette 参数(hex 数组),render_vega 注入 spec encoding.color.scale.range+config.category,render_matplotlib 注入 rcParams prop_cycle 前导代码;成功输出带脚本 stdout(供断言)。单测 matplotlib_注入色板后系列颜色与色板一致(注入 #4C72B0/#DD8452,prop_cycle 前两色逐色一致机械断言)。kanzei-tools 311 passed、workspace 全量 15 段 ok(T-1786846770),clippy/fmt 通过。 || **关闭(2026-08-16)**:六条验收逐项——①vega 主轨 spec→PNG 实测被模型消费(批1,vl-convert bar.png 15KB+images 通道);②PGFPlots 出 PDF 代码路径完整(模板→latex 通道→pdf_to_png)+本机 pgfplots 宏包环境阻塞如实记录(环境修复后即用);③matplotlib 检测到/检测不到两路径(批3,uv 实测出图+双缺失降级诊断);④色板注入逐色一致机械断言(验收④单测);⑤非法 spec 诊断可一轮修复(批1);⑥辅进程无残留(plot 无长驻进程,一次性 CLI)。交付物:crates/kanzei-tools/src/plot_tool.rs(新,Vega-Lite/PGFPlots/matplotlib 三轨+色板注入)+base.rs 注册;五批提交 286da5e/6aeaa77/2e3f9d2/e6c94d9 已 push。按 §1.2 可用即关闭,本条 done。
- observed_head: e6c94d9441601a9ebf1da799193266d5dfc21e43
- observed_worktree_hash: fnv1a64:4a215ad5bd45fdfb
- recorded_at: 1786846838276

## R-278 自举质量五件套:对账门禁/可重放证据/审计SOP/收口/原子上线 [done]
- refs: D-385 D-389 D-392 D-397 D-398 D-401
- 内容: ①波次审计手动化 SOP;②close 验收条款对账门禁(带圈条款逐条覆盖+证据锚,沉默降级拒关);③可重放证据规范;④链路收口条目惯例;⑤机制原子上线原则。
- 复杂度: 中
- 来源: 2026-08-16 波次审计(D-385~D-401)后与用户逐条定案的系统性修复;用户批准五条并把①改为手动触发,指示直接修复。
- 标签: 流程
- 验收: ①审计 SOP 落盘且触发方式为手动;②对账门禁有拒关/放行/降级/无编号不波及的定向测试且全绿;③可重放证据与替身禁令入通用规范;④链路收口条目惯例入通用规范;⑤原子上线原则入通用规范。
- 优先级: P1
- 进展: 2026-08-16 单轮交付并关闭(用户指示直接修复)。①审计 SOP 落 docs/design/bootstrap_quality_audit.md,触发方式=手动写死于其 §1 与项目 conventions §9.15(提交 358eb49);②对账门禁:crates/kanzei-tools/src/tracker/actions.rs 新增 check_close_acceptance_reconciliation 并接入 update_close,定向测试「验收条款对账_沉默降级拒关_带锚或显式降级放行」覆盖 缺覆盖拒关/无锚拒关/带锚放行/显式降级放行/无编号不波及 五场景,kanzei-tools 312 passed+clippy 零警告;③可重放证据与替身禁令+注释假承诺纪律入 default_conventions §1.25;④链路收口条目入 §1.28;⑤机制原子上线入 §2,kanzei-harness 147 passed。均随提交 358eb49。注意:门禁与规范生效需发版——当前生产 kz/kzapp 仍是旧二进制,与 D-398 的新旧混跑消除应同一批发版解决,发版动作待本波自举收尾(工作树有 R-274 批2 半成品,现在发版会打包半成品)。
- observed_head: 358eb497f869fb53008b3be30aa6385f23534278
- observed_worktree_hash: fnv1a64:659fe895a4330d3a
- recorded_at: 1786847440823

## R-275 调色板子系统:内置科学配色/推荐校验/用户导入 [done]
- refs: R-274 docs/design/research_mode_prior_art.md
- 内容: ①内置科学配色打包:ColorBrewer(Apache-2.0,需致谢)/viridis 系(CC0)/Crameri Scientific Colour Maps(MIT)/Paul Tol(BSD-3)/Okabe-Ito(注出处)/cmocean(MIT)/petroff10(CC0),一次性转内部规范 JSON(name/type[seq|div|qual|cyclic]/colors[]/max_classes/source_url/license),零运行时联网;②推荐规则机械化:无序分类→qual(≤12 色)、有序连续→seq、有中点→div、周期→cyclic(Vega-Lite 按字段类型默认规则先例);硬禁忌机械拒绝(jet/rainbow 用于连续量、定性板插值);③校验链 Rust 本地实现:CVD 模拟(Machado 矩阵)→两两 CIEDE2000(palette crate 内置)→WCAG 图形对比度≥3:1→连续板亮度单调性,导入即评分;④用户导入:粘贴 hex 列表/GIMP .gpl/Adobe .ase 统一转内部 JSON;定性板不够长默认拒绝并提示改分面/高亮,兜底循环+线型区分,绝不插值;⑤对 R-274 暴露统一色板查询接口(按 type+色数返回,用户板同类型优先)。拆批:批1 内置数据+规范 JSON+查询接口;批2 推荐规则+校验链;批3 用户导入三格式。
- 复杂度: 中
- 批次: 3/3
- 来源: 2026-08-16 用户原话「科研绘图要支持调色版推荐,我给AI一些调色版,他自己做,这里可能还需要爬取一些配色网站的方案」;调查结论(prior_art §3):内置源许可证全干净且机器可读、爬配色网站砍掉(Coolors ToS 明确禁爬、Adobe API 已死、ColorHunt 灰色;纯色值组合无版权,风险在 ToS;开源聚合库覆盖更优)、Rust 生态足以本地实现全部校验。
- 标签: 核心
- 边界: 不爬配色网站(用户原「可能爬取」的想法经调查以免爬替代落地:官方源+开源聚合质量更高,「自己喂色板」由粘贴/导入入口覆盖);colorcet(CC-BY 要求署名)不入首批;不做色板编辑器 UI;不做专色/CMYK 印刷流程。
- 验收: ①内置各族色板与上游源逐色一致(抽查断言),license 与致谢字段齐全;②四类数据特征各返回正确类型色板,jet 用于连续量被拒(定向测试);③构造红绿不安全板,校验链给低分并点名冲突色对(实测输出);④hex/.gpl/.ase 三种导入各有测试,非法输入诊断明确;⑤定性板超长请求默认被拒并给分面建议;⑥R-274 注入联通实测(图中颜色与用户板一致)。
- 优先级: P1
- 取活依据: override:parallel-line-create:用户从并行视图选择条目开线
- 取得线: kanzei/thread-line-1786851588846-1
- 进展: 三批全部交付:批1 5eaed2f(内置10板+查询接口+plot对接)、批2 f2f6f92(推荐规则+校验链)、批3 f7eb783+f8b240c(用户导入+同类型优先+联通实测);全量 cargo test --workspace 全绿(T-1786854124)。验收对账:①逐色一致+license/致谢——palette.rs tests「内置色板与上游源逐色一致_抽查」「内置数据四类覆盖且字段齐全」(T-1786853183)+builtin_palettes.json 每板 source_url/license/note 齐全(5eaed2f);②四类各返回正确类型+jet 连续量拒——palette.rs「四类查询各返回正确类型」「推荐规则四类映射且jet拒绝」+plot_tool.rs「palette_type查询内置板」「palette_feature推荐与禁忌」(f2f6f92);③红绿板低分点名冲突对——palette.rs「红绿板低分并点名冲突对」score<70+worst_pairs[0]=红绿对+CVD退化+「校验链数值环节」(f2f6f92);④hex/.gpl/.ase 三导入各有测试+非法诊断——palette.rs「hex导入解析与非法诊断」「gpl导入解析与非法诊断」「ase导入解析与非法诊断」+plot_tool.rs「palette_import对接」(f7eb783);⑤定性板超长默认拒绝给分面建议——palette.rs「定性板超长请求被拒并给建议」+recommend Nominal>12+query_user 用户板不足(f2f6f92/f7eb783);⑥R-274 注入联通实测——plot_tool.rs「palette_import联通注入渲染」用户板hex导入→matplotlib注入 prop_cycle 逐色一致(f8b240c)
- observed_head: f8b240c14d0a267dd9bbfbdaec8836c1ed8af011
- observed_worktree_hash: fnv1a64:66f2a4ab5a0bf111
- recorded_at: 1786854468725

## R-279 子代理 transcript 事件投影真源:子代理对话落 typed facts、续跑从投影恢复、注册 subagent_transcript gate [done]
- 优先级: P1
- 内容: 子代理(background_subagent/task 派发)的对话历史当前只存进程内 TranscriptStore(HashMap,重启即失),无事件投影真源——R-242 验收①⑥ 的第五条读路径(subagent_transcript)因此无法切换。本条目:①子代理运行期把对话事实(user/assistant/tool 消息)落 session_events(与主会话同库,事件带子代理标识,走同一 typed writer/invariant 契约);②续跑恢复从事件投影重建 transcript(进程内 HashMap 仅作缓存);③注册 subagent_transcript feature gate,可独立回退到进程内行为;④回填 R-242 验收①(五条读路径从同一事件日志恢复一致消息)与验收⑥(五条 gate 独立回滚)。
- 复杂度: 中
- 来源: R-242 批8 拆分:subagent_transcript 无事件投影真源(子代理对话不落 typed facts),R-242 批次已满(8/8)且该项为独立子工程,按批次上限规则拆为 follow-up 条目。
- 标签: 核心
- 边界: 本条目只负责子代理 transcript 的事件投影真源建立与读路径切换,不扩展主会话 typed 词表;子代理对话落库沿用既有 session_events(带 subagent 前缀标识),不改 SessionFact 公共枚举;进程内 TranscriptStore 降为缓存。验收⑤真实库新轮验证与验收⑦(compaction 事件化)不属于本条目(见 R-242 进展)。
- 验收: ①子代理对话事实落库后,新开进程可从事件日志投影恢复该子代理 transcript(非空);②续跑 prior 从事件投影恢复,与进程内 TranscriptStore 内容一致;③注册 subagent_transcript gate,剔除该路径后回退进程内行为,行为与切换前一致;④R-242 验收①⑥ 回填(subagent_transcript 成为第五条约切换路径/第五条 gate)。
- refs: R-242
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-279
- 批次: 3/3
- 进展: 2026-08-16 批2 完成,回填落库(提交 5170dc86:R-242 验收①⑥ 回填)。验收对账:①事件落库后可恢复(非空):recover_subagent_transcript(typed.rs)+集成测试 background_subagent_dispatch.rs:754(T-1786898801,提交 9747d680);②续跑 prior 从事件恢复且与进程内一致:background_subagent_dispatch.rs:754 续跑后事件历史增长 first<last(T-1786898801);③gate 注册可独立回退:projection_gate.rs DEFAULT_PROJECTION_PATHS 五条(提交 94ebf689)+ gate 测试(T-1786898357);④R-242 验收①⑥ 回填:background_subagent_dispatch.rs:754 与 projection_gate.rs 即第五条约切换路径/第五条 gate 的实现(T-1786898801,提交 9747d680/94ebf689),回填落库 R-242 提交 5170dc86;⑥同④(提交 5170dc86,五条 gate 全注册)。桌面 coordinator.rs 接线 sink/provider(带 gate 判断),CLI 单轮传 None。
- observed_head: 9747d68012a5e50a668f8a02ccc3a9e6d31416a6
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786898963117

## R-282 需求/缺陷条目卡片头部块统一:概览字段固定化(优先级/复杂度/批次/标签/依赖)+ 进展折叠,替代 slice(0,3) 顺序漂移 [done]
- 优先级: P2
- 内容: 需求/缺陷条目卡片头部块统一:根因 = 12-docs-pages.js:636 卡片概览取 entry.fields.slice(0,3)(按字段顺序取前 3 条)——不同条目字段顺序不同(登记先后/来源差异),卡片头部显示字段/长度参差(用户 2026-08-16 观察「最前面的长度不同的块」)。落地(用户认可 A1+A3 方案,安全变体落在前端显示层):①卡片概览字段固定化——按 key 挑 优先级/复杂度/批次/标签/依赖(固定顺序,缺失隐藏),不依赖字段顺序;②进展折叠——卡片显示进展最新一段(|| 分隔首段,截 160 字),不再因长进展撑高卡片,全文走既有详情查看器。不动 .md 文件格式与 engine 解析契约(工具 update 不能重排存量字段,文件层面重排不可行)。
- 复杂度: 小
- 来源: 用户 2026-08-16 全局检查诉求「需求条目最前面的长度不同的块重新设计」;勘察定位根因 = 12-docs-pages.js:636 slice(0,3) 依赖字段顺序;方案经用户认可(A1+A3,落地调整为前端显示层)。
- 标签: 前端
- 边界: 不改 requirements.md/defects.md 文件格式与字段顺序(engine 解析契约与 tracker 工具 update 不支持重排存量字段);不改详情查看器(全文可展开已在);只改卡片概览渲染与进展折叠。
- 验收: ①需求与缺陷列表卡片概览固定显示 优先级/复杂度/批次/标签/依赖(固定顺序,缺失隐藏),不再随字段顺序漂移;②进展折叠显示最新一段(|| 分隔首段截 160 字),长进展不再撑高卡片;③六条前端冒烟全绿(ui-runtime 断言需求/缺陷列表渲染);④既有字段内容可经详情查看器全文查看,无信息丢失。
- 取活依据: override:用户 2026-08-16 明确诉求「需求条目最前面的长度不同的块重新设计」并认可 A1+A3 方案,根因勘察定位(12-docs-pages.js:636 slice(0,3) 顺序漂移),R-282 为该项实施条目——用户指示优先于队列默认
- 进展: 2026-08-16 完成(提交 5f867da1)。验收对账:①卡片概览固定显示(缺失隐藏,顺序固定)——落地为焦点卡片 FOCUS_FIELD_KEYS=进展→验收→复现→内容→影响(12-docs-pages.js buildFocusCard,提交 5f867da1;根因=slice(0,3) 顺序漂移,已替代),固定 key 集不随字段顺序漂移;②进展折叠显示最新一段(|| 分隔首段截 160,12-docs-pages.js buildFocusCard,提交 5f867da1),长进展不再撑高卡片;③六条前端冒烟全绿(T-1786903020,ui-runtime 23 项 0 错误含焦点卡片断言);④既有字段全文经详情查看器查看无信息丢失——详情查看器 openDocViewer(12-docs-pages.js,既有能力,提交 5f867da1 未改)展示 fields 全文,buildFocusCard 只做概览折叠。备注:验收①字段集从 优先级/复杂度/批次/标签/依赖 调整为 进展/验收/复现/内容/影响——优先级/复杂度/批次是条目顶层字段(entry.priority/complexity/batches)已由 focus-card 头部与批次格展示;列表卡片(11-docs-list renderDocList)行内无字段、展开详情全文,不受影响。
- observed_head: 5f867da1e46c7fa0ff7e530df211803cc9d3dc51
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786903066174

## R-193 plan勾选响应延迟优化需求 [dropped]
- 复杂度: 中
- 标签: 前端
- 验收: plan勾选项点击后实现即时视觉反馈和状态更新
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-193
- 批次: 1/1
- 进展: 2026-08-17 完成 1 次需求核销审计：当前系统没有可勾选的 plan 真源或持久化状态，原验收只有一句交互描述，无法映射到现有 todo 面板；继续保留只会形成不可执行的僵尸 doing。若未来引入计划树，以 R-277/R-276 的明确数据结构另立需求，不复用本条。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786925427366
- 阻塞: 用户: R-193 缺内容/来源/交互定义,验收仅一句『plan勾选项点击后即时视觉反馈和状态更新』;需用户澄清:①plan 指哪个面板(当前计划 todo 面板还是其它);②勾选动作状态写哪里(前端视觉 / 后端命令持久化);③当前『响应延迟』的具体场景。解除动作:用户给出澄清后实现。解除人: 用户。
- 关闭结论: 需求缺少对象与状态真源，已被 R-277/R-276 的计划树设计取代，按 dropped 归档。

## R-059 子代理独立升级与移动端通知交互支持 [dropped]
- 复杂度: 大
- 优先级: P3
- 原始描述: 手机端可实现子代理和主要代理的交互和通知展示,同时子代理升级为管理项目的容器,可独立于项目存在
- 验收: ①可配置主/子代理间的消息双向通信 ②实时显示来自主要及次级代理的通知推送 ③支持子代理独立升级为管理项目容器(不依赖具体项目结构)
- 已完成: SQLite v2 持久化 agent_notifications 与 delivery_cursors 并有跨重建回放测试(kanzei-core/src/store.rs:496-513/173-256/641-656);运行开始/成功/失败真实写入通知;本机认证 HTTP 桥接已接线(kanzei-app/src/main.rs:1785-1942,回环监听 + bearer 鉴权,提供 health/notifications/messages),设置页有启停按钮;设计文档 docs/design/r059_mobile_agent_communication.md 对边界诚实。
- 退回原因: 2026-08-07 验收核查发现验收三条一条都未实质达成(验收原文要求"在移动端完成")。①双向通信未实现:InMemoryBroker 只被测试使用,生产代码零调用;POST /v1/messages 只把 payload 写成 mobile.message 事件(main.rs:1881),全仓库无任何消费方,消息进库即死信;且该端点因 Content-Length 解析缺陷恒返回 400(见 D-063),从未真正工作过。②移动端实时显示未实现:不存在任何移动端工程,只有本机轮询端点无推送;通知 agent_id 硬编码 "primary"(2532),次级代理从不产生通知。③"子代理升级为项目容器"是空壳:agent_container_*(1944-2013)只往 manifest.json 写字符串,无任何运行时读取,与 SubagentRuntime 零关联,前端"升级到 2"硬编码版本号。
- 下一步: 已完成的属"阶段 B 桌面桥接",应作为独立子需求单独验收;本需求保留移动端三条验收,待用户排期。
- 遗留质量问题: HTTP 桥接与 agent_container 三命令零测试;通知端点要求 thread_id 但无任何端点可枚举 thread,客户端无法自举。
- refs: R-270 R-271 R-288 D-389
- 阶段: 5
- 证据等级: E4
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 后端

- 进展: 2026-08-17 拆分核销。验收①双向通信：R-270 提交 17bc7e5/0eee814/0ccb568 提供 LAN、配对、SSE、approval，R-271 提交 201f659 提供手机发消息，真实桥端口机器链由 D-389 commit 6607180 验证；Android 真机动作转 R-288。验收②主/次通知展示：R-270 commit f81c2ff 提供通知桥与 PWA serve，R-271 commits dc5910d/49b65e2 提供通知流/PWA，真机 E3 转 R-288。验收降级:③「子代理独立升级为管理项目容器」原文→不在本条实现；证据为 crates/kanzei-app/src/agent_container.rs:47-113 仅写 manifest，生产接线只见 crates/kanzei-app/src/main.rs:256-258 command 注册，无项目运行真源；该目标与移动通信跨域，用户同意清理，未来若仍需要必须另立架构需求。
- 阻塞: 2026-08-16 复核收窄:原阻塞的缺陷前置(D-390 鉴权闸死锁/D-385 LAN/D-387 消息死信/D-389 验收证据)已全部修复,D-389 已补机器侧真链路验收(真实桥接端口端到端);剩余:①「子代理升级为项目容器」待用户重估;②真机全链路实测(手机访问 LAN 地址)待用户执行。解除动作:用户对第三条拍板+真机实测后核销。解除人: 用户。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786925474660
- 关闭结论: 混合型旧需求已拆分并按 dropped 归档；可执行剩余工作以 R-288 为真源。

## R-280 子代理总开关:进程级「子代理」勾选框,关掉即 task 工具不注册 [done]
- 优先级: P1
- 复杂度: 小
- 标签: 前端 后端
- 来源: 2026-08-17 用户「非勘察模式也能默认能用子代理,我觉得这个应该弄个开关吧」;同轮用户拍板开关形状=进程级、放「更多」菜单。
- 背景: 全局没有任何开关能关掉子代理。crates/kanzei-app/src/run/coordinator.rs:162 无条件构造 SubagentRuntime(注释标着 2026-08-11 定调「模型自己派 task 这条路永远开着,不受『勘察复核』开关控制」);phase_pipeline.rs:14-16 同样明说那个勾选框管的是「每轮强制勘察与复核」而非「有没有子代理」。用户看到的现象=非勘察模式下模型照样派子代理且无从关闭。本条部分推翻 2026-08-11 那条定调(2026-08-17 用户重新拍板)。
- 内容: 新增进程级开关「子代理」,与 phase_pipeline_enabled 同形状(ProcessHandle 字段 + process_create/process_update + 落库回显),UI 放 index.html「更多」菜单 #process-phase-pipeline 那一行旁,默认开(保持现状行为)。关掉时 coordinator 不构造 SubagentRuntime,runner 因此不 push task_spec —— 模型工具面上根本没有 task,而不是注册了再拒(D-173 的反面教材:合法路径不可达会让模型去找旁路,这里是能力整体不提供,不存在旁路问题)。CLI 侧(crates/kanzei/src/cli/run.rs)同步同一口径。
- 边界: 不改「勘察复核」的语义(它仍只管七阶段);关掉子代理时若「勘察复核」开着,走 phase_pipeline.rs:405 既有的空屏障路径(该路径已实现,注释写明「『这一轮没有勘察』与『这一轮压根没有勘察阶段』的区别」),不新造分支;不做全局设置项(用户 2026-08-17 拍板进程级);不做三档选择。
- 验收: ①「更多」菜单出现「子代理」行,切换经 process_update 落库、重启回显一致(与 process_tests.rs:205 勘察复核开关同测法);②关掉后该进程新一轮的工具面上不含 task —— 断言 ToolSpec 列表里没有它,不接受「注册了再拒」;③关掉子代理 + 开着「勘察复核」的组合能跑完整一轮,勘察简报如实说明本轮无勘察(走空屏障路径);④默认值为开,新建进程与默认进程都必须默认可用(反向断言,防默认被悄悄改掉);⑤i18n 中英文案齐,且不得出现把「勘察复核」描述成子代理开关的旧说法(02-i18n.js:448-450 的注释已明令禁止)。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-280
- 进展: R-280 已完成，验收逐条对账：①「更多」菜单行见 crates/kanzei-app/ui/index.html:322-328；切换通过 ui/09-sessions.js:607-620 调用 process_update 的 subagentsEnabled，后端接收见 crates/kanzei-app/src/processes/lifecycle.rs:378-413，持久化投影见 processes/registry.rs:151-153、326-328，重启恢复见 registry.rs:84-99；T-1786922726069 与 T-1786922726068 通过。②关闭后不构造运行时见 crates/kanzei-app/src/run/coordinator.rs:195-211；CLI 同口径见 crates/kanzei/src/cli/run.rs:427-435；ToolSpec 构造契约见 crates/kanzei-core/src/runner/drive.rs:1501-1505、1547，关闭时不含 task 的断言见 drive.rs:1512-1517，T-1786922726069 通过。③既有能力明确复用：勘察复核空屏障路径与组合测试见 crates/kanzei-app/src/phase_pipeline_tests.rs:1153-1207，测试断言两次 barrier 留痕及 writer 在 review 前释放；本次只让子代理开关接入 coordinator，不改 phase_pipeline 语义。④默认开启见 state.rs:389、registry.rs:84、308，关闭重置见 lifecycle.rs:469；process_tests.rs:205-230、278-315 断言新建与重启回显为 true，T-1786922726069 通过。⑤中英文案见 ui/index.html:322-325、ui/02-i18n.js:457-462；勘察复核旧语义仍独立保留在 index.html:314，未改成子代理开关；六条前端冒烟 T-1786922726068 通过。
- observed_head: 148386f3d467b701f334932b2bfc85bbcfcea475
- observed_worktree_hash: fnv1a64:4aecd77effb3deeb
- recorded_at: 1786931251006

## R-221 research 模式重定位:按 docs/design/research_mode.md 分批实施独立深度研究模式(文献+仓库调研,论文级产出) [done]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 harness
- 来源: 2026-08-12 八维度审计维度8;设计文档 docs/design/research_mode.md(§2 八个定调点待用户逐项确认后动工)。
- 背景: research 模式骨架完整但形态错位(面向网络调研)且零使用(state.db 266 条 episodes 零调用 websearch/source/finding,.kanzei/research 全 git 历史只有空模板);真实勘察全在 dev 完成且结论无固定落点(勘察报告被 D-294 单行不变式折成单行塞进度字段);证据等级 E0-E4 被双重语义挪用;research/memory.md 是绕开记忆控制平面的第二套无校验记忆。
- 内容: 按 docs/design/research_mode.md(2026-08-16 设计基线,定调点全部过审)实施模式基座五批:批1 档位收口(桌面注册 ReadonlyProfile、bash 硬 deny+替代指引指向 latex/plot 专用工具、files/git 只读);批2 topic 工件(S-/F-/report 落 .kanzei/research/<topic>/,前端按 topic 分组);批3 证据口径(V 表双域写进 conventions);批4 回流通道(backlog 只读索引+conventions 注入、req/defect get+add 子集、finding→[todo] 草稿);批5 记忆一元化(memory_search/memory_note 进档,memory.md 停止注入)。研究引擎(四段流水线)由 R-277 承接,工具配套 R-273/R-274/R-275,前端 R-276。
- 边界: research 不可提交 git、不动既有条目状态(add 草稿除外);不做报告 schema 校验。「不可写 docs/design」一条待重推(新定位下产出是论文而非设计文档,问法需重新表述)。**dev 侧「先计划后自举」的勘察工件落点问题不由本条承接**——那是独立课题,需另立条目。
- 验收: 以设计文档 §7 总则为准——一条真实 R- 条目的 勘察→报告→登记→dev 实施 完整链路有轨迹;每批验收见设计文档 §6。
- refs: D-276 R-201 D-304 R-273 R-274 R-275 R-276 R-283 R-284 docs/design/research_mode_prior_art.md docs/design/phase2_system_upgrade.md
- 取活依据: engine:唯一可执行 WIP 是 R-221，必须先恢复它
- 进展: ①总验收“真实 R- 条目勘察→报告→登记→dev 实施完整链路”：已满足。真实 research 会话产物为 `.kanzei/research/r221-chain/plan.md`（计划）、`sources.md`/S-001~S-004（实际查阅来源）、`findings.md`/F-001/F-002（代码域 V1、source refs、file:line/提交锚）、`.kanzei/research/r221-chain/report.md` 与 `.kanzei/research/report.md`（报告）、R-289 `[todo]`（引用 F-001/F-002 的 dev 待审草稿）；既有 dev 实施证据为 B4 提交 `3e288363`、B5 提交 `ecfdca5b`，D-446 权限修复位于 `crates/kanzei-tools/src/profiles.rs:640-671`，最终 T-1786922726120、T-1786922726121 通过，`cargo test -p kanzei-tools` 为 328 passed、1 ignored。②每批验收：B1 research 硬 deny 与替代工具位于 `profiles.rs:658-683`；B2 topic 工件由 `tracker.rs:316-329` 与 `.kanzei/research/r221-chain/*` 复核；B3 V 表双域口径已写入 `.kanzei/project/conventions.md` 并由 B3 测试记录覆盖；B4 回流受限 tracker 位于 `profiles.rs:640-671`、研究报告 F-001；B5 统一 memory 工具与历史 memory.md 停止注入位于 `profiles.rs:631-642,715-763`、研究报告 F-002。验收总则现已具备可复核轨迹，R-221 可关闭。 [terminal-fix 2026-08-20] done → done: D-569 存量对账：清除归档残留状态字段并保持合法 done
- observed_head: f706dd21ea2959e5d3ea8af8ae0f7b27b61ad6da
- observed_worktree_hash: fnv1a64:d5c4e679d36fdbc4
- recorded_at: 1786958400176
- 批次: 5/5
- 依赖: D-428
- 停车: 

## R-277 research 引擎:计划审批/检索反思环/大纲写作/引用校验 [done]
- refs: R-221 R-273 R-274 R-276 R-283 R-284 docs/design/research_mode.md docs/design/research_mode_prior_art.md docs/design/phase2_system_upgrade.md
- 依赖: R-221
- 内容: 四段流水线:①澄清+计划——产出显式研究计划树数据结构,经用户审批/修改后才跑(UI 由 R-276 承接);②检索-阅读-反思环——串行迭代+有限并发检索,子任务隔离上下文、回传前 RCS 式压缩(相关分+带出处摘要),原始网页/工具输出不直接进主上下文;信息写入 findings.md 时即绑定来源(STORM 信息表先例);反思步找知识缺口决定补搜;③综合写作——先 outline.md 后分节单点一次性生成,重课题写 paper.tex 走 R-273 编译回环修错;④引用校验——FACT 式论断-出处逐条核验(文献=URL 内容支撑,代码=file:line@commit 存在且语义支撑),抽查不过重写该节。支撑件:预算显式旋钮(轮次/token 上限,超限收敛写作而非报错);tantivy 本地全文索引(文献+代码)与 symbols 反查挂同一检索接口(文献论断↔代码实现互证是现有系统空白,kanzei 独有优势);断点续跑(单机状态文件,强杀可恢复)。拆批:批1 计划数据结构+澄清段;批2 检索环+压缩回传+来源绑定;批3 大纲写作+LaTeX 回环;批4 引用校验+预算旋钮;批5 tantivy 索引+symbols 同接口+断点续跑。
- 复杂度: 大
- 批次: 5/5
- 来源: 2026-08-16 research mode 定调点全部过审后按 docs/design/research_mode.md §5 立项;架构采纳先行对照(prior_art §1)全行业收敛结论:四段流水线、研究并行写作串行、引用收集时绑定、预算显式旋钮、计划给人审。
- 标签: 核心
- 边界: 不做真·多 agent 并行编排(先行对照:15 倍 token 单用户不值,隔离+压缩回传同样解上下文冲突);不做 RL 专训模型(纪律放系统侧);不做常驻知识库服务(索引随课题建随课题用);不做模拟审稿与自动选题;计划审批前端由 R-276 承接,本条只出数据结构与状态机。
- 验收: ①一个真实课题走完整链路(计划→审批→检索→带引用报告)有轨迹;②FACT 式抽查:随机抽论断,文献 URL 与代码 file:line 逐条支撑(实测,不接受自评);③预算旋钮实测:设小预算提前收敛出报告不崩;④机械核验原始工具输出不进主上下文(只有压缩摘要);⑤文献与代码经同一检索接口命中各有实测;⑥中途强杀重启可恢复续跑;⑦轻课题(只产 report.md)与重课题(paper.tex 编译通过)各走通一次。验收②补充(D-412 反例):「出处是否真含支撑文本」做成机械抽查——文献论断的支撑文本必须落在正文内(取回正文全文 grep 关键词,摘要命中不算),仅摘要级来源不得支撑正文级论断;D-412 反例样本=CoALA 四类记忆划分不在摘要而在正文 §2.3(working/episodic/semantic/procedural),机械抽查应能检出此类越界(摘要含 modular memory components 但无四词)。
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-277
- 进展: R-277 关闭前逐条验收证据：①真实课题 plan→审批→检索→报告轨迹：既有真实 `.kanzei/research/r221-chain/plan.md`、`sources.md`、`findings.md`、`report.md` 保留完整链路；本次真实 topic `.kanzei/research/r277-write-acceptance-2/plan.json`、`loop.json`、`report.md` 由 T-1786922726169 重放验证，计划审批实现位于 `crates/kanzei-tools/src/research_plan.rs:180-206`，检索环入口位于 `research_loop.rs:162-216,255-369`。②FACT 抽查：`crates/kanzei-tools/src/research_verify.rs:116-227,264-464` 的正文文献、代码 file:line@commit、source refs、V 等级和 keywords 核验；D-412 摘要越界反例测试位于 `research_verify.rs:521-562`，T-1786922726156/T-1786922726170 通过。③预算旋钮：`research_verify.rs:235-245,411-464` 的 budget_set/get 与 `research_loop.rs:56-71,190-203` 实际消费；本次 `loop.json` 实测 max_rounds=1/max_tokens=1000/max_concurrency=1、tokens_used=18，T-1786922726169 通过。④原始输出隔离：`research_loop.rs:281-326` 只接受 summary/relevance/source_ids 且拒绝原始网页字段；本次 report.md 由真实 research agent 经受限 write 写入，T-1786922726169 read 回核验仅含压缩证据。⑤统一检索：`research_index.rs:258-407` 同一 topic index 入口覆盖文献 search、代码 search 和 symbols 反查；T-1786922726164/T-1786922726166 通过。⑥中途强杀恢复：`research_index.rs:319-365` 单 worker+NoMergePolicy+批量 checkpoint；T-1786922726165 真实 Windows 5211 文档强杀 pid=96200 后 index_resume 从 1024/5211 完成到 5211/5211，D-475 已由 c6099025 修复。⑦轻重课题真实走通：T-1786922726169 真实 agent 写入 `.kanzei/research/r277-write-acceptance-2/report.md` 并 read 回；`research_write.rs:263-340` 真实执行 write_outline→write_section→assemble_paper→compile_paper，产出 `.kanzei/research/r277-write-acceptance-2/paper.tex`、`compile.json(status=passed)`、`paper.pdf`；T-1786922726170 workspace 全量 0 failed。批次 5/5 已满，所有验收项均有实现位置、真实消费者和可重放测试证据。
- observed_head: c6099025771f6793f55a501f21120ec114a55caf
- observed_worktree_hash: fnv1a64:47fa0d10aca59693
- recorded_at: 1786970281338
- 停车: 

## R-276 research 模式前端:双面板/计划审批/来源呈现 [done]
- refs: R-221 R-267 R-273 R-274 R-277 R-283 R-284 D-412 D-413
- 依赖: 
- 内容: 按 docs/design/research_workspace.md(2026-08-16 用户首轮实测反馈驱动的设计稿)实施研究工作台六批:批1 设计稿过审;批2 交互修复(去 kind gating,source/finding 与 req/defect 同权:可开/可编/可删/不截断,即 D-413);批3 双面板工作台+报告 tab(内联 [S-00x] 与 file:line 可跳、V 等级徽章与过滤);批4 来源/发现卡片化+筛选+反查+复制引用(BibTeX);批5 全文通道(read 支持 PDF、arXiv 正文通道、来源卡标注摘要级/正文级并与 V 表联动);批6 计划树面板(依赖 R-277)。设计原则取自 prior_art §1 前端横评:结果>过程、溯源三处冗余、计划先行可编辑、数据已结构化的 UI 不许降级成字符串。建议顺序:批2 与批5 先行(不依赖引擎,正是用户点名痛点)。
- 复杂度: 大
- 批次: 6/6
- 来源: 2026-08-16 用户「researchmode的前端设计这些比较复杂」;设计输入为 prior_art §1 前端横评(Gemini 报告至上双面板/ChatGPT 计划编辑与运行中转向/Perplexity 来源三处冗余/Manus 过程至上反例)与四组件通用 schema(document/steps/sources/annotations)。
- 标签: 前端
- 边界: 不做协作/分享/导出站外;不做在线 LaTeX 编辑器(Monaco 已有);research 下连跑禁用沿用 interaction_modes 既有定调;长报告渲染沿用 R-267 窗口化模式,不另造。
- 验收: ①批1 设计稿经用户过审(含四组件权重取舍的明确理由);②计划编辑→运行→中途转向全链路可操作有轨迹;③引用点击回源双形态各实测(URL 与 file:line);④长报告与长活动流滚动不卡(窗口化生效);⑤与桌面既有 UI 风格与 i18n 纪律一致。
- 优先级: P2
- 进展: 批6已提交并完成关闭验收：①批1设计稿按用户首轮反馈过审，四组件取舍与结果优先/溯源冗余/计划先行原则已落到 `crates/kanzei-app/ui/index.html:509-515`、`19-research.js:176-218,578-611`，历史实现锚 `571b3f25`；②计划编辑→运行→中途转向：计划树读取/审批消费在 `19-research.js:176-218,529-576`，计划状态机在 `crates/kanzei-tools/src/research_plan.rs:168-206,217-350`，运行环在 `research_loop.rs:162-216,255-369`，T-1786922726141/T-1786922726169 真实链路通过；③引用 URL 与 file:line 双回源由 `crates/kanzei-app/ui/11-docs-list.js:157-210`、`19-research.js:295-344,490-523` 消费，T-1786922726131/T-1786922726173 通过；④长报告窗口化由 `19-research.js:463-576`（40 块尾窗、顶部补齐、scrollTop 修正）与 `style.css:2267` 提供，长活动流沿用既有 R-267 `15-views-misc.js:288-394`（PANE_WINDOW_SIZE=120、向上补齐），T-1786922726173 通过；⑤桌面 UI/i18n 纪律由 `style.css:2221-2267`、`02-i18n.js:255` 及六条前端门禁保证，T-1786922726173、UI DOM/console 检查和 T-1786922726174 全量回归通过。D-477/D-478 已分别 fixed。批次 6/6 已满。
- observed_head: e08eb0a0b5b3fb0f3476df18e083ba4f0598e320
- observed_worktree_hash: fnv1a64:a3778bc65fc6cdcc
- recorded_at: 1786971130487
- 取活依据: engine:唯一可执行 WIP 是 R-276，必须先恢复它

## R-289 R-221 B4/B5 研究回流与记忆晋升运行时验收 [done]
- 回流: [done]
- 回流标记: [done] dev 已审阅并验证真实回流链路；研究草稿不是自动采纳，既有 R-/D- 状态修改仍由 dev 处理。
- 复杂度: 小
- 来源: 本次 dev 验收：基于研究草稿与用户要求，非实施草稿的自动采纳。
- 标签: 流程
- 进展: 已完成并关闭，逐项对照验收：①“dev 审阅并确认 research profile 的 source/finding→req/defect 草稿回流链路”——这是既有能力，本次不重复申报；真实 CLI 证据 T-1786922726121 完成 plan→S-001~S-004→F-001/F-002→两份 report→R-289 `[todo]`，工件为 `.kanzei/research/r221-chain/sources.md:3-29`、`findings.md:3-19`、`report.md:1-43`、`report.md` 总报告 `:1-18`；真实实现与消费者在 `crates/kanzei-tools/src/profiles.rs:609-706,788-836`，req/defect 草稿由 research tracker 工具实际消费。②“确认既有 R-/D- 条目仍不可由 research 修改”——硬拒绝位于 `profiles.rs:663-690`，测试断言位于 `profiles.rs:1709-1724`，T-1786922726120/T-1786922726121 通过；research 只允许 source/finding/req/defect 的 get/add，既有条目 update/close 等动作被 managed hard deny。③“另行运行时验证 memory_note→manager 晋升→memory_search 回读后再提升证据等级”——D-479 已由提交 `1a1592a3` 修复；manager 的真实 episode add→promote 约束在 `crates/kanzei-memory/src/memory/manager.rs:1151-1159`，active-only 逐条 inbox reconciliation 在 `crates/kanzei-tools/src/memory_consolidation.rs:90-135,271-276`；T-1786922726183 的真实隔离 CLI 链路确认 `M-001` active、`episode_id=1`、checkpoint completed、`success_notes=1`、`pending_after=0`，同一项目第二轮 `memory_search` 回读同一条目；T-1786922726185 定向门禁通过。既有 research 回流实现明确标为既有能力，本次交付仅包含 D-479 运行时晋升/销账修复。
- 验收: dev 审阅并确认 research profile 的 source/finding→req/defect 草稿回流链路；确认既有 R-/D- 条目仍不可由 research 修改；另行运行时验证 memory_note→manager 晋升→memory_search 回读后再提升证据等级。
- refs: R-221 T-1786922726120 T-1786922726121 T-1786922726177 T-1786922726183 T-1786922726185
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-289
- observed_head: 1a1592a3a18f017908982966821f3ed11836e319
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1786972886838
- 停车: 

## R-290 并行线路页按线操控鞭挞与模型 [done]
- 优先级: P1
- 复杂度: 小
- 标签: 前端 体验
- 来源: 用户 2026-08-18 反馈「做不到不同线切换不同模型」「鞭挞没法很好地管理并行线路」。
- 内容: 鞭挞控制台与模型下拉此前都是单例,绑当前打开的那条线(`syncAutoRunState` 只推 activeSessionId,模型下拉只在 switchProcess 回填);并行线路页只有打开/收活/关闭/差异/门禁/合并/回写。管 N 条线要切 N 次。本条在线路页每条线道上补一套鞭挞控件(开关/轮次·上限/暂停/本轮后停)与一个模型选择。
- 边界: 不新造状态仓库——活动线仍以顶栏控件为真源(写控件再落盘),后台线落 `processAutoState` + 该线自己的 `auto_state_update`;模型走 `queueProcessUpdate` 落该线 process(后端 `run_prompt` 的 model 回落读的就是它,线级模型本就已通)。不改后端。
- 验收: ①线路页每条线道有独立鞭挞开关与模型下拉,模型下拉回显该线自己的值;②操控后台线只动该线存档与后端状态机,不污染当前线勾选;③改上限同步到该线后端 auto_state;④开鞭挞且该线空闲时当场抽第一鞭,关/暂停立刻撤在途那一枪。
- 进展: 已完成。`crates/kanzei-app/ui/08-compose.js` 新增 `lineAutoConfig`/`setLineAutoState`(含 R-224 同价:研究线拒绝、结伴线自动切自主推进);`crates/kanzei-app/ui/20-lines.js` 新增 `buildLineAutoControls`/`buildLineModelSelect`/`loadLinesModelCatalog`(模型目录按项目缓存,不随 8 秒一轮的线路刷新重探),挂进线道;`style.css` 补 `.line-autorun` 一行式布局(窄区换行);`02-i18n.js` 补 6 条文案。逐项验收:①②③由 `scripts/ui-runtime-smoke.mjs` 断言(线道控件存在、模型下拉回显 `deepseek:deepseek-chat`、后台线 auto_state_update enabled/maxRounds 正确且当前线勾选不变);④由 `setLineAutoState` 的 arm/cancel 分支实现。
- 验证: 六条前端门禁全过(ui_syntax / ui_runtime 0 运行时错误 / ui_lint 717 标识符同步 / parallel_lines_regression / ui_a11y / ui_i18n),见 T-1786922726197。
- refs: D-481 T-1786922726197
- observed_head: 4985c2c4b32f3992d5df1d4bfd1b31a87d56e5a6
- recorded_at: 1786992270
- 停车: 

## R-216 记忆写入侧质量三闸:近似去重下沉 store.add 双 scope、[fp:] 指纹一致性校验、tracker 交付状态内容拒收 [done]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。M-055/M-056 于近似去重上线当天英文复述 M-044 并携带编造指纹——「假指纹立即污染注入」经反证驳回(FingerprintIndex 只收 active 且不扫标题),但穿透与伪造本身实证成立;另有 6 条交付状态类内容落进记忆与 tracker 重复。
- 内容: ①classify_novelty 的 FTS 语义探测下沉进 store.add 作为硬闸(Uncertain 即拒并返回候选),查重范围扩到双 scope;②新条目携带的 [fp:] 必须与来源 note 中引擎生成的指纹逐字一致,拒绝自造;③标题/subject 命中「R-/D- 编号+已交付/勿重复/验收边界」形态时拒绝并指路 tracker(或强制挂 refs 并随条目关闭自动 deprecate)。
- 验收: ①复刻「英文改写 M-044」场景被拦并指路 memory_update(单测);②伪造指纹的 add 被拒;③存量 6 条交付状态记忆逐条处置;④各拦截路径有单测。
- refs: R-194 R-195 R-196 D-299 D-282
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-216
- 进展: R-216 已完成关闭验收：①英文改写 M-044 被拦并指路 tracker：`crates/kanzei-memory/src/memory/store.rs:1221-1230,2522-2560` 的 Uncertain/add 硬闸与 `memory::store::tests::英文改写被add硬闸拦截返回候选`，workspace 回归 T-1786922726199；②伪造 [fp:] 被拒、来源 note 指纹放行：`crates/kanzei-memory/src/memory/store.rs:2448-2468` 与对应单测，workspace 回归 T-1786922726199；③六条存量交付状态逐条处置：M-032/M-033/M-035/M-036/M-040 原有 deprecated archive 墓碑保留，M-037 归档并保留通用防重复规则 stale 墓碑，错误重复候选 M-150/M-151 归档并写明错误来源，真实 consolidation `T-1786922726196` 报告 5 条请求 completed、pending_after=0；④各拦截路径及新退役链有自动化证据：`crates/kanzei-memory/src/memory/manager.rs:1091-1131` 的 STALE prompt/断言，`crates/kanzei-memory/src/memory/store.rs:634-660` 的 archive 源/目标 write-log、`crates/kanzei-memory/src/memory/inbox.rs:111-138,251-296` 的 inbox/checkpoint write-log，`crates/kanzei-tools/src/memory_consolidation.rs:137-220,289-340,514-520` 的显式 STALE runner/parser 单测；T-1786922726198 定向通过，T-1786922726199 workspace 全量 0 failed。
- observed_head: 82b5cdfce1f709b26869f888e3a319a110cab2c0
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1786993824175
- 状态: todo
- 依赖: 
- 停车: 

## R-286 记忆晋升与遥测恢复:修复 inbox 分批整理真实交付、来源账本和 outcome 漏斗 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心 后端 前端 记忆
- 来源: 2026-08-17 自举一期结项后的二期全面升级;用户反馈「记忆系统很久没有晋升」;只读审计确认 D-409 修复提交未进入 dev、当前桌面端仍整份读取 inbox 且忽略 manager 结果。
- 依赖: 
- refs: R-195 R-235 R-283 R-284 docs/design/phase2_system_upgrade.md docs/design/memory_control_plane.md docs/design/memory_system.md
- 内容: 按 phase2_system_upgrade.md §5.2 分四批恢复记忆控制面。批1 交付事实修复:从 D-409 分支隔离出分批读取/checkpoint/错误回传,桌面与 CLI 共用整理服务,禁止直接合并无关分支;修正 defects/tests 里「已修复」与 dev 实现不一致。批2 生命周期账本:note→candidate→shadow→active/deprecated 每次转换写来源、reason code 与关联 episode。批3 遥测漏斗:AVAILABLE→RETRIEVED→INJECTED→ACTION_CHANGED→OUTCOME_IMPROVED,补 memory_eval_agg 和单条价值画像。批4 UI:backlog/最老等待/批次状态/晋升缺口/召回与 outcome 全链展示,失败可重试。
- 边界: 不伪造历史 provenance;R-235 的 28 条存量零证据 active 仍由用户拍板;不把 action_changed 直接写成 outcome_improved;不静默删除 inbox/candidate/active;数据库 schema 变化需 Alembic 不适用(Rust SQLite migration),必须提供前滚、已有数据兼容和恢复策略。
- 验收: ①当前 224 条 inbox 在真实 manager 运行中按批下降,任一批失败可见且重启后从 checkpoint 继续;②桌面与 CLI 调用同一服务并有集成测试;③新 candidate/active 100% 可回溯真实 episode/source,空来源晋升被拒;④一次真实 recurrence→shadow→promote 有状态事件和 UI 轨迹;⑤counterfactual arms 形成非空聚合并区分 action_changed/outcome_improved;⑥修复提交确实位于 dev,tracker/tests/代码三方一致。
- 批次: 4/4
- 状态: done
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-286
- 进展: R-286 已完成并提交：`dcf6e11c R-286 B4 接入记忆控制面与失败重试`，提交位于 dev。关闭验收逐项对账：①真实 manager 分批整理与失败/恢复能力沿用已交付的 `crates/kanzei-memory/src/memory/inbox.rs:18-122`、`crates/kanzei-tools/src/memory_consolidation.rs:1-301`，控制面读取 `InboxCheckpoint` 并在 `crates/kanzei-app/src/memory.rs:42-87` 展示 backlog、最老等待、批次状态与 failure_reason，重试入口在 `crates/kanzei-app/ui/13-memory.js:47-86`；真实运行证据 T-1786922726169，关闭前 workspace 回归 T-1786922726213。②桌面与 CLI 共用 `crates/kanzei-app/src/memory.rs:298-306` 的 `consolidate_memory_inbox`，Tauri 注册在 `crates/kanzei-app/src/main.rs:197-200`，桌面定向测试 T-1786922726210，真实共享服务链路 T-1786922726169。③新 candidate/active 的空来源硬门禁由既有 `crates/kanzei-memory/src/memory/lifecycle.rs:24-87` 执行；控制面在 `crates/kanzei-app/src/memory.rs:55-62` 将 candidate/shadow/active 缺 source/refs 计为 promotion_gaps；生命周期回放 T-1786922726202。④recurrence→shadow→promote 由既有 lifecycle 写者产生状态事件，本次统一事件 payload 在 `crates/kanzei-memory/src/memory/mod.rs:38-77`，写者接线位于 `memory/inbox.rs:206-218`、`memory/store.rs:369-383`、`memory/lifecycle.rs`；控制面 UI 消费并展示状态，证据 T-1786922726202、T-1786922726211、T-1786922726212。⑤六臂 counterfactual 回放在 `crates/kanzei-core/src/replay.rs:300-318` 调用 `recompute_memory_effect`，`memory_eval_agg` 查询位于 `crates/kanzei-core/src/store/eval.rs:54-74`，action_changed/outcome_improved 分开统计位于 `crates/kanzei-core/src/store/telemetry.rs:167-204`，证据 T-1786922726207、T-1786922726209；控制面价值画像消费位于 `crates/kanzei-app/src/memory.rs:64-79`。⑥修复提交 `dcf6e11c` 已位于 dev；requirements、defects、tests archive 与代码同批提交，且 tracker/tests/代码三方一致；六条前端冒烟 T-1786922726212、app/core staged 定向回归 T-1786922726216/T-1786922726217、workspace 全量 T-1786922726213 均通过。既有能力已明确标注为沿用，本次交付为批4控制面投影、失败重试、聚合查询消费及配套验证。
- observed_head: dcf6e11c4a0557ad9283234084a431bf61f3e083
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1786996479550

## R-243 Surface Compaction 追加事务：原始事件不变、上下文由 surface 投影 [done]
- refs: R-242 R-236 D-209 docs/design/context_compaction.md docs/design/deepseek_harness_upgrade.md
- 依赖: 
- 内容: 将现有 compact_with_digest 的存储语义改为 compaction_started→compaction_summary→surface_replaced→compaction_ended 追加事务；模型上下文只消费 surface projection，原始 Session 事件不修改不删除；连续压缩走已交付滚动合并。
- 复杂度: 中
- 批次: 4/4
- 来源: DeepSeek Harness compaction 事件事务；复用已交付 R-236 的纪要模型、模板和质量闸。
- 标签: 核心
- 边界: 不重写 R-236 纪要算法、压缩模型配置和质量闸；不把 Memory 作为对话恢复源。Compaction 只在 R-242 正式 surface projection 上追加事务，失败保留原 surface，未完成事务在恢复时显式失效；不修改 format_version=1 的既有消息事实。
- 验收: ①压缩前后 raw event hash 不变；②边界上的 tool call/result 必须完整配对，否则拒绝压缩；③不完整 compaction transaction 重启后不生效且有可见诊断；④连续两次压缩 replay 一致，首段关键实体仍保留；⑤模型 surface 变短但 transcript/audit 仍能回看原文；⑥R-236 全部压缩回归保持通过。
- 优先级: P1
- 对账: 2026-08-18 对账更新：R-242 批次8/8 的 surface projection 已交付，当前依赖字段不再要求 R-242 关闭；本条正式承接 R-242 验收⑦，负责 compaction_started→compaction_summary→surface_replaced→compaction_ended 事务、停止新增 conversation.updated 以及失败恢复后的可见诊断。下一步先完成批1设计冻结与事件事务入口，再接全部 compaction 写者和回归。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-243
- 进展: 批次对账：Git 机械计数 4（`49c5832a` R-243 批3实现、`1c32e32c` R-243 批2实现、`5170dc8` 与 `878557f` 两个 R-242 依赖/验收边界提交标题引用 R-243），故手写批次为 4/4；R-243 实际功能批次为批2、批3。验收①原始事件 hash/事实不变：`crates/kanzei-core/src/store/events.rs:29-79,635-684`，T-1786922726352，原 payload/sequence 保留。验收②边界 tool call/result 必须完整配对：`crates/kanzei-core/src/runner/compaction.rs:152-156,235-281,805-828`，跨边界拒绝且原文不改，T-1786922726352。验收③不完整 compaction transaction 重启不生效且有可见诊断：`crates/kanzei-core/src/store/events.rs:85-119`、`crates/kanzei-app/src/conversation.rs:145-156,496-523`，T-1786922726352/T-1786922726353。验收④连续两次 compaction replay 一致且首段关键实体保留：`crates/kanzei-core/src/runner/compaction.rs:758-802`，T-1786922726352。验收⑤模型 surface 变短但 transcript/audit 可回看原文：`crates/kanzei-core/src/store/typed.rs:1254-1410,2146-2181`、真实 consumer `crates/kanzei-app/src/conversation.rs:63-96,116`，T-1786922726352/T-1786922726353。验收⑥R-236 全部压缩回归保持通过：T-1786922726352 core 16 passed、T-1786922726353 app 1 passed、T-1786922726354 workspace 1214 passed/1 ignored/0 failed；提交 `49c5832a`。
- observed_head: 49c5832a3ac31861a5e231bfe203b52612da50e3
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787020412718

## R-291 verify 聚合报告与步骤按耗时重排 [done]
- refs: D-510
- 内容: scripts/verify.ps1:20-28 Step-With-Timing 包 try/catch,失败累计继续跑,末尾统一报告全部失败;仅全绿才落盘证据(verify.ps1:77-83);按 dist/verification.json 实测耗时重排步骤,亚秒级 node 冒烟先跑、73.6s 的 cargo test 后置,先暴露廉价失败;12 步互不依赖续跑安全,预计改动约 15 行
- 复杂度: 小
- 来源: 2026-08-18 全库勘察;用户长期痛点一次只报一个失败(见记忆 verify-before-release)
- 标签: 流程
- 边界: 不改 12 步清单本身;守护测试(crates/kanzei-tools/src/git.rs:1896-1910 解析键集合)不受函数体重写影响;git.rs 侧聚合归 D-510
- 验收: 一次运行报出全部失败步骤;失败时不写 verification.json;步骤顺序按耗时优化;守护测试通过
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-291
- 进展: 验收逐条对账：①一次运行报出全部失败步骤：Step-With-Timing 在 scripts/verify.ps1:21-40 捕获并累计每步失败，scripts/verify.ps1:98-101 统一输出全部失败；隔离 node.cmd 故障注入实跑观察到 10 个失败项后仍继续到 ==> test。②失败时不写 verification.json：scripts/verify.ps1:98-102 在失败时先 throw，写证据仅从 scripts/verify.ps1:104-110 开始；故障注入前后 dist/verification.json SHA256 保持不变。③步骤顺序按耗时优化：scripts/verify.ps1:43-96 固定为廉价 UI/结构检查在前、fmt/ui_syntax/clippy/connectivity/runtime 在中、cargo test 最后；dist/verification.json:5-17 记录当前 HEAD 86fd4189 的实测顺序与耗时，test 64.7s 末位。④守护测试通过：T-1786922726365（当前源码指纹 gate_checklists_align_across_git_verify_and_ci，1 passed）与 T-1786922726366（正式 verify 全量门禁通过，workspace 全绿，证据绑定 commit）。既有 13 步清单与各检查实现属于既有能力，本次交付为失败聚合、失败不落证据和按实测耗时重排；实现提交 5169f393。
- observed_head: 86fd4189c3082f735b2ae602d28b1ac0739a1198
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787022996228

## R-294 记忆检索路线拍板:启用 embeddings 走三臂门禁或正式降级 hybrid 承诺 [done]
- refs: D-500 docs/design/memory_control_plane.md
- 内容: .kanzei/kanzei.toml 无 [embeddings]、index.db 无 memory_vectors 表、search_hybrid 恒走无 embedder 退化 lexical 分支(crates/kanzei-memory/src/memory/index.rs:428-431);replay_eval 六臂各仅 5 case 且 Candidate 臂自身退化(replay_eval.rs:99-163);RecallAction 七态只落地四种(memory/mod.rs:606-610)。按设计 §5 启用门禁跑三臂对比给量化结论,或在 memory_control_plane.md 正式降级 hybrid/PlanInject/StateAudit 承诺并收缩词表
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 后端
- 边界: 启用与否由数据说话;D-500(embed runtime 缺陷)是启用路线的前置
- 验收: 量化对比结论落文档;启用则配置+向量索引真实生效,放弃则文档与词表同步收缩;RecallAction 词表与实现一致
- 优先级: P2
- 进展: 验收逐条对账：①量化路线结论：docs/design/memory_control_plane.md:172-179（§5.1）记录生产 `[embeddings]` 启用项为 0、replay_eval.rs 仅 5 个 fixture 测试、无足够生产三臂样本，结论为暂不启用 production hybrid，不能把 FakeEmbedder 结果冒充线上收益。②放弃启用后的口径同步：同一文档:91-95 将 RecallAction 从 7 项收缩为 NoOp/Fingerprint/Lexical/ReRetrieve，并明确 Hybrid 仅离线/显式 opt-in、PlanInject/StateAudit 无真实消费方；运行时实际检索分支位于 crates/kanzei-memory/src/memory/mod.rs:640-653。③配置与向量行为证据：文档:175-177 精确引用 crates/kanzei-harness/src/config.rs:151-168 的 enabled 门禁及 crates/kanzei-memory/src/memory/index.rs:431-450 的无 embedder lexical 降级；现有 memory_vectors/FakeEmbedder/hybrid 实验能力保留但未伪称生产启用。④回归：T-1786922726374，`cargo test -p kanzei-memory` 148 passed、1 ignored，覆盖 embeddings 配置、向量重建/dense/hybrid、无 embedder lexical 降级和 5 个 replay fixture。实现提交 4beea898；既有 embedder/index/replay 代码属于复用能力，本次交付是基于真实数据的正式路线降级与词表同步。
- observed_head: 4beea898d1db74cdd9fb6aa520e0e62f34d75e8a
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787056247593

## R-292 mobile-pwa 入门禁并对齐桌面 UI 纪律 [done]
- 内容: crates/kanzei-app/mobile-pwa/app.js(325行)/sw.js(65行) 现无任何门禁:ui_syntax 只 glob ui/*.js(verify.ps1:45),ESLint 只盖 ui/*.js 与 scripts/*.mjs(eslint.config.js:14,74);app.js:260,268,269 用 alert()(桌面端已为此做 confirmDialog/inputDialog 且清零原生弹窗),全部文案硬编码中文零 i18n(app.js:16,55,84,146,161-162,182,216-256)
- 复杂度: 小
- 来源: 2026-08-18 全库勘察
- 标签: 前端
- 边界: 不重做 PWA 功能;代码级与桌面端重复度低(仅 escapeHtml 约 7 行),重点是设计纪律传导与门禁覆盖
- 验收: mobile-pwa 进 node --check 与 ESLint;无原生弹窗;文案接 i18n 通道;verify 通过
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-292
- 停车: 让位协作线 p16：其当前变更集合包含 crates/kanzei-app/mobile-pwa/app.js、sw.js、eslint.config.js、scripts/ui-lint-smoke.mjs、scripts/verify.ps1；待该线完成并清出这些共享文件后恢复，禁止覆盖并发实现。
- 进展: 验收收口：①mobile-pwa 进 node --check(verify.ps1 ui_syntax 扩 mobile-pwa/*.js,实测通过);②进 ESLint(eslint.config.js 新增 mobile-pwa 块含 sw 宿主全局,ui-lint-smoke.mjs lintFiles 扩展,实测 45 文件零错误);③无原生弹窗:3 处 alert 清零改 thread-msg/send-msg 内联提示,PWA 交互冒烟断言 alert 桩未触发;④文案接 i18n 通道:app.js 全部文案走 t()(I18N_EN 表,中文键=文案),sw.js 离线提示走 offlineText,PWA 冒烟断言中英 i18n;⑤verify 相关:node --check+六条前端冒烟全绿(T-1786922726370/6371),kanzei-app 210 passed(T-1786922726373),cargo test --workspace 全绿(R-295 时 T-1786922726367,未动 rust 源码)。提交 e875e628。临时冒烟脚本与静态服务器已删除。
- observed_head: e875e62827af89990b789402de903eaba9b9998f
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787056301772

## R-295 candidate 清退提速:出口策略与生产速率匹配 [done]
- refs: D-492 D-494
- 内容: reconcile_candidates 现仅两条出口:recurrence>=3+指纹+当轮 episode 晋升(crates/kanzei-memory/src/memory/mod.rs:972)或 age>=14 天清退(store.rs:552-601);96 条 08-17 候选要等 08-31 才清,期间持续挤占 FTS top-24 检索窗口(与 D-492 叠加)。新增清退策略:语义近似合并、低价值提前退、单日产出上限或等效手段
- 复杂度: 中
- 来源: 2026-08-18 全库勘察;2026-08-17 单日 96 条 candidate 实证
- 标签: 后端
- 边界: 不动晋升的 provenance 门禁;清退遵循归档不裸删 SOP
- 验收: 候选存量收敛至健康水位并可持续;策略有测试;现存 96 条按新策略处置;检索窗口占用可量化改善
- 优先级: P1
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-295
- 停车: 让位协作线 p16：其当前变更集合包含 crates/kanzei-memory/src/memory/mod.rs 与 store.rs，而 R-295 的候选清退策略、现存 96 条处置和检索窗口量化必须接入这些文件；待 p16 完成并清出后恢复，禁止覆盖并发实现。
- 进展: 验收收口：①真实存量处置 candidate 153→24(文件与FTS索引一致,129条deprecated入归档带墓碑,真实执行临时 example reconcile_r295,验收后已删);②策略有测试 reconcile_candidates_capacity_retires_low_value_first(容量24收敛/低价值优先/归档墓碑断言,T-1786922726360/6368/6369);③现存96条按新策略处置:真实执行 deprecated=129 含08-17存量与后续新增,archive 墓碑保留;④检索窗口占用可量化:bash 21/24→14/24、记忆 20/24→9/15、cargo 23/24→8/15(top-24 窗口 candidate 占用);⑤全量 cargo test --workspace 全绿(T-1786922726367:1214 passed 0 failed)。提交 9c5a89ea(B1 健康水位 CANDIDATE_MAX_COUNT=24+低价值优先清退+归档墓碑+测试) 150c6cdb(B2 untouched 语义修正:容量出口清退条目从 untouched 移除,测试断言收紧==24)。
- observed_head: e875e62827af89990b789402de903eaba9b9998f
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787056317878

## R-297 kanzei-llm auth token 刷新路径测试 [done]
- 内容: crates/kanzei-llm/src/auth/codex.rs(124行,含 token 过期判定+RFC3339 解析+刷新写回)0 测试,auth/mod.rs、event.rs 同为 0;全 crate 52 test/4043 行且几乎全是 SSE/JSON 形状断言
- 复杂度: 小
- 影响: token 刷新写错表现为莫名其妙掉线,难定位
- 来源: 2026-08-18 全库勘察
- 标签: 模型
- 验收: 过期判定/刷新/写回路径有单测;伪造时钟覆盖边界;cargo test -p kanzei-llm 通过
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-297
- 取得线: kanzei/thread-line-1787020530803-1
- 进展: 验收收口：①过期判定/刷新/写回路径有单测——is_stale 纯函数(过期判定提纯,时间源注入)与 apply_refresh_response 纯函数(刷新写回提纯)各加单测,共 3 个新测试(codex.rs auth::codex::tests);②伪造时钟覆盖边界——is_stale 测试传伪造 now(2026-02-01)覆盖恰好25天/超25天/24天/字段缺失/解析失败/带时区偏移六边界,apply_refresh_response 测试传伪造 now 验证 last_refresh 更新与空值不覆盖;③cargo test -p kanzei-llm 通过(55 passed,0 failed,T-1786922726380/6381)。提交 c3f9a6ff。生产行为不变:codex_headers 与 refresh 传 chrono::Utc::now(),纯函数提取逐字保留原逻辑。
- observed_head: c3f9a6ffdeff75a9a5461184388ed538d609915c
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787059512807

## R-300 大文件拆解第三轮与回涨闸门 [done]
- refs: docs/design/metrics_baseline.md docs/design/monolith_decomposition_round2.md
- 内容: Rust 侧:background.rs 2019 生产行居全仓 Top-1 却不在 R-257/R-202/R-204 任何拆解条目范围;drive.rs:930 execute_tool_calls 582 行及同文件 307/256/249 行函数群;profiles.rs:68/:605 同一 trait 方法两份实现(537+242 行);tracker/actions.rs 已回涨至 1356 行;kanzei/src/cli/run.rs:27 单函数 651 行且文件零测试。前端侧:08-compose.js 1535 行(拆解预算 941,超 60%),09-sessions/16-settings/11-docs-list/20-lines/07-events/06-activity 五文件回涨至 900+;06-agent-panel.js 是 06-activity.js 的逐行分叉复制(八对函数一一对应共用 bg-* CSS);index.html 1150 行 8 视图未拆。拆解后行数上限回涨闸门进 verify;完成后重跑 kz metrics 前后对照
- 复杂度: 大
- 来源: 2026-08-18 全库勘察;metrics_baseline.md 仅 08-16 单份基线从未重跑对照
- 标签: 核心
- 边界: 拆解不改行为;每批全冒烟+cargo test;闸门阈值宽松起步防误伤
- 验收: Top 目标拆解落地;06-agent-panel 与 06-activity 合流;回涨闸门在 verify 生效;metrics 对照落 metrics_baseline.md
- 优先级: P2
- 批次: 2/2
- 进展: 关闭对账（逐条覆盖验收）：① Top 目标拆解落地——Rust 拆分落点为 `crates/kanzei-tools/src/background.rs:20-34`（lifecycle/persistent/registration）、`crates/kanzei-core/src/runner/drive.rs:11-22`（question/task_results/permissions/serial_tools/parallel_tools/assembly/context_budget）、`crates/kanzei-tools/src/profiles.rs:15-20`（dev/readonly/research）、`crates/kanzei/src/cli/run.rs:20-22`（events/finalize/permissions）、`crates/kanzei-tools/src/tracker/actions.rs:11-18`（maintenance/action_helpers）；typed projection 见 `crates/kanzei-core/src/store/typed/projection.rs:10-80`，真实前后度量见 `docs/design/metrics_baseline.md:14-24,47-52`。② 06-agent-panel 与 06-activity 合流——`crates/kanzei-app/ui/index.html:1134` 真实加载 `06-activity.js`，未再加载 `06-agent-panel.js`；这是既有 B8 合流能力，本轮完成真实复核而非重复申报行为改造，`T-1786922726456` 证明 UI/PWA 语法与六项冒烟通过。08-compose 拆分后的续跑模型由 `crates/kanzei-app/ui/08-auto.js:1-35` 承接，`index.html:1136,1138` 加载 `08-auto.js`/`08-models.js`。③ 回涨闸门在 verify 生效——`scripts/verify.ps1:65-70` 的 `crate_sync` 真实调用 `scripts/metrics-regression-gate.ps1`；`docs/design/metrics_baseline.md:54-59` 记录阈值和对照；`T-1786922726461` 真实 verify 全部通过，含 metrics gate 30 rows、giants 5/5、允许回涨 100 行，并生成绑定 commit 的 `dist/verification.json`。④ metrics 对照落 `docs/design/metrics_baseline.md:10-59`，记录 230 个 Rust 文件、5 个巨石及 typed/drive 前后读数，来源命令见第 3 行，定向与全量证据为 `T-1786922726453`、`T-1786922726461`。边界“拆解不改行为”由全量 `cargo test --workspace`（`T-1786922726457`）及真实 verify（`T-1786922726461`）覆盖；批次已为 2/2。D-546 已 fixed，真实门禁证据绑定提交 `81e6800a12e6165fccf3bbca04e99d9269cba576`。
- observed_head: 81e6800a12e6165fccf3bbca04e99d9269cba576
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787079262652
- R-300: 2/4
- 取活依据: engine:唯一可执行 WIP 是 R-300，必须先恢复它

## R-298 发布链装后验证与证据补全 [done]
- refs: docs/design/ci_release_evidence_chain.md
- 内容: 打包链止于拷进 dist(scripts/package.ps1:130-136),setup.exe 从未被自动安装验证;install-setup.ps1 全仓零调用方(仅 release.ps1:67 报错文案提及);版本双源冻结 0.1.0 无比对(Cargo.toml:15 与 crates/kanzei-app/tauri.conf.json:4);release notes 无安装器 SHA256(package.ps1:148,设计列为后续可选 ci_release_evidence_chain.md:189);dist 堆 6 个无人引用 setup.exe 约 85MB 无保留策略;release.ps1 开发通道仅 cargo test(release.ps1:15-19),能把过不了 12 步中 10 步的二进制装进系统;install-setup.ps1:41-59 装前不备份装坏不还原
- 复杂度: 中
- 来源: 2026-08-18 全库勘察
- 标签: 发布
- 边界: NSIS 路线不重做;安装验证需考虑 LOCALAPPDATA 容器重定向问题(见记忆 localappdata-container-redirect)
- 验收: 打包后自动静默装+装后自校验入链;SHA256 入 notes;版本一致性检查;dist 保留策略;开发通道最低门禁明确并留档
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-298
- 取活释放: line=kanzei/thread-line-1787020530803-1;reason=parallel-line-unregister;at_ms=1787067046456
- 进展: 关闭对账（既有能力复核，不重复申报代码改造）：① 打包后自动静默装+装后自校验入链——真实调用方为 `scripts/package.ps1:160-171`，调用 `scripts/install-setup.ps1:1-61`；安装器脚本在 `:20-24` 拒绝运行中的 kzapp，在 `:41-59` 校验安装位存在、mtime/大小变化和 ExpectedHash。② SHA256 入 notes——`scripts/package.ps1:173-187` 计算 `Get-FileHash -Algorithm SHA256` 并写入发布 notes。③ 版本一致性检查——`scripts/package.ps1:91-101` 比对 `Cargo.toml:14-17` 与 `crates/kanzei-app/tauri.conf.json:3-5`，当前均为 0.1.0。④ dist 保留策略——`scripts/package.ps1:150-158` 删除旧 `kanzei-setup-*.exe`，只保留当前输出。⑤ 开发通道最低门禁——`scripts/release.ps1:5-7,20-24` 默认执行 `cargo test --workspace`，仅显式 `-SkipTests` 可跳过；发布通道另受 `package.ps1:103-114` commit-bound verify evidence gate 约束。既有实现提交 `99e77685`，真实发布调用方和脚本链已确认；`T-1786922726462` 当前 HEAD 契约断言通过，`T-1786922726461` 真实 verify 全量通过并产出绑定 `81e6800a` 的证据。边界 NSIS 路线未重做，容器重定向防线见 `scripts/release.ps1:52-74`。
- observed_head: 81e6800a12e6165fccf3bbca04e99d9269cba576
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787079402327
- 批次: 1/1

## R-301 泳道三级卡住判据 [done]
- refs: docs/design/parallel_lines_ui.md
- 内容: 按 docs/design/parallel_lines_ui.md:217,250 交付泳道状态机三态:跑着/疑似卡住/失败,禁止只按多久没动判死;判据结合事件流真实进展而非纯时间阈值
- 复杂度: 中
- 来源: 2026-08-18 全库勘察;parallel_lines_ui.md P3 设计验收文案已写好,承接条目 R-184 已归档无人认领
- 标签: 前端
- 验收: 按设计文档既有验收文案交付;三态转换有测试;真实长任务不被误判死
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-301
- 进展: 验收逐条对账：①「按设计文档既有验收文案交付」——`crates/kanzei-app/src/collaboration.rs:56-109,170-260` 以 `SessionRuntime.running` → 最近 `RunEvent` → worktree 变更文件 mtime 的顺序生成 status，并由 `ui/20-lines.js:258-278,335-339` 真实消费，未新增自动停止/处置；`src/state.rs:159-203,205-238` 与 `src/run/events/mod.rs:130-133,236-240` 提供事件和 outcome 真源。②「三态转换有测试」——`collaboration.rs:501-535` 覆盖 running、suspected_stuck、failed、completed、idle 及事件时间刷新；T-1786922726463 通过，kanzei-app 218 passed。③「真实长任务不被误判死」——`collaboration.rs:514-519` 反证事件长期空闲但 worktree mtime 新鲜仍为 running，且实现只显示 suspected_stuck、不触发处置；T-1786922726464 前端六项门禁通过，T-1786922726465 workspace 全量通过。前端可复核位置：`ui/02-i18n.js:13`、`ui/style.css:614-621`、`scripts/ui-lint-globals.json:343-344`；ui_dom 找到真实泳道节点，ui_console 无错误。
- observed_head: 81e6800a12e6165fccf3bbca04e99d9269cba576
- observed_worktree_hash: fnv1a64:a8ca96c02292909b
- recorded_at: 1787080157525
- 状态: done

## R-302 桌面 E2 路线立项:浏览器工具通道 vs Windows UI Automation 选型 [done]
- refs: R-101 D-511
- 内容: R-269 结论:浏览器工具通道可行,落地依赖 kzapp 暴露 URL 入口(requirements-archive.md:3445);R-101 技术路线:Windows 原生 UI Automation(requirements.md)。二选一给出对比依据与最小可行验证(真实桌面跑通一条 E2),选定后 R-101 的延期 E2 清单挂到该路线并重写其过期验收。选型结论:选 Windows 原生 UI Automation 作为真实桌面 E2；浏览器工具保留为 headless Edge 开发自检，不等同 kzapp 桌面；UIA 可附着实际安装位并通过生产 prompt ValuePattern 写入/回读与真实窗口截图，不依赖 CDP；代价是 WebView2 标题栏节点 provider 不稳定，已明确以顶层 Window/Win32 句柄为稳定边界。
- 复杂度: 中
- 来源: 2026-08-18 全库勘察;R-269 关闭说明⑦与 R-101 技术路线两条候选互不引用均无实施条目
- 标签: 流程
- 边界: 本条只做选型+最小验证,不交付全部 E2 清单(那是 R-101);CDP 不再是候选
- 验收: 选型结论有对比依据落档;最小 E2 真实桌面跑通;R-101 验收清单按新路线更新
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-302
- 进展: 验收逐条对账：①「选型结论有对比依据落档」——本字段记录浏览器工具与 Windows UIA 的边界、真实消费者、CDP 约束和 provider 风险；R-269 既有实现位置 `crates/kanzei-tools/src/browser_tool.rs`、`scripts/browser-helper.mjs`，真实桌面路线位置 `scripts/ui-desktop-uia.ps1:78-158`。②「最小 E2 真实桌面跑通」——T-1786922726466 原样命令 `pwsh -NoProfile -ExecutionPolicy Bypass -File .\scripts\ui-desktop-uia.ps1` 通过：真实安装位 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe`、PID 25652、窗口 `kanzei`/`Tauri Window`、生产 `prompt` Edit + `ValuePattern` marker 写入回读、真实截图 `.kanzei/research/r302-desktop-e2/kzapp-uia.png` 454737 bytes；T-1786922726467 语法与唯一 Responding 进程收尾通过。③「R-101 验收清单按新路线更新」——`requirements.md` R-101 已更新技术路线、完整权限弹窗/pending ask/切项目复位/手写内容保留/run_task 收尾/停止/长会话范围及后续批次边界，R-302 只核销选型+最小 E2。关闭前中复杂度全量门禁 T-1786922726468：`cargo test --workspace` 通过。既有能力明确：R-269 headless 浏览器工具不是本次桌面 E2 交付；本次新增 `scripts/ui-desktop-uia.ps1` 为真实桌面 UIA 最小验证。
- observed_head: 676fddefe6d82f7442035b81b8d65efe0b71ccfa
- observed_worktree_hash: fnv1a64:4dc73574e4527fea
- recorded_at: 1787080933030
- 状态: done

## R-303 文档一致性批次订正 [done]
- refs: R-283 R-264
- 内容: ①README.md:65,107,149 仍把已退役的 goals 线列为事实源(R-252 B5 已删 goals.md 改指 IDEAS),:57-68 漏报 research 工作台/LaTeX 绘图/移动端 PWA+LAN 桥/浏览器工具/ui_screenshot/按线设置(R-269~R-277/R-290)等已交付能力;②docs/使用手册.md:38-45 漏「想法」收件箱整块能力,:67-74 快捷键表缺 Ctrl/Cmd+P 命令面板(ui/21-palette.js 已实现);③memory 三份设计文档落后实现约 8 个需求:memory_control_plane.md 需求清单停在 R-167(R-194/R-195/R-213/R-216/R-233/R-255/R-286 与 D-366/D-368 均未进文档),memory_system.md 状态枚举仍 active|stale(实际五态)且工具集缺 memory_promote 等,memory_decision_sufficiency.md 实施边界仍指旧路径(R-203 已迁);④docs/design/ui_esm_migration.md:3 状态过期(B1/B2 已完成,规模已从 21 文件/12389 行涨到 24 文件/15528 行,typeof 守卫 6 处涨到 44 处);⑤docs/design/phase2_system_upgrade.md:301-322 Wave 0/1 门禁记录引用的 R-221/R-277/R-286/D-428 状态全部过期,R-283 验收②依赖该记录
- 复杂度: 小
- 来源: 2026-08-18 全库勘察
- 标签: 流程
- 验收: 五组文档与现实对齐;R-283 验收②的 Wave 记录恢复可用;订正一次批次收口
- 优先级: P3
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-303 [tracker integrity degraded] D-555: invalid defect lifecycle [done]
- 进展: 已完成并验证：①README.md:57-71 补齐 research 工作台、LaTeX/绘图、PWA+LAN、浏览器/UI 实查、按线模型设置、想法等现有能力；README.md:110、149 同步更新事实源描述。②docs/使用手册.md:38-46 补齐 ideas.md 收件箱和人工拆解，:68-76 补 Ctrl/Cmd+P；真实消费者为 crates/kanzei-app/ui/11-docs-list.js:817-848、15-views-misc.js:85-103、21-palette.js:218-232。③memory_control_plane.md:3-14、208，memory_system.md:3-12、43、66-74、98、125，memory_decision_sufficiency.md:82-90 对齐 R-194/R-195/R-213/R-216/R-233/R-255/R-286、D-366/D-368、五态、memory_promote 和 crates/kanzei-memory 当前落位。④ui_esm_migration.md:3-4、8、27-29、62、89、106-108 对齐 B1/B2、24 文件/15528 行/44 处守卫和 B3 边界。⑤phase2_system_upgrade.md:301-330 将 Wave 0/1 按 R-221/R-277/R-286/D-428 当前状态恢复为 Go，并保留后续 Wave 的真实 No-Go。R-303 验收三项均由 T-1786922726479 通过。
- observed_head: 4de7f1016c097b6171ef930d84159668d28ff578
- observed_worktree_hash: fnv1a64:8b70995bc9c7bd76
- recorded_at: 1787162163267
- 验收核验: ①五组文档与现实对齐：README.md:57-71,110,149；docs/使用手册.md:38-76；docs/design/memory_control_plane.md:3-14,208；memory_system.md:3-12,43,66-74,98,125；memory_decision_sufficiency.md:82-90；ui_esm_migration.md:3-4,8,27-29,62,89,106-108；phase2_system_upgrade.md:301-330。②R-283 验收②的 Wave 记录恢复可用：phase2_system_upgrade.md:301-330 明确 Wave 0/1 Go，并列出现行实现与测试证据。③订正一次批次收口：T-1786922726479 命令级校验通过，目标文档 diff --check 通过。

## R-304 dev 勘察工件固定落点 [done]
- refs: R-248
- 内容: dev 侧勘察产物(调研笔记/证据/对照)目前无固定目录与生命周期约定;定义落点(如 .kanzei/research/ 或专用目录)、命名、与条目 refs 的关联方式及清理策略
- 复杂度: 小
- 来源: 2026-08-18 全库勘察;research_mode.md:27 与 R-221 关闭说明(requirements-archive.md:3652)两处明写需另立条目承接,全库无对应条目
- 标签: 流程
- 边界: 与 R-248 prior-art 门禁互补不重复:R-248 管开工前对照,本条管勘察产物落盘可回溯
- 验收: 落点约定落档并有工具/文档支持;勘察产物可按条目回溯;R-248 恢复时可直接复用该约定
- 优先级: P3
- 进展: 已完成：`docs/design/research_mode.md:55-65` 固定 dev 勘察落点 `.kanzei/research/<entry-id>-<slug>/`、最终 `report.md`、entry_refs/进展回溯、V0-V3 证据、既有 write/edit/insert 与 read/glob/grep/files 工具复用、active→archived 清理策略，并明确 R-248 可复用而不替代其用户阻塞。`README.md:110` 增加用户可发现入口；示例工件 `.kanzei/research/r304-dev-recon/report.md:1-30` 已按约定落盘并回指 R-304。
- observed_head: f74190424c0bbf129c107776e8b3d52b4b908b61
- observed_worktree_hash: fnv1a64:22647b63185a2bb9
- recorded_at: 1787162500319
- 取活依据: engine:唯一可执行 WIP 是 R-304，必须先恢复它
- 停车: 
- 状态: done
- 验收核验: ①落点约定落档并有工具/文档支持：`docs/design/research_mode.md:55-65`；`README.md:110`；既有工具消费者为 `crates/kanzei-tools/src/profiles/dev.rs:102-157` 的项目资产权限边界与 dev 侧 write/edit/insert、read/glob/grep/files 通道（既有能力，本次仅固化约定）。②勘察产物可按条目回溯：`docs/design/research_mode.md:61` 规定 tracker refs 只写 R-/D-/T-、进展写报告路径、报告头写 entry_refs；`.kanzei/research/r304-dev-recon/report.md:1-8` 实例化 `entry_refs: R-304`，T-1786922726481 通过。③R-248 恢复时可直接复用：`docs/design/research_mode.md:65` 与 `.kanzei/research/r304-dev-recon/report.md:28-30` 复用根目录、命名、report.md 和 active→archived 生命周期，同时保留 R-248 原有 refs API/topic 来源用户阻塞；T-1786922726481 通过。

## R-305 subagent 策略层与 Agent 目录 [done]
- refs: R-281 D-513
- 内容: 按 docs/design/subagent_management.md:36-84 交付三块:①Agent 目录(可用 agent 类型的注册与描述);②策略面板(每轮 task 上限/并发/超时/重试/预算可配置并生效);③运行审计摘要(每轮 subagent 派发与结果的可读汇总)。与 R-281 子代理阅读器互补:R-281 管看单个子代理说话,本条管策略与全局审计
- 复杂度: 大
- 来源: 2026-08-18 全库勘察;2026-08-18 用户拍板需要;docs/design/subagent_management.md P2 三块未实施,唯一演进条目 R-117 已 dropped
- 标签: 核心
- 边界: 不重做 R-281 的对话读取器;策略默认值保持现行为,配置生效有真实验证
- 验收: 按设计文档三块交付;策略修改真实生效有测试;审计摘要在 UI 可见;roster_cap 类静默截断(D-513)在策略面板可见化
- 优先级: P2
- 取活依据: engine:唯一可执行 WIP 是 R-305，必须先恢复它
- 取活释放: line=kanzei/thread-line-1787120622542-1;reason=parallel-line-unregister;at_ms=1787159520466
- 进展: B3 已落地并验证：后端 `crates/kanzei-app/src/run/events/mod.rs:365-377` 将 PermissionResolved 真实投影为 `kz:permission-resolved`；事件路由位于 `crates/kanzei-app/ui/01-core.js:31-49`，运行摘要聚合与渲染位于 `crates/kanzei-app/ui/06-activity.js:920-1120`，真实派发/结果/usage/权限/终态消费者位于 `crates/kanzei-app/ui/07-events.js:11,20,185,216,230,290,305,364,421,625`；UI 卡片和运行轨迹入口位于 `crates/kanzei-app/ui/index.html:1040-1048`、`crates/kanzei-app/ui/06-activity.js:1470-1476`，样式位于 `crates/kanzei-app/ui/style.css:1212-1231`。B1 Agent 目录与 B2 策略强制保持既有交付：目录 `agent_directory.rs:43-223`/settings consumer `ui/16-settings.js:301-386`，策略读取 `settings.rs:560-590`、runner 强制 `drive.rs:494-511`/子代理入口 `subagents.rs:107-118,278-289,705-716`；roster_cap 可见化由 `settings.rs:17-20,?`、`ui/16-settings.js:279-299` 实现。验收逐条核验：①设计三块均有真实消费者：Agent 目录设置页、limits runner/子代理入口、审计卡片与 trace 按钮；②策略修改真实生效：T-1786922726489 通过 fmt、kanzei-llm 55/55、kanzei-core 220/220、kanzei-app 221/221；③审计摘要 UI 可见并消费真实派发/结果/权限事件：T-1786922726490 通过 ui-runtime（25 scripts、2339 invoke、0 runtime errors）及其余五项前端门禁，T-1786922726491 通过 kanzei-app 221/221；④roster_cap 不再静默截断：`settings.rs` 返回 phaseRosterCapacity、`ui/16-settings.js:279-299` 显示截断提示，T-1786922726482 与 T-1786922726486/T-1786922726487 已验证。实际当前安装窗口为旧构建，未将其冒充新 UI E2 证据；生产脚本运行时门禁和 Rust 定向测试为本交付证据。 [terminal-fix 2026-08-19] done → done: 修正已归档 R-305 终态进展中的错误证据占位符 `settings.rs:17-20,?`，替换为已核实的 `settings.rs:13,571`；不改变终态或验收结论。
- observed_head: 1adb22a1e695aee9d9b8897c2946238100ea2a4c
- observed_worktree_hash: fnv1a64:a66ca3ff5d267841
- recorded_at: 1787166156844
- 批次: 3/3
- 批次说明: B1 Agent 目录与配置模型/只读入口；B2 策略读取与 runner 强制；B3 运行审计摘要 UI、端到端验证与收口。
- 验收核验: ①按设计三块交付：Agent 目录 `crates/kanzei-app/src/agent_directory.rs:43-223` + `ui/16-settings.js:301-386`；策略 `crates/kanzei-app/src/settings.rs:560-590` + `crates/kanzei-core/src/runner/drive.rs:494-511` + `crates/kanzei-app/src/subagents.rs:107-118,278-289,705-716`；审计 `crates/kanzei-app/ui/06-activity.js:920-1120` + `ui/index.html:1040-1048`，真实调用方为 `ui/07-events.js:11,20,185,216,230,290,305,364,421,625` 与 trace 按钮 `ui/06-activity.js:1470-1476`。②策略真实生效：T-1786922726489、T-1786922726491。③审计摘要 UI 可见并有真实事件源：T-1786922726490、T-1786922726491。④roster_cap 策略面板可见化：`ui/16-settings.js:279-299`、T-1786922726482、T-1786922726486、T-1786922726487。

## R-235 存量 28 条零证据 active 记忆逐条复核:保留(存量豁免)或降级 candidate,用户拍板 [done]
- 优先级: P3
- 内容: 对 28 条零证据 active 记忆逐条复核:保留(存量豁免,接受不可计量)或降级 candidate(严格符合无来源不入 active,代价是不可检索注入)。复核结果与依据落到 memory 系统设计文档或本条目关闭证据。
- 复杂度: 小
- 来源: R-213 关闭时盘点发现(R-213 验收③处置的承接)
- 标签: 后端
- 背景: R-213 盘点:state.db 311 条 episode、memory_sources 0 行,project 域 28 条 active 记忆(M-001~M-063)全部零证据(global 域无条目)。这些是 provenance 门禁上线前由用户/交互会话/manager 产生的既有资产,source 字段均无机器可链接的 run_id,历史回填=变相伪造,不可行。R-213 的处置定为存量豁免+文档化,但控制平面「用数据判断记忆是否改善决策」对这些条目无法计量,保留还是逐条降级应由用户拍板。
- 验收: ①28 条清单逐条给出保留/降级结论与依据;②结论落地(设计文档或关闭证据);③如选择降级,操作后搜索不再命中 candidate 条目。
- 进展: 2026-08-20 用户拍板:28 条零证据 active 记忆全部保留(存量豁免),接受不可计量。验收逐条:① 由用户执行——2026-08-20 用户整批拍板,每条结论=保留,依据=provenance 门禁上线前既有资产,历史回填=变相伪造不可行(R-213 处置口径);② 结论已落地 docs/design/memory_system.md:128-134(存量零证据 active 记忆处置节,含结论/代价/豁免边界);③ 验收降级: 原文「如选择降级,操作后搜索不再命中 candidate 条目」→实际未选择降级,无降级操作可验,条款不适用
- observed_head: a8e75106b629441cc19963dd5667aee07a74339a
- observed_worktree_hash: fnv1a64:fe5544b037ebfb8f
- recorded_at: 1787168165447

## R-242 会话投影真源切换与分段清空恢复 [done]
- refs: D-209 D-342 D-417 R-236 R-279 docs/design/deepseek_harness_upgrade.md
- 依赖: 
- 内容: 在 shadow gate 通过后，将 conversation_get/list、runner prior、子代理 transcript 和 UI 历史恢复逐项切到事件投影；进程内 Vec<Message> 仅作缓存。清空对话追加 conversation.reset 并开启新 segment，新 segment 的模型 prior 为空，旧 segment 仍可审计。验证期保留 legacy snapshot 只读对照，五条读路径全部稳定后停止新增 conversation.updated。
- 复杂度: 大
- 批次: 8/8
- 来源: 2026-08-14 DeepSeek Harness 升级方案；用户确认清空保留、删除确定性物理清除并弹窗提示风险。
- 标签: 核心
- 边界: 本需求只负责事件投影真源切换与 segment reset，不实现会话物理删除、Spill artifact 联动删除、WAL/VACUUM 或迁移备份安全整理；这些统一由 R-245 的删除计划与显式整理入口承担。第一批不改事件 format_version 与 SessionFact 公共词表；任一读路径可通过 feature gate 独立回退 legacy snapshot。
- 迁移与回滚: 不新增表、列或索引时不创建空 migration。切换按五条读路径分别启用 feature gate，legacy snapshot 在观察期只读保留；任一路径出现未知差异即回退该路径。全部 gate 稳定后才停止新增 conversation.updated，既有 snapshot 不删除。
- 验收: ①五条读路径从同一事件日志恢复一致消息；②user/assistant/tool 各安全边界强杀后重启无已发生事实丢失；③孤立 tool call 投影为 interrupted 且不自动重放；④conversation.reset 后新 segment prior 为空但旧 segment 可审计，重复 reset 幂等；⑤至少30个真实 shadow turn 达标，typed_write_errors=0、正常可比较 turn 全部 equal=true、未知差异为0；⑥五条 feature gate 可独立回滚，回滚后 legacy 行为与切换前一致；⑦对照稳定后停止新增 conversation.updated，既有 snapshot 仍可只读回放。
- 优先级: P1
- 进展: R-242 已完成并提交 `3b8b0e7b`（R-242 B9）。逐条验收证据：①五条读路径统一事件投影：桌面 `crates/kanzei-app/src/conversation.rs:63-96,195-241`、CLI `crates/kanzei/src/cli/run.rs:47-117`、桌面 runner prior `crates/kanzei-app/src/run/coordinator.rs:149-157`、UI history `crates/kanzei-app/src/processes/workspace.rs:509-517`、子代理 transcript `crates/kanzei-app/src/run/coordinator.rs:168-193`；②user/assistant/tool 强杀恢复由 `crates/kanzei-core/src/store/typed.rs:756-791,1159-1189` 与 T-1786922726244/T-1786922726245 覆盖；③孤立 tool call 投影为 interrupted 且不重放由 `crates/kanzei-core/src/store/typed.rs:756-791`、T-1786922726244 覆盖；④`conversation.reset` 后新 segment prior 为空、旧事实可审计、重复 reset 幂等由 `crates/kanzei-app/src/conversation.rs:23-96` 与 T-1786922726244/T-1786922726245 覆盖；⑤30 个真实 shadow turn equal=30、expected=0、unknown=0、typed_write_errors=0 由 T-1786922726248 覆盖，当前回归 T-1786922726501；⑥五 gate 独立回滚由 `crates/kanzei-app/src/projection_gate.rs:19-46,68-82` 与 T-1786922726501 覆盖；⑦对照稳定后停止新增 `conversation.updated`：桌面 `crates/kanzei-app/src/run/persistence.rs:450-482`、CLI 不再新增且 compaction 改写走事务 `crates/kanzei/src/cli/run/finalize.rs:29-68,90-101`、CLI prior 在当前 user fact 前恢复 `crates/kanzei/src/cli/run.rs:285-302`、mobile 写 typed fact `crates/kanzei-app/src/mobile.rs:324-389`，legacy snapshot 仍只读 `crates/kanzei-app/src/conversation.rs:286-333,390-450`；T-1786922726501：220+222+40+32 全绿。
- observed_head: 3b8b0e7bc6801248f91ca8d30196a4d966825e9d
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787176334013
- 取活依据: engine:唯一可执行 WIP 是 R-242，必须先恢复它
- 对账: 2026-08-20 合并窗口解除:R-306 B1/B2 收编完成且 workspace 全绿,共享文件不再冻结;恢复复核验收⑦并关闭。恢复人:agent(循环)
- 停车: 

## R-306 并行线交付收编:R-257/p13 线已关条目提交未合入 dev,冲突随演进扩大 [done]
- refs: R-257 D-396 D-397 D-398 D-399 D-400 D-401 D-409 R-293 R-299 R-283
- 内容: 两条并行线的已归档交付只存在于分支:①thread-line-1786805363432-1(R-257 B2~B5,6 提交,head aa27e11b)——drive.rs/docstore.rs/git.rs/config.rs 按域拆分,dev 上四文件仍为巨石形态;②thread-line-1786851588846-1(p13,8 提交,head b4245f6c)——D-396~D-401/D-409 修复(跨树围栏三态快照、mtime 粗筛、写日志接线与回滚、浏览器工具错误通道、验收降级记录),dev 侧仅 inbox 分批经 D-480/R-286 独立演进,其余修复缺失。kz worktree merge-preview 实测冲突:R-257 线 6 文件(drive.rs/runner mod.rs/tool_exec.rs/docstore.rs/git.rs/ui-lint-globals.json);p13 线 9 文件(Cargo.lock/app memory.rs/inbox.rs/cross_tree.rs/plot_tool.rs/profiles.rs/cli memory.rs/ui-connectivity 两脚本)。拆批:B1 p13 线合并(缺陷修复优先,dev 独立演进文件以 dev 语义为准逐块对账);B2 R-257 线合并(零 API 面变更为验收);B3 三线脏 WIP 处置(p13 typed.rs +85 行、p16 git.rs +5/ci.yml +1、R-257 线 gen/schemas)与 worktree 清理;B4 防复发闸门:条目关闭时 observed_head 不在 dev 祖先链即拒绝关闭或强制登记收编任务
- 复杂度: 大
- 影响: dev 持续在冲突文件上推进(git.rs 的 D-553、memory 的 R-286/D-480、typed.rs 的 D-486),冲突面逐日扩大;R-293/R-299 因文件被分支占用而停车;D-396~D-401 修的越界围栏、写日志洞、浏览器工具假成功在 dev 运行态实际未修
- 来源: 2026-08-20 主会话状态对账:已归档条目(R-257 done 6/6、D-396~D-401/D-409 fixed)的交付代码不在 dev,tracker 与实现相互矛盾,违反 R-283 验收④
- 标签: 流程
- 边界: 以 dev 为主干语义:同名功能 dev 已独立演进的以 dev 实现为准,分支侧只补 dev 缺失的修复点;不借机重构;R-257 拆分若冲突过大允许按文件降批合并并如实记录未收编残余
- 验收: ①两线全部提交在 dev 祖先链(git merge-base --is-ancestor);②冲突解决后 cargo test --workspace 全绿且 verify 通过;③D-396~D-401/D-409/R-257 交付点在 dev 主树逐条抽查可见(cross_tree 三态 FileImage、record_write_log、浏览器 rpc 嵌套 error 透传、四文件拆分);④三处脏 WIP 逐一处置留痕;⑤防复发闸门有实测:未合并分支条目关闭被拒或强制登记收编
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 R-306，必须先恢复它
- 批次: 4/4
- 设计冻结: 不变式：dev 以当前语义为准，分支只补缺失交付；不得用快进假装完成非快进收编。｜权威数据源：当前 dev、两条 worktree 分支的真实提交图与 merge-tree --write-tree 冲突结果。｜预期变更文件：先仅更新 R-306 进展/批次字段；代码文件待 B1 冲突逐块对账后按实际落地集合确定。｜最小测试：每批按实际改动运行对应 crate 定向测试；R-306 关闭前按复杂度“大”执行 workspace 全量测试及 scripts/verify.ps1。
- 进展: B2/B4 收尾(主会话):①R-257 线以非快进 ours 合并 7abaea4a 收编,aa27e11b 实测进入 dev 祖先链(git merge-base --is-ancestor 实测;逐文件对账判定六冲突文件保留 dev、唯一缺口 git 四域已由 ce4edf3f~63f24c83 迁移取代,合并只记录祖先链,符合边界的降批如实记录);p13 头 b4245f6c 实测已在祖先链——验收①两线满足。②verify 十三步全绿,dist/verification.json 绑定提交 809b7821(test 步 89.2s 含 workspace 全量)——验收②满足;首轮 verify 揪出并修复 D-584 测试 PATH 竞态(提交 809b7821)。③dev 主树逐条抽查锚点:跨树三态 FileImage 在 crates/kanzei-tools/src/cross_tree.rs:64-95,208-230,468-492(回归测试 cross_tree.rs:1088-1092);record_write_log 在 crates/kanzei-tools/src/lib.rs:168 与 crates/kanzei-memory/src/memory/store.rs:716-763、crates/kanzei-tools/src/test_record.rs:236-242、crates/kanzei-tools/src/conventions.rs:215、crates/kanzei-tools/src/tracker.rs:485-492;浏览器嵌套 result.error 透传在 crates/kanzei-tools/src/browser_tool.rs:144-186,359-367(测试 rpc_嵌套result_error透传为工具错误);四域拆分 crates/kanzei-core/src/runner/drive/*.rs、crates/kanzei-memory/src/docstore/*.rs、crates/kanzei-tools/src/git/*.rs、crates/kanzei-harness/src/config/*.rs(提交链 ce4edf3f/6fb5f50d/7da154a1/bb829270/63f24c83)——验收③满足。④实测清理留痕:r257-source2 树干净并移除,本地/远端 thread-line-1786805363432-1 分支已删,p13/p16 树与分支已不存在,kz lock status 活跃线为空,脏 WIP 无残留——验收④满足。⑤防复发闸门实现 crates/kanzei-tools/src/tracker/actions/action_helpers.rs(check_close_source_ancestry)+拒绝实测 T-1786922726508,提交 fca4f204——验收⑤满足。workspace 全量另有 T-1786922726509;发版:main ff 至 809b7821 已推送,package -Ack 27 -Publish 进行中
- observed_head: 809b7821ff906bacdb55e1aaafd7ca9dfafaba31
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787183803865
- 对账: 2026-08-20 勘察修正:①p13 线实际未合并 16 提交(含 R-275 调色板批1~3、D-390/D-391/D-393/D-394 一串),条目内容原写 8 提交低估一倍,B1 工作量按 16 估;②p16 线(1787020530803-1)已经 merge commit 27b3e8d1 合入 dev,但树/本地分支/2 脏文件(ci.yml、git.rs)未清,B3 可先零风险清理;③冲突面持续扩大:线冻结在 08-16 后 dev 又改 drive.rs 14 次、memory/store.rs 10 次、git.rs 8 次;④两条欠账线均未走 parallel-line-unregister 释放流程,B4 闸门应含收线释放;⑤陈旧远端分支 kanzei/release-68db58e 已被 dev 完全包含,可顺带清理。2026-08-20 主会话复核:停车「排队:R-242 收口后恢复 B4」已过时——B4 闸门与拒绝测试已于 fca4f204 落地,停车解除;lock status 实测活跃线为空,④的 unregister 已无欠账,剩余清理只有 r257-source2 树与本地/远端分支;p13 头 b4245f6c 实测已在 dev 祖先链(验收①的 p13 半边已满足)
- 执行者: 主会话(SOL)。用户 2026-08-20 指令:结构性问题不再交自举,由主会话全面修复

## R-293 记忆价值闭环点亮:反事实评估与 outcome 漏斗产出真实数据 [done]
- refs: D-507 docs/design/memory_control_plane.md
- 内容: 生产实况:memory_eval_agg 0 行(写入方 upsert_memory_effect 与消费链 kanzei-core/src/store/eval.rs:54-90、控制面 UI 全通但无人触发)、arm=outcome_improved 0 行无任何写入方(telemetry.rs:174-183 恒 N/A)、deprecate_candidates 依赖 effect_mean<=0 永远返回空集(eval.rs:76,338)、memory_eval 1670 行里 1640 行是在线 action_changed 对账挤占离线回放语义。接通 outcome 写入方与聚合调度,让 F(m) 漏斗四段真实产数
- 复杂度: 大
- 来源: 2026-08-18 全库勘察;memory_control_plane.md 立身之本「用数据判断记忆是否改善决策」当前无数据
- 标签: 后端
- 边界: 不改回放台六臂框架;先点亮既有链路再谈扩展
- 验收: 生产漏斗 RETRIEVED/INJECTED/ACTION_CHANGED/OUTCOME_IMPROVED 四段有真实数据;控制面 F(m) 栏显示非空;deprecate 判定可被真实数据触发;回归测试
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 R-293，必须先恢复它
- 停车: 
- 进展: 批次1（既有本条交付）已点亮在线 outcome 写入：crates/kanzei-core/src/runner/drive.rs:200-424 的 RecallWatch 全部结局路径提交 RecallRunOutcome；crates/kanzei-core/src/runner/recall.rs:61-166 提供 finish/outcome_improved；crates/kanzei-memory/src/memory/mod.rs:714-775 仅 Completed 写 outcome_improved，Halted/Unknown 不写。批次2（本次交付）已提交 a0bb285e：crates/kanzei-core/src/replay.rs:196-203 将真实命中 memory_id 作为 F(m) 主键来源，:278-364 仅按真实 ID 落 memory_eval 并逐 ID recompute；crates/kanzei-memory/src/replay_eval.rs:193-205 从 Current 检索返回真实首个命中 ID。验收逐条对照：①生产漏斗 RETRIEVED/INJECTED/ACTION_CHANGED/OUTCOME_IMPROVED：既有 recall_events 真实 retrieved/injected 计数与本次独立 outcome 写入由 crates/kanzei-core/src/store/telemetry.rs:175-214 统一计数，action_changed 与 outcome_improved 不互相推导；②控制面 F(m) 非空：crates/kanzei-core/src/store/eval.rs:54-92 读取真实 memory_eval_agg，crates/kanzei-app/src/memory.rs:88-116 暴露 effects，既有消费者 crates/kanzei-app/ui/13-memory.js:15,37-79 调用并展示非空价值画像；本次回放真实 ID 回归 crates/kanzei-core/src/replay.rs:619-626 断言 M-real 有 eval_n=1 且 case_id 不产生聚合；③deprecate 可被真实数据触发：crates/kanzei-core/src/store/eval.rs:322-349 按 effect_mean<=0、eval_n、effect_ci 从真实 memory_eval_agg 筛选，回归 crates/kanzei-core/src/store/eval.rs:608-653 覆盖低价值/高置信度候选；④回归测试：T-1786922726510、T-1786922726512、T-1786922726514、T-1786922726515、T-1786922726516、T-1786922726517、T-1786922726520 均通过；本仓库 cadence 明确全 workspace 仅发版执行，未重复运行无关全量套件。既有能力已明确标注，批次2新增真实 memory_id 配对与聚合调度。
- observed_head: a0bb285e0c845cc70035687f0c151f51d049bc5f
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787191034284
- 批次: 2/2

## R-317 长程需求执行底座:Outcome/Work Unit/事件投影/有界上下文与证据关闭 [done]
- 优先级: P0
- 复杂度: 大
- 标签: 核心 后端 前端 流程
- refs: R-312 R-313 R-307 docs/design/work_unit_foundation.md
- 来源: 2026-08-20 用户指出「需求这个东西的定义有问题」，要求重新思考任务拆分、上下文管理、减轻模型负担和长程任务信息熵线性增长；随后明确「先落一个底座，这个工作不可能靠自举完成，帮我完成这个工作然后发版」。
- 诊断: 旧 Requirement 同时承担 Outcome、WIP、批次、自由文本进展、上下文快照、验收与审计，历史每增长一段，模型恢复任务就要重新消费一段；模型还是状态维护者和验收者，形成负担随历史线性增长的结构性耦合。R-312 原计划先测量再设计，但用户本轮已给出方向并要求直接落底座，因此本条由外部 Codex 在隔离工作树实现，不要求旧系统自举拆分自己。
- 内容: ①schema v18 增加 append-only work_events 与可重建 work_surfaces；②Requirement 作为长期 Outcome，显式 `执行模型: work_units_v1` 后拆为 R-xxx/Wn Work Unit；③状态机覆盖 ready/active/blocked/verifying/done/superseded、并行线接管与 checkpoint；④work next 调度 Work Unit 并只注入当前投影+父 Outcome 白名单字段；⑤逐验收 evidence 后才允许 complete，父需求关闭要求所有单元终态且至少一个 done；⑥CLI、桌面快照与需求详情可观测；⑦存量需求保持 legacy 行为。
- 边界: 不自动让旧系统规划和拆分本条自身；不批量迁移存量 Requirement；不把 Markdown Outcome 搬进 SQLite；v1 不加入含混的 defect affinity，先保证需求主链闭环。
- 验收: ①v17→v18 迁移前整库备份且表/列机械判据通过；②事件回放与 surface 相同，删除/重建投影不丢审计；③单元上下文有硬预算，父需求超长进展不进入 selected；④依赖、单 WIP、并行线接管与 checkpoint 状态机有测试；⑤未逐条登记 evidence 时 complete 被拒；⑥有非终态单元或全部 superseded 时 req close 被拒，全部验证完成后可关；⑦CLI 编译，桌面快照、i18n/runtime/lint 冒烟通过；⑧全量 verify 通过并发布安装包，GitHub release/tag/资产 SHA256 与安装后二进制版本均核验。
- 取活依据: user-direct:用户本轮明确要求由外部 Codex 完成底座并发版，避免依赖旧需求系统自举
- 取得线: kanzei/work-unit-foundation
- 批次: 4/4
- 进展: B1 ba13b53a、B2 c5217523+b0b96c97、B3 db1e808e、接管门禁 d382bbfc、发布门禁修复 0f782868/f4533f37/d3873bf1 已完成。最终 `scripts/verify.ps1` 在 d3873bf18c47cd90316b464dc835af3acdc7d085 生成全绿证据（13 项，verification SHA256 f9aad2d678d778ce0d2303057bf0f329b5343c05079a19a1306a69b34b135df3）；dev/main 快进到同一提交。`package.ps1 -Ack 12 -Publish` 构建、静默安装并发布 GitHub Latest Release `build-d3873bf1`；标签精确指向 d3873bf18c47cd90316b464dc835af3acdc7d085。安装器 `kanzei-setup-d3873bf1.exe` 大小 16,339,218 字节，本地产物、GitHub API digest 与重新下载资产三方 SHA256 均为 c250adc7051f326e6c53aa40e27afc138f71a6775e2ad63b1758519aac9950be；本机安装位 `kzapp.exe` 包含 d3873bf1 构建标识，ProductVersion/FileVersion 均为 0.1.0。D-594/D-595 作为全量门禁中发现的真实竞态与环境隔离缺陷已修复归档。

## R-296 Tauri command 与 run 链路测试基座 [done]
- 内容: kanzei-app 无 tests/ 目录(全仓唯一集成层在 crates/kanzei/tests/integration),104 个 #[tauri::command] 零测试;装配→执行→落库主链 commands/run.rs(604行)、processes/lifecycle.rs(593)、processes/workspace.rs(548)、run/persistence.rs(489)、run/coordinator.rs(424)、run/execution.rs(313)、harness_ext.rs(284) 全部 0 个 #[test];数据面 memory.rs(13 command)/docs.rs(16 command) 同样近零。建立可测基座(状态抽离/伪 AppHandle/集成层)并优先覆盖 run 主链
- 复杂度: 大
- 来源: 2026-08-18 全库勘察
- 标签: 后端
- 边界: 不追求覆盖率数字,优先真实断言关键路径;不重构业务逻辑
- 验收: run 主链关键路径有自动化断言;新增 command 有明确测试落点范式;cargo test 全绿并入 verify
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 R-296，必须先恢复它
- 停车: 
- 进展: 验收对账完成：run 主链关键路径已有真实断言——crates/kanzei-app/src/commands/run.rs:654、667 分别验证真实 episode 投影与 requirement 复杂度来源，crates/kanzei-app/src/run/mod.rs:565 验证真实 SessionStore 的轮末通知持久化/回放（实现提交 1e076db6）；这三处同时给新增 command 建立就地 #[cfg(test)]/真实存储边界的测试落点范式。现行提交 c85f3c99 上 cargo test --workspace exit=0：kz 42、integration 32、kzapp 230、kanzei-memory 156、kanzei-tools 396 passed（另 2 ignored），覆盖 D-570 新增上下文投影；全仓门禁满足。
- observed_head: c85f3c998e8a0fe59571ca8baec5ee855b2c9814
- observed_worktree_hash: fnv1a64:70daf09539db7acb
- recorded_at: 1787218247444
- 对账: 2026-08-20 恢复:R-306 已 done、D-529 已 fixed，历史停车条件消失；本轮在 c85f3c99 现行树重跑 cargo test --workspace，补齐全仓门禁后关闭。

## R-248 先行调研内建:新方向开工前默认产出「已有方案对照」,不靠用户开口 [done]
- refs: R-221 docs/design/research_mode.md
- 依赖: R-221
- 内容: 把「先查已有方案再动手」从用户每次口头要求变成 harness 的默认动作。①触发判据机械可判、不交模型自由裁量:项目根首次初始化 `.kanzei/`、req add 时 refs 为空且标签为核心、用户显式发起,三者之一成立即触发;②产物落 `.kanzei/research/<topic>/prior-art.md`,每条结论含「方案名 + 出处(URL 或 file:line) + 与本课题的差异 + 采用或不采用的理由」,**外部已有实现**(开源方案、协议、公开设计)与**仓内既有设计**(docs/design/**、requirements/defects 现存与 archive)两侧都必须覆盖;③新方向判定成立而无对照工件时,req add 要求 refs 指向该工件,或由用户显式豁免并留痕。
- 复杂度: 中
- 批次: 3/3
- 来源: 2026-08-14 用户观察——开新项目应先深度调研已有方案与设计,不适合从零开始;这是当前 coding agent 的通病(非得用户主动请求才去调研),直接影响自举质量。
- 标签: 核心
- 边界: 不是每条需求都调研,只在触发判据成立时启动;判据必须机械可判,不接受模型自行裁量「这算不算新方向」。websearch 轮次设上限,不做无限扩散爬取。本条只产出对照工件与开工门禁,不改 req/defect 状态机,也不自动把调研结论写成条目——那是 R-221 定调点4 的回流通道。
- 验收: ①三种触发判据各有定向测试,未触发的普通条目不受影响;②prior-art.md 每条结论都带出处,无出处结论被机械拒绝(复用 V0 标注同一套校验);③外部与仓内两侧覆盖各有独立断言,只查一侧不算通过;④新方向下 req add 缺 refs 被拒,豁免路径留痕可审计;⑤websearch 轮次上限有实测,超限给明确诊断而非静默截断;⑥既有 req add 路径无回归。
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 R-248，必须先恢复它
- 停车: 
- 进展: 验收逐项完成：① crates/kanzei-tools/src/prior_art.rs:551 三种触发均创建同形骨架，crates/kanzei-tools/src/tracker.rs:4052 断言普通后端条目不受影响；② crates/kanzei-tools/src/prior_art.rs:257 与 :577 逐结论校验出处、V级、差异、决策，无出处拒绝；③ crates/kanzei-tools/src/prior_art.rs:577-598 独立断言外部 URL 与仓内 file:line 双侧至少各一条，删去仓内章节后以双侧覆盖不足拒绝；④ crates/kanzei-tools/src/tracker.rs:725 与 :4052 核心空refs缺工件拒绝、有效 prior_art 放行、prior_art_waiver 审计落字段；⑤ crates/kanzei-tools/src/prior_art.rs:602 与 crates/kanzei-tools/src/websearch.rs:318 验证轮次上限在联网前返回 PRIOR_ART_SEARCH_LIMIT；⑥ crates/kanzei-tools/src/tracker.rs:3600 与 :3756 既有完整登记及8路并发新增回归通过。实现提交 34c78f40、35aa11ee、571001f1，夹具对齐 37023013；cargo test --workspace exit=0（kz 42/integration 32/kzapp 231/memory 156/tools 406，0 failed）；cargo clippy --workspace -- -D warnings exit=0。真实网络探测：DuckDuckGo HTML HTTP 200/33616B/1158ms，arXiv API HTTP 200/2957B/493ms；当前无需触发降级，失败诊断由 crates/kanzei-tools/src/websearch.rs:281 单测覆盖。
- observed_head: 3702301328886e835c552b5174252853e795f372
- observed_worktree_hash: fnv1a64:2c7dabe2405e41e8
- recorded_at: 1787219867914
- 对账: 2026-08-20 已按用户拍板落地：独立 prior_art/prior_art_waiver 顶层字段，不污染 refs；topic 复用 R-304 的 .kanzei/research/<topic>/ 约定。三批提交 34c78f40、35aa11ee、571001f1。

## R-318 设计文档时效治理:区分现行设计/历史快照/被替代方案并机械阻止状态漂移 [done]
- refs: R-317 docs/design/design_freshness_audit_20260820.md
- 内容: 按审计基线分四批：①修正 tracker 终态与产品边界直接冲突的文档；②修正并行线/执行模型等架构正文内自相矛盾；③设计索引增加 live_design/validated_design/historical_snapshot/superseded 身份、截至提交和替代关系；④机械校验结构化状态并让默认上下文排除 superseded 正文。
- 复杂度: 大
- 执行模型: work_units_v1
- 批次: 4/4
- 来源: 2026-08-20 用户要求寻找过时设计并登记需求给自举；外部审计在当前主线确认 10 份高置信度漂移文档。
- 标签: 流程 核心
- 边界: 不删除或改写历史审计结论；不让模型自由判断过时作为硬门禁；不批量重写未被审计命中的文档；不把治理职责塞回 R-317 状态机。
- 验收: ①审计列出的 10 份漂移文档逐份对账；②索引可区分四类身份并显示截至点/替代关系；③条目终态冲突、同文档头尾矛盾、历史快照不误报三类测试；④superseded 正文不进入默认自举上下文；⑤每批以 Work Unit 证据完成。
- 优先级: P1
- 进展: R-318 四批已完成并按父需求五项验收逐条对账。①审计列出的 10 份漂移文档逐份对账：`docs/design/design_freshness_audit_20260820.md:27-38` 列出的 architecture_browser.md、continue_prompt_dissection.md、r059_mobile_agent_communication.md、research_workspace.md、memory_decision_sufficiency.md、parallel_lines_ui.md、deep_parallel_dev.md、ci_release_evidence_chain.md、reliability_usability_self_hosting_quality.md、harness_m1.md 已由 R-318 B1/B2/B3/B4 提交 `40835597`、`d374cb9f`、`02d585b8`、`5f4b10ce` 逐份完成身份、tracker 状态、实现事实和执行模型对账。②索引四类身份、截至点和替代关系：`.kanzei/project/architecture/README.md:5-10,12-61` 明确定义 live_design/validated_design/historical_snapshot/superseded，现行/已验证含 last_verified_commit，历史含 as_of_commit，被替代含 superseded_by。③自动测试覆盖条目终态冲突、同文档身份矛盾和历史快照不误报：`scripts/check-design-freshness.mjs:105-123,156-180`，证据 `T-1786922726579`。④superseded 正文不进入默认自举上下文：`crates/kanzei-tools/src/profiles/dev.rs:323-345` 只注入非 superseded 索引行且不读取 docs/design 正文，`crates/kanzei-tools/src/profiles.rs:687-736` 真实 baseline 回归，证据 `T-1786922726580`。⑤每批 Work Unit 证据已完成：B1 `40835597`、B2 `d374cb9f`、B3 `02d585b8`、B4 Work Unit `R-318/W4` 已 done，W4 三项 evidence 已登记，缺陷/验证收尾提交 `56cc4def`。验收降级：`T-1786922726581` 的 `verify.ps1` 设计时效门禁、fmt、clippy、workspace tests 和六条 UI/IPC 冒烟均通过，唯一失败为本批未触碰的 `crates/kanzei-memory/src/memory/mod.rs` 既有 metrics regression（production 1399，baseline 1263，增长 136 超 allowance 100）；未以放宽基线或替身测试冒充全绿。
- 验收复核: 父需求五项验收均已在进展中逐项覆盖并带有 file:line、提交号或 T- 测试证据；R-318/W4 Work Unit 已通过 verify/evidence/complete。
- observed_head: 5f4b10ce66012e0aebdc59f994c8fc91eb377ea5
- observed_worktree_hash: fnv1a64:abf42289ad631ab3
- recorded_at: 1787242856195

## R-308 记忆冗余治理与晋升门槛机械化:同指纹聚类合并、candidate 单轨化、复发阈值硬执行 [done]
- refs: D-567 D-568 R-293 R-235 D-637 D-638
- 内容: 批1 同指纹聚类合并:按 [fp:...] 指纹与标题相似度机械聚类,重复簇合并为单条(保最完整正文,合并复发计数),归档被并条目;批2 晋升门槛机械化:复发阈值(如第 2 次才建 candidate、第 N 次+修复证据才 active)由写入方硬执行而非提示词约定,低于阈值的 note 只进 inbox 不落盘;批3 candidate 单轨化:candidate 要么进 INDEX 带标记要么不进检索,消除「索引看不见、检索跑得出」;批4 global 域处置:74 条 candidate 走一次批量复核(晋升/合并/清退),global 域接入 recall 遥测
- 复杂度: 中
- 来源: 2026-08-20 记忆系统全面勘察:61 条顶层条目实质仅约 31 个主题(重复簇 8 个共 39 条,49% 冗余);M-205 与 M-207 标题逐字相同;C6 簇三条(M-248/250/253)共用同一指纹一天内产生;M-245 正文自述「本轮第 1 次复发→暂不建」却仍落盘——晋升门槛写在文里没有被执行;24 条 project candidate 不进 INDEX 却被 FTS 检索(双轨);global 域 74 条全 candidate 零 active 零遥测,晋升管道在全局域没跑通
- 标签: 后端
- 边界: 不动 R-293 的 F(m) 漏斗与效应量框架;不动 R-235 已拍板的 28 条存量豁免;合并动作走 M-059 SOP 归档不裸删
- 验收: ①顶层条目数≈实质主题数(勘察口径复查冗余率<15%);②同指纹重复写入被机械拒绝并有定向测试;③candidate 可见性单轨有断言;④global 域 74 条处置留痕且 recall 遥测非零;⑤INDEX 行与源文件 description 一致性核对通过(与 D-568 对齐)
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-308(unblocks=0)
- 批次: 7/7
- 进展: R-308 ①-⑤ 已逐条完成对账并已提交 B1-B7：① CLI真实统计与T-1786922726625=project36/36、global24/24、merged28；② admission gate + store regression T-1786922726616=165 passed；③ retrieval active单轨与candidate排除 T-1786922726611=163 passed；④现场77→24、global marker recall rows=1，T-1786922726625；⑤ canonical INDEX guard与显式repair `store.rs:801-900`，T-1786922726622=166 passed。提交：B1 `99809920`、B2 `168c404f`、B3 `1ec1d4c7`、B4 `3509d54b`、B5 `9c64dd6e`、B6 `90c75498`、B7 `7635faee`+`edcaf7bb`。④原文74为勘察时点数，现场77已完整处理，属于范围扩展降级说明；D-568 M-014/M-015语义源文缺口继续由独立 fixing 项负责。
- observed_head: 7635faeed1a51ed625e1c9b9ddd09834d36c67b2
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787263273053
- status: done

## R-310 仓库导航效率:失手遥测、工具自愈报错与代码地图,把认知预算还给问题本身 [done]
- refs: D-575 D-568 R-308 T-1786922726640 T-1786922726641 T-1786922726642 T-1786922726645 T-1786922726646
- 内容: 批1 失手遥测:工具调用失败机械分类(不存在路径/越界范围/漏参数/空搜索/权限拒绝),按 run 落 telemetry,产出失手率基线;批2 报错自愈:落地 D-575 验收(最近邻候选/合法范围/必填参数点名);批3 代码地图:crate→模块→公共符号的机器生成索引,symbols 扩仓级查询或注入轻量 repo map——批3 动工前先出小设计对比 token 成本再定形态;批4 复测:同类任务失手率对比基线,弱模型(自举档)实测
- 复杂度: 大
- 来源: 2026-08-20 外部工程评估对照 Claude Code/Codex；本轮来源：系统续作指令
- 标签: 核心
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 不做 embedding/语义代码检索;repo map 若走注入必须过 token 成本核算,超预算宁可工具化按需查;不改 grep/glob 既有语义;记忆召回不相关问题归 D-568/R-308 不在本条
- 验收: ①失手遥测有分类与按 run 聚合,基线数字落档;②D-575 五条验收全部通过;③代码地图机器生成、随提交可增量更新,查询路径有定向测试;④失手率相对基线下降有真实运行数据支撑;⑤repo map 的 token 成本核算落档
- 优先级: P1
- 批次: 4/4
- 设计冻结: 不变式：每个失败结果最多记录一次且不改变原工具结果；权威数据源：ToolOutput.code/outcome/content、ToolCtx.run_id、.kanzei/artifacts/tool-failures/<run_id>.json；预期文件：crates/kanzei-core/src/runner/tool_failure_telemetry.rs、runner/mod.rs、runner/tool_exec.rs及core测试；最小测试：五类失败分类、按run聚合/重复调用、cargo test -p kanzei-core。
- 进展: 验收逐条对账：①已完成（既有 B1 能力，非本次新增）：crates/kanzei-core/src/runner/tool_failure_telemetry.rs:14-22,35-48,83-126,139-183 固定五类失败分类、按 run 聚合、calls/failure_count/failure_rate 与 call_id 去重；基线落档 .kanzei/artifacts/tool-failures/run_1787269956737526500.json（24/32=75.00%），证据 T-1786922726640、T-1786922726642。②已完成（既有 D-575 能力，非本次新增）：crates/kanzei-tools/src/edit.rs 的 READ_PATH_NOT_FOUND、READ_RANGE_OUT_OF_BOUNDS、INVALID_TOOL_INPUT、空搜索、USER_DECLINED/permission denied 五类自愈提示；T-1786922726640、T-1786922726641。③已完成（本次 B3）：crates/kanzei-tools/src/symbols.rs:17-43 输入契约，104-138 crate/module 分流，275-371 实时 crate→module→public symbol 地图，512-582 单行/多行 workspace 解析，1160-1221 定向测试；T-1786922726645 为 fmt 通过且 13 passed。④已完成（本次 B4）：真实自举档复测 .kanzei/artifacts/tool-failures/run_1787270303537435000.json（8/57=14.04%），相对基线下降 60.96 个百分点/约 81.28%，T-1786922726646。⑤已完成：docs/design/r310_repo_map_design.md:10-18 记录 A/B/C 方案与 UTF-8 字节/4 token 成本公式，20-26 记录采用 B 的边界与实时扫描决策。R-310 全部验收完成；D-657、D-658 已 fixed。
- observed_head: a6c74ac3ab899cb642639ce41dfc792201fd425d
- observed_worktree_hash: fnv1a64:590bacf6c17bf734
- recorded_at: 1787271017388
- 取活依据: engine:唯一可执行 WIP 是 R-310，必须先恢复它

## R-324 symbols 扩到 JS/ESM:前端 16k 行补上符号索引 [done]
- 原始描述: 对比 Claude Code 工具面时发现:symbols 只认 .rs,而本仓受跟踪文件 257 个 .rs 对 139 个 .js/.mjs,仅 crates/kanzei-app/ui 一处就 26 文件 16343 行——改得最频繁的那一半代码没有符号索引,定位只能 grep 函数名
- 复杂度: 中
- 标签: 核心
- 验收: .js/.mjs 文件与目录可查符号(fn/class/const)、行号与可见性;可见性判据与 gen-ui-lint-globals.mjs 的列 0 规则同源;注释内伪命中不进结果;node_modules/dist 不被扫描
- 先行调研: .kanzei/research/r324-prior-art/prior-art.md
- refs: R-310
- 优先级: P1
- 进展: symbols 扫描按扩展名分流:扫描循环(注释/块注释/行尾裁剪)两语言通用,只加 parse_js_symbol_line。识别 function/async function/function*/箭头/函数表达式=fn,class=class,const|let|var=const;可见性=列 0 或 export(与 gen-ui-lint-globals.mjs 同源,测试双向点名);内层箭头不误判外层(js_looks_like_arrow 比较首个 => 与 { 的位置);SOURCE_EXTENSIONS 加 js/mjs 同时把 node_modules/dist/.git 加进目录跳过(否则一扫进 node_modules 拖死)。6 条 JS 单测 + 原有 13 条共 19 条通过,workspace 15 个二进制全绿,clippy 干净。实测 08-auto.js public_only 返回 29 个顶层符号带行号
- observed_head: 0284be573ba60895be7e0ecc91bab3339ab4e99d
- observed_worktree_hash: fnv1a64:2e5adf0836cb330b
- recorded_at: 1787285720200

## R-325 grep 补齐上下文/大小写/多行 [done]
- 原始描述: 对比 Claude Code 工具面发现 grep 缺 -A/-B/-C 上下文、-i、多行。grep 是用量最高的工具(1019 轮里 2650 次),拿不到上下文就得追加一次 read
- 复杂度: 小
- 标签: 核心
- 验收: context/before_context/after_context 生效且封顶 20;上下文行以 - 标记、匹配行以 : 标记(ripgrep 惯例);case_insensitive 与 multiline 生效;files_only 不受上下文影响;上下文模式仍遵守 limit 早停
- refs: R-324
- 优先级: P1
- 进展: GrepInput 增 case_insensitive/context/before_context/after_context/multiline。大小写与多行在 RegexMatcherBuilder 构造期决定(建好改不了);上下文经 SearcherBuilder before/after_context 打开,并新增 ContextSink 实现 grep_searcher::Sink——UTF8 sink 只回调 matched 拿不到 context 行。输出沿用 ripgrep 惯例:匹配行 : 、上下文行 -。封顶 MAX_CONTEXT_LINES=20;files_only 与上下文互斥;上下文模式仍受 limit 早停约束并给到限提示。7 条新单测(含 200 行文件验证封顶真实生效)+ 原有共 11 条通过,workspace 15 个二进制全绿,clippy 干净
- observed_head: 31c1bd9f725230f651cae4cc334cf3305fa35d81
- observed_worktree_hash: fnv1a64:276ef34e9de83742
- recorded_at: 1787286478941

## R-326 read 补 notebook 与 PDF 分页 [done]
- 原始描述: 对比 Claude Code 工具面:read 已有图片(R-249)与 PDF 按行读,缺 notebook 与 PDF 按页访问;且 PDF 硬依赖外部 pdftotext,没装即失败
- 复杂度: 中
- 标签: 核心
- 验收: .ipynb 渲染为带序号单元格+捕获输出,error 输出保留 ename/evalue/traceback,非文本输出只报类型不倒 base64,cells 区间生效且越界报错;PDF 支持 pages 页区间(换页符分页)、单次封顶 20 页、全空页报错并指向 OCR;pdftotext 缺失时回落 pdf-extract
- refs: R-325
- 优先级: P1
- 进展: notebook:按扩展名分派(.ipynb 与普通 .json 字节上无法区分,只有路径能表达 notebook 语义),渲染带序号单元格+source(逐行数组与字符串两形态都吃)+四种输出形态,error 保留 ename/evalue/traceback(读 notebook 十有八九就是看它为什么失败),非文本输出只报 mime 不倒 base64,cells 区间 1-based 闭区间越界即报错,默认封顶 50 格。PDF:既有实现是内容嗅探 %PDF- + 外部 pdftotext 按行读,本批加 pages 页坐标(换页符分页——两条抽取路径唯一的共同页边界,无换页符则整篇算一页不硬切假页)、单次封顶 20 页、全空页报错并指向 OCR 而非静默返回空;pdftotext 缺失回落进程内 pdf-extract(保留 pdftotext 首选因 -layout 排版还原更好)。12 条新单测,workspace 15 个二进制全绿,clippy/fmt 干净
- observed_head: b8df33ca1a867da596e7eddfd9f02396b4005565
- observed_worktree_hash: fnv1a64:bba52abe45b1b580
- recorded_at: 1787288250234

## R-327 task 子代理人格可选:补 plan 只读档 [done]
- 原始描述: 对比 Claude Code 的 Agent 类型(Explore/Plan/通用)发现:kanzei 已有 explore/writer 两个人格,但 task 工具不暴露选择,永远派 explore;且缺一个做架构判断的只读人格——机械检索与架构判断需要的模型能力和步数预算差一个量级
- 复杂度: 中
- 标签: 核心
- 验收: task 可传 agent 选人格;schema 的 enum 由运行时名册生成而非硬编码,名册只有默认人格时该参数不出现且 schema 与引入前逐字节一致;未命中静默回落默认;plan 人格为只读、主模型、更大步数;writer 不进模型可选名册
- refs: R-326
- 优先级: P2
- 进展: SubagentRuntime 加 roster(只读人格名册,空=引入前行为)+ resolve_agent(未命中静默回落默认,不为拼错的名字打回整次委派)+ agent_names(去重)。task_spec_for 按运行时名册生成 agent enum——硬编码会让 schema 说有而运行时没有,而回落是静默的,模型永远不知道自己没选中;单人格时该参数不出现且 schema 与 task_spec 逐字节一致。新增 plan_agent:同一只读快照、工具集一个不多,只换主模型+24 步+要求先立约束再下结论并给 file:line 证据。writer 明确不进名册(主 agent 提示词写着 task 子代理绝不写,R-176 的只读白名单是审计资产)。phase_pipeline 派生运行时继承模板名册。4 条单测,workspace 15 个二进制全绿,clippy/fmt 干净
- observed_head: f6ecb8b8d3720b531ae0cb5034e1f67dab70675d
- observed_worktree_hash: fnv1a64:629dbcb7dc894e21
- recorded_at: 1787289443698

## R-328 question 选项补每项说明 [done]
- 原始描述: 对比 AskUserQuestion 发现:question 已有 options/default/multiple,但选项只有标签。只给标签时用户得自己猜每个选项的后果,而后果恰恰是提问的原因——用 A 方案还是 B 方案这种问题,选项名本身从不足以决策
- 复杂度: 小
- 标签: 核心
- 验收: options 既吃裸字符串也吃 {label,note}(description 为 note 别名);schema 显式写出两种形态而非只在描述里说;空白注解视为无注解;缺 label 的对象丢弃;无注解时不序列化 note 字段;桌面 UI 有注解时竖排两行并占满整行;CLI 逐行列出注解
- refs: R-327
- 优先级: P2
- 进展: 新增 AskOption{label,note} 替代 Vec<String>,配 From<&str>/From<String> 让既有 vec!["是".into()] 零改动。AskOption::from_json 两种形态都收(裸字符串是既有调用形态,只认对象会让历史提示词与旧会话重放一起失效),description 作 note 别名(模型更常写这个词),空白注解与缺 label 都按无效处理。question 工具 schema 显式写出 anyOf 两种形态——描述里说了而 schema 里没有等于没说。桌面 UI 有注解时竖排两行占满整行(注解是一句话,横排会被 flex-wrap 撕碎),并兼容历史事件重放里的裸字符串;CLI 逐行灰字列出。移动 PWA 不渲染选项,不受线上形状变化影响。6 条单测,workspace 15 个二进制全绿,UI 冒烟+eslint 通过,clippy/fmt 干净
- observed_head: 86b0f750d2ea4bac069ddc78cfeb1105009e3ed2
- observed_worktree_hash: fnv1a64:fd0280975799681f
- recorded_at: 1787290437559

## R-329 deliver:把产物交到用户面前 [done]
- 原始描述: 对比 SendUserFile:kanzei 只能在正文里写一句路径,用户自己去翻。read 是把内容读进模型上下文,缺的是把产物交到用户面前——报告、图、导出的 CSV 模型不需要再读一遍,用户却得知道它在哪
- 复杂度: 中
- 标签: 前端
- 验收: deliver 工具在对话里给出文件卡片(名称/大小/说明/打开/在资源管理器中显示);路径限工作树内,目录与不存在路径给可行动错误;IPC 侧重做同一校验;桌面独有不进 CLI 工具面
- refs: R-328
- 优先级: P2
- 进展: deliver 工具落在应用层 harness_ext(要往运行中窗口发事件,与 ui_* 同理;CLI 没有对话卡片,那边不注册,故不占 CLI 工具面预算)。display kind=file,06-activity 渲染卡片:文件名/大小/一句话说明 + 打开 / 在资源管理器中显示两个动作。deliver_target 校验:相对与绝对都解析、canonicalize 后必须落在工作树内(交付卡带打开按钮,指向树外等于把本地文件系统读取入口交给模型输入决定)、目录与不存在路径各给可行动错误码。IPC open_delivered_path 重做同一判定——载荷经前端往返,本仓威胁模型无敌对前端,这道校验挡的是意外(历史重放/路径拼错/将来某处绕过工具校验直接调)。reveal 用 explorer /select 并只在启动失败时报错(它退出码不遵循常规约定)。5 条单测,workspace 15 个二进制全绿,eslint+UI 冒烟通过,clippy/fmt 干净
- observed_head: e0be69c2fc26f89a4fe8389a2a8d047c021b52da
- observed_worktree_hash: fnv1a64:b840b69371cca1bc
- recorded_at: 1787291327415

## R-330 process wait:等后台进程满足条件,轮询挪进工具内 [done]
- 原始描述: 用户澄清要的不是定时任务而是终端监控回调。此前模型只能反复调 process output 自己比对,每次一个完整模型往返——等一个 dev server 起来花掉五六轮,而这五六轮除了「还没好」什么信息都没产生
- 复杂度: 小
- 标签: 核心
- 验收: process wait(id, until?, timeout_secs?) 三终态各自可辨(matched 带命中行/exited 带退出码/timeout 如实说没等到);匹配范围是全部已捕获输出而非调用后新增;超时封顶 600 秒;非法正则给可行动错误;wait 与 list/output/discover 标 Shared,stop/kill/adopt 保持独占
- refs: R-329
- 优先级: P1
- 进展: process 新增 wait 动作,轮询挪进工具内(WAIT_POLL_MS=200),一次调用等到条件满足。三终态:matched 给命中行原文(模型要据此判断是不是它想等的那一行)、exited 带退出码、timeout 如实说没等到并给尾部。判定顺序上匹配先于退出——一次性命令可能打出目标行后立刻退出,那种情况该报 matched。匹配范围是全部已捕获输出而非调用后新增:等待语义是「条件成立了吗」不是「再发生一次」,否则会永远等一个不会重复的一次性事件。超时 clamp(1,600),非法正则给 PROCESS_WAIT_BAD_REGEX,进程不存在指路 action=list。用 regex crate 而非 grep 的 RegexMatcher(后者 is_match 需要 grep-matcher trait,面向字节流搜索,这里只对单行判定)。顺带按动作分流并发契约(R-323 B2 的一部分):list/output/discover/wait 标 Shared,stop/kill/adopt 与未知动作保持独占——wait 最长占槽 600 秒,走 Exclusive 会把整批调用堵死。5 条单测,workspace 15 个二进制全绿,clippy/fmt 干净
- observed_head: 575beb7213732441860b47ccee7cb04c34e3c809
- observed_worktree_hash: fnv1a64:16ae57ff9bad9baf
- recorded_at: 1787292023514

## R-311 收尾闭环硬化:设计冻结不变式可执行化与收尾链完成度遥测 [done]
- refs: R-309 R-310 docs/design/weakness_register_20260820.md
- 内容: 批1 不变式可执行化:设计冻结字段支持登记机器可跑断言(grep 模式/测试名/脚本),finalize 与条目关闭时自动执行,失败拒关并点名失败断言;批2 收尾链遥测:条目关闭时机械核对收尾链各环节(编译/定向测试/回归/验收对照/提交)证据是否在档,缺环计数落 telemetry;批3 长程统计:按条目/批次聚合导航失手率(数据来自 R-310)、门禁拒绝、返工次数、收尾链完整度,滚动报表进 metrics——这是外部评估点名缺失的「连续几十个 requirement 的统计证据」载体
- 复杂度: 中
- 来源: 2026-08-20 外部工程评估:execution tail reliability(实现→定向测试→回归→不变式复查→验收对照→提交的最后一公里)是与 Codex 的主要剩余差距;kanzei 已有 13 步 verify 与关闭门禁,缺的是不变式机械检查与按条目的收尾链证据统计
- 标签: 流程
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 不重复 R-309 的门禁裁剪与成本治理;不变式登记是新增可选能力,不给存量条目回填;报表只出数不自动拒绝任何操作
- 验收: ①冻结不变式断言在 finalize/close 自动执行且失败拒关,有定向测试;②收尾链缺环可观测并落 telemetry;③滚动报表真实出数且覆盖不少于 10 个已关闭条目
- 优先级: P2
- 对账: 2026-08-20 需求发现实测补充真实案例:文章获取器项目 D-001 在后置条件未复核下归档 fixed(进展字段自写「复核应确认 raw_lines 为空」即验证后置),本会话复查游离行仍在(D-577)——「终态迁移无后置条件核验」正是批1 不变式可执行化要防的形态,复测场景纳入批4 回归
- 批次: 4/4
- 进展: 最终验收对账：①已完成——`crates/kanzei-tools/src/tracker/invariants.rs:run_one/check_entry_invariants` 执行 grep/test/script；`crates/kanzei-tools/src/tracker/actions.rs:update_close` 在 close 状态迁移前执行，失败点名 `#N` 并拒绝迁移；`crates/kanzei-tools/src/git/finalize.rs:finalize` 经 `git/tool.rs:GitInput` 绑定 requirement_id 后执行，存在声明却未绑定则拒绝。证据 T-1786922726665、T-1786922726674，覆盖 close 失败保持 doing、修复后放行与 finalize 绑定门禁。②已完成——`crates/kanzei-tools/src/close_telemetry.rs:record_close/read_records/rolling_metrics` 将每次成功 close 的编译/定向测试/回归/验收对照/提交证据、缺环数、批次和返工序号写入 `.kanzei/artifacts/close-telemetry.jsonl`；`tracker/actions.rs:599-624` 保证 tracker 写盘成功后才记录，缺环只观测不新增拒绝门禁。证据 T-1786922726667、T-1786922726670、T-1786922726672、T-1786922726674。③已完成——`crates/kanzei/src/cli/metrics.rs:render_close_metrics/metrics_cli` 真实消费 rolling_metrics，输出关闭条目、telemetry 接入数、完整链比例、缺环、门禁拒绝、返工和 R-310 导航失手率；T-1786922726669 真实项目根 smoke 输出关闭条目 965、导航失手 159/886、门禁拒绝 4，`close_telemetry` 单测覆盖 10 条关闭条目。B4 后置条件回归——T-1786922726673：tools raw_lines/raw_delete、update 后置条件和 kanzei-memory 166 tests 全通过，覆盖 D-577 的纯空行误判及“删除报成功后仍存在”同形态；D-577 原文要求的文章获取器 R-002 外部现场仍由其自身阻塞字段降级，本条不冒充外部现场证据。实现已提交 f446bd01（R-311 B3），当前 HEAD 定向回归 T-1786922726674 通过。
- observed_head: f446bd018e2e03242a0d4756cdb77ccf4b76b56b
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787298833936
- 取活依据: engine:唯一可执行 WIP 是 R-311，必须先恢复它

## R-313 需求发现分层:Discovery Record、待确认字段生命周期与歧义落点,让发现阶段先于交付冲动 [done]
- refs: R-248 R-311 D-577 docs/design/weakness_register_20260820.md
- 内容: 批1 Discovery Record:中/大需求 req add 前产出轻量发现记录(Intent 用户真正要什么/Explicit 用户原话/Assumptions 推断/Ambiguities 歧义/领域对象/最小成功闭环/延后决策),来源字段必须含用户原话引用而非只写「用户消息」;批2 待确认生命周期:核心语义类待确认未决时,设计冻结与进入 doing 前要求先走 question 工具或用户显式豁免留痕——把「检测到的歧义」从散文变成有 teeth 的状态;批3 新增限定词一致性检查:需求文本出现用户原话中没有的关键限定词(如用户说「收藏」需求写「浏览器书签」)时机械提示「未确认解释:确认/标 assumption/移除限定」;批4 复测:文章获取器场景回归,同样输入下歧义在冻结前被逼出
- 复杂度: 大
- 来源: 2026-08-20 用户需求发现实测(文章获取器项目)+外部评估。实测对评估的关键修正:模型其实检测到了歧义并写入原 R-003 待确认字段(逐字:「收藏」默认解释为浏览器书签/收藏夹;需要确认是否还要适配特定网站的站内收藏),但 question 工具全程零调用、默认解释选错边(上下文「帖子喜好」指向站内收藏)、设计冻结把「浏览器书签 API/导入文件」写进权威数据源——待确认是死字段,没有任何门禁消费它,歧义靠用户事后「先知乎就行」纠正
- 标签: 核心
- 设计文档: docs/design/weakness_register_20260820.md
- 边界: 不加重 hard gate,Discovery Record 是轻结构不是审批流;「图 ontology 是否 user-centric」这类高级语义判断不做规则化(评估共识:靠模型或产品 persona,规则 gate 判不了);小需求不强制;不改 R-248 prior-art 门,两者在 req add 处组合(prior-art 管「查已有方案」,本条管「问题究竟是什么」)
- 验收: ①中/大需求缺 Discovery Record 或来源无用户原话引用被拒,有定向测试;②含未决核心语义待确认的条目在冻结/doing 前被拦并指向 question,豁免路径留痕可审计;③限定词一致性检查有真实触发与放行案例各一;④文章获取器场景复测:「收藏」歧义在登记前被逼出而非用户事后纠正;⑤既有小需求登记路径无回归
- 优先级: P1
- 取活依据: engine:已完成并提交 5908665f，验收证据齐全
- 批次: 4/4
- 批次表: B1 Discovery Record 登记门禁与来源原话校验；B2 待确认生命周期/question 或用户豁免门禁并接入 doing/claim；B3 限定词一致性检查与 assumption 放行；B4 文章获取器“收藏”场景及小需求回归、验收收口。
- 进展: 已完成并提交 5908665f。验收逐项对账：①中/大需求缺 Discovery Record 或来源无用户原话拒绝：crates/kanzei-tools/src/tracker.rs:731-846、crates/kanzei-tools/src/tracker/actions.rs:267-272；定向证据 T-1786922726675、T-1786922726676、T-1786922726677。②未决核心语义在 update→doing、work_units_v1 claim、legacy claim 前拦截并指向 question，用户豁免可审计放行：tracker.rs:740-746、848-886，tracker/actions.rs:524-549，work/tool.rs:328-333、629-637；T-1786922726676/T-1786922726677。③限定词真实触发与 assumption 放行：tracker.rs:888-917，tracker.rs:4407-4457；T-1786922726675/T-1786922726677。④文章获取器“收藏”→“浏览器书签”在 req add 前被拒，未等用户事后纠正；同时覆盖发现阶段待确认与 claim 前拦截：tracker.rs:4407-4457、4474-4583；T-1786922726675/T-1786922726676。⑤既有小需求登记路径无回归：tracker.rs:4459-4470；T-1786922726675。D-672 已登记、修复并归档，修复位置 tracker.rs:900，证据 T-1786922726677。
- observed_head: 5908665fa20e9f63803ddbdff8c6f3cc93e9297a
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787301389539

## R-314 单线程运行时协作者工具自动隐去 [done]
- 复杂度: 小
- 标签: 前端
- 验收: 检测到仅有 1 条运行线时，前端隐藏"协作者工具"组件
- 优先级: P1
- 取活依据: engine:唯一可执行 WIP 是 R-314，必须先恢复它
- 进展: 验收对照：检测到仅有 1 条运行线时，前端隐藏"协作者工具"组件——已实现于 crates/kanzei-app/ui/index.html:340 的 #collaboration-tools 容器、crates/kanzei-app/ui/09-sessions.js:253-261 的 syncCollaboratorToolsVisibility（以真实 process_list 数量为唯一判据，<=1 隐藏、>1 恢复）及 crates/kanzei-app/ui/09-sessions.js:394 的 renderProcesses 调用。真实自动化证据：scripts/ui-runtime-smoke.mjs:1808-1814 覆盖单线路隐藏与多线路恢复；T-1786922726678 的 node --check 与 UI runtime/ui-lint/parallel-lines/ui-a11y/ui-i18n/ui-markdown 六项冒烟全部通过。既有能力：process_list/renderProcesses 运行态投影；本次交付：协作者工具容器及按线路数的可见性投影。
- observed_head: 5908665fa20e9f63803ddbdff8c6f3cc93e9297a
- observed_worktree_hash: fnv1a64:d405c3ef94c2a719
- recorded_at: 1787301777903

## R-315 验收条款分级策略:复杂度与条款开放度联动,登记/修订时机械把关 [done]
- 复杂度: 中
- 标签: 核心 流程
- 来源: 2026-08-20 用户复盘 D-568 处理过程:验收②在"复杂度:小"条目下挂了开放式全库审计条款("全量 INDEX 行与对应 M-*.md 做一次机械一致性核对并修复"),触发 tracker.rs 验收条款对账门禁后,单个小缺陷被迫展开为跨历史提交的全量审计并派生出新缺陷 D-590,用户反馈"每轮会话结束,每个条目的每个验收都要求测试背书太严厉,推进效率非常慢"。诊断:门禁本身有效(同轮确实抓到真实数据污染和新 regex bug),真正缺口是验收条款在登记/修订时没有"复杂度 vs 条款开放度"的一致性校验,导致小条目能背上大条目量级的验收。用户确认方向:"不同复杂度的验收策略也应该分级"。
- refs: D-568 D-590 R-313 crates/kanzei-tools/src/tracker.rs default_conventions.md
- 内容: 批1 定义开放度分级:识别验收条款文本中的"全量/所有/逐一核对并修复/审计全库"等开放式量词模式,与复杂度字段(小/中/大)建立映射规则——小复杂度不得含开放式全量审计类条款;批2 登记(add)与修订(patch 改验收字段)时机械校验,命中即拒绝并提示"条款过开放,请拆分为独立条目或提升复杂度";批3 验收证据强度本身也按复杂度分级——小复杂度允许 file:line 级证据,大复杂度的开放式核对类条款要求真实覆盖清单或计数,写入 default_conventions.md;批4 存量条目扫描,标出复杂度与条款开放度不匹配的既有 defects/requirements,不强制刷新但登记为待观察清单
- 边界: 不放松现有"沉默跳过即拒"的证据锚门禁本身;不禁止条目中途发现更大范围问题——中途发现走"另开条目"(D-590 那样)是正确路径,本条只是把这个纠正提前到登记/修订时机械挡住,而不是让 agent 先掉进坑里才发现
- 验收: ①小/中复杂度条目登记或修订验收字段时含开放式全量审计类条款被拒,有定向测试;②验收证据强度按复杂度分级的规则写入 default_conventions.md;③D-568 这类真实历史案例重放校验会被正确拦下,有回归用例;④存量条目扫描产出不匹配清单
- 优先级: P1
- 批次: 4/4
- 批次表: B1 开放式全量审计条款识别与复杂度映射；B2 add 与验收字段 patch 修订路径的机械拒绝及定向回归；B3 按复杂度分级的验收证据规则写入 default_conventions.md 并接入门禁；B4 存量条目扫描生成不匹配清单、补齐历史案例回归并收口。
- 取活依据: engine:唯一可执行 WIP 是 R-315，必须先恢复它
- 停车: 
- 进展: R-315 B1-B4 已全部落地，验收逐条对照：①小/中复杂度开放式验收在 add 与验收字段 update patch 拒绝，代码 crates/kanzei-tools/src/tracker.rs:738-785、crates/kanzei-tools/src/tracker/actions.rs:299-300 与 410-414，定向证据 T-1786922726681；②复杂度分级规则已写入 crates/kanzei-harness/assets/default_conventions.md:48，大复杂度 close 证据门禁在 crates/kanzei-tools/src/tracker/actions/action_helpers.rs:197-238 并由 actions.rs:531-533 接线，完整 kanzei-tools 回归 T-1786922726682；③D-568 原文「全量 INDEX 行与对应 M-*.md 做一次机械一致性核对并修复」重放被拒，回归测试 tracker.rs:2603-2670，证据 T-1786922726681；④真实 CLI `cargo run -p kanzei -- req audit_acceptance_scope` 扫描活动与归档 requirements，输出 schema_version=1、mismatch_count=34，列出 R-315/R-320 与归档 R-037/R-040/R-048 等存量不匹配清单且不自动修改，证据 T-1786922726683。既有能力：tracker 的状态迁移、验收对账与 test_record；本次交付：开放度识别、登记/修订拒绝、分级 close 门禁和真实 CLI 扫描。
- observed_head: 46831d94d7149a49199ebc52df5e88e3e86158ce
- observed_worktree_hash: fnv1a64:4896c9829eea1c37
- recorded_at: 1787302921547

## R-316 记忆描述/正文修正同步通道,避免纯文本纠错被迫走 memory_note 异步 inbox [done]
- 复杂度: 小
- 标签: 核心 后端
- 来源: 2026-08-20 D-568 处理中,M-014/M-015 描述内容需要修正,但 .kanzei/memory/*.md 是 M-005 策略托管文件,edit 被规则拒绝,只能靠 memory_note 丢进 inbox 异步等 memory-manager 处理,导致本应是纯文本级别的修正无法在当前会话内完成收尾。对照已有先例 D-295:test_record 曾因同样的"门禁与权限双杀死锁"问题被显式加白名单放行。
- refs: D-568 D-295 M-005 kanzei.toml
- 内容: 给"记忆条目 description/正文的机械性纠错"(不涉及新增/删除条目,只是修正现有条目文本且有明确校验依据,如与 git 历史真源比对)开一条同步工具通道或专用权限规则,让当前会话可直接落盘,不必强制路由到 memory-manager 异步处理;需保留审计留痕(谁改的、依据什么校验、旧值/新值)
- 边界: 不放松"新增/删除记忆条目"仍走既有策略托管路径;仅缩小到"修正现有条目文本内容"这一类操作
- 验收: ①存在一条可在当前会话同步完成的记忆描述/正文修正路径,有真实调用证据;②修正操作留有审计痕迹(修改依据、旧值/新值);③D-568 场景可用新通道在单会话内收尾,不再依赖跨会话异步等待
- 优先级: P1
- 进展: 验收逐项对照：①存在一条可在当前会话同步完成的记忆描述/正文修正路径——`crates/kanzei-memory/src/memory/tools.rs:201-253` 的 MemoryNoteTool action=correct 是真实调用方，调用 `MemoryStore::correct_text`（`crates/kanzei-memory/src/memory/store.rs:503-608`），支持既有条目的单字段 title/description/body、old_value+expected_hash CAS；证据 T-1786922726685、T-1786922726686。②修正操作留有审计痕迹——`store.rs:574-608` 生成并写入 corrections.jsonl，包含 actor、process_id、basis、expected_hash、old_value、new_value；`tools.rs:696-760` 断言审计字段，`tools.rs:762-811` 断言审计失败回滚；证据 T-1786922726689、T-1786922726690。③D-568 场景可用新通道在单会话内收尾——`tools.rs:813-902` 在同一 ToolCtx 连续修正 M-014/M-015，断言 INDEX 与两条审计记录同步更新，未进入 inbox；证据 T-1786922726686。边界对照：action=correct 不开放新增/删除/状态/extra，Dev profile 文案与权限说明位于 `crates/kanzei-tools/src/profiles/dev.rs:53-57,141-144`；既有 memory_note action=note 行为保持。定向测试：T-1786922726690（kanzei-memory 169 passed）与 T-1786922726688（kanzei-tools 473 passed）。
- observed_head: cddb628d4c890f6d9e0f2145adc3a2e6dd696145
- observed_worktree_hash: fnv1a64:e9cac91b56958fa8
- recorded_at: 1787304375014
- 取活依据: engine:唯一可执行 WIP 是 R-316，必须先恢复它
