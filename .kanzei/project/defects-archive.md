# Defects Archive

## D-001 docstore 标题末尾括号被误剥为 severity [fixed] (medium)

## D-002 GUI 运行失败时卡在运行中,错误未展示且无法中止 [fixed] (medium)

## D-003 LLM 请求无超时,网络不通时永久挂起且无提示 [fixed] (medium)

## D-004 前端发送被拒时静默无反馈(running 标志卡死后点发送毫无反应) [fixed] (medium)

## D-005 Tauri 缺 capabilities,前端 event.listen 被 ACL 静默拒绝,所有运行事件收不到 [fixed] (medium)

## D-006 release.ps1 在 kzapp 运行时无法覆盖安装包 [fixed] (medium)
- 复现: 保持已安装的 kzapp.exe 运行，执行 .\scripts\release.ps1；cargo build --release -p kanzei-app 成功，但 Copy-Item 覆盖 ~/.cargo/bin/kzapp.exe 失败并中止脚本。
- 影响: 新构建产物位于 target/release/kzapp.exe，但用户目录中的 kzapp.exe 仍是旧版本，安装流程未完成。
- 验收: 脚本在应用运行时给出明确提示并完成可恢复安装，或在构建成功后明确区分构建成功与安装失败。
- 修复: .\scripts\release.ps1 在 kzapp.exe 占用目标文件时捕获安装异常，将构建产物保存为 ~/.cargo/bin/kzapp.exe.pending，输出明确的构建成功/安装失败提示并保留失败状态；关闭应用后重新执行脚本即可恢复安装。
- 验证: PowerShell AST 语法检查通过；恢复分支静态检查通过；未执行真实发布以避免覆盖用户目录可执行文件。

## D-007 总是允许只对后续运行生效,当前运行内换个命令仍反复弹窗 [fixed] (medium)

## D-008 上下文超限错误未被正确处理 [fixed] (medium)
- 来源: 用户反馈：context overflow: input exceeds the context window
- 现象: 请求输入超过模型上下文窗口时返回 context_length_exceeded 错误，当前流程未提供可恢复处理。
- 验收: 能够识别上下文超限，保留必要会话信息并通过截断、摘要或提示用户等明确策略继续对话，不再直接中断。
- 修复: kanzei-core runner 识别 LlmError::ContextOverflow，在 provider 尚未建立流时仅执行一次有界历史压缩并重试；再次超限或流已建立后不重放，避免死循环和工具副作用重复执行。
- 影响范围: LLM agent 运行循环；不改变 provider API、数据模型和权限行为。
- 限制: 当前仅压缩本次运行内的旧工具轨迹；超大的初始用户输入仍会在一次安全重试失败后返回 provider 错误。
- 验证: cargo test -p kanzei-core -p kanzei-llm 通过；新增 compact_retry_keeps_prompt_and_bounded_tool_history 测试。

## D-009 SQLite 事件 ID 可能在不同会话间碰撞 [fixed] (medium)
- 发现: SessionStore::append_event 使用时间戳和会话内 sequence 生成 event_id；不同 session 在同一毫秒首次写入会生成相同主键。
- 影响: 一个会话的事件写入可能因全局 event_id 主键冲突失败。
- 验收: event_id 在不同 session 的快速连续写入场景下保持唯一，并通过回归测试验证。
- 修复: event_id 改为 evt_<session_id>_<sequence>，补充不同会话快速写入唯一性测试。
- 验证: cargo test -p kanzei-core 通过（5 个测试）。

## D-011 桌面端启动时短暂弹出黑色终端窗口 [fixed] (medium)
- 复现: 启动桌面端 kzapp（尤其通过发版脚本或安装后的启动方式）时，短暂出现黑色终端窗口，随后自动关闭。
- 影响: 桌面端启动体验异常，用户误以为同时启动了命令行程序。
- 验收: 从发版安装入口启动 kzapp，全程不出现可见黑色终端窗口；若确有后台命令需要执行，应使用隐藏窗口方式。
- 修复: 桌面端 app 的 cmd/git 外部进程统一通过 hidden_command 设置 Windows CREATE_NO_WINDOW(0x08000000)，避免外部命令创建控制台窗口。
- 验证: cargo test --workspace 全绿；cargo build --release -p kanzei-app 成功。需关闭当前 kzapp 后重新安装 pending 并实机启动验证。

## D-012 SQLite 会话状态未随 runner 生命周期更新 [fixed] (medium)
- 修复计划: 为 SessionStore 增加受约束的状态更新方法，并在 CLI/桌面端运行开始、完成、失败路径更新状态，同时追加状态事件。
- 发现: R-003 接入 CLI 和桌面端后，sessions.status 始终初始化为 idle，运行中或失败后仍无法反映真实状态。
- 影响: 恢复、调度和 UI 无法依据持久化状态判断会话是否运行中。
- 验收: 运行开始为 running；成功或用户拒绝后为 idle；异常后为 failed；事件日志包含对应状态变更。
- 修复内容: SessionStore 新增 set_status；CLI 与桌面端在运行开始、成功/用户拒绝、异常路径更新 sessions.status，并追加 session.status_changed 事件。
- 验证: 已运行 cargo test -p kanzei-core、cargo build -p kanzei、cargo build -p kanzei-app；新增状态更新单元测试通过。

## D-013 桌面端编辑 diff 默认展开导致对话过长 [fixed] (low)
- 修复计划: diff 内容块默认添加 hidden 类，仅保留头部的文件路径与增删统计；沿用现有工具头部点击切换逻辑。
- 发现: 桌面端 ui/main.js 在收到 diff 展示时直接创建可见 diff 内容，点击工具头部才收起。
- 影响: 编辑操作会在对话流中默认展开大量代码，用户只能看到细节，改变量摘要不够突出。
- 验收: 编辑操作后 diff 默认不可见但摘要可见；点击工具头部展开，再次点击收起。
- 修复内容: ui/main.js 中 diff 详情块默认使用 hidden 类，保留头部文件路径与增删统计，复用现有头部点击切换展开/收起。
- 验证: node --check crates/kanzei-app/ui/main.js 通过；cargo build -p kanzei-app 通过。

## D-014 SessionStore::cancel_input 未校验会话归属，可能跨会话取消 pending 输入 [fixed] (high)
- 修复计划: 将取消 API 改为显式接收 session_id，并在 UPDATE 条件中同时校验 session_id、input_id 与 pending；补充跨会话隔离测试。
- 复现: 调用 cancel_input(input_id) 时只按全局 input_id 更新；上层若误用其他会话的 input_id，仍可取消该输入。
- 影响: R-003 inbox 取消接口缺少会话边界，后续 R-012 子Agent调度复用时可能误取消其他会话任务。
- 验收: 错误 session_id 返回 false 且输入仍为 pending；正确 session_id 可取消；promoted/cancelled 仍不可重复取消。
- 修复: cancel_input(session_id, input_id) 同时校验会话和输入 ID，仅更新 pending；补充跨会话隔离测试。
- 验证: cargo test -p kanzei-core：8 passed。

## D-015 设置页保存丢失 provider 的 context_limit(表单无此列,保存后 ctx 百分比失效) [fixed] (medium)

## D-016 agent 完成工作后从不 git 提交,多波改动堆积在工作区无法追溯回滚 [fixed] (medium)

## D-017 运行中提交的 queue 输入被拒绝且不会自动排队执行 [fixed] (high)
- 复现: 桌面端任务运行期间再次提交提示词，run_prompt 因 running=true 直接返回“已有任务在运行”，输入未写入 session_inputs，当前任务结束后也无法自动执行。
- 影响: 桌面端 queue 调度无法在运行中接纳输入，R-003 的 queue admission/drain 未闭环。
- 验收: 运行中提交提示词返回成功并显示排队状态；当前任务完成后按 FIFO 自动提升并执行；停止运行取消仍 pending 的输入。
- 修复: 桌面端运行中同项目提示词写入 session_inputs queue；当前任务完成后按 created_at FIFO promote 并在同一 runner loop 执行；不同项目仍明确拒绝。
- 验证: cargo test --workspace 全部通过；core queue FIFO、取消 pending 回归测试通过。

## D-018 steer drain 单测补充造成 store 测试模块语法错误 [fixed] (medium)
- 复现: 补充 steer drain 单测时误删重复 admission 测试函数声明，cargo fmt/cargo test 报 store.rs unexpected closing delimiter。
- 影响: 核心 crate 暂时无法编译。
- 验收: 恢复测试函数结构后 cargo test -p kanzei-core 与 workspace 全部通过。
- 修复: 恢复并补充 steer drain 测试，SessionStore 测试模块结构正常。新增 promote_next_input：优先提升 pending steer，无 steer 时按 FIFO 提升 queue。
- 验证: cargo test --workspace 全部通过：kanzei-core 9 tests passed；node --check crates/kanzei-app/ui/main.js 通过。

## D-019 事件恢复改动误删 SessionStore admit_input 方法声明 [fixed] (medium)
- 复现: 新增 SessionStore::latest_event 时替换了 admit_input 方法声明，导致 store.rs 出现孤立参数列表；cargo test --workspace 编译报 unexpected closing delimiter。
- 影响: 事件恢复改动暂时无法编译。
- 验收: 恢复 admit_input 声明后 cargo test --workspace 与 node --check 全部通过。
- 验证: 恢复 admit_input 方法声明后，cargo test --workspace 全部通过。

## D-020 插入 latest_event 测试误删相邻事件 ID 测试声明 [fixed] (medium)
- 复现: 插入 latest_event 回归测试时匹配并替换了“不同会话的事件_id_保持唯一”测试声明，仅保留函数体，cargo test 报测试模块 unexpected closing delimiter。
- 影响: 核心测试模块无法编译。
- 验收: 恢复不同会话事件 ID 测试声明后，store 与 workspace 测试全部通过。
- 验证: 恢复相邻事件 ID 测试声明后，cargo test --workspace 全部通过。

## D-021 子代理递归 runner future 不满足 Send 导致 workspace 无法编译 [fixed] (high)
- 复现: 工作区已有 R-012 子代理实现编译时，kanzei-core runner 将 run_once 强制装箱为 Send future；递归 run_subagent future 仍捕获非 Send 状态，cargo test --workspace 报 future cannot be sent between threads safely。
- 影响: 当前 workspace 无法通过编译，事件恢复改动无法完成验证和提交。
- 验收: 调整子代理递归 future/回调的 Send 边界后，cargo test --workspace 全部通过。
- 修复: 将 run_once 改为显式生命周期的 Send boxed future，打断子代理递归 async future 的类型环并满足线程安全边界。
- 验证: cargo test --workspace 全部通过；kanzei-core 10 项测试通过。

## D-022 桌面端事件恢复读取使用已 drop 的 SessionStore [fixed] (medium)
- 复现: 事件恢复代码在 run_task 中先 drop 初始 SessionStore，再调用 recover_messages(&store, ...)，cargo test --workspace 编译报 borrow of moved value: store。
- 影响: 桌面端事件恢复代码无法编译。
- 验收: 调整 store 生命周期后 cargo test --workspace 与 node --check 全部通过。
- 修复: 恢复 run_task 中 SessionStore 生命周期，事件恢复读取与最终 conversation.updated 写入共用有效 store。
- 验证: cargo test --workspace 与 node --check crates/kanzei-app/ui/main.js 全部通过。

## D-023 多个 pending steer 被一次性提升但仅消费第一条，导致后续 steer 丢失 [fixed] (high)
- 修复计划: 将 steer 提升改为每次仅提升 FIFO 第一条，并补充连续 steer 与 queue 的回归测试。
- 复现: 同一会话 admission 两条 steer 后连续调用 promote_next_input；第一条返回，第二条已标记 promoted 但不再返回。
- 影响: 运行中 steer 调度会丢失用户输入，破坏 R-003 的 steer 优先与输入可靠性。
- 验证: SessionStore 新增 promote_next_steer，每次仅提升一条 steer；新增 drain_依次提升全部_steer_再取_queue 回归测试。cargo test -p kanzei-core 与 cargo test --workspace 全部通过。

## D-025 并行 task 的 tool-end 靠全局 currentTool 配对,多块并行时结果张冠李戴、后续 end 事件被丢弃,task 块永远停在 running(看似卡死);且子代理运行全程无状态反馈 [fixed] (medium)

## D-024 运行中 queue admission 与 drain 收尾之间存在竞态导致 pending 输入遗留 [fixed] (high)
- 修复计划: 为运行中 admission、worker 最终 drain 检查与 running=false 增加统一生命周期锁，消除最后检查与新输入提交之间的竞态；补充可验证测试或至少完成编译/全量回归。
- 复现: worker 在 promote_next_input 返回 None 后、running.store(false) 前，run_prompt 观察到 running=true 并写入 pending；worker 随后退出，pending 输入无人提升。
- 影响: 运行结束边界提交的输入可能永久停留 pending，用户看到任务结束但输入未执行。
- 验证: run_prompt 的运行中 admission 与 drain 尾部 promote_next_input/running=false 统一由 lifecycle 锁串行化；错误路径也在锁内释放 running。cargo test -p kanzei-app 与 cargo test --workspace 全部通过，且同步更新 M2 调度文档。

## D-010 桌面端重启后历史对话不可恢复 [fixed] (high)
- 原因: run_task 只将消息保存在 AppState.conversation 内存中；SQLite 目前仅写入 prompt/run 边界事件，没有保存消息内容，也没有历史会话列表/加载 command。
- 复现: 在 kzapp 中发送对话并关闭应用，再次打开同一项目；消息区域为空，无法查看或继续之前会话。
- 影响: 违反 R-009/R-013 的重启恢复和历史会话加载验收。
- 验收: 重启后可看到项目会话列表，打开任意会话可恢复消息并继续对话；存储读取失败需明确提示。
- 进展: 已落地当前项目会话恢复基础链路：新增 conversation_get Tauri command，启动/项目切换加载 conversation.updated，前端重建 user/assistant/工具摘要；conversation_clear 现在按项目写入空消息投影，避免新对话被旧历史重新恢复。cargo test --workspace 与 node --check 通过。尚未满足完整会话列表/任意历史会话打开验收。
- 验证: 新增 conversation_list：列出 conversation.updated 快照的序号、首条用户消息和消息数；conversation_get 支持按序号加载并同步 AppState 会话上下文；桌面端历史对话列表可打开任意快照并继续对话。启动/切换恢复、新对话空投影均已覆盖。cargo test --workspace 与 node --check crates/kanzei-app/ui/main.js 通过。

## D-026 R-014 多模态运行入口未导出新 runner API 导致桌面端编译失败 [fixed] (medium)
- 修复: 补充 pub use 导出并回归测试
- 原因: kanzei-app 引用了 run_once_with_parts，但 kanzei-core lib.rs 未公开导出
- 复现: cargo test -p kanzei-core -p kanzei-app
- 优先级: P1

## D-027 最后一步收走工具但未告知模型:codex 把工具调用当纯文本狂喷 JSON 并反复自我纠正,收尾轮完全失效 [fixed] (medium)

## D-028 openai 协议图片部件类型误写为 image(规范为 image_url),moonshot 等严格 provider 400 拒收含图历史 [fixed] (medium)

## D-029 顶栏不自适应:窄窗口按钮挤成竖排文字、右侧控件溢出;开发规范章节铺满侧边栏 [fixed] (medium)

## D-030 R-035 diff 查看器新增结构缺少对应样式与行号回归覆盖 [fixed] (medium)
- 复现: 打开活动面板查看 write/edit diff，检查新生成的 diff 结构及多文件汇总。
- 影响: 升级后的统一/并排视图及语法高亮无法达到预期视觉效果，后续改动容易回归。
- 现象: 前端已生成 diff-file-header、diff-row、diff-split-row、syntax-* 等节点，但 style.css 尚无对应规则；后端 diff 行号与多行变更行为也没有测试覆盖。
- 计划: 补齐 CSS，并为 diff_display 增加结构化字段、语言识别、行号和截断测试。
- 优先级: P1
- 修复: 补齐 diff 查看器及多文件汇总 CSS；新增 diff_display 结构字段、语言识别、行号与大输出截断测试。
- 验证: cargo test -p kanzei-tools、cargo test --workspace、node --check crates/kanzei-app/ui/main.js 均通过。

## D-035 需求与缺陷菜单长标题遮挡状态信息 [fixed] (high)
- 原始描述: 需求菜单状态图标/状态标签让实际标题看不见，缺陷菜单存在同样问题。
- 复现: 1. 在需求或缺陷中准备较长标题；2. 打开侧栏菜单或独立文档页；3. 观察状态标签、优先级/复杂度和标题区域。
- 根因: renderDocList 创建的 .doc-row 没有横向 flex 布局，.title 没有 flex:1/min-width:0；在固定 280px 侧栏中，长标题无法收缩，挤压或覆盖状态信息。
- 验收: 两类菜单标题在可用宽度内稳定显示，过长时省略，不遮挡状态、优先级、复杂度；展开详情仍显示完整标题。
- 优先级: P0
- 修复: 为 .doc-row 增加横向 flex 布局与 min-width:0，为标题增加 flex:1/min-width:0/省略号；需求与缺陷侧栏及独立文档列表共用该布局。
- 验证: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test -p kanzei-app 通过。

## D-037 需求复杂度信息覆盖行标题悬浮提示 [fixed] (low)
- 原始描述: 需求行原本设置的“ID 标题(点击展开)”悬浮提示，会在渲染复杂度时被覆盖成“复杂度:小/中/大”。
- 复现: 打开带复杂度的需求菜单，将鼠标悬停在行上，观察 title 提示。
- 根因: renderDocList 在设置 row.title 后又将 item.title 改为复杂度提示，且提示目标从行变为外层条目。
- 验收: 悬停需求行仍能看到完整的 ID 与标题提示，并保留复杂度信息的可见表达；不影响点击展开。
- 优先级: P2
- 修复: 复杂度信息继续通过 complexity-badge 展示，改为追加到 row.title，不再覆盖行原有的 ID+标题悬浮提示。
- 验证: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test -p kanzei-app 通过。

## D-036 独立需求/缺陷页面共用状态筛选导致切换页签空列表 [fixed] (medium)
- 原始描述: 独立文档页的状态筛选选项同时包含需求状态和缺陷状态，但需求与缺陷共用一个筛选器。
- 复现: 1. 打开独立需求/缺陷页面；2. 在需求页选择 todo 或 doing；3. 切换到缺陷页；4. 观察缺陷列表。反向从缺陷状态切到需求页同理。
- 根因: documents-status-filter 同时服务两类文档，筛选值没有按当前页签隔离或动态裁剪。
- 验收: 切换页签后筛选器选项与当前类型匹配，或需求/缺陷各自保存筛选状态；不会因另一类型状态值导致列表无故为空。
- 优先级: P1
- 修复: 独立文档页按需求/缺陷分别保存筛选状态；需求使用 todo/doing/done/dropped，缺陷使用 open/fixing/fixed/wontfix；缺陷页隐藏无效优先级筛选；需求列表改用独立筛选对象，不再临时篡改侧栏 reqFilters。
- 验证: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test -p kanzei-app 通过；静态核对页签切换、刷新和筛选事件均读取对应类型状态。
