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


## D-032 需求和缺陷拖拽未正确实现 [fixed] (medium)
- 原始描述: 需求和缺陷的拖拽行为与用户预期不一致。当前代码仅在需求处于手动排序且无筛选时启用拖拽，缺陷列表没有拖拽实现。
- 复现: 在需求和缺陷菜单分别尝试拖拽条目；再对需求启用筛选或切换非手动排序后尝试拖拽。
- 当前发现: 需求仍仅在手动排序且无筛选时可拖拽；缺陷完整列表可拖拽，筛选后的独立缺陷列表禁用拖拽。拖拽排序失败会刷新列表并显示提示。
- 验收: 产品明确需求/缺陷是否都支持排序；若支持，拖拽、禁用条件、放置反馈和保存失败提示一致；若不支持，界面不暗示可拖拽。
- 修复: 将需求拖拽提交抽象为按文档类型的通用 reorder；缺陷侧栏列表现在支持拖拽，独立缺陷页仅在全部状态时允许拖拽，避免提交筛选后的不完整顺序；统一使用 data-doc-id 并按实际 kind 提交。
- 验证: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test -p kanzei-app 通过。


## D-034 需求和缺陷按钮展开/收纳功能异常 [fixed] (medium)
- 原始描述: 需求和缺陷菜单的展开/收纳触发与状态反馈异常。
- 复现: 点击需求或缺陷标题文字进行折叠，再刷新或切换视图观察状态。
- 优先级: medium
- 当前发现: 折叠事件绑定在 .section-title > span:first-child，依赖标题文字点击；筛选器和操作按钮位于另一 span。当前没有过渡动画，且需要继续验证局部点击区域、localStorage 状态恢复和文档页切换后的表现。
- 验收: 标题区有清晰、稳定的展开/收纳触发区域；需求和缺陷状态独立持久化；切换页面或刷新后状态符合最近一次操作；按钮和筛选器不会误触发折叠。
- 修复: 为所有侧栏分区增加稳定 data-collapse-key；折叠状态优先读取稳定 key，并兼容迁移旧的标题派生 key；需求和缺陷标题支持 Enter/Space，增加 aria-expanded 和展开/收起视觉反馈；标题区域保持与筛选器、操作按钮事件隔离。
- 验证: node --check crates/kanzei-app/ui/main.js、git diff --check、cargo test -p kanzei-app 通过；静态核对需求/缺陷标题点击、键盘操作、刷新与视图切换逻辑。


## D-039 R-059 broker 线程隔离测试未匹配发布后 sequence [fixed] (low)
- 原始描述: R-059 内存 broker 线程隔离测试复用了未注入 sequence 的通知构造器，导致发布后返回 sequence=1 而期望仍为 0。
- 复现: 执行 cargo test -p kanzei-core；notification::tests::thread_replay_does_not_leak_notifications_between_threads 失败，left 为已发布 sequence=1，right 为构造器默认 sequence=0。
- 根因: 测试期望直接比较 notification(id,status)，未反映 broker 发布时统一分配 sequence 的协议语义。
- 验收: 线程隔离测试按发布后的 sequence 比较；core、app 和 POC 验收脚本全部通过。
- 优先级: P2
- 修复: 调整线程隔离测试的期望值，显式设置 broker 发布后的 sequence（thread_a=1、thread_b=2），保留对跨线程不泄漏的断言。
- 验证: cargo test -p kanzei-core 19 项通过；cargo test -p kanzei-app 1 项通过；scripts/r050-poc-check.ps1、git diff --check 通过。


## D-040 R-059 消息幂等键未按 thread_id 隔离 [fixed] (medium)
- 原始描述: R-059 内存 broker 的消息幂等键当前全局去重，不同 thread_id 使用相同 idempotency_key 时会被误判为重复消息。
- 复现: 1. 向两个不同 thread_id 发布相同 idempotency_key 的 AgentMessage；2. 观察第二条消息被返回为 Duplicate。
- 根因: InMemoryBroker.messages 使用 HashMap<String, AgentMessage>，key 未包含 thread_id。
- 验收: 同一 thread_id 内相同幂等键仍返回 Duplicate；不同 thread_id 即使幂等键相同也各自 Accepted，消息不互相覆盖。
- 优先级: P1
- 修复: 将 broker 消息存储 key 改为 `(thread_id, idempotency_key)`，同线程仍幂等去重，不同线程相同 key 独立接受。新增跨线程相同 key 回归测试。
- 验证: cargo test -p kanzei-core 20 项通过；cargo test -p kanzei-app 1 项通过；scripts/r050-poc-check.ps1、git diff --check 通过。


## D-041 R-059 通知 sequence 未按 thread_id 隔离 [fixed] (medium)
- 原始描述: R-059 通知 broker 的 sequence 当前全局递增，不同 thread_id 的通知交错时，单线程订阅会看到其他线程造成的跳号。
- 复现: 1. 发布 thread_a 通知；2. 发布 thread_b 通知；3. 再发布 thread_a 通知；4. 以 thread_a cursor replay，观察 sequence 不连续。
- 根因: InMemoryBroker 只有一个 next_sequence，replay_notifications_for_thread 过滤线程后仍使用全局序号。
- 验收: 每个 thread_id 的通知 sequence 从 1 独立递增；线程订阅 cursor 不受其他线程通知影响；全局 replay 行为有明确且不与线程订阅混淆的语义。
- 优先级: P1
- 修复: 将通知 sequence 从 broker 全局计数改为按 thread_id 独立计数；补充交错发布 A1/B1/A2 时 A=1/2、B=1 的回归测试，thread cursor 不再受其他线程影响。
- 验证: cargo test -p kanzei-core 21 项通过；cargo test -p kanzei-app 1 项通过；scripts/r050-poc-check.ps1、git diff --check 通过。


## D-042 上下文超限错误未能稳定自动恢复，前端显示致命错误 [fixed] (high)
- 原始描述: 偶发收到 context_length_exceeded / invalid_request_error，提示输入超过上下文窗口后直接停止；需要优先压缩并继续。
- 复现: 长对话或工具输出累积后发起下一轮请求，provider 返回 context_length_exceeded；当前仅尝试一次固定压缩，失败后直接以致命错误结束。
- 影响: 用户当前任务被中断，无法在压缩上下文后继续。
- 计划: 先修复 core 的受控上下文超限恢复与错误提示，补回归测试；不得无限重试或重放已产生副作用的工具调用。
- 优先级: P0
- 修复: provider HTTP 400/413/422 超限分类扩展；runner 保留当前用户消息，压缩历史后最多再做一次仅当前输入的安全重试；前端将上下文超限标记为可压缩重试。
- 验证: cargo test -p kanzei-llm -p kanzei-core（18+24 全部通过）；node --check crates/kanzei-app/ui/main.js；git diff --check。


## D-044 鞭挞触发两缺陷:空闲勾选不启动第一轮(需手点继续);阻塞时写日记提交绕过无实质动作刹车,连烧20+空转轮 [fixed] (medium)


## D-031 自主选择后刷新导致进入页面选项异常 [fixed] (high)
- 原始描述: 自主推进的模式我选了之后，似乎每次进来选项会被刷新
- 复现: 1.选择/开启自主推进模式; 2.进入页面或返回查看
- 优先级: medium
- 修复: 持久化模式下拉框选择，启动时仅恢复合法值；模式切换时同步保存，避免重载回到默认模式。
- 验证: node --check crates/kanzei-app/ui/main.js；手工流程为选择“自主推进”后重载页面，仍保持“自主推进”。


## D-045 需求列表字段过长导致展开渲染异常 [fixed] (low)
- 原始描述: 需求列表字段长度长了之后，展开渲染有问题
- 复现: 需求列表字段内容长度增加超过阈值时，展开渲染出现异常
- 优先级: medium
- 修复: 允许条目容器换行，将展开详情设置为完整行并限制最小宽度；长标题和字段按单词边界换行，不再撑坏列表布局。
- 验证: node --check crates/kanzei-app/ui/main.js；手工流程为展开带超长标题/字段的需求，详情在条目下方完整换行显示。


## D-046 运行闸门使用原始项目路径比较导致规范化路径绕过停止边界 [fixed] (medium)
- 复现: run_prompt 设置 running_project 时使用 discover_project_root 后的规范化路径，但运行中分支直接将原始 project_dir 与其比较；相对路径、项目子目录或等价路径可能被误判为其他项目。
- 影响: R-050 的项目运行/停止边界不稳定，等价路径可能无法排队或停止对应运行。
- 验收: 统一通过项目根路径规范化后比较，补充等价路径回归测试。
- refs: R-050
- 优先级: P1
- 修复: 新增 normalized_project_root，运行闸门、停止边界和 session_id 均使用可发现且 canonicalize 的项目根路径；等价路径不再被误判为其他项目。
- 验证: cargo test -p kanzei-app（6项通过）；node --check crates/kanzei-app/ui/main.js；git diff --check。


## D-033 子代理调用慢 - 可能未启用并发导致？ [fixed] (medium)
- 原始描述: 主要模型调用的子代理似乎比较慢，是因为没启用并发吗？
- 复现: 观察主模型调用时，检查是否启用了并发机制。
- 修复: 核实 runner 同轮通过 FuturesUnordered 并行执行 task；增加每轮最多 8 个子代理的硬上限，避免过量并发拖慢本地模型或耗尽连接资源。
- 验证: cargo test -p kanzei-core 26 项通过；代码路径确认同轮 task 使用 FuturesUnordered，超出上限返回明确工具错误。


## D-038 队列输入相关问题 [fixed] (medium)
- 原始描述: 排队输入相关的功能可能有点问题
- 复现: 等价相对路径/子目录路径提交排队输入后，运行 session 可能无法提升该输入。
- 修复: 队列 admission、promotion 统一使用 canonical 项目根路径生成 session_id；保留 steer 优先、queue FIFO、撤销和停止清理语义。
- 验证: cargo test -p kanzei-core 26 项通过；cargo test -p kanzei-app 6 项通过；前端语法检查通过。


## D-043 缺陷评估请求(无具体描述) [wontfix] (low)
- 原始描述: 评估一下缺陷
- 复现: 无法推断:原文未提供具体缺陷现象、环境或步骤
- 处理: 已完成可操作性评估；缺少现象、环境和复现步骤，无法形成代码修复或验收用例。若再次出现，请重新记录具体复现信息。
- 验证: 评估结论已记录；不对不可复现条目伪造修复。


## D-047 需求优先级调整功能存在大量bug [fixed] (high)
- 原始描述: 需求优先级的调整现在很多bug，然后缺陷一样有优先级和复杂度评估
- 复现: 在系统中对需求进行优先级调整操作时会出现多种bug（具体操作步骤未说明）
- 修复: 统一需求与缺陷的 P0-P3 优先级 schema，前端两类文档均支持优先级调整/筛选和复杂度编辑；更新已有 priority 英文字段时原位修改，避免重复字段。
- 验证: cargo test -p kanzei-tools 13 项通过（含英文 priority 原位更新回归）；node --check crates/kanzei-app/ui/main.js；git diff --check。


## D-048 前端 externalBlocked 未定义导致文档列表渲染崩溃,开发规范等侧栏区域全部消失 [fixed] (high)
- 原始描述: 用户反馈"我的开发规范也不见了"
- 复现: 打开 kzapp 任意含需求/缺陷条目的项目;refreshDocs → renderDocList 渲染第一条条目时在 ui/main.js:2081 抛 ReferenceError: externalBlocked is not defined,异常被 refreshDocs 的 catch 静默吞掉。
- 根因: R-071 的外部阻塞标识实现把 externalBlocked 定义误贴进 renderProjects(引用不存在的 entry,启动即崩);后续提交 8fa8c45 删除了误放的定义,但 renderDocList(main.js:2081、2100)仍引用该未定义变量。两个版本都会让侧栏文档区不可用。
- 影响: 需求/缺陷/目标/来源/发现列表渲染中断;renderConventions、历史会话列表、测试记录、工作树刷新、语言刷新全部不执行,用户看到开发规范区域消失。
- 验收: 打开项目后侧栏正常显示开发规范入口与各文档列表;外部阻塞标识在 renderDocList 内按 entry.fields 正确计算;增加能捕获未定义变量的冒烟验证手段(node --check 检不出运行时 ReferenceError)。
- 优先级: P0
- refs: R-071
- 修复: ui/main.js:2081 起在 renderDocList 的 entry 循环内定义 externalBlocked,按 entry.fields 计算;两处引用恢复正常。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-052 CRLF 文件 frontmatter 偏移算错,自定义 agent 提示词被污染或整体清空 [fixed] (high)
- 复现: 在 Windows 上创建 CRLF 编码的 `~/.kanzei/agents/*.md`(git autocrlf 检出、记事本/VSCode 默认均为 CRLF),加载该 agent 后观察 system prompt。
- 根因: kanzei-harness/src/markdown.rs:44-68 用 `line.len() + 1` 逐行累加 body_start,但 `str::lines()` 剥掉的是 `\r\n` 两个字节,每行少算 1 字节;n 个 frontmatter 键累计欠 n+2 字节,落点回退进收尾 `---` 行甚至上一行。n=1 正文变 `-\r\n正文`,n=3~5 整个分隔符混入正文;落点若切在多字节 UTF-8 字符中间(frontmatter 值含中文时常见),`text.get(body_start..)` 返回 None,:66 的 `.unwrap_or("")` 让 body 直接变空。
- 影响: 用户自定义 agent/command 的提示词被静默加前缀、混入分隔符或整体丢失,无任何告警,行为异常极难归因。Windows 是主平台,现有测试(markdown.rs:183)只覆盖 `\n` 故未暴露。
- 验收: 按字节定位收尾 `---` 的真实偏移,不用 lines() 重建;补 CRLF + 中文 frontmatter + 多键的解析回归测试,断言 body 与 LF 版本完全一致。
- 优先级: P0
- 修复: parse_frontmatter 改为按剩余文本真实字节逐行切分,不再用 lines() 重建偏移;新增 crlf_与_lf_解析结果一致 测试覆盖 1~6 个键 + 中文值,断言 body 非空、不残留分隔符且与 LF 版本一致。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-057 队列 drain 循环变量遮蔽导致排队输入顺序反转并重复入库 [fixed] (high)
- 复现: 运行中排队两条输入 X(先)、Y(后);本轮结束进入 drain,状态栏提示"开始执行排队输入(X)",实际执行的是 Y,X 的内容延后到再下一轮。
- 根因: kanzei-app/src/main.rs:2685 用 `let next_input = {...}` 新建绑定遮蔽了 2643 行的外层变量,promote 出的输入被丢弃;2707 只做 `next_prompt = input.prompt.clone()`(clone 暗示本想存回)。下一轮 run_task 收到 promoted_input=None → is_new_input=true → 以相同文本重新 admit(2852-2876),而 store 的 promote_next_input 按 FIFO 弹出最早的 pending(store.rs:400-427),弹出的是 Y 而非刚 admit 的 X'。
- 影响: 队列执行顺序反转违背"依次执行"承诺;每条排队输入被重复写入 prompt.admitted/promoted 事件,事件溯源里 input_id 张冠李戴;重新 admit 时沿用首条消息的 delivery,排队 steer 输入的交付模式被改写。
- 验收: 2707 处改为 `next_input = Some(input);` 并去掉 2685 的 let;补"排队两条输入按提交顺序执行且不重复 admit"的回归测试。
- 优先级: P0
- 修复: main.rs:2685 改为写回外层 next_input(去掉遮蔽的 let),取用处改 next_input.clone();promote 出的输入不再被丢弃,队列按提交顺序执行且不重复 admit。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-058 会话 ID 读写路径 canonical 化不一致,历史对话与运行数据互相不可见 [fixed] (high)
- 复现: 正常使用即触发。运行若干轮后重启应用,历史恢复为空;历史对话列表、轨迹回放读不到内容;点"新对话"后下一轮仍带旧上下文。
- 根因: 运行/写侧统一走 normalized_project_root(canonicalize,Windows 返回 `\\?\C:\...`),而读侧 workspace_snapshot(main.rs:875-877)、conversation_clear(2244-2246)、conversation_get(2274-2276)、conversation_trace_get(2297-2299)、conversation_list(2331-2333)、conversation_delete(2395-2397)仍用裸 discover_project_root;project_session_id 对路径 lowercase 哈希但不剥 `\\?\` 前缀(kanzei-core/src/store.rs:19-27),两侧算出的 session_id 必然不同。二阶错乱:process_session_id 里 default_process_id 用当前函数的 root,前端 activeProcessId 来自 canonical 的 process_list,传入非 canonical 命令时默认进程被误判为非默认,再挂 `#d` 后缀,与运行侧 base 三重错位。
- 影响: 重启恢复、历史列表、轨迹回放读空;conversation_clear 清的是错误 session 的投影和错误 runtime 的内存(2259-2264),运行侧内存历史不受影响;工作区卡片的 status/conversation 与 pending(走 canonical 的 list_pending_inputs)在同一张卡里来自两个不同 session。
- 说明: fc51205(D-046)只把 run_prompt/admit/stop 侧换成 normalized_project_root,conversation_* 与 workspace_snapshot 未同步,属该修复的遗漏面。
- 验收: 读侧统一改用 normalized_project_root;补"写入后立即按前端路径读回"的回归测试覆盖 conversation_get/list/clear 与 workspace_snapshot。
- 优先级: P0
- refs: D-046 D-038
- 修复: conversation_clear/get/trace_get/list/delete 五处与 workspace_snapshot 统一改用 normalized_project_root,与运行/写入侧同源;历史恢复、清空、删除、工作区卡片不再落到另一个会话。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-062 bash 超时丢弃全部已捕获输出且标记为成功 [fixed] (high)
- 复现: 执行一条会超时的命令(如测试跑到 119s 已打印几百行失败详情后卡住),观察工具返回。
- 根因: kanzei-tools/src/bash.rs:102-159 把 out_buf/err_buf 声明在 capture async 块内部(104),`tokio::time::timeout` 超时走 Err 分支(150)时整个 future 连同两个缓冲一起 drop,只返回一句 "timeout: true"(155),而超时前进程可能已产出接近 1 MiB 输出。
- 影响: 模型对"命令卡在哪、已经跑到哪一步"零信息,只能盲目加大 timeout 重跑并重复副作用;且用 ToolOutput::ok 返回,语义上把超时标记为成功,上层按成功统计。对照 Claude Code/opencode:超时均回传已捕获的部分输出。
- 验收: 超时时回传已捕获的 stdout/stderr 并明确标注被截断与超时原因,返回值改为错误语义;补超时保留部分输出的测试。
- 优先级: P1
- 修复: stdout/stderr 缓冲移到 capture future 外,超时时回传已捕获的部分输出并标注 [partial stdout/stderr before timeout];超时改为 ToolOutput::error(不再标记为成功),display 增加 timeout 字段。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-063 移动端桥接 Content-Length 未 trim,POST /v1/messages 恒 400 [fixed] (medium)
- 复现: 用任何标准客户端(curl/reqwest/OkHttp)向本机桥接 POST /v1/messages。
- 根因: kanzei-app/src/main.rs:1836-1846 用 `strip_prefix("content-length:")` 后直接 `parse::<usize>()`;标准头是 `Content-Length: 123`,冒号后带空格,strip 后剩 `" 123"`,而 str::parse 不接受前导空白 → 恒回退 0 → body 为空 → serde_json 解析失败 → unwrap_or_default 得 Null → thread_id 缺失 → 400(1873-1887)。
- 影响: 移动端上行消息通道完全不可用(GET 端点不受影响)。R-059 声称的"双向 message 接口"其中一向从未工作过。
- 验收: 改为 `value.trim().parse()`;补带标准头的 POST 请求解析测试。
- 优先级: P1
- refs: R-059
- 修复: Content-Length 解析改为 value.trim().parse();POST /v1/messages 可正常读取 body。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-069 websearch snippet 偏移链丢失一级基址,产出脏文本且可能 panic [fixed] (medium)
- 复现: 任意一次 websearch,观察返回的 snippet 内容。
- 根因: kanzei-tools/src/websearch.rs:143-153 的第三个闭包中 offset 是 shadowing 后的 G,但切片基址写成 `title_end + 4 + G + 1`,丢掉了第一级基址 A,起点比正确位置提前 A 字节,落在 `<a class="result__snippet"...>` 开标签内部。
- 影响: 每条搜索结果的 snippet 都带 `snippet">` 类垃圾前缀喂给模型;若错位起点落在多字节字符(中文摘要前的非 ASCII href)中间则直接 panic。本文件测试只断言 url/title 未断言 snippet,故漏网。
- 验收: 修正基址累加链;补断言 snippet 内容正确的测试,并覆盖含中文摘要的 HTML。
- 优先级: P2
- 修复: snippet 抽取改为逐级把基址累进 rest,不再丢失 result__snippet 一级偏移;补断言 snippet 内容的测试与中文摘要不 panic 的测试。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-070 docstore 标题尾部方括号被无条件剥为 status,不做枚举校验 [fixed] (medium)
- 复现: `req add "支持 vec[index] 语法"` 或标题含 `[DONE]`;入库时 title 完整,下次 load 被解析成 title="支持 vec"、status="index]"。
- 根因: kanzei-tools/src/docstore.rs:264-270 的 status 剥离没有白名单校验(而紧邻的 severity 剥离有,257 行,正是 D-002 的修复方式);又因 transition_allowed 对未知 from 状态放行(219),后续 update 不报错,错误状态永久固化并显示在 index 注入里。
- 影响: 标题静默损坏并产生非法状态。与已修复的 D-001/D-002 同族,修复未对称应用到 [status]。
- 验收: status 剥离加合法状态白名单校验,不匹配则视为标题的一部分;补含方括号标题的解析测试。
- 优先级: P2
- 修复: status 剥离增加合法状态白名单校验(与 severity 对称);新增 title_with_brackets_survives_roundtrip 测试,覆盖 vec[index]、[DONE] 帧与合法状态仍正常剥离。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-071 grep/glob 默认跳过隐藏文件,项目核心文档搜不到 [fixed] (medium)
- 复现: 在项目根用 grep 搜索 defects.md 中的原文,或用 glob 匹配 `.kanzei/**`,均返回无结果。
- 根因: kanzei-tools/src/grep.rs:92 与 glob.rs:75 使用 `ignore::WalkBuilder::new(base).build()` 默认配置,默认 hidden(true) 过滤所有点开头目录/文件。另 grep.rs:112 的 `let _ = searcher.search_path(...)` 把搜索错误整个吞掉,该文件剩余部分静默缺失。
- 影响: 系统性假阴性——模型会得出"文件不存在/无匹配"的错误结论,而本项目的需求/缺陷/规范文档恰好全在 .kanzei/ 隐藏目录下;.github/、.claude/ 同样不可见。
- 验收: 默认包含隐藏文件(仍尊重 .gitignore),或提供显式开关并在结果中说明;grep 的文件级错误需上报而非吞掉;补搜索 .kanzei 下内容能命中的测试。
- 优先级: P2
- 修复: grep 与 glob 的 WalkBuilder 均设 hidden(false),仍尊重 .gitignore;.kanzei/、.github/、.claude/ 下内容可被检索。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-072 process_update 的 model 为 null 时无法清除模型覆盖 [fixed] (medium)
- 复现: 给某进程选一个具体模型,再把下拉切回"模型:agent 默认",发起运行——界面显示默认,实际仍以旧覆盖模型运行,且无法通过 UI 恢复默认。
- 根因: 前端 `model: $("model-select").value || null`(ui/main.js:1618-1624)在选空时发 null;后端 `model: Option<String>` 把 None 当"不修改"(src/main.rs:548-550),真正能清除覆盖的 Some("") 永远发不出;run_prompt 的 `model.or_else(|| process.model...)`(2613-2614)回落到进程上残留的旧覆盖。
- 影响: 每进程独立模型选择在"恢复默认"这一路径上失效,只能删掉进程重建。
- 验收: 清除时发空串或引入显式 clear 语义;补切回默认后运行使用 agent 默认模型的验证。
- 优先级: P2
- refs: R-030
- 修复: 前端选"agent 默认"时改发空串(后端已按空串清除覆盖),不再发 null 被当作"不修改"。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-073 模型下拉跨进程串值,每进程独立模型选择在 UI 上泄漏 [fixed] (medium)
- 复现: 在进程 p2 选模型 X,切回默认进程,下拉仍显示 X 且发送时直接传 X。
- 根因: ui/main.js:1618-1624 把选择同时写入全局 localStorage `kz-model`;switchProcess(1847)只在目标进程显式设过 model 时才覆盖下拉,否则保留上一个进程的显示值。
- 影响: R-030 承诺的"每进程独立模型选择"在 UI 流程上不成立,用户会在不知情下用错模型跑任务。
- 验收: 模型下拉按进程回显(未设置则显示 agent 默认),全局 localStorage 仅作新进程的初始值;补切换进程后下拉与实际使用模型一致的验证。
- 优先级: P2
- refs: R-030 D-072
- 修复: switchProcess 改为 `$("model-select").value = target.model || ""`,未设覆盖时回到 agent 默认,不再保留上一个进程的选择。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-076 缺陷条目复杂度下拉硬编码 kind:"req",保存必失败 [fixed] (low)
- 复现: 展开任一未关闭缺陷,修改复杂度下拉,提示"复杂度保存失败"。
- 根因: ui/main.js:2203-2214 的分支条件包含 defect,但 invoke 时 kind 写死 "req";按需求表查 D-xxx 必然找不到。同段 `complexitySelect.title = "设置需求复杂度"` 也说明是从需求分支复制未改。
- 影响: 缺陷复杂度编辑确定性失效(R-047 声称已统一需求与缺陷的优先级/复杂度编辑,此处未覆盖)。
- 优先级: P2
- refs: D-047
- 修复: 复杂度下拉提交改用实际 kind,title 按类型区分;缺陷复杂度可正常保存。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-081 update_check 对 dev 构建恒判有新版本,启动每次误报 [fixed] (low)
- 复现: 运行 dev 构建(未注入 KANZEI_BUILD_INFO)且仓库存在任意 release,启动 3 秒后必弹"发现新版本"。
- 根因: kanzei-app/src/main.rs:1498-1499 中 current_hash 对 dev 构建为 "dev",1530 的 `newer = !tag.is_empty() && !tag.contains("dev")` 恒为 true;前端启动静默检查(ui/main.js:3150-3155)据此弹 toast。
- 影响: 每次启动误报;若用户照做会用 release 覆盖本地 dev 版本,丢失未发布的改动。
- 优先级: P3
- 修复: update_check 增加 current_hash != "dev" 判定,dev 构建不再恒判有新版本。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-091 CSS 变量 --muted/--danger 未定义,失败状态失去红色语义 [fixed] (low)
- 复现: 侧栏出现 failed 测试记录或工作区失败状态,颜色与普通文字相同。
- 根因: style.css:144/153/154/174/657 使用 var(--danger)/var(--muted),而 :root 定义块(5-15)只有 --dim/--err,未定义的 var 使 color 回退为 inherit。
- 影响: 失败态失去颜色语义,红色警示恰恰在最需要的地方缺席。
- 验收: 补齐变量定义或改用既有 --err/--dim。
- 优先级: P3
- 修复: :root 补齐 --danger(映射 --err)与 --muted(映射 --dim);失败测试与工作区失败态恢复红色语义。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。


## D-049 快记按钮不应依赖展示展开按钮 [fixed] (medium)
- 原始描述: 快速记录需求和缺陷的按钮不应该依赖于展开按钮
- 复现: 检查快速记录和缺陷记录的UI，确认它们是否直接可用而无需先展开
- 修复: 快记表单改为挂载到对应分区的 section-title 内，并在标题中独占一行；折叠分区只隐藏标题之外的子节点，需求/缺陷快记不再依赖先展开分区。
- 验证: node --check crates/kanzei-app/ui/main.js；cargo test -p kanzei-app（7 项通过）；手工验收：折叠需求或缺陷分区后点击 ✎，表单仍可见、可输入、可取消/提交。


## D-097 鞭挞一轮无工具调用即刹车,模型被提示词诱导过早声明阻塞 [fixed] (high)
- 原始描述: 用户反馈"老是自动停止鞭挞了,停止得太早了,一下子就阻塞了,感觉是提示词的问题"
- 复现: 开启自主推进并勾选鞭挞;当需求列表顶部是复杂度大的 doing 项(如 R-050、R-083)时,模型第一轮就回纯文本声明阻塞,鞭挞立即停止。
- 根因: 两处叠加。①刹车判定过于武断:kz:done 里 `p.steps <= 1 && autoRounds > 0` 一律停,而 steps<=1 只说明本轮没有工具调用,不等于无事可做;②提示词把"阻塞"出口写得太好走:旧 DEFAULT_CONTINUE_PROMPT 是一整句长文,"若活跃目标/需求全部被阻塞或无可推进项…纯文本停住"占了末尾 40%(位置最显著),同时"收尾优先:已是 doing 的事项先关闭再开新的,doing 同时不超过 2 个"会让模型在 doing 满员且都是大项时推断出"无可推进项",直接走纯文本出口。
- 影响: 自主推进形同虚设,用户每次都要手点继续;大复杂度需求(现在队列顶部全是)恰好最容易触发误判。
- 验收: 大项不因"工作量大/需多轮"被判阻塞;仅在确实缺外部输入时停止;单轮无动作不立即刹车但也不得空转烧钱。
- 优先级: P0
- 修复: ①提示词重写为分条指令,明确"大项拆着做、本轮只需推进最小可执行步骤","工作量大/要改多个文件/需要多轮都不是阻塞","doing 满 2 个意味着继续推进这两项而不是停下";阻塞定义收窄为"确实缺外部输入(等用户拍板/缺凭据权限/依赖外部服务或他人)",并约定以【阻塞】开头的纯文本作为唯一刹车信号。②刹车逻辑改为三分支:声明【阻塞】立即停;首次无动作先追加一次具体推进指令(NUDGE_PROMPT,要求直接给出最小可执行步骤并执行);连续第二次无动作才停。③连数上限检查提到无动作处理之前,追加的推进指令也占一轮,不能借此冲破上限。④存量 localStorage 中若是旧默认文案则静默升级,用户自定义过的不动。
- 验证: node --check crates/kanzei-app/ui/main.js;cargo build -p kanzei-app 通过。待实机观察:队列顶部为大项时鞭挞应持续推进而非首轮即停。
- refs: D-044 R-076


## D-098 运行中文档变更不刷新侧栏,状态与状态流转按钮全程陈旧 [fixed] (medium)
- 原始描述: 用户反馈"各类状态按钮得调整似乎不是实时刷新"
- 复现: 开始一次运行,让 agent 通过 req/defect/goal 工具改条目状态;侧栏列表、计数与展开后的状态流转按钮保持开跑前的样子,直到本轮结束才更新。
- 根因: kz:tool-end 处理器只用工具结果更新了"工作焦点"文字(ui/main.js:840-842),从不触发文档刷新;refreshDocs 仅在 kz:done、视图切换和用户手动操作后调用。git 徽章同理,只在 kz:done 刷新,agent 改完文件或提交后不更新。
- 影响: 长时间运行(尤其鞭挞连跑)时侧栏信息与真实状态长期不一致,用户据此判断进度会被误导;状态流转按钮基于陈旧的 nextStatuses 渲染,点下去可能是对已变更条目的非法流转。
- 验收: agent 改动需求/缺陷/目标后侧栏在秒级内跟随更新;工作区徽章在改文件/提交后更新;刷新不得打断用户正在进行的操作。
- 优先级: P1
- 修复: 拆出 renderDocsSnapshot(只重绘文档列表与计数),新增 refreshDocsSoon/refreshGitSoon 两个防抖刷新(400ms/600ms);kz:tool-end 在 req/defect/goal/source/finding 成功后触发文档刷新,在 write/edit/multiedit/bash 成功后触发 git 徽章刷新。同时修掉重绘的两个副作用:renderDocList 跨重绘保留展开状态(此前重绘会把用户刚展开的条目弹回收起),快记表单打开或拖拽进行中时推迟刷新(避免 innerHTML 清空正在输入的内容)。
- 验证: node --check crates/kanzei-app/ui/main.js;cargo build -p kanzei-app 通过。


## D-111 「本轮后停」被持久化,每次启动都重新武装导致鞭挞跑一轮必停 [fixed] (high)
- 原始描述: 用户反馈"怎么还是停止了我服了" —— 鞭挞在完成 14 轮实质工作后仍然停止
- 复现: 勾选一次顶栏「本轮后停」;此后每次启动 kzapp,鞭挞都在第一轮结束后停止,提示"已完成本轮,鞭挞停止",且无论怎么重开鞭挞都复现。
- 根因: 「本轮后停」是一次性意图却被当作偏好持久化。ui/main.js:1577 启动时从 localStorage 读 `kz-auto-stop-round` 并据此设置复选框与 autoStopAfterRound;触发分支(927-931)只把内存变量和复选框复位,从不清除 localStorage。于是该键永远是 "1",每次启动重新武装,一次勾选等于永久生效。
- 影响: 自主推进实际不可用——用户以为是提示词或阻塞判定的问题,反复调整文案都无效,真实原因是一个跨重启复活的一次性开关。停止提示只说"已完成本轮"而不点名是哪个开关所致,进一步掩盖了根因。
- 验收: 「本轮后停」不跨重启存活,启动时一律为未勾选;停止提示需点名触发原因与恢复方式。
- 修复: 移除该开关的持久化(不再读写 localStorage,并在启动时清除存量键);change 事件改为写日志说明当前意图;停止分支的提示改为"鞭挞停止:你勾了顶栏的「本轮后停」(已自动取消勾选,再点鞭挞即可继续)"并写入停止原因。顺带把"暂停中"分支的提示也改为点名原因,五个停止分支现在都会说明是什么条件触发的。
- 验证: node --check crates/kanzei-app/ui/main.js;cargo test --workspace 全绿。存量用户启动一次即自动解除武装。
- 优先级: P0
- 阶段: 1
- 不变量: 界面状态:一次性动作不得跨重启存活;停止必须可归因
- 证据等级: E3


## D-053 上下文压缩重试在工具循环中段产生孤儿 tool_result,恢复请求被 API 400 拒绝 [fixed] (high)
- 复现: 长工具循环(大文件读取、grep 结果堆积)中触发上下文超限,step ≥ 2 时进入压缩重试。
- 根因: compact_messages_for_retry 用 `rposition(|m| m.role == Role::User)` 找"当前用户消息"并原样保留(kanzei-core/src/runner.rs:757-787),但工具结果按 Anthropic 语义也是 User 角色(kanzei-llm/src/request.rs:72-78),工具循环中最后一条 User 消息正是 tool_results。压缩后对应的 assistant ToolCall 消息已被清空,留下无配对 tool_use 的 tool_result;build_body 原样透传不做配对修复(protocol/anthropic.rs:93-104),API 返回 400 invalid_request,该错误不含超限关键词故不被 is_overflow_message 识别,直接上抛导致整次运行失败。compact_messages_aggressively(789-798)同样问题。
- 影响: D-042 的上下文超限恢复在其最常见场景下不生效,超限直接变成运行失败,用户看到的是费解的 invalid_request 而非超限提示。单测 compact_retry_keeps_prompt_and_bounded_tool_history(runner.rs:840-859)只构造了最后一条为纯文本 user 消息的用例,掩盖了此缺陷。
- 验收: 压缩时把末尾 ToolResult 消息降级为纯文本摘要,或回找最后一条纯文本 User 消息;补"工具循环中段触发超限"的回归测试,断言重建请求不含孤儿 tool_result。
- 优先级: P0
- refs: D-042
- 阶段: 1
- 不变量: 消息历史:每个 ToolCall 有且仅有一个对应 ToolResult
- 证据等级: E2
- 进展: 已修复并提交（8696f18）：压缩与激进压缩只回找包含 Part::Text 的 User 消息，工具循环尾部的 tool_results 不再被当作当前提示保留；新增 compact_retry_drops_orphan_tool_results_in_tool_loop，断言压缩结果不含 ToolCall/ToolResult。cargo test -p kanzei-core 全部 28 项通过（仅保留既有 final_text unused_assignments 警告）。
- 验收证据: crates/kanzei-core/src/runner.rs: compact_messages_for_retry、compact_messages_aggressively、is_text_user_message；runner::tests::compact_retry_drops_orphan_tool_results_in_tool_loop。


## D-050 权限路径规范化未覆盖 Windows 大小写等价,硬 deny 仍可降级为询问 [fixed] (high)
- 复现: 在 Windows dev 模式请求写入 `.KANZEI/project/requirements.md`;文件系统把它与 `.kanzei/project/requirements.md` 视为同一路径,权限匹配却大小写敏感,无法命中 `*.kanzei/project/*` 的硬 deny,降级为 Ask。
- 根因: 首轮修复只让 normalize_resource 消解 `.`/`..`/重复斜杠(kanzei-harness/src/permission.rs:70-98),wildcard_match 仍逐字符大小写敏感(:114-130),没有实现原验收要求的 Windows case-fold,也没有把路径解析为与实际落点同源的绝对路径。
- 已完成部分: `..` 穿越、`./` 插入和重复斜杠已被规范化,原始 research 目录穿越路径已不能直接命中放行规则。
- 未完成风险: dev 项目文档硬 deny 仍能被大小写变体绕成 Ask;UNC/盘符等 Windows 路径等价类也没有契约测试。权限弹窗一旦被用户顺手允许,保护文件仍会被写入。
- 验收: 路径类资源与工具实际落点使用同一个规范化函数;Windows 下大小写折叠并覆盖盘符、UNC、反斜杠、`.`/`..`;大小写变体必须保持 Deny 而非 Ask。
- 优先级: P0
- refs: R-083
- 阶段: 1
- 不变量: 阶段 1；权限决策对象与实际执行对象经同一套规范化，大小写变体不得把硬 Deny 降级为 Ask。
- 证据等级: E2
- 缺口: 尚未达到原验收 E2：缺少真实工具调用跨权限门禁与文件系统落点的集成/故障注入测试；bash workdir/命令内部路径仍未纳入同一落点契约。保持 open。
- 证据: E2（部分）：crates/kanzei-tools/src/write.rs 的异步测试真实调用 WriteTool，跨 kanzei-tools/kanzei-harness 并检查文件系统落点；cargo test -p kanzei-tools write::tests::permission_path_and_write落点使用同一规范化结果 通过。完整 E2 仍缺 runner 真实权限门禁集成与故障注入，bash workdir/命令内部路径未覆盖。
- 进展: 验收已满足并关闭：1) Ruleset::evaluate 与 runner 权限门禁统一经 resource_match/normalize_resource；read/write/edit 的实际落点均复用 normalize_resource 后再 join cwd；2) Windows 盘符、UNC、反斜杠、`.`/`..` 与大小写变体已有 permission 回归；3) DevProfile hard deny 独立于普通规则，后置 Ask/Allow 不可覆盖；4) kanzei-tools 的 runner_hard_deny_blocks_real_write_tool_before_filesystem_side_effect 通过本地 SSE 真实触发 kanzei-core runner 与 WriteTool，Ask 回调为 0，文件未创建。bash workdir/命令内部路径属于后续 D-051 范围，不阻塞本条路径资源验收。
- 验收证据: crates/kanzei-harness/src/permission.rs: normalize_resource/resource_match/Ruleset hard deny 与 13 项权限测试；crates/kanzei-tools/src/profiles.rs: DevProfile+ConfigComponent 回归；crates/kanzei-tools/src/write.rs: 本地 SSE runner 真实 WriteTool 门禁回归；cargo test -p kanzei-harness -p kanzei-tools -p kanzei-core 全部通过（25/18/28）。


## D-054 用户拒绝权限时丢弃同批已执行工具结果,历史留未配对 ToolCall 永久毒化会话 [fixed] (high)
- 复现: 一次运行中对任意权限询问点「拒绝」,随后在同一会话继续对话。
- 根因: 工具批次结果累积在局部 results,全部执行完才 push(kanzei-core/src/runner.rs:476-618);Gate::UserDeclined 分支在 runner.rs:589 直接 return,results 被整体丢弃,包括同批排在前面、已实际执行且有副作用的工具结果。返回的 messages 最后一条是含 Part::ToolCall 的 assistant 消息且无 tool_results 跟随,而调用方无条件把该历史当 prior 复用(kanzei-app/src/main.rs:3097-3101/2984/3052、kanzei/src/main.rs:280/145/262)。
- 影响: 拒绝后会话永久损坏,后续每次请求都因 "tool_use ids were found without tool_result blocks" 返回 400,用户只能弃掉会话;同批已执行工具(如已写盘的 edit)的结果既未进历史也未喂给模型,模型对已发生的副作用一无所知,续跑时可能重复执行。
- 验收: 拒绝时为每一个 ToolCall 补配对 ToolResult(已执行的用真实结果,被拒与未执行的用取消占位),push 后再返回;补"拒绝后继续对话"的回归测试。
- 优先级: P0
- 阶段: 1
- 不变量: 消息历史:拒绝与取消也必须配对
- 证据等级: E2
- 进展: 验收已满足并关闭：runner 拒绝权限时为当前/后续每个 ToolCall 补 ToolResult，已执行工具保留真实结果；新增 CLI 真实同批 Write 成功+Bash 拒绝 E2、拒绝后第二次对话恢复 E2、旧损坏 conversation.updated 启动过滤 E2；新增 kanzei-core history filter，接入 CLI prior 与桌面 recover_messages_at/conversation_get；桌面 conversation_prior 与坏快照恢复单测通过。cargo test -p kanzei-core -p kanzei-app -p kanzei --test always_allow_bash 通过（CLI 3 项 E2，桌面既有 11 项，core 历史/runner回归已通过）。停止/promoted 输入属于 D-066，不作为本条关闭阻塞；桌面真实 UI harness 缺口已记录在 D-051。
- 验收证据: crates/kanzei-core/src/runner.rs: Gate::UserDeclined、append_declined_tool_results；crates/kanzei-core/src/history.rs: filter_message_history；crates/kanzei/src/tests/always_allow_bash.rs: 同批拒绝、拒绝后恢复、旧孤儿快照三项 CLI E2；crates/kanzei-app/src/main.rs: recover_messages_at、conversation_prior 与恢复单测。


## D-059 webfetch/websearch 大小写转换后字节偏移错位,可致 panic 与脏文本 [fixed] (high)
- 复现: 用 webfetch 抓取含 U+0130 'İ'(土耳其语页面几乎必含)或 U+1E9E 'ẞ' 的页面。
- 根因: kanzei-tools/src/webfetch.rs:118-137 先 `html.to_lowercase()`,再用 `html.char_indices()` 的字节偏移去切 `lower[i..]`,并把在 lower 中 find 到的位置直接当原文坐标。to_lowercase 会改变部分字符的字节长度(İ 2→3 字节,ẞ 3→2 字节),此后两串坐标永久错位;错位点若落在 lower 的多字节字符中间,`lower[i..]` 直接 panic("byte index is not a char boundary")。
- 影响: webfetch 在 async 上下文内 panic(不像 read/grep 有 spawn_blocking 兜底),会 unwind 掉整个 agent turn;websearch 的 title/snippet 复用同一函数同样中招。research 模式主力工具存在内容依赖型崩溃。
- 验收: 改为在原文上做大小写不敏感匹配(或建立 lower→原文的偏移映射);补含 İ/ẞ 的 HTML 解析测试,断言不 panic 且 script/style 区间正确跳过。
- 优先级: P0
- 阶段: 1
- 不变量: Provider:工具执行不得因内容触发 panic
- 证据等级: E1
- 进展: 已修复：html_to_text 改用 to_ascii_lowercase，所有待匹配标签均为 ASCII，因此原文 char_indices 与 lower 字符串的字节偏移保持一致，不再因 İ/ẞ 等 Unicode 大小写扩展/收缩而 panic 或错位；webfetch 与 websearch 继续沿用同一 helper。新增 webfetch::tests::unicode_text_does_not_shift_script_and_style_offsets 与 websearch::tests::unicode_title_keeps_visible_text_and_skips_script，均验证可见文本保留、script/style 内容跳过；cargo test -p kanzei-tools 22 项通过。改动位置 crates/kanzei-tools/src/webfetch.rs:118-119、184-198、crates/kanzei-tools/src/websearch.rs:212-221。


## D-065 通知 sequence 分配与插入非原子,INSERT OR IGNORE 吞掉冲突静默丢通知 [fixed] (medium)
- 复现: 同一会话/线程有两个并发通知源(如运行结束通知与状态通知同时落库)。
- 根因: kanzei-core/src/store.rs:192-199 的 next_notification_sequence(MAX+1 读取)与 173-190 的 append_notification 是两个独立公开方法,中间无事务包裹(调用方 kanzei-app/src/main.rs:2528-2540 先取后插);两个并发写入方对同一 thread_id 取到相同 sequence,而 INSERT OR IGNORE(178)会忽略任何约束冲突——既包括预期幂等的 event_id 主键,也包括 UNIQUE(thread_id, sequence)(502),第二条通知被静默丢弃,无错误无日志。
- 影响: 通知永久丢失且不可观测,移动端按 cursor 回放永远看不到。
- 验收: 在单个事务内完成 MAX+1 与插入;OR IGNORE 只用于 event_id 幂等重放,(thread_id, sequence) 冲突需报错或重算;补并发写入不丢通知的测试。
- 优先级: P1
- 阶段: 1
- 不变量: 持久化:序号分配与业务写入同事务
- 证据等级: E2
- 进展: 已修复：新增 SessionStore::append_notification_atomic，在 BEGIN IMMEDIATE 事务内读取 thread sequence、生成通知并插入；append_notification 改为 `ON CONFLICT(event_id) DO NOTHING`，不再吞掉 `(thread_id, sequence)` 冲突。app 的 append_run_notification 已切换到原子入口，不再组合 next_notification_sequence + append_notification。新增 4 连接共 80 条通知并发回归，断言 sequence 1..=80 且回放完整；新增相同 thread/sequence 不同 event_id 冲突可见测试。无需 schema 迁移；cargo test -p kanzei-core -p kanzei-app 35/12 项通过。改动位置 crates/kanzei-core/src/store.rs:107-116、182-261、923-1020、crates/kanzei-app/src/main.rs:2737-2753。


## D-067 anthropic 协议遇未知 content_block 类型直接杀流 [fixed] (medium)
- 复现: Anthropic 侧响应中出现新的 block 类型(已有先例:server_tool_use、web_search_tool_result),或 OAuth beta 通道服务端注入新块。
- 根因: kanzei-llm/src/protocol/anthropic.rs:169-173 的 content_block_start 对未知 type 兜底返回 `Err(LlmError::Protocol)`;而同文件 262-264 对未知 SSE 事件只 tracing::debug 忽略——同一宽容原则没有贯彻到 block 类型。官方明确要求客户端忽略未知类型。
- 影响: 响应流中途报错,本轮已生成内容作废。属前向兼容炸弹,服务端一旦推新类型即"所有请求全挂"。
- 验收: 未知 block 类型改为记录并忽略;补含未知 block 的流解析测试,断言不中断且已知内容完整。
- 优先级: P1
- 阶段: 1
- 不变量: Provider:保留原始错误,按结构化状态分类
- 证据等级: E2
- 进展: 已修复：AnthropicState 新增 ignored_blocks，未知 content_block_start 仅记录 debug 并登记索引，不再返回协议错误；该索引的 delta/stop 全部忽略，后续同索引出现已知 block 时可恢复正常。新增 unknown_content_block_is_ignored_without_poisoning_following_blocks，验证未知 block 生命周期不产事件且后续已知文本仍完整；cargo test -p kanzei-llm 24 项通过。改动位置 crates/kanzei-llm/src/protocol/anthropic.rs:5、128-133、151-223、306-342。


## D-087 kz --help 与拼错的子命令被当作 prompt 发给模型 [fixed] (low)
- 复现: 执行 `kz --help` 或 `kz -h`,或把 tracker 子命令打错一个字母。
- 根因: kanzei/src/main.rs:28-44 的顶层 match 只识别版本、五个 tracker 名词和 run,`Some(_) => run_cli(&args)` 把其余一切拼成 prompt 进入完整 agent 循环。
- 影响: 用户期待帮助文本,得到的是模型对字符串 "--help" 的自由发挥,外加 token 花费;该 prompt 还被写入 conversation.updated 持久化,后续每次运行都携带。
- 验收: 显式处理 -h/--help/help 并输出用法;以 `-` 开头的未知参数报错退出而非当 prompt。
- 优先级: P3
- 进展: 已修复(7364448):顶层 match 显式处理 -h/--help/help 输出用法,`-` 开头未知参数 usage 后报错退出;当前 crates/kanzei/src/main.rs:37-44 可见。
- 备注: 条目在 7364448 归档后归档文件遭回滚而丢失,2026-08-07 自 git 历史(880aeec)恢复至归档(见 D-112)。


## D-099 条目内容已丢失,无法恢复 [wontfix]
- 说明: 该 ID 曾被引擎分配(D-104 分配时活动∪归档最大编号已达 103),但内容从未进入任何 git 提交,state.db 事件中亦无踪迹。墓碑条目用于恢复 ID 空间完整性,丢失机制见 D-112。


## D-100 条目内容已丢失,无法恢复 [wontfix]
- 说明: 同 D-099,内容不可恢复,墓碑条目,见 D-112。


## D-101 条目内容已丢失,无法恢复 [wontfix]
- 说明: 同 D-099,内容不可恢复,墓碑条目,见 D-112。


## D-102 条目内容已丢失,无法恢复 [wontfix]
- 说明: 同 D-099,内容不可恢复,墓碑条目,见 D-112。


## D-103 条目内容已丢失,无法恢复 [wontfix]
- 说明: 同 D-099,内容不可恢复,墓碑条目,见 D-112。

## D-082 settings_save 以默认值重建全局配置,表单外字段静默丢失 [fixed] (medium)
- 复现: 手工编辑 ~/.kanzei/kanzei.toml 加入 [permissions] 规则,随后在设置页点一次保存,规则消失。
- 根因: kanzei-app/src/main.rs:1282-1323 用 `KanzeiConfig::default()` 起底,仅回填表单字段(models/proxy/profile.default/providers)后整体覆写全局配置文件,无备份。
- 影响: 用户手写的权限规则等非表单管理内容被静默抹掉;与"kanzei.toml schema 变更必须向后兼容、设置页表单必须透传新字段、禁止保存时丢字段"的项目规范直接冲突。
- 验收: 保存前先 load 现有配置再按字段合并;补"手写字段在保存后仍存在"的测试。
- 优先级: P1
- 阶段: 1
- 不变量: 配置与文档:写入保留未知字段
- 证据等级: E2
- 进展: 首轮修复(5d52281)完成表单字段合并;46a461a 补完全部缺口:settings_save_at_path 改 toml_edit 文本级修改,只动表单管理的键,注释/排版/未知字段/手写规则原样保留;现有配置解析失败时拒绝覆盖保存并报错,不再回退默认值销毁配置;行尾注释随值装饰保留;"缺省即默认"键(proxy/reasoning/profile.default)回落时写显式默认值,避免删键连带删注释;配合 D-084 宽容 schema,未知字段不再触发整份回退。
- 验证: settings_save_preserves_handwritten_permission_rules、settings_save_preserves_comments_and_unknown_fields、settings_save_refuses_to_overwrite_unparseable_config 三项回归;cargo test --workspace 152 项全绿。
- 备注: 本条目曾在 b8698e7 被归档后归档文件遭回滚,从两份文档中同时消失;2026-08-07 自 git 历史恢复(见 D-112)。













## D-083 「总是允许」持久化失败被静默吞掉,成功时抹掉配置注释 [fixed] (medium)
- 复现: 项目 kanzei.toml 含未知字段或磁盘只读时按「总是允许」,本次运行生效但下次运行又弹窗,无任何提示;正常情况下保存后配置文件的注释与排版丢失。
- 根因: kanzei/src/main.rs:220 用 `let _ = append_allow_rule(...)` 吞掉错误;而 append_allow_rule 内部要求项目配置能被本二进制的严格 schema 解析(kanzei-harness/src/config.rs:216),失败即 Err;成功路径是整文件反序列化后 `toml::to_string_pretty` 重写(228),用户手写的注释、排版、键序全部丢失。
- 影响: 表现为"总是允许时灵时不灵"且无从排查;配置文件被引擎重排。
- 验收: 持久化失败时明确告知原因;改为文本级追加规则片段,不做整文件 round-trip。
- 优先级: P2
- refs: D-051
- 阶段: 1
- 不变量: 权限+配置:授权持久化失败必须可见
- 证据等级: E2
- 进展: CLI 与桌面 helper 的持久化失败路径和文本保留实现已存在；本轮执行桌面端 persist_always_allow 成功/失败回归（2 项）均通过，确认失败不返回 AlwaysAllow。46a461a 完成第二项验收:append_allow_rule 改 toml_edit 文本级追加,注释/排版/未知字段原样保留(append_allow_rule_preserves_comments_and_unknown_fields 回归);宽容 schema 后配置含未知字段不再导致持久化失败。UI 事件链 E2 属 D-055 系前端 harness 缺口,不再阻塞本条。













## D-084 配置结构体全量 deny_unknown_fields,新增字段会让旧二进制拒绝启动 [fixed] (medium)
- 复现: 桌面端升级后写入新配置节,再用旧版 kz 运行任意项目,直接报 "unknown field" 退出。
- 根因: kanzei-harness/src/config.rs:12/28/35/54/61 全部标注 deny_unknown_fields;load() 对全局与项目配置任一解析失败即返回错误(76-86),kz run 在 main.rs:62 直接 `?` 退出。
- 影响: CLI 与桌面端共享同一配置文件,严格模式使两端必须严格同版本;一处新字段炸掉所有项目,且报错无"删除该字段或升级"的引导。与项目规范"kanzei.toml schema 变更必须向后兼容(serde default)"冲突。
- 验收: 未知字段降级为告警并忽略,保留类型错误炸启动;补旧版本读取含新字段配置仍可运行的测试。
- 优先级: P2
- 阶段: 1
- 不变量: 配置与文档:schema 向后兼容
- 证据等级: E1



- 阻塞: (已解除)用户 2026-08-07 在会话中确认按"告警并忽略"处理,告警通道为 CLI stderr 与桌面 kz:status。
- 进展: 已修复(46a461a):移除全部 5 处 deny_unknown_fields;未知键经 load_with_warnings 收集告警(CLI stderr 黄字/桌面 run_task kz:status;load() 兜底 tracing::warn),语法与类型错误仍炸启动;unknown_keys 手写 schema 清单由 unknown_keys_schema_matches_struct 测试守护不漂移。
- 验证: unknown_fields_are_tolerated_and_reported 覆盖"含新版本字段的配置在旧 schema 下仍可解析且告警"场景;cargo test --workspace 152 项全绿。








## D-085 无 Ctrl+C 处理,CLI 中断后会话状态永久卡 running [fixed] (medium)
- 复现: 用 Ctrl+C 中断 kz run(CLI 唯一的停止手段),之后在桌面端查看该项目——显示为正在运行的幽灵会话。
- 根因: kanzei/src/main.rs:139 在 LLM 循环前 set_status("running"),复位只存在于 run_once 正常返回后的 Ok/Err 分支(268-296),Ctrl+C 直接杀进程两个分支都到不了;create_session 是 ON CONFLICT DO NOTHING(store.rs:118),下次运行不会先复位。CLI 与桌面端共用同一 project session id。
- 影响: state.db 中该会话永远 running,桌面端渲染成正在运行;本次对话的 conversation.updated 也未落库,中断轮次的历史丢失。
- 验收: 监听 ctrl_c 后落状态再退出,或启动时对 status=running 且无活跃进程的会话做陈旧性复位。
- 优先级: P2
- 阶段: 1
- 不变量: 会话控制:进程崩溃或中断后能恢复或明确终止
- 证据等级: E2
- 进展: 96955b0 实现 tokio::select! 监听 ctrl_c;33cd72d 收尾原子化:SessionStore::finalize_interrupt 在单事务内完成状态复位、stopped_by_user 事件、pending/promoted 输入取消,CLI ctrl_c 分支改调该入口,信号臂只剩接线。
- 验证: store 单测"中断收尾恢复空闲并原子取消未完成输入"+集成测试 crates/kanzei/tests/ctrl_c_finalize.rs(复刻 CLI 启动序列→中断→重开数据库,断言无 running 幽灵会话、事件与输入终态正确)。真实 SIGINT 投递在测试基座无法可靠模拟(Windows 需共享控制台),ctrl_c 分支与测试走同一入口,残余未测面为 3 行接线。
- 备注: 本条目曾在 58cde12 被归档后归档文件遭回滚,从两份文档中同时消失;2026-08-07 自 git 历史恢复(见 D-112)。










## D-112 tracker 归档条目在"仅提交活动文档+回滚归档"后从两份文档同时消失 [fixed] (high)
- 复现: agent 对终态缺陷执行 defect archive(条目移入 defects-archive.md),随后只提交 defects.md 并把归档文件 checkout 回滚——条目在活动与归档文档中都不存在,requirements.md 的依赖引用悬空。
- 根因: archive 是"活动文件删除+归档文件追加"的两文件操作,引擎无法阻止后续 git 操作只保留其一;工具输出未列出被移动的 ID 与两个必须同行提交的文件;tracker 无删除操作、ID 顺序分配,因此活动∪归档中的缺号即数据丢失,但没有任何检查暴露它。
- 影响: 已确认丢失 8 个 ID:D-082/D-085/D-087(自 git 历史恢复)与 D-099~D-103(内容不可恢复,归档中已立墓碑);"缺陷已闭环"的记录被静默销毁,审计链断裂。
- 验收: tracker 每次调用对活动∪归档做缺号/重复检测并在输出中告警;archive 动作后回读校验移动的 ID 确实落在归档文件并列出两个待提交文件;bash 工具在 git commit 成功后自动附带实际提交文件清单供核对。
- 优先级: P0
- refs: D-060
- 阶段: 1
- 不变量: 配置与文档:追踪条目不因归档流程丢失
- 证据等级: E2
- 进展: 已修复。数据恢复(a31efba):D-082/D-085/D-087 自 git 历史恢复,D-099~D-103 立墓碑,缺陷 ID 空间重归完整。门禁落地(1d5e294):tracker 每次调用对活动∪归档做缺号/重复检测并在输出告警(DocStore::integrity_issues);archive 动作回读校验移动的 ID 确实落在归档文件,输出列出移动 ID 并要求两文件同一提交;bash 在 git commit 成功后自动附带 HEAD --stat 实际提交文件清单;dev 提示词禁止 checkout 引擎管理的 tracker 文件。
- 验证: integrity_detects_missing_and_duplicated_ids、integrity_warning_surfaces_in_tool_output、archive_reports_moved_ids_and_requires_paired_commit、git_commit_detection 四项回归;cargo test --workspace 152 项全绿。





## D-113 edit 连续失败后 agent 转用 Set-Content 全文件重写,产生大规模意外差异 [fixed] (medium)
- 复现: edit 的 old_string 因 CRLF/空白差异连续未命中,agent 改用 PowerShell Set-Content 重写整个 main.rs,产生约 3461 行意外差异,只能 git checkout 恢复重做。
- 根因: edit 未命中时的纠错反馈不含文件实际内容,模型只能盲目重试;对 \r\n 与 \n 的差异既不容忍也不指出;bash 工具对 Set-Content/Out-File 整文件覆写毫无拦截,为绕过 edit 的语法校验敞开大门。
- 影响: 单次修复膨胀出 10+ 次可避免的终端调用;全文件重写绕过 edit/write 的语法校验与 diff 展示,极易静默破坏文件。
- 验收: edit 对换行符差异自动容忍;同一文件连续 2 次未命中后在错误反馈中附带文件实际片段(强制对齐);bash 拦截 Set-Content/Out-File 并指引 edit/write;单文件已知函数的缺陷不启动子代理(提示词门禁)。
- 优先级: P1
- refs: D-112
- 阶段: 1
- 不变量: 工具:编辑失败反馈可操作,不诱导整文件重写
- 证据等级: E2
- 进展: 已修复(1d5e294):edit 自动容忍 \r\n/\n 差异,归一命中后按文件主导换行风格写回;同一文件连续 2 次未命中,错误反馈附带以最近似行为中心、带行号的实际内容片段(等于替模型重读)并明确禁止整文件重写;成功一次即清零计数。bash 按词边界拦截 Set-Content/Out-File(不误伤 Get-Content)并指引 edit/write。dev 提示词:缺陷已注明文件+函数时直接读代码,不派子代理重新探索。
- 验证: crlf_mismatch_is_tolerated_and_file_keeps_crlf、second_consecutive_miss_includes_file_excerpt、whole_file_write_cmdlets_are_detected_with_word_boundaries、set_content_command_is_blocked_before_spawn 四项回归;cargo test --workspace 152 项全绿。

