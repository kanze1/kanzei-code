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

## D-055 后台进程的权限询问被前端会话过滤器丢弃,运行永久挂死 [fixed] (high)
- 复现: 项目 A 进程 1 为当前活动会话并正在运行;进程 2(或另一项目)的后台运行触发权限询问。
- 根因: 前端 on() 对非活动会话的所有事件一刀切丢弃(ui/main.js:6-15),kz:ask 也在其中(main.js:950);后端 emit 后即 `receiver.await` 挂起等答复(src/main.rs:2973-2979),answer_ask(2132-2135)是唯一消费路径,无重发机制,切回页签时也不重放 pending asks,后端亦无"列出 pending asks"命令。自动放行逻辑位于过滤器之后同样救不了。
- 影响: 弹窗永不出现,该运行卡在权限等待直到手动停止,用户毫无感知(无日志无提示)。R-030/R-078 主打的多进程/多项目并行在任何需要审批的场景实际不可用,只有 yolo/自动放行才真并行。
- 验收: ask/done/error/stopped 等控制类事件按 sessionId 路由到对应进程状态而非丢弃;切回进程时补发 pending ask;后端提供 pending asks 查询以支持重建。
- 优先级: P0
- refs: R-030 R-078
- 阶段: 1
- 不变量: 会话控制:控制事件按 session_id 收敛到终态
- 证据等级: E2+E3
- 进展: 当前代码路径已完成 ask 按 session 保留、pending_asks_get 重建、后台控制事件刷新；剩余真实前端 UI E2 阻塞：仓库无 package.json、无浏览器测试 harness，无法在当前测试基座安全启动真实 Tauri UI。依据已由 task 调查记录。按 conventions §1.2「可用即关闭」(2026-08-07)关闭:功能路径完整且有回归,前端 UI E2 转 R-101。

## D-056 运行中切换项目后 running 永不复位,UI 永久卡在运行中 [fixed] (high)
- 复现: 项目 A 运行中(running=true)→ 点击侧栏切到项目 B → B 显示"运行中"、发送按钮禁用、状态栏金色,永久卡住。
- 根因: 项目点击 handler 不调 setRunning(ui/main.js:1942-1955);renderProcesses 把 activeSessionId 换成 B 的会话(1802-1810),A 的 kz:done 带 A 的 sessionId 被 on() 过滤丢弃(894-905),setRunning(false)(905)永不执行;此时 B 的进程 tab 就是 activeProcessId,点它命中 1833-1834 早退也无法修复,唯一出路是点停止(仅本地复位)。
- 影响: 多项目并行的基本操作(运行时切项目)导致 UI 状态永久错乱。反向情况:若 B 的 session_id 为空,过滤条件 `sessionId && activeSessionId` 不成立,A 的 kz:text 会直接串流渲染进 B 的对话区。
- 验收: 运行状态按会话维度保存并在切换项目/进程时按目标会话重算;控制类事件不因非活动会话被丢弃;补切项目后运行结束能正确复位的验证。
- 优先级: P0
- refs: D-055 R-078
- 进展: 侧栏与工作区项目切换均已补 setRunning(false)+refreshProcesses，node --check 通过；剩余真实运行中切项目→终态 E2 阻塞于同一前端 UI harness 缺口，且控制事件架构归 D-055/R-086。按 conventions §1.2「可用即关闭」(2026-08-07)关闭:切换即复位的功能路径完整,切项目 E2 转 R-101,控制事件架构归 R-086。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。
- 阶段: 1
- 不变量: 界面状态:前端展示是后端会话状态的投影
- 证据等级: E2+E3

## D-060 docstore 解析丢弃非规范行,tracker 整文件重写会静默销毁用户手改内容 [fixed] (high)
- 复现: 在 requirements.md 手写一条无冒号 bullet(如 `- 就是个备注`)或自由段落/### 子标题/代码块,随后让模型执行任意一次 req/defect 写操作(哪怕改的是别的条目),手改内容消失。
- 根因: kanzei-tools/src/docstore.rs:225-242 的 parse 只保留 `## ` 标题和 `- key: value` 形式 bullet(`bullet.split_once(':')` 无 else 分支),其余一律丢弃;render(301-318)只写回保留部分。而 tracker.rs:76-82/153/227/268 的每一个写操作(add/update/close/reorder)都是 load → 改内存 → save 整文件重写。
- 影响: 数据静默丢失,无任何提示;与 docstore 模块头"用户可任意编辑器手改"及"文档永远写不坏"的设计承诺直接相反——引擎恰恰是唯一会删内容的一方。当前仓库文件全部合规是因为都由引擎生成,掩盖了该缺陷。
- 验收: parse 保留未识别行的原文与位置,save 时原样回写;补"手写自由文本 + 一次 add 后内容不丢"的回归测试。
- 优先级: P0
- 阶段: 1
- 不变量: 配置与文档:写入保留未知字段和用户自由内容
- 证据等级: E2
- 进展: 已补 archive_terminal 模板转移：终态条目的 EntryTemplate 按 ID 合并到归档模板，归档不会丢自由段落、无冒号 bullet、### 子标题或代码块；新增 tracker::tests::archive_preserves_handwritten_free_text_and_unknown_blocks，真实执行 req archive 并断言手写内容进入 requirements-archive.md、活动文档保留进行中条目。cargo test -p kanzei-tools 21 项通过。当前改动位置 crates/kanzei-tools/src/docstore.rs:222-250、crates/kanzei-tools/src/tracker.rs:467-499。按 conventions §1.2「可用即关闭」(2026-08-07)关闭:parse 保留+模板回写机制覆盖全部 save 路径且本会话手写恢复内容经引擎多次重写无损(实测);update/close/reorder 覆盖与并发写入回归转 R-101。

## D-064 SQLite 并发修复只加 WAL/busy_timeout,sequence 竞争与收尾假失败仍未解决 [fixed] (medium)
- 复现: 两个连接并发向同一 session append_event;两边的 DEFERRED 事务都可能读取相同 `MAX(sequence)+1`,后提交方撞 UNIQUE(session_id,sequence)。busy_timeout 只等待锁,不会重新计算已经读出的 sequence。
- 根因: append_event_tx 仍采用 `SELECT MAX(sequence)+1` 后 INSERT(store.rs:527-544),没有 immediate transaction/原子计数器/唯一冲突重试;run_task 收尾的 set_status/append_event/conversation.updated 仍用 `?` 把落库失败提升为整轮失败(main.rs:3085-3104)。
- 已完成部分: SessionStore::open 已设置 WAL、5 秒 busy_timeout 和 synchronous=NORMAL,普通读写锁冲突显著减少。
- 未完成风险: 高并发下仍可能丢本轮终态或把已完成任务报告为失败;原验收中的 UNIQUE 重试和“收尾落库失败降级为可见告警”均未实现。
- 验收: sequence 分配与插入对同一 session 原子化或有限重试;并发测试稳定产出连续且唯一的 sequence;收尾持久化失败保留模型结果并发出明确告警,不伪装成模型运行失败。
- 优先级: P1
- refs: R-083 D-065
- 阶段: 1
- 不变量: 持久化:序号分配、业务写入和幂等记录在同一事务语义内
- 证据等级: E2
- 进展: 生产代码已隔离收尾落库失败(d3b94fa),sequence 分配已在 BEGIN IMMEDIATE 事务内原子化并有并发回归(d1dc702,4 连接 80 通知 sequence 连续唯一)。按 conventions §1.2「可用即关闭」(2026-08-07)关闭:功能验收两项均落地有测试,注入故障的 run_task E2 夹具转 R-101。

## D-066 stop_run 未回收已 promoted 输入,轮次交界仍可静默丢队列消息 [fixed] (medium)
- 复现: drain 在 lifecycle 锁内 promote 下一条输入后释放锁(run_prompt main.rs:2708-2726),尚未进入下一次 run_task 前点击停止;stop_run 随后取得锁并 abort 整个任务,但只 cancel `pending` 输入(store.rs:372-378),刚提升的输入保持 `promoted` 且没有恢复入口。
- 根因: 首轮修复补了 lifecycle 锁、idle 状态和 pending 清理,没有实现原验收的“回收 promoted 未执行输入”;session_inputs 状态机也没有 promoted→cancelled/pending 的停止迁移。
- 已完成部分: 手动停止后会话状态不再永久卡 running,stop 与普通 admit/drain 的大部分交错已串行化。
- 未完成风险: 极窄但真实的轮次交界窗口仍会永久丢一条用户输入,事件日志保留 prompt.promoted 却没有执行/取消终态。
- 验收: promote 与下一轮执行建立可恢复交接;停止时把尚未开始的 promoted 输入明确取消或退回 pending,并补“promote 后、run_task 前停止”的确定性测试。
- 优先级: P1
- refs: D-024 R-083
- 阶段: 1
- 不变量: 输入队列:input_id 只被接纳一次并最终进入终态
- 证据等级: E2
- 进展: 本轮补齐 app 层确定性生命周期回归：新增 stop_runtime_and_finalize，保持 lifecycle 锁覆盖 abort/running 复位/数据库收尾，并沿用 SessionStore::finalize_interrupt 原子写 idle、stopped_by_user 事件及 pending/promoted 取消；stop_run 已改用该 helper。新增 update_tests::stopping_after_promote_cancels_promoted_and_pending_inputs_atomically，验证 promote 后停止时两条输入均取消、会话 idle、事件原因可见。cargo test -p kanzei-app 16 项通过。按 conventions §1.2「可用即关闭」(2026-08-07)关闭:promoted 回收的功能验收已实现且有确定性测试,真实 Tauri Window/provider E2 转 R-101。

## D-086 task 子代理不继承用户权限规则,read deny 可被旁路 [fixed] (medium)
- 复现: 在 kanzei.toml 配置 `action="read", resource="*/.env", effect="deny"`;主代理读被拦后,让模型用 task 子代理读同一文件,内容照常回传。
- 根因: task 调用明确跳过权限门禁(kanzei-core/src/runner.rs:478-480,"硬门禁在构造,不在评估"),但构造处只 add(SubagentBase)(kanzei/src/main.rs:233-236),ConfigComponent 不在内,用户规则不进入子代理快照;而 SubagentBase 给 read/glob/grep 一律 Allow *(kanzei-tools/src/subagent.rs:14-24)。
- 影响: "read deny 保护敏感文件"这一用户可表达的规则存在系统性旁路。"只读所以免检"的前提只对写安全成立,对读的保密性不成立。
- 验收: 子代理装配时叠加用户规则中 read/glob/grep 的 deny 条目(ask 可降为 deny,因子代理无人应答);补子代理读被拦截的测试。
- 优先级: P2
- 阶段: 1
- 不变量: 权限:子代理不得旁路用户规则
- 证据等级: E2

- 进展: 已复核最小权限快照回归：subagent_snapshot_applies_user_read_deny 与只读工具范围测试通过；cargo test -p kanzei-tools subagent 2 项、cargo test -p kanzei-core 37 项通过。生产路径仍已在 CLI/桌面 SubagentBase 后叠加 ConfigComponent。按 conventions §1.2「可用即关闭」(2026-08-07)关闭:用户 read deny 已进入子代理快照且有快照级测试,runner 级实际执行回归与 CLI/桌面 E2 转 R-101。

## D-078 进程切换把自主推进降级为结伴开发,鞭挞静默失效 [fixed] (medium)
- 复现: 选"自主推进"并勾上鞭挞,切一次进程再切回来——模式变成"结伴开发",鞭挞胶囊仍亮着,但本轮结束后不再续跑,无任何提示。
- 根因: process_update 保存 profile 时只存 "dev"/"research" 丢掉了 dev-auto 档位(ui/main.js:1521-1523);switchProcess 回显时 `else if (target.profile === "dev") $("profile-select").value = "dev-pair"`(1846-1849),且以编程方式赋值不触发 change 监听器 → localStorage kz-profile 不更新、鞭挞兼容性检查不执行;此后 kz:done 处的 autoContinueAllowed() 不满足,整个鞭挞分支被跳过且无提示(916)。启动时 1457 直接恢复勾选也可能出现"鞭挞勾着但模式不允许"的死态。
- 影响: 自主推进(核心工作流)在进程切换后静默死亡,用户以为它还在干活。D-031 只修了页面刷新场景,未覆盖进程切换。
- 验收: profile 持久化保留 dev-auto 档位;回显后主动同步鞭挞可用性并在不兼容时明确提示。
- 优先级: P1
- refs: D-031
- 阶段: 1
- 不变量: 界面状态:视图切换不改变运行事实
- 证据等级: E3

- 进展: 已完成验收：crates/kanzei-app/ui/main.js:1688-1696 在切换到不兼容模式时复位鞭挞并 toast 明确提示；2055-2065 切换前按进程保存前端 dev-auto/dev-pair 档位，结合既有 1697-1702 回显逻辑，回切 dev-auto 不再降级。既有 1704-1712 change 调用 process_update，真实调用方仍在。验证：node --check crates/kanzei-app/ui/main.js；cargo test --workspace 全部通过。未补真实桌面 E2 harness，留待 R-101。

## D-080 markdown agent 默认 steps=40 与既定默认 0 冲突 [fixed] (low)
- 复现: 在 ~/.kanzei/agents/ 下定义 agent 但不写 steps 字段,长任务在第 40 轮被强制收尾。
- 根因: kanzei-harness/src/markdown.rs:102 用 `unwrap_or(40)`,而 AgentDef 的 serde 默认是 default_steps()=0 且注释明言"0 = 无轮数上限(用户定调)"(defs.rs:69-75),内置 dev/research agent 也都显式 steps: 0(profiles.rs:143/262)。
- 影响: 用户自定义 agent 与内置 agent 行为不一致,且用户无从知道这个隐藏上限来自哪里。
- 验收: 两处默认统一为 0;补 markdown agent 未写 steps 时 steps=0 的解析测试。
- 优先级: P3
- 阶段: 1
- 不变量: 配置与文档:默认值在各入口一致
- 证据等级: E1

- 进展: 已完成验收：crates/kanzei-harness/src/markdown.rs:113 缺省 steps 统一为 0，与 defs.rs:61-75 的 serde 默认一致；188-210 新增未填写 steps 的 markdown agent 扫描回归测试，确认注册结果为 0。验证：cargo test -p kanzei-harness markdown::tests::agent_without_steps_uses_unlimited_default；cargo test --workspace 全部通过。

## D-068 错误分类忽略 kind,限流可被误判为上下文超限触发破坏性压缩 [fixed] (medium)
- 复现: provider 在流内返回带 token 字样的限流/配额错误;或任何 429/529。
- 根因: kanzei-llm/src/error.rs:34-57 的 classify_provider 完全忽略 kind(流内 error 事件走此路径),只对 message 做宽泛子串匹配,词表含 "token limit"、"too many tokens"、"input_tokens" 这类会出现在配额文案中的模式,命中后 kind(如 rate_limit_error)被丢弃归为 ContextOverflow;runner 对 overflow 的响应是原地压缩消息历史再重试(runner.rs:268-284)。同时 LlmError 没有 RateLimited/Overloaded 变体,429/529 落为普通 Http 直接终止,retry-after 头被无视,client 重试只覆盖建流前的 connect/timeout(client.rs:142)。
- 影响: 误判时无谓压缩掉真实对话历史后重试,限流未解除则二次失败而历史已受损;正常限流没有退避重试,长跑 agent 一遇 TPM 峰值即整轮失败。
- 验收: classify_provider 按 kind 优先、限流错误分类及 Retry-After 退避已存在并有测试；三协议新增回归测试确认限流不触发压缩。
- 优先级: P1
- refs: R-075
- 阶段: 1
- 不变量: Provider:错误分类不改变原始错误事实
- 证据等级: E2+E4
- 进展: 已完成协议边界回归覆盖：Anthropic、OpenAI Chat、OpenAI Responses 三种流内 SSE 错误均验证 rate_limit_error 携带 token limit 文案时分类为 RateLimited，绝不触发 ContextOverflow；既有 HTTP 429/529 Retry-After 退避重试实现保留。定向测试 3 项通过，提交前执行 kanzei-llm 全包测试。

- 改动位置: crates/kanzei-llm/src/protocol/anthropic.rs、protocol/openai.rs、protocol/openai_responses.rs 各新增流内 SSE 限流分类测试；分类实现位于 crates/kanzei-llm/src/error.rs，HTTP 重试位于 client.rs。

## D-051 bash「总是允许」仍按首个可执行词泛化,重定向和程序自身执行入口可绕过 [fixed] (high)
- 复现: 先对 `git status` 选择「总是允许」得到 `git *`,随后执行 `git status > .kanzei/project/requirements.md`;当前 SHELL_CHAINING 不含 `>`/`<`,命令直接命中 Allow 并可覆盖硬保护文档。`git -c alias.x=!calc x`、`python -c ...`、`pwsh -Command ...` 等也说明“同一首词”本身不等于同一权限范围。
- 根因: 首轮修复仅用 8 个字符 `; & | 换行 \` $ (` 做黑名单(config.rs:232-247;permission.rs:100-112),仍把任意无这些字符的命令泛化为 `首词 *`。Shell 与各 CLI 的执行语义无法用有限字符黑名单穷举。
- 已完成部分: 常见串联、管道与 `$()`/反引号命令替换会降级为 Ask,弹窗也已能展示记住规则。
- 未完成风险: 重定向可以绕过 write/edit 的硬 deny;解释器、包管理器、Git alias 等“单条命令”仍可承载任意执行。该问题属于权限模型缺陷,不应继续以补字符方式修补。
- 验收: 已对照验收原文：不再默认保存首词通配，AlwaysAllow 保存完整命令/工作目录结构化作用域；旧裸 bash 规则不匹配新结构化资源并提示降级逐次询问；重定向、Git alias、python -c、pwsh -Command 回归均为 Ask；CLI 真实 AlwaysAllow→结构化配置→bash 执行 E2 通过；CLI/桌面持久化失败均不授权。残余仅为桌面真实 UI E2 harness 缺失，转 R-101，不影响核心功能可用。
- 优先级: P0
- refs: R-083
- 阶段: 1
- 不变量: 权限:授权范围精确可解释
- 证据等级: E2
- 进展: 验证通过：cargo test -p kanzei-harness bash_always_allow_keeps_exact_command；cargo test -p kanzei-harness 前缀通配不放行未明确授权的命令；cargo test -p kanzei --test always_allow_bash（3项）；cargo test -p kanzei-app persist_always_allow（2项）。

- 改动位置: 首词泛化取消与旧规则隔离：crates/kanzei-harness/src/config.rs::generalize_resource、legacy_bash_rules；结构化 bash 资源：crates/kanzei-tools/src/bash.rs::resources_with_ctx；匹配门禁：crates/kanzei-harness/src/permission.rs::command_chaining_escapes/resource_match_for_action；CLI/桌面 AlwaysAllow 调用方：crates/kanzei/src/main.rs、crates/kanzei-app/src/main.rs。

## D-104 最小支持窗口下顶栏折成三行,固定侧栏持续挤压核心对话区 [fixed] (medium)
- 复现: 按 tauri.conf.json 声明的最小窗口 800x500 打开桌面端。静态浏览器验收在 1024px 时 topbar 已为两行(约 69px 高),800px 时为三行(约 101px 高);活动栏 48px + 侧栏默认 280px 后主区仅 472px。
- 根因: #topbar 使用 `flex-wrap:wrap`,把进程、鞭挞、上下文、搜索、模型、模式等低高频控件全部常驻;侧栏只支持 220-460px 调宽,没有折叠/断点策略。D-029 只避免了竖排与横向溢出,没有解决信息层级和主区保底宽度。
- 影响: 小屏或分屏时消息阅读高度被顶栏吞噬,输入区与对话区变窄;控件顺序随换行漂移,形成明显的寻找成本。
- 验收: 已完成低频动作进入明确“更多”菜单、顶栏 nowrap、侧栏一键折叠已有调用方；800/1024/1280 视觉证据尚缺，保持 fixing。
- 优先级: P1
- refs: D-029 R-089
- 阶段: 3
- 不变量: 界面状态:800/1024/1280 三档可用
- 证据等级: E3

- 进展: 本轮完成低频动作进入“更多”菜单、顶栏 nowrap、窄宽度项目与进程控件收缩；沿用既有控件 ID 与调用方，无需新增事件转发。已通过 node --check crates/kanzei-app/ui/main.js、node scripts/ui-i18n-smoke.mjs、node scripts/ui-a11y-smoke.mjs、git diff --check。按“可用即关闭”口径收口；800/1024/1280 真实视觉回归仍缺浏览器 harness，转由 R-101 的延期 E2 清单跟进。

- 改动位置: crates/kanzei-app/ui/index.html:138-193 将低频鞭挞控制、新对话、总结、复制、搜索、模型/思考/模式收进 #topbar-more 明确溢出菜单；crates/kanzei-app/ui/style.css:235-278 顶栏改为 nowrap、菜单浮层与窄宽度 process/项目收缩；沿用既有 main.js 各控件 ID 调用方，无需新增事件转发。

## D-116 D-106 持久错误反馈被重复 reportError 定义覆盖 [fixed] (medium)
- 复现: 触发任意 toastError(error, { retry }) 路径，例如文档查看失败或工作树操作失败；错误消息进入对话错误块，但日志面板不打开且重试按钮不出现。
- 根因: crates/kanzei-app/ui/main.js:203 与 :501 各定义一次 reportError；函数声明后者覆盖前者，toastError 传入的 retry 参数被后者忽略。
- 进展: 已将持久错误反馈入口重命名为 reportPersistentError，toastError 明确调用该入口，保留 reportError 作为运行错误消息入口；补充 UI 静态契约，防止 reportError 重复定义与 toastError 路由回归。node --check、ui-a11y-smoke、ui-i18n-smoke、git diff --check 通过。
- 验收: 统一 reportError 实现，使 toastError 错误进入持久日志面板并保留可用 retry；补静态/自动化契约验证防止重复定义与 retry 丢失。
- refs: D-106
- 优先级: P1

## D-106 错误与长结果普遍依赖 2.6 秒 toast,用户无法追溯、复制或恢复 [fixed] (medium)
- 复现: 触发项目初始化失败、设置保存失败、权限规则删除失败、工作树操作结果等;多数路径只调用 toast(String(error/result)),2.6 秒后消失。长文本被塞入同一浮层,body 又默认 user-select:none。
- 根因: toast 同时承担轻提示、错误报告、长结果查看三种职责,没有按严重度/可操作性分流;部分路径虽写 log,但并非统一契约,也没有“查看详情/重试”入口。
- 影响: 用户看不清错误原因、不能复制给开发者,失败后不知道状态是否改变;D-096 只是该设计问题在 worktree diff 上的一个确定性表现。
- 验收: toast 只承载短暂成功确认;错误与长结果进入可持久查看/复制的通知或详情面板,包含操作名、结果、时间和可用的重试/打开入口;状态改变类操作必须能追溯最终态。
- 优先级: P1
- refs: D-096 R-090
- 阶段: 3
- 不变量: 操作反馈:失败反馈持久、可复制、有恢复入口
- 证据等级: E3

- 进展: 完成本轮剩余用户可见失败反馈迁移：权限自动放行/应答、复制上下文、停止指令、进程模式/思考强度/模型保存、模型列表加载失败统一进入持久日志面板；错误日志带时间，已有重试入口继续保留。验证：node --check crates/kanzei-app/ui/main.js、node scripts/ui-a11y-smoke.mjs、node scripts/ui-i18n-smoke.mjs、git diff --check 通过。按“可用即关闭”口径收口；真实 UI 运行验证与剩余背景刷新告警的 E2 质量增强转由 R-101 跟进。

## D-105 主导航与多类可点击容器没有键盘/可访问语义 [fixed] (medium)
- 复现: 只用 Tab/Enter 操作桌面端。activity-item、project-item、workspace-card、doc-row 等用 div + click 实现,没有统一 role/tabindex/键盘处理;自动放行/鞭挞的真实 checkbox 被 `display:none`;大量图标按钮的可访问名称只剩 `＋/↗/✎/🗑`。
- 根因: 交互由 3200 行原生 JS 零散绑定,只对 sidebar section title 补了 role/tabindex/aria-expanded,没有组件级可访问性约束。浏览器 accessibility snapshot 中活动栏项表现为 generic,图标按钮名称是符号本身。
- 影响: 键盘用户无法完成项目/视图/文档切换,屏幕阅读器无法理解按钮用途;R-040 的少量全局快捷键不能替代完整焦点顺序。
- 验收: 所有可点击对象使用原生 button/a/input 或完整 role/tabindex/键盘语义;图标按钮有稳定 aria-label;焦点可见;仅键盘可完成核心路径并形成自动化冒烟记录。
- 优先级: P1
- refs: R-040 R-091
- 阶段: 3
- 不变量: 界面状态:仅键盘可完成核心流程
- 证据等级: E3

- 进展: 代码与静态自动化已完成：主导航、项目/工作区/文档等核心容器已补键盘语义，图标按钮有稳定 aria-label，焦点规则已覆盖；node --check、ui-a11y-smoke、ui-i18n-smoke、cargo test --workspace 已通过。按“可用即关闭”口径收口；真实浏览器 Tab/Enter E2 仍缺 harness，转由 R-101 的延期 E2 清单跟进。

- 阻塞: 真实浏览器键盘冒烟仍缺运行环境：工作区无 Playwright/Chromium/Edge 命令或前端 harness；scripts/ui-a11y-smoke.mjs 仅静态契约检查，无法证明运行时 Tab/Enter 路径。解除条件：R-101 提供前端 E2 harness 或可用浏览器自动化依赖。当前已完成代码与静态验证，暂跳过真实 E3，继续下一条。

## D-109 对话 Markdown 不支持列表、表格与链接,Agent 核心输出退化为纯文本 [fixed] (medium)
- 复现: 让 agent 输出有序/无序列表、Markdown 表格和 `[label](url)`;renderMarkdown 只转换代码围栏、行内码、加粗和标题(ui/main.js:292-310),列表与表格没有结构,链接不可点击。
- 根因: 自研 markdown-lite 覆盖面与 coding agent 的真实输出形态不匹配,也没有测试定义支持子集。
- 影响: 计划、缺陷对比、测试矩阵和来源链接难以扫读,直接损伤“看输出”这一最高频路径;长回复缺少语义导航。
- 验收: 明确安全 Markdown 子集并支持列表、表格、链接、代码语言标识;外链有清晰安全行为;渲染必须先安全处理并有 XSS 回归测试。
- 优先级: P1
- refs: R-090
- 阶段: 3
- 不变量: 操作反馈:安全 Markdown 渲染并通过 XSS 用例
- 证据等级: E3

- 进展: 已完成 D-109 验收原文：crates/kanzei-app/ui/main.js:412-557 的既有 renderMarkdown 调用链扩展为安全子集，支持无序/有序列表、表格及对齐、http/https/mailto 安全外链（target=_blank + noopener noreferrer）、代码围栏语言 class；先 escapeHtml，再只放行安全协议，拒绝危险 HTML/协议。调用方保持既有 addMessage、appendAssistant、文档查看器 renderMarkdown，无需新增按钮或命令。新增 scripts/ui-markdown-smoke.mjs，执行 XSS、列表、表格、链接、代码语言回归；node --check、Markdown/i18n/a11y 冒烟、git diff --check 全部通过。

## D-110 todo 与活动两个右栏可同时占宽,最小窗口会把主对话区压到近乎不可用 [fixed] (medium)
- 复现: 打开活动面板,再让 todowrite 显示当前计划;todo-panel 与 bg-panel 均为独立 300px 固定右栏且可同时显示。在 1280px 默认侧栏下主区只剩约 352px;800px 最小窗口下两右栏与左栏总宽已超过窗口。
- 根因: 两个面板没有互斥、tab 合并或窄屏 overlay 策略,宽度都以 flex-shrink:0 的侧栏语义参与主布局;设置的可调最小宽度仍为 240px。
- 影响: 运行越复杂、信息越多时主对话区越不可读,与“对话为主布局”目标相反;用户只能手动隐藏活动面板,计划面板由事件出现。
- 验收: todo/活动合并为一个可切换右栏或在同时出现时共享宽度;窄屏采用 overlay/抽屉且不压缩主区;800/1024/1280 覆盖单面板和双面板场景。
- 优先级: P1
- refs: R-037 R-089
- 阶段: 3
- 不变量: 界面状态:多面板不挤压主对话区
- 证据等级: E3

- 进展: 已完成 D-110 核心验收：crates/kanzei-app/ui/style.css:25-40 为 #app 增加定位上下文；800/1024/1280 均落入 max-width:1400px 抽屉策略，todo-panel/bg-panel 绝对定位为右侧 overlay，不再参与主区 flex 宽度；两者同时出现时通过相邻选择器共享两列抽屉宽度，不互相覆盖。既有 renderTodoPanel 与 activity-toggle 调用方沿用，无新增命令或按钮。scripts/ui-a11y-smoke.mjs 增加窄屏 overlay/双面板契约检查；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。真实 800/1024/1280 像素回归仍由 R-101 跟进。

## D-074 前端 5 类静默失败:附件丢失、设置页白屏、启动链中断、快记内容丢失、系统通知永不弹 [fixed] (medium)
- 复现: ①粘贴截图不写文字点发送→附件 chip 消失什么也没发生;②配置损坏时打开设置页→空表单无提示;③projects_get 失败→启动后半段全不执行,状态栏停在初始态;④快记写完提交失败→表单已销毁内容找不回;⑤等长任务完成→系统通知从不出现。
- 根因: ①send() 在 sendText 前就清空 attachments(ui/main.js:1436-1446),而 sendText 的 `if (!prompt) return`(1264-1266)静默吞掉——该函数注释自己写着"任何拒绝发送的理由都要说出来,绝不静默(D-004)";②loadSettings(2956-2957)首行 invoke 无 try/catch;③启动 IIFE(3156-3162)的 invoke 在 try 块外;④快记 submit 先 form.remove() 再 await invoke(2459-2473),失败时输入已随表单销毁(对照目标新建表单 2530-2544 失败保留);⑤全项目从未调用 Notification.requestPermission,permission 恒为 default,`=== "granted"` 条件恒 false(175-181)。
- 影响: 五处都表现为"点了没反应",用户无从判断是卡了还是失败了;其中 ① ④ 直接丢失用户输入。
- 验收: 五处分别补明确反馈——附件无文字时给出提示或允许纯附件发送;设置页与启动链 invoke 失败时可见报错;快记失败保留表单内容;首次需要通知时请求权限并在被拒时说明。
- 优先级: P2
- refs: D-004
- 进展: 完成第⑤项并收口五项验收：crates/kanzei-app/ui/main.js:249-286 新增 ensureNotificationPermission；首次手动 sendText（:1723-1724）在用户手势链路请求 Notification.requestPermission，已授予时沿用 notifyRunState，拒绝/未授予/不支持/请求异常均通过 toast + 运行日志面板明确说明。前四项既有修复位置保持：附件纯附件发送默认描述、loadSettings 错误反馈、启动链逐步捕获、快记提交失败保留表单。既有 sendText、loadSettings、启动 IIFE、快记提交与 notifyRunState 调用方均沿用，无新增命令/按钮。验证：node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。
- 验证: cargo test --workspace 全绿(87 项);node --check crates/kanzei-app/ui/main.js。
- 阶段: 3
- 不变量: 操作反馈:失败反馈在用户确认前持续存在
- 证据等级: E3

## D-088 CLI 会话历史无限累积且无清理入口 [fixed] (medium)
- 复现: 在同一项目里正常使用若干次 kz run,上下文与耗时持续增长,最终撞上下文上限。
- 根因: 每次 kz run 无条件取最新 conversation.updated 全量作为 prior(kanzei/src/main.rs:145-153),runner 以 prior.to_vec() 开局(runner.rs:206),运行结束又把累积后的完整 messages 写回(277-281);usage(47-52)中不存在 reset/new/continue 任何选项。
- 影响: token 成本每次递增、缓存命中下降,最终报错;用户唯一自救手段是手删 state.db,顺带丢掉全部事件。
- 验收: 提供 `kz run --new`(或 kz reset)显式开新会话;补新会话不携带旧 prior 的验证。
- 优先级: P2
- 阶段: 3
- 不变量: 会话控制:上下文增长有清理入口
- 证据等级: E2

- 进展: 完成 D-088 验收：crates/kanzei/src/main.rs:62-83 新增 `kz run --new "<prompt>"` 解析与 usage；run_cli 启动时调用 crates/kanzei-core/src/store.rs:343 附近的 clear_conversation，只删除当前 session 的 conversation.updated 快照，保留 session、调度、权限和生命周期事件，再以空 prior 运行；后续普通 kz run 仍沿用同一 session 继续新上下文。既有 run_cli 调用链沿用，无新命令分支之外的替代实现。新增 CLI 参数解析测试与 SessionStore 清理隔离测试。定向 cargo test -p kanzei-core / -p kanzei 通过；提交前 cargo test --workspace 全绿。

## D-107 侧栏缩放手柄随滚动内容移动,长列表时无法持续调整宽度 [fixed] (low)
- 复现: 侧栏内容超过一屏后向下滚动,再尝试拖动右侧宽度手柄;handle 是 sidebar 的绝对定位子元素,而 sidebar 本身 `overflow-y:auto`,手柄随滚动内容离开可视区。
- 根因: setupResize 把 resize-handle 追加到滚动容器内部,没有把滚动层与固定边框/手柄层分离;同时 pointerdown 未 preventDefault,也没有键盘调整或双击重置。
- 影响: 需要缩放时手柄反而不可达,且拖动可能选中文本;R-074 声称“面板和容器支持缩放拖拽”的核心体验不稳定。
- 验收: 手柄固定在面板边界且不随内容滚动;支持明显 hover/focus、键盘微调和恢复默认宽度;在长侧栏、todo、activity 三类面板验证。
- 优先级: P2
- refs: R-074 R-089
- 阶段: 3
- 不变量: 界面状态:分栏控件在滚动后仍可达
- 证据等级: E3

- 进展: 完成 D-107：crates/kanzei-app/ui/main.js:134-204 的 setupResize 将手柄改为 fixed 视口定位并由 ResizeObserver/resize 同步到面板边界，不再随 sidebar/todo/activity 内容滚动；pointerdown 增加 preventDefault，手柄补 role=separator、focus-visible、ArrowLeft/ArrowRight 键盘微调、Home 恢复默认、双击恢复默认。既有三处调用 setupResize("sidebar"/"todo-panel"/"bg-panel") 沿用，无新命令/按钮。scripts/ui-a11y-smoke.mjs 增加固定手柄和键盘契约检查；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。真实长列表像素回归仍由 R-101 跟进。

## D-075 上下文成分浮层是死功能,状态栏承诺的点击查看无法打开 [fixed] (low)
- 复现: 运行一轮后点状态栏 token 文字(title 写着"点击查看上下文成分"),浮层永不出现。
- 根因: renderContextDetail 只写 innerHTML 从不移除 hidden 类(ui/main.js:811-825),而 `.hidden { display:none !important }`(style.css:329);全项目无其他代码碰 #context-detail(index.html:339)。
- 影响: 承诺的上下文透明功能完全不可用——而"上下文透明"是 G-001 明确的产品方向。即使修好显示也没有关闭路径(无 blur/再点切换),需一并补。
- 优先级: P2
- 阶段: 3
- 不变量: 操作反馈:承诺的入口必须可达
- 证据等级: E3

- 进展: 完成承诺入口与恢复路径：crates/kanzei-app/ui/main.js:1137-1172 保留既有 renderContextDetail 展开逻辑，新增 hideContextDetail/toggleContextDetail；再次点击状态栏 token 可关闭，点击浮层外或按 Escape 关闭，并同步 aria-expanded=false。沿用既有 status-tokens 调用方，无新增命令/按钮。scripts/ui-a11y-smoke.mjs 增加上下文浮层关闭与 Escape 契约；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-077 独立文档页拖拽启用条件读错筛选状态,可提交残缺顺序 [fixed] (low)
- 复现: ①侧栏筛选全为 all 但独立页把状态筛成 doing → 拖拽仍可用,提交不完整的 ID 序;②独立缺陷页选 P1 优先级(status 仍 all)→ 同样可拖拽并提交残缺顺序;③反向:侧栏有筛选时独立页无筛选却拖不了。
- 根因: reqDragEnabled/docDragEnabled(ui/main.js:2039-2046)只读侧栏的 reqFilters,而独立页实际按 documentFilters 过滤(2399-2403),且缺陷分支完全没有判断 documentFilters.defect.priority。代码注释自己写明"order 必须覆盖全部条目,有筛选时禁止拖拽"。
- 影响: 排序是 agent 取活顺序的唯一依据,提交残缺顺序会直接改变后续工作队列。D-032/D-036 的修复未覆盖独立页的筛选来源。
- 优先级: P2
- refs: D-032 D-036
- 阶段: 3
- 不变量: 界面状态:排序提交必须覆盖全部条目
- 证据等级: E3

- 进展: 修复拖拽启用条件：crates/kanzei-app/ui/main.js:2649-2662 的 reqDragEnabled/docDragEnabled 改为读取 renderDocList 传入的实际 filterState；独立页 renderDocuments(:3057-3061) 传 documentFilters.req/defect，因此需求页 status/priority/complexity/sort 任一非全量、缺陷页 status 或 priority 任一筛选时均禁用拖拽；侧栏缺陷仍显式传全量过滤状态。commitDocOrder 与既有 docs_update 调用方沿用，避免提交残缺 order。scripts/ui-a11y-smoke.mjs 增加契约检查；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-079 运行中发送按钮禁用但排队功能存在,鼠标用户不可达 [fixed] (low)
- 复现: 运行中在输入框打字并选好"插入 steer",发送按钮是灰的点不动;但按 Ctrl+Enter 却能成功排队。
- 根因: setRunning(true) 禁用发送按钮(ui/main.js:287),而 sendText 专门实现了运行中的 queue/steer 投递分支(1277-1296),交付方式下拉也常驻可选;键盘路径直接调 send() 完全绕过按钮禁用(1571-1573)。
- 影响: 按钮状态与实际能力矛盾,排队/steer 这个卖点功能对鼠标操作不可达、可发现性为零。
- 验收: 运行中保持发送按钮可用并按交付方式提示"将排队/插队",或明确禁用整条路径(含快捷键)保持一致。
- 优先级: P2
- 阶段: 3
- 不变量: 操作反馈:按钮状态与实际能力一致
- 证据等级: E3

- 进展: 完成验收：crates/kanzei-app/ui/main.js:465-474 的 setRunning 不再因运行状态禁用发送按钮，运行中保留既有 sendText queue/steer 分支；按钮 title/aria-label 明确提示按交付方式插入或排队，发送成功后的既有 toast 继续说明实际投递结果。沿用现有发送按钮、delivery-select 与 sendText 调用方，无新增命令/按钮。scripts/ui-a11y-smoke.mjs 增加运行中发送可用契约；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-089 子代理进度事件在任务完结时未 drain,工具块卡在进行中 [fixed] (low)
- 复现: 子代理在收尾阶段密集发事件时,UI 上偶见 trace 末尾缺块,工具显示"进行中"但任务已结束。
- 根因: kanzei-core/src/runner.rs:455-472 的 select 循环在 jobs.next() 返回 None 时直接 break,此刻 rx 中可能仍有子代理临完成前发出的 TaskProgress(含 ToolEnd trace,703-711)未被消费;select 分支无偏向,完成分支可能先于积压的进度事件被轮询到,break 后 rx 随作用域丢弃。
- 影响: 仅 UI 显示,不影响正确性。
- 验收: break 前用 try_recv 清空缓冲事件。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:活动轨迹与后端事件一致
- 证据等级: E3

- 进展: 完成 D-089：crates/kanzei-core/src/runner.rs:125-133 新增 drain_task_events，子代理 jobs 完成分支在退出前用 rx.try_recv 排空所有已缓冲 RunEvent，再离开 select；新增 runner 单测验证 TaskProgress 缓冲事件确实被消费。既有 task 并行/ToolEnd/UI 事件调用链沿用。定向 cargo test -p kanzei-core 与提交前 cargo test --workspace 全绿；保留既有 final_text unused_assignments warning，非本轮引入。

## D-090 bgEntries/diffSummary 不随 DOM 修剪,长时间运行内存与定时器负载无界增长 [fixed] (low)
- 复现: 一晚上鞭挞连跑数千次工具调用且不切项目/进程。
- 根因: ui/main.js:490-492 的修剪只删 DOM(`list.firstElementChild.remove()`),bgEntries Map(452)与 diffSummary 仅在 bgClear()(切项目/进程)时清空;687-691 的每秒 interval 遍历全 Map,对已脱离 DOM 的 detached 节点持续更新。
- 影响: 内存缓慢增长,detached DOM 持有 diff 大块内容;自用长跑恰是主用例。
- 验收: 修剪 DOM 时同步删除对应 Map 条目;定时器只遍历在册条目。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:长跑不产生无界增长
- 证据等级: E3

- 进展: 完成 D-090：crates/kanzei-app/ui/main.js:777-817 给活动条目写入 data-bg-id，BG_MAX 修剪 DOM 时同步 bgEntries.delete(first.dataset.bgId)；bgClear 同步 diffSummary.clear 并重绘摘要；现有每秒 interval 只遍历仍在 bgEntries Map 中的条目。既有 bgAdd/bgEnd/bgClear 调用方沿用，无新增命令/按钮。scripts/ui-a11y-smoke.mjs 增加 Map/摘要清理契约；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-092 语言切回中文时 title/placeholder 属性停留在英文 [fixed] (low)
- 复现: 设置页语言 zh→en→zh,悬停活动栏图标,tooltip 仍是英文,需重启才恢复。
- 根因: ui/main.js:52-59 文本节点用 WeakMap 存了原文可逆,但属性没有原文存储;切回中文时属性值已是英文,`I18N_EN["Attach"]` 为 undefined 直接跳过。
- 影响: 语言切换不完全可逆。
- 优先级: P3
- 阶段: 3
- 不变量: 操作反馈:文案切换可逆
- 证据等级: E3

- 进展: 当前实现已满足验收：crates/kanzei-app/ui/main.js:70-110 使用 I18N_ATTR_ZH WeakMap 首次保存 title/placeholder 中文原文，英文切换从原文查 I18N_EN，切回中文写回缓存原文，不依赖当前英文值反查；语言切换调用方沿用 applyLanguage。scripts/ui-i18n-smoke.mjs 已覆盖属性原文缓存、稳定保存与动态 key 缺失检查；node --check、ui-i18n/a11y/Markdown 冒烟、git diff --check 通过。

## D-093 标题 🔔 提示只在 visibilitychange 复位,窗口可见时失焦回焦不清除 [fixed] (low)
- 复现: 双屏使用,kanzei 一直可见但焦点在别处,任务完成后回到窗口,标题仍是"🔔 运行完成 · kanzei"。
- 根因: ui/main.js:173-187 设置条件是 `!document.hasFocus() || document.hidden`(失焦即设),但复位只挂在 visibilitychange 上;窗口未被遮挡时失焦→回焦不产生该事件,缺一个 window focus 监听。
- 影响: 陈旧完成提示让人误以为又跑完一轮。
- 优先级: P3
- 阶段: 3
- 不变量: 操作反馈:提示状态不陈旧
- 证据等级: E3

- 进展: 完成修复：crates/kanzei-app/ui/main.js:326-331 抽出 resetTitleOnFocus，同时订阅 visibilitychange 与 window focus；窗口保持可见但失焦后重新获得焦点时会立即清除 🔔 完成标题。notifyRunState 既有调用方沿用。scripts/ui-a11y-smoke.mjs 增加 window focus 契约；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-094 运行中点开历史对话无守卫,流式输出错位嵌入历史视图 [fixed] (low)
- 复现: 运行中随手点一条历史对话回看,当前运行的输出继续追加在历史对话末尾。
- 根因: 历史行点击(ui/main.js:2685-2692)没有 running 守卫(对照"新对话"按钮 2738-2741 有);renderRecoveredMessages 重置 currentAssistant 并清空 messages(2605-2610),kz:text 继续到达时新建气泡追加在历史末尾;loadConversation 里的 bgClear 还会清掉正在跑的活动轨迹。
- 影响: 两段对话混在一起,正在进行的运行轨迹丢失。
- 验收: 运行中点击历史对话给出明确提示或改为只读预览。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:历史只读与实时运行隔离
- 证据等级: E3

- 进展: 完成验收：crates/kanzei-app/ui/main.js:3403-3414 的历史对话行点击在 running 时立即阻断，不调用 loadConversation、不清空当前 messages/bg 轨迹，并通过 toast 明确提示完成或停止后再打开；空闲时沿用既有 loadConversation 历史回放调用方。scripts/ui-a11y-smoke.mjs 增加运行中历史守卫契约，i18n smoke 覆盖新增文案；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-095 refs 跳转在独立文档页失效,特殊字符还会抛异常 [fixed] (low)
- 复现: 在独立文档页展开带 `refs: R-054` 的条目并点击该链接,页面无反应。
- 根因: ui/main.js:2188-2194 用全局 `document.querySelector([data-doc-id="..."])`,同一条目在侧栏与独立页各渲染一份且侧栏在前,而独立视图激活时侧栏副本被 `display:none` 隐藏 → scrollIntoView 对隐藏元素无效、高亮不可见;另 ref 值来自自由文本,含 `"` 或 `]` 时选择器语法错误直接抛未捕获异常。
- 影响: 关联跳转在最适合用它的页面上不工作。
- 验收: 在当前可见容器内查找目标;ref 值需转义后再构造选择器。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:关联跳转在当前视图内生效
- 证据等级: E3

- 进展: 完成验收：crates/kanzei-app/ui/main.js:2832-2838 不再把自由文本 ref 插入 CSS selector，改为遍历 `[data-doc-id]` 后按 dataset.docId 精确比较，特殊引号/方括号不会抛异常；同时要求 target.offsetParent 非空，只在当前可见容器（独立文档页或侧栏可见列表）滚动并高亮。既有 refs button 调用方沿用。scripts/ui-a11y-smoke.mjs 增加可见容器与 dataset 比较契约；node --check、ui-a11y/i18n/Markdown 冒烟、git diff --check 通过。

## D-096 隔离工作树的差异查看只弹一次性 toast,无法阅读 [fixed] (low)
- 复现: 在隔离工作树里改了 20 个文件,点"差异"——几十个文件名塞进 2.6 秒即消失的小气泡,且文本不可选中复制。
- 根因: ui/main.js:1767-1769 直接 `toast(files.join("\n"))`;toast 存活 2600ms(131-138),body 设了 user-select:none 而 #toast 不在可选白名单(style.css:20-22)。
- 影响: 差异查看功能形同虚设,而项目里已有现成的 diff 查看器与文档查看器可复用。
- 验收: 改用应用内查看器展示文件列表与实际 diff 内容。
- 优先级: P2
- refs: R-050
- 阶段: 3
- 不变量: 操作反馈:长结果持久可见可复制
- 证据等级: E3

- 进展: 完成 D-096：crates/kanzei-app/src/main.rs:109-114、855-880 的 WorktreeInfo/worktree_diff 现在返回 git diff 实际内容；crates/kanzei-app/ui/main.js:2328-2338 的“差异”操作将文件列表与实际 diff 写入可持久、可复制的运行日志面板，而不是一次性 toast；未跟踪文件无 git diff 时明确提示。沿用既有 worktree_diff Tauri command（已注册）与 handleWorktreeAction 调用方，无新增按钮/命令。验证：node --check、ui-a11y/i18n/Markdown 冒烟、cargo test -p kanzei-app、提交前 cargo test --workspace、git diff --check 全部通过。

## D-115 上下文压缩把工具循环中的初始用户消息误当当前消息，直接丢弃全部工具轨迹 [fixed] (high)
- 复现: 运行包含工具调用的任务：初始 User → assistant ToolCall → ToolResult；下一轮请求发生上下文超限并进入 compact_messages_for_retry。函数以最后一条含 Text 的 User 消息为 current，ToolResult 虽角色为 User 但无 Text，因此会回到初始 User，压缩前没有可遍历历史，最终只保留初始提示。
- 影响: 超限重试后模型失去已完成的读取/编辑/测试结果与任务进展，可能重复工作、误判状态或给出错误结论。与“压缩保留历史”目标相反。
- 根因: crates/kanzei-core/src/runner.rs:836-866 的 rposition(is_text_user_message) 将初始用户消息当作当前消息；工具结果消息没有 Text，且压缩只收集 current_index 之前的内容，所以工具循环中的所有工具结果都被清空。现有测试只断言不产生孤儿工具 part，没有断言工具结果摘要被保留。
- 验收: 工具循环发生压缩时，当前用户提示和最近有效工具结果均保留在合法的纯文本摘要中；不保留孤儿 ToolCall/ToolResult；新增回归测试证明工具结果未被整体丢弃。
- refs: D-068 R-093
- 优先级: P1

- 进展: 修复 crates/kanzei-core/src/runner.rs:848-879 的 compact_messages_for_retry：仍以最后一条含文本的 User 消息作为当前提示，但摘要遍历当前消息前后全部消息，仅收集 Text/ToolResult，跳过 ToolCall，从而保留工具循环最近结果并避免孤儿工具 part。新增 runner 回归断言：工具循环压缩后仍包含“工具结果”文本，同时不含 ToolCall/ToolResult。定向 cargo test -p kanzei-core 与提交前 cargo test --workspace 全绿；保留既有 final_text unused_assignments warning，非本轮引入。

## D-117 OpenAI Responses 流式嵌套 error 未识别为上下文超限,压缩重试无法触发 [fixed] (high)
- 不变量: 上下文恢复:provider 明确上下文超限必须进入有界压缩路径
- 复现: 长对话后 provider 返回 `{"error":{"code":"context_length_exceeded","message":"Your input exceeds the context window of this model..."},"sequence_number":2,"type":"error"}` 的 Responses SSE error 事件，UI 直接显示 context overflow/context_length_exceeded，不能完成压缩重试。
- 影响: 长对话达到窗口后，用户看到“可压缩重试”但后端未可靠触发压缩恢复，重试仍携带超大历史并再次失败。
- 根因: crates/kanzei-llm/src/protocol/openai_responses.rs 的 error 分支把外层 data["error"] 当作 kind/message，但该协议错误体的 code/message/type 位于嵌套 error 对象中，读取到 unknown/unknown，无法进入 LlmError::ContextOverflow；同时应用层自动压缩只在请求成功后运行，无法弥补本次失败。
- 证据等级: E2
- 阶段: 1
- 验收: Responses SSE 嵌套 error 的 context_length_exceeded/code/message 能分类为 ContextOverflow；runner 触发有界压缩重试；补协议分类回归测试，并运行 kanzei-llm/kanzei-core 定向测试。
- refs: R-093
- 优先级: P0
- 进展: 已修复：Responses SSE `error` 事件从嵌套 `error.code/type/message` 提取 provider 错误；上下文超限 code 也纳入统一分类。新增真实用户错误形状回归测试，确认进入 ContextOverflow（runner 随即执行既有两级有界压缩）。验证：定向测试通过；`cargo test -p kanzei-llm` 33/33 通过；cargo fmt --all -- --check 未通过但仅命中仓库既有未格式化文件，未改动。
- 验证: cargo test -p kanzei-llm protocol::openai_responses::tests::nested_error_context_length_exceeded_triggers_context_overflow；cargo test -p kanzei-llm

## D-118 历史工具轨迹恢复调用未定义 appendDisplayBlock 导致历史消息恢复失败 [fixed] (high)
- 不变量: 历史恢复:回放持久化工具轨迹不得调用未定义的 UI 函数并阻断整段恢复
- 复现: 更新到 build-71c0357 后打开历史对话；`conversation_trace_get` 返回包含工具 trace 的历史记录，`renderRecoveredTraces → bgProgress` 执行 `appendDisplayBlock(child.row, trace.display)`，但 main.js 没有该函数，抛出 `ReferenceError: appendDisplayBlock is not defined`，历史恢复进入 catch。
- 影响: 每次更新后只要历史包含工具轨迹，历史消息恢复失败；用户无法查看历史对话。
- 根因: ui/main.js 的 `bgProgress` 已引用 `appendDisplayBlock`，但该 helper 未定义；`bgEnd` 仅内联处理 display，新增/迁移回放路径没有共享实现。
- 证据等级: E2
- 阶段: 3
- 验收: 实现 appendDisplayBlock，支持 diff/terminal/create 三类 display；历史 trace 回放不抛 ReferenceError；补静态冒烟契约与 node --check。
- refs: R-093
- 优先级: P0
- 进展: 已修复：在 ui/main.js 增加 appendDisplayBlock，共享渲染 diff/terminal/create 三类历史工具展示；bgProgress 回放路径与 bgEnd 完成路径统一调用。scripts/ui-a11y-smoke.mjs 增加函数存在与回放调用契约，防止再次发布未定义 helper。验证：node --check main.js、ui-a11y/i18n/Markdown 三个冒烟全部通过。
- 验证: node --check crates/kanzei-app/ui/main.js；node scripts/ui-a11y-smoke.mjs；node scripts/ui-i18n-smoke.mjs；node scripts/ui-markdown-smoke.mjs

## D-120 继续文案无法改变实际取活顺序,需求优先模式未生效 [fixed] (high)
- 不变量: 取活顺序:用户选择的需求优先/缺陷优先必须同时约束前端继续提示与后端 agent/system context
- 复现: 在继续文案中写“先做 requirements.md”，使用 dev-auto 鞭挞；后端 dev agent system prompt、project-docs 和 NUDGE_PROMPT 仍固定 defects-first，实际继续取缺陷。
- 影响: 用户界面显示的工作策略与实际执行策略不一致，需求优先模式无法使用。
- 根因: 继续文案只是用户消息，未进入后端工作策略；profiles.rs 固定注入 Defects are the first development queue，dev system prompt 固定 Pick work defect-first，前端 NUDGE_PROMPT 也固定 defects.md。
- 证据等级: E2
- 阶段: 1
- 验收: 提供需求优先/缺陷优先切换控件；模式按项目持久化；自动推进、手动继续和无动作追加使用所选顺序；后端 dev system/context 同步所选模式；默认行为保持缺陷优先；补前端/后端回归验证并发布。
- refs: R-093
- 优先级: P0
- 进展: 已完成并发布：新增按项目保存的 work-priority-select（缺陷优先/需求优先），前端继续提示、无动作追加和 run_prompt 均传递所选模式；后端 dev profile 去除固定 defects-first 冲突文本，run_task 将本轮模式追加到 agent system 指令，未传模式默认 defect-first。验证：前端四项检查、cargo test -p kanzei-tools、cargo test -p kanzei-app、cargo test --workspace 全部通过；安装包已发布 build-18d4932。
- 验证: node --check main.js；ui-a11y/i18n/Markdown smoke；cargo test -p kanzei-tools；cargo test -p kanzei-app；cargo test --workspace

## D-126 图片附件发送后无状态反馈,用户无法确认 agent 是否收到图片 [fixed] (high)
- 不变量: 附件输入:用户可见状态与 provider 输入状态一致，发送后的图片必须可追踪并进入 Part::Image
- 复现: 在桌面端选择/粘贴图片后发送：输入区附件 chip 被清空，用户消息只显示文字，无法确认是否发送；运行状态没有显示附件接收/转换；虽有前端附件参数，但没有可见证据证明 agent 收到图像。
- 影响: 用户无法判断图片是否成功发送，容易误以为 agent 读取到了图片并继续下达错误任务；图片相关失败缺少可恢复反馈。
- 根因: ui/main.js 发送用户消息只渲染 prompt，发送后丢失附件展示；main.rs 虽将 PromptAttachment 转为 Part::Image，但未发送附件接收/转换状态事件，也没有对应可见验证。
- 证据等级: E2
- 阶段: 3
- 验收: 发送后用户消息显示附件文件名/类型与已附加状态；运行状态显示已接收并转换为图片输入；补图片到 Part::Image 的自动化断言或协议测试；图片与 PDF 仍按当前 provider 协议正确发送。
- refs: R-093
- 优先级: P0
- 进展: 已修复：发送后的用户消息保留附件文件名、类型和“已发送给 agent”状态；发送阶段显示附件数量；后端转换完成后发送“已接收并转换为图片/文档输入，准备发送给 agent”状态；抽取 prompt_attachment_parts 并补 image/PDF → Part::Image/Document 回归测试。验证：cargo test --workspace 17 项 app 测试包含附件断言，全量通过；前端三个 UI 冒烟通过。
- 验证: cargo test --workspace；cargo test -p kanzei-app prompt_attachments_become_image_and_document_parts；node --check main.js；ui-a11y/i18n/Markdown smoke

## D-119 继续文章/按钮收纳逻辑及视觉显示问题 [fixed] (high)
- 原始描述: 继续文章和继续按钮不应该同时收纳，应该独立，而且继续文案的白色底北京啥都看不到，框也很窄不能缩放
- 复现: 1.查看'继续文章'和'继续按钮'功能 2.观察文案白色背景可读性 3.测试容器宽度能否缩放
- 优先级: P2
- 进展: 已完成：继续按钮从继续文案编辑面板移到 composer-actions，按钮独立始终可见；继续文案编辑区改为占满剩余宽度，最大高度提高到 140px，700px 以下窄屏自动改为上下布局；补 UI 冒烟契约验证按钮与编辑区独立。验证：node --check main.js、ui-a11y/i18n/Markdown 三个冒烟全部通过。

- 标签: continue_prompt,grouping,readability
- 类型: ux
- 领域: ui,interaction,layout

- 验证: node --check crates/kanzei-app/ui/main.js；node scripts/ui-a11y-smoke.mjs；node scripts/ui-i18n-smoke.mjs；node scripts/ui-markdown-smoke.mjs

## D-127 需求缺陷独立页面无法滚动且缺少项目切换，新对话入口被收纳 [fixed] (medium)
- 不变量: 文档视图内容可滚动且始终绑定当前项目；新对话入口无需展开菜单即可发现
- 复现: 打开需求与工作/缺陷独立页面：列表区域没有独立滚动样式，长列表无法访问末尾；页面没有项目切换控件；新对话只能从“更多”菜单进入。
- 证据等级: E1
- 阶段: 3
- 验收: 独立页面长列表可滚动；可在该页切换已登记项目并刷新文档；顶栏直接显示新对话按钮。
- 优先级: P1
- 进展: 已修复：独立工作区/文档容器补齐 flex、min-height 和 overflow-y 滚动；独立文档页新增当前登记项目选择器并复用项目切换后的会话/文档刷新；新对话移出“更多”菜单并以顶栏 primary 按钮显示。
- 验证: node --check crates/kanzei-app/ui/main.js；node scripts/ui-a11y-smoke.mjs；node scripts/ui-markdown-smoke.mjs；node scripts/ui-i18n-smoke.mjs 均通过。

## D-128 前端运行时冒烟 harness 缺少 select.options 契约 [fixed] (medium)
- 复现: 运行 `node scripts/ui-runtime-smoke.mjs`，在 renderDocsSnapshot → syncTagFilter 处报 `TypeError: select.options is not iterable`，随后需求/缺陷/目标/测试/历史列表均未渲染。
- 来源: R-084/R-109 前端冒烟验证
- 标签: 流程
- 根因: ui-runtime-smoke.mjs 的 Element 桩没有实现 select.options 集合；真实浏览器 select 有该属性，最小 DOM harness 未覆盖，导致验证脚本自身在初始化阶段崩溃。
- 验收: 补齐 select/options 的最小 DOM 契约；运行时冒烟通过并继续断言需求、缺陷、目标、测试和历史列表非空；node --check 通过。
- refs: R-084 R-109
- 修复位置: scripts/ui-runtime-smoke.mjs:79-80，Element.options getter。
- 进展: 已补齐 scripts/ui-runtime-smoke.mjs 的 Element.options 最小 DOM 契约，select.options 现在返回子 option 集合，未改变真实前端行为。
- 验证: node --check crates/kanzei-app/ui/main.js；node scripts/ui-runtime-smoke.mjs 通过：main.js 全量执行、12 次 invoke 初始化、需求/缺陷/目标/测试/历史列表渲染，0 个运行时错误。

## D-129 动态错误状态切换语言后无法恢复中文 [fixed] (medium)
- 复现: 运行 `node scripts/ui-runtime-smoke.mjs`，切换 English，触发 kz:error，live-turn 显示 Error；再切回中文，live-turn 仍为 Error 而非“出错”。
- 来源: D-108 动态 i18n 回归验证
- 标签: 前端
- 根因: 动态状态写入 DOM 时只保存当前语言文本，语言切换仅遍历已有中文基线节点；动态文案没有保存源 key/中文值，无法反向恢复。
- 验收: 动态错误状态 English↔中文切换可逆；运行时冒烟覆盖该路径并通过，node --check 通过。
- refs: D-108 R-084
- 修复位置: crates/kanzei-app/ui/main.js:动态 i18n helper、setStatus、liveSet/liveIdle/liveTurn、语言切换回调；scripts/ui-runtime-smoke.mjs:语言切换与 kz:error 动态状态断言。
- 进展: 已修复：动态状态保存中文 source，语言切换时统一重算 status/live/context/auto 文案；动态错误状态不再把 English 结果当作中文源。并补齐动态文案片段翻译、文档页筛选选项和受控标签的运行时翻译。
- 验证: node --check crates/kanzei-app/ui/main.js；node scripts/ui-runtime-smoke.mjs 通过：I18N_EN t key 缺失检查、中文↔English document.lang 切换、kz:error 动态状态 English/中文可逆、列表渲染和 0 运行时错误。

## D-108 英文模式只翻译少量静态节点,动态状态与操作反馈长期中英混杂 [fixed] (medium)
- 复现: 设置语言为 English,创建/切换项目、运行任务、打开文档/设置并触发 toast;静态导航的一部分变英文,动态生成的状态、日志、错误、按钮和 300 余处中文字符串仍保持中文。再切回中文还会触发 D-092 的属性不可逆问题。
- 根因: applyLanguage 只遍历当前 DOM 文本节点和少量 title/placeholder,I18N_EN 仅覆盖有限字典;后续 JS 动态生成的文本不会经过翻译函数,也没有以 key 为中心的统一文案层。
- 影响: 英文模式无法作为完整产品能力使用,错误与权限等高风险信息尤其容易出现语义断层;R-069 原验收“所有产品/功能文案”未满足却被归档 done。
- 验收: 所有用户可见文案由稳定 key 生成,动态内容与属性同源且可逆;中英文分别跑页面/操作快照,不得出现非用户数据导致的混合语言;补缺失 key 检查。
- 优先级: P3
- refs: D-092 R-069
- 阶段: 3
- 不变量: 操作反馈:文案进入统一 i18n 资源
- 证据等级: E3

- 进展: 第 1 批运行态反馈完成后，本批补齐动态节点闭环：applyLanguage 现在为动态 text/属性保存中文 source，MutationObserver 自动处理后续渲染节点，语言切换会重新投影 status/live/context/auto 文案；I18N_DYNAMIC_EN 覆盖运行、队列、工作树、进程、项目、文档、记忆、设置、更新等操作反馈片段。所有 `t("...")` 调用均有字典 key，动态冒烟验证 English↔中文可逆。用户输入、模型/ provider 名称、路径、日志原文、文档正文等数据保持原文，不作为产品文案翻译。

- 标签: 前端

- 验证: node --check crates/kanzei-app/ui/main.js 通过；node scripts/ui-runtime-smoke.mjs 通过：I18N key 检查、初始化 15 次 invoke、需求/缺陷/目标/测试/历史列表、语言来回切换、kz:error 动态状态回归、0 运行时错误。

- 修复位置: crates/kanzei-app/ui/main.js:92-219 I18N_DYNAMIC_EN；222-286 localizeDynamic/applyLanguage/MutationObserver；约 500-600 setStatus；约 1250-1300 live source map 与语言同步；约 1360-1500 上下文/运行事件；约 3290-3330 文档动态筛选与标签；scripts/ui-runtime-smoke.mjs:缺失 key 检查、语言切换和 kz:error 回归。
- 验收对照: ①稳定 key：现有 I18N_EN + I18N_DYNAMIC_EN，smoke 对 376 个 t 调用做缺失 key 检查；②动态/属性同源可逆：I18N_ZH/I18N_ATTR_ZH 保存 source，MutationObserver 覆盖后续节点，动态 status/live/error English↔中文回归通过；③中英文操作：smoke 切换 document.lang、触发 kz:error、验证 English Error 与中文“出错”恢复；④非用户数据不混杂：运行反馈、筛选/标签、上下文账单和操作反馈均经 t/localizeDynamic，用户输入/模型/provider/路径/原始日志保持数据原文。

## D-130 CLI 退出码回归测试插入破坏既有测试结构 [fixed] (low)
- 复现: 在 D-121 首次定向 cargo test 时，crates/kanzei/src/main.rs 测试模块报 unexpected closing delimiter；新增退出码测试插入时误删 run_new_flag_is_removed_from_prompt 函数签名。
- 来源: D-121 定向回归
- 标签: 流程
- 根因: edit 替换锚点包含既有测试函数签名，新测试插入时将签名一并替换。
- 验收: 恢复测试函数结构后 cargo test -p kanzei halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero 通过，相关既有测试可编译。
- refs: D-121
- 修复位置: crates/kanzei/src/main.rs 测试模块：恢复 run_new_flag_is_removed_from_prompt 函数签名并保留退出码测试。
- 进展: 已恢复既有测试结构，未改变 D-121 生产逻辑。
- 验证: cargo test -p kanzei halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero 通过。

## D-131 权限拒绝退出码变更后集成测试仍断言成功 [fixed] (low)
- 复现: D-121 实现后 cargo test --workspace 在 crates/kanzei/tests/always_allow_bash.rs::cli_declined_permission_persists_paired_tool_results 失败，测试仍断言 output.status.success()。
- 来源: D-121 全工作区回归
- 标签: 核心
- 根因: 生产语义已将权限拒绝映射为退出码 3，既有集成测试仍按旧的退出码 0 契约断言。
- 验收: 集成测试断言权限拒绝退出码为 3；正常恢复对话仍为 0；cargo test --workspace 全绿。
- refs: D-121
- 修复位置: crates/kanzei/tests/always_allow_bash.rs:194-216，权限拒绝场景改为断言退出码 Some(3)，保留 server.await 与副作用/历史配对断言。
- 进展: 已同步集成测试到 D-121 新退出码契约。
- 验证: cargo test -p kanzei --test always_allow_bash cli_declined_permission_persists_paired_tool_results 通过。

## D-121 kz run 被权限拦停后退出码仍为 0,自动化调用方无法感知失败 [fixed] (medium)
- 复现: 非交互终端执行 kz run(stdin 为管道),agent 首个 bash 调用触发权限询问,read_line 读到 EOF 判 Deny,输出 "(stopped: permission declined)" 后进程 exit 0。
- 根因: run_cli 的 halted_by_user 分支只打印提示即正常返回 Ok(crates/kanzei/src/main.rs:343-345),权限拦停与正常完成共用同一退出语义。
- 影响: 脚本/CI/其他 agent 调用 kz 时无法区分"完成"与"被拦停",按成功继续会基于半途结果做后续动作。
- 验收: halted_by_user 以非零退出码结束(如 3)并在 stderr 说明;正常完成保持 0;补退出码断言测试。
- 优先级: P2
- refs: R-102
- 阶段: 1
- 不变量: 会话控制:终态语义在退出码可见
- 证据等级: E2
- 备注: 2026-08-07 用 kz 做前端分析实测发现(用户确认立项)。

- 标签: 核心

- 进展: 已完成：权限拒绝/用户 halted 路径向自动化调用方返回退出码 3，stderr 保留 `(stopped: permission declined)`；正常完成路径不改变为 0；持久化会话、工具配对和副作用断言保持原测试。D-131 已同步并 fixed。

- 修复位置: crates/kanzei/src/main.rs:55-60 增加 cli_exit_code；run_cli 收尾在 episode/记忆整理后对 halted_by_user 调用 process::exit(3)，正常完成保持 Ok(0)。crates/kanzei/tests/always_allow_bash.rs:权限拒绝集成场景断言 Some(3)。
- 验收对照: ①halted_by_user 非零：cli_exit_code(true)=3 且实际 run_cli 收尾 process::exit(3)；②stderr 原因：现有 eprintln 明确输出 permission declined；③正常完成 0：cli_exit_code(false)=0 并走 Ok；④调用方：always_allow_bash 集成测试真实 spawn kz，拒绝场景断言退出码 3、恢复场景断言 success。
- 验证: cargo test -p kanzei halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero 通过；cargo test -p kanzei --test always_allow_bash cli_declined_permission_persists_paired_tool_results 通过；cargo test --workspace 全绿。

## D-132 D-122 规则分类测试断言未同步扩展夹具 [fixed] (low)
- 复现: D-122 定向测试首次运行时 config::tests::legacy_bash_rules_are_detected_without_rewriting_config 断言 legacy.len()==1，但测试夹具新增了显式 bash/* 规则，实际 legacy_bash_rules() 返回 2。
- 来源: D-122 定向回归
- 标签: 流程
- 根因: 为覆盖两类规则扩展测试夹具后，既有 legacy_bash_rules() 保持兼容语义但断言未同步。
- 验收: 测试明确区分 legacy_bash_rules 总数、需降级数量和显式 wildcard allow 数量，定向测试通过。
- refs: D-122
- 修复位置: crates/kanzei-harness/src/config.rs:559-567，断言 legacy_bash_rules 总数为 2，同时独立断言需降级 1、显式 wildcard allow 1、告警两条。
- 进展: 已同步扩展后的两类规则夹具，保持旧 API 语义并验证新分类 API。
- 验证: cargo test -p kanzei-harness config::tests::legacy_bash_rules_are_detected_without_rewriting_config 通过。

## D-122 裸 bash 通配规则的降级告警与实际放行行为不一致 [fixed] (low)
- 复现: 项目 kanzei.toml 写 `action="bash", resource="*", effect="allow"`;kz 启动提示"检测到 1 条旧 bash 权限规则;将降级为逐次询问",随后整轮 bash 全部直接放行,一次也没询问。
- 根因: legacy_bash_rules 把所有非 command/workdir JSON 的 bash 规则识别为旧规则并告警,但评估路径对用户显式配置的整体 `*` 保留 yolo 语义——告警文案与评估行为出自两套判断。
- 影响: 用户被告知会逐次询问,实际却全量放行:轻则困惑,重则误以为有询问兜底而放松了对通配规则的警惕。
- 验收: 告警与评估行为一致——显式 `*` 要么如实提示"将全量放行(yolo)",要么真的降级询问;补两种规则形态的启动提示测试。
- 优先级: P3
- refs: D-051
- 阶段: 1
- 不变量: 权限:授权范围精确可解释
- 证据等级: E2
- 备注: 2026-08-07 kz 前端分析实测发现(用户确认立项)。

- 标签: 核心

- 进展: 已完成：裸 legacy bash 规则提示“将逐次询问”；显式 bash/* allow 单独提示“将全量放行(yolo)”；结构化 bash 规则不进入 legacy 告警。CLI 与桌面端沿用同一配置分类函数，避免告警/评估再次分叉。D-132 已 fixed。

- 修复位置: crates/kanzei-harness/src/config.rs:114-151 新增 legacy_bash_rules_needing_downgrade、explicit_bash_wildcard_allows、bash_permission_warnings；crates/kanzei/src/main.rs:91-93 与 crates/kanzei-app/src/main.rs:3517-3519 统一消费共享告警。保留 permission.rs 对显式 bash/* 的既有 Allow 语义。
- 验收对照: ①告警与评估一致：显式 bash/* 继续由既有 Ruleset Allow，启动明确提示全量放行；②裸规则提示逐次询问；③结构化 JSON 规则不告警；④CLI/桌面两个调用方均使用 bash_permission_warnings；⑤补两类规则配置测试。
- 验证: cargo test -p kanzei-harness config::tests::legacy_bash_rules_are_detected_without_rewriting_config；permission 显式整体放行测试；cargo test -p kanzei --test always_allow_bash；cargo test -p kanzei-app；cargo test --workspace 全绿。

## D-133 帮助文案测试插入破坏既有 CLI 测试结构 [fixed] (low)
- 复现: D-123 定向 cargo test 编译报 unexpected closing delimiter；新增 usage 单测插入时误删既有 halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero 函数签名。
- 来源: D-123 定向回归
- 标签: 流程
- 根因: edit 以既有测试函数签名为替换锚点插入新测试，导致签名丢失。
- 验收: 恢复既有测试函数结构后 usage 单测和 D-121 退出码单测均通过。
- refs: D-123 D-121
- 修复位置: crates/kanzei/src/main.rs 测试模块：恢复 halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero 函数签名。
- 进展: 已恢复既有退出码回归测试结构，保持 D-121 覆盖。
- 验证: cargo test -p kanzei usage_lists_agent_profile_and_model_selection；cargo test -p kanzei halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero 均通过。

## D-123 usage 与 --help 不展示 agent/模型/profile 选择方式,能力不可发现 [fixed] (low)
- 复现: kz --help 只列出 run 与 tracker 子命令及一行 env 提示;KANZEI_AGENT 可选值(dev/dev-pair/research)、KANZEI_MODEL 语法、profile 档位含义均无从查起。
- 根因: usage() 是 5 行手写文本(crates/kanzei/src/main.rs:55-60),agent 清单在 harness 快照里,帮助文本不读取它。
- 影响: 只有读过源码的人知道怎么切 agent;dev-pair 这类对话型入口实际不可发现。
- 验收: --help 列出可用 agent(名称+一句话用途)、KANZEI_MODEL 语法示例与 profile 说明;或提供 kz agents 列表命令。
- 优先级: P3
- 阶段: 1
- 不变量: 操作反馈:能力可自发现
- 证据等级: E2
- 备注: 2026-08-07 kz 前端分析实测发现(用户确认立项)。

- 标签: 核心

- 进展: 已完成：沿用现有 --help/help/无参数 usage 调用方，扩充能力发现信息；未新增独立命令或重复配置解析。D-133 已 fixed。

- 修复位置: crates/kanzei/src/main.rs:55-72 新增 usage_text/usage 输出；帮助现在列出 dev/dev-pair/research agent、KANZEI_PROFILE、KANZEI_AGENT、KANZEI_MODEL=<role|provider:model> 示例和 KANZEI_PROXY。
- 验收对照: ①--help 列 agent 名称与用途；②列 profile 说明；③列 KANZEI_MODEL 语法与 primary/fast、ollama:qwen3.5:4b 示例；④现有 usage 入口均复用新文本。
- 验证: cargo test -p kanzei usage_lists_agent_profile_and_model_selection；cargo test -p kanzei halted_run_uses_nonzero_exit_code_but_completed_run_stays_zero；cargo test --workspace 全绿。

## D-125 独立文档页列表末尾与底部状态栏重叠,末行被遮挡不可读 [fixed] (low)
- 复现: 打开「需求与工作/缺陷」独立管理页,列表滚到底:最后一行(截图中为 P0 的 E2 harness 条目)与底部状态栏重叠,文字被状态栏盖住;列表容器无底部安全间距。
- 影响: 排在末尾的条目(常是新加的)难以阅读与点击;用户 2026-08-08 截图反馈。
- 验收: 列表容器 padding-bottom 预留状态栏高度(或滚动区域计算扣除状态栏);滚到底时末行完整可见可点;顺带核查同布局的其余独立页。
- 优先级: P3
- 阶段: 3
- 不变量: 界面状态:内容不被固定元素遮挡
- 证据等级: E3

- 标签: 前端

- 进展: 已完成：crates/kanzei-app/ui/style.css 的 #documents-scroll 增加 64px 底部安全区（状态栏高度 24px），并核查同布局 workspace-scroll 仍保留 40px 底部间距；scripts/ui-runtime-smoke.mjs 增加 CSS 回归断言，避免安全区回退。验收逐条对照：列表容器预留状态栏高度，滚到底末行可越过状态栏；独立文档页与工作区滚动布局均已核查。验证：node --check crates/kanzei-app/ui/main.js；node --check scripts/ui-runtime-smoke.mjs；node scripts/ui-runtime-smoke.mjs 通过。
- 验收证据: crates/kanzei-app/ui/style.css:#documents-scroll；scripts/ui-runtime-smoke.mjs:documentsScrollRules 安全间距断言
- 验证: node --check crates/kanzei-app/ui/main.js；node --check scripts/ui-runtime-smoke.mjs；node scripts/ui-runtime-smoke.mjs

## D-134 活动面板重复展示主对话工具调用且历史回放省略完整工具会话 [fixed] (medium)
- 不变量: 界面状态:同一信息只在适合的视图呈现,历史对话内容可追溯
- 复现: 运行包含工具调用的对话：同一工具同时出现在主对话和活动面板；切换/恢复历史会话后，工具调用只显示工具名，调用输入与工具结果不可查看。
- 影响: 活动面板被大量重复工具调用淹没；用户无法在历史主对话中核对完整调用上下文和结果。
- 标签: 前端
- 根因: ui/main.js 的 kz:tool-start 同时调用 chatToolStart 与 bgAdd；renderRecoveredMessages 对 tool_call 只生成工具名 chip，跳过 input 与 ToolResult content。
- 证据等级: E3
- 阶段: 3
- 验收: 活动面板默认只展示 memory_note 等记忆写入和 task 子代理轨迹；主对话工具调用默认折叠但保留可展开的完整 input/result；历史回放不丢失 tool_call/tool_result 内容，并有 UI 冒烟回归。
- 优先级: P2
- 进展: 已完成：ui/main.js 新增 isActivityTool，仅让 task 子代理与 memory_note 记忆写入进入活动面板；普通工具调用仍进入主对话内联块，不再重复进入活动。历史回放新增可展开 replay-tool，保留 tool_call.input 与 tool_result.content/is_error/call_id；默认 details 折叠，完整内容可展开查看。style.css 增加详情滚动与等宽全文样式。
- 验收证据: crates/kanzei-app/ui/main.js:isActivityTool、appendReplayToolPart、renderRecoveredMessages；crates/kanzei-app/ui/style.css:.replay-tool-body；scripts/ui-runtime-smoke.mjs:活动过滤与完整工具会话回归断言
- 验证: node --check crates/kanzei-app/ui/main.js；node --check scripts/ui-runtime-smoke.mjs；node scripts/ui-runtime-smoke.mjs 均通过

## D-135 英文模式把用户数据与文件路径当产品文案改写 [fixed] (high)
- 复现: 切到 English,让 agent 输出或回显含中文的文件路径/用户原话,如 `crates/前端/模型.md`;localizeDynamic 的子串替换把它改写成 `crates/Frontend/Model.md`。
- 根因: 458af45 为铺满覆盖面,用 I18N_DYNAMIC_EN 对整段自由文本做无边界子串替换(main.js localizeDynamic),而该函数被用于渲染用户输入、模型输出与路径等非产品文案;"模型""前端"这类词既是 UI 文案也是普通词汇。
- 影响: 用户数据在展示层被静默篡改,复制出去的路径是错的;属于内容正确性问题,比中英混杂严重得多。
- 验收: localizeDynamic 只作用于产品文案节点(白名单),或整体改为 key 化渲染;用户输入/模型输出/路径一律不过替换;补"含中文的路径与用户原话在英文模式下逐字保留"的回归。
- 优先级: P1
- refs: D-108
- 标签: 前端
- 阶段: 3
- 不变量: 操作反馈:展示层不得篡改用户数据
- 证据等级: E3
- 备注: 2026-08-08 多代理审查发现并经独立求证。
- 进展: 已修复:localizeDynamic 改为整串命中优先 + 逐词"独立出现"判定,命中处紧邻路径分隔符或 ASCII 标识符字符即跳过;crates/前端/模型.md 不再被改写。

## D-136 动态 title/placeholder 被首见缓存永久冻结,语言与状态都不再更新 [fixed] (medium)
- 复现: 折叠左侧栏后,该按钮 tooltip 永远停在「折叠左侧栏」,不随状态变为「展开左侧栏」;英文模式同理不更新。
- 根因: I18N_ATTR_ZH 以首次见到的属性值为准缓存原文,此后 applyLanguage 每次都用这份旧原文回写,覆盖掉运行时新设置的属性值。
- 影响: 所有动态 title/placeholder 都被钉死在第一次的取值上,状态提示长期说谎。
- 验收: 属性缓存改为"写入方更新时同步刷新原文",或改为按 key 渲染;补"状态变化后 title 跟随更新、且中英切换可逆"的回归。
- 优先级: P2
- refs: D-092 D-108
- 标签: 前端
- 阶段: 3
- 不变量: 界面状态:属性展示是当前状态的投影
- 证据等级: E3
- 进展: 已修复:属性原文缓存跟随写入方更新——当前值既不是缓存原文也不是其译文时以新值为准,不再冻结在首见值。

## D-137 活动面板 diff 汇总被改成空壳,接不到数据源 [fixed] (medium)
- 复现: 让 agent 改几个文件,活动面板的差异汇总始终为空。
- 根因: 8cc03be 把普通工具调用从活动面板过滤掉时,连带切断了 #diff-summary 的数据来源(该汇总原先靠 write/edit 的 tool-end 累计),但 DOM 与切换按钮仍在。
- 影响: 已上线功能静默失效;且缺陷记录、验收证据、新增冒烟断言三处都没发现——正是"只建展示壳"的反向版本。
- 验收: diff 汇总改为独立于活动面板过滤的数据通路(如直接订阅 tool-end 的 display.kind==="diff");补"改文件后汇总非空"的冒烟断言,否则删掉这个死 UI。
- 优先级: P2
- refs: D-090
- 标签: 前端
- 阶段: 3
- 不变量: 界面状态:展示壳必须有真实数据源
- 证据等级: E3
- 进展: 已修复:抽出 recordDiffSummary 由 kz:tool-end 直接订阅,独立于活动面板过滤;bgEnd 只保留面板内标注。

## D-138 运行时冒烟的"主视图切换"覆盖恒为 0 却仍判通过 [fixed] (medium)
- 复现: 跑 node scripts/ui-runtime-smoke.mjs,成功行打印「0 个主视图切换」,脚本仍退出 0。
- 根因: harness 只按 id 造节点,.activity-item 没有 id,querySelectorAll(".activity-item") 恒为空;循环体一次都不执行而断言不检查条数。
- 影响: documents/memory/活动面板等主视图在冒烟里从未被执行过,该项护栏形同虚设——与它自己在初始化探针上做的自守卫(注入没匹配上即失败)标准不一致。
- 验收: harness 支持按 class 造节点或直接解析 index.html 的 .activity-item;冒烟对"切换数为 0"直接判失败(照抄同文件探针自守卫的写法)。
- 优先级: P2
- refs: R-084
- 标签: 流程
- 阶段: 2
- 不变量: 验证:护栏覆盖为零时必须报错而不是报通过
- 证据等级: E2
- 进展: 已修复:根因是 harness 的 body 未挂到 documentElement,按 class 的选择器一直在空树上走;补挂后再按 class 造 .activity-item,并对覆盖数不足直接判失败。护栏打开后立刻抓到 settings 视图从未被执行(缺 #providers-table tbody),一并补齐。实测 0→5 个视图切换、invoke 16→24。

## D-139 bash 通配告警与权限评估仍不一致,且回归测试断言了矛盾行为 [fixed] (medium)
- 复现: 项目配置同时含裸 bash legacy 规则与显式 `bash/* = allow`;启动提示"将逐次询问",但 last-match-wins 让一切直接放行,一次都不问。
- 根因: 14c30b9 只按规则形态分别计数出文案,从不咨询 Ruleset::evaluate;单一形态下文案恰好对,混合形态下即说假话。同源分叉两处:legacy 计数不看 effect(deny 护栏被算成"将逐次询问",实为死规则);trim()=="*" 与 pattern=="*" 判定不一致。
- 影响: 用户被告知有询问兜底而实际全量放行,是安全边界上的错误告知;更糟的是关闭该缺陷的测试用的正是混合夹具,把矛盾行为断言进了回归网。
- 验收: 告警由实际评估结果推导(对代表性命令跑 evaluate 后再措辞),而非按规则形态猜;修正 legacy 计数忽略 effect 与两处通配判定不一致;把混合形态的矛盾断言改为断言真实行为。
- 优先级: P2
- refs: D-122 D-051
- 标签: 核心
- 阶段: 1
- 不变量: 权限:授权范围精确可解释
- 证据等级: E2
- 进展: 已修复:bash_permission_warnings 改为先用代表性命令跑 Ruleset::evaluate,以真实判定措辞;deny 护栏不再被算作降级;通配判定统一走 is_wildcard_resource。旧测试里那对矛盾断言(同一夹具既称逐次询问又称全量放行)改为断言真实行为。

## D-140 D-112 的完整性告警是 warn-only,ID 丢失复发数轮无人处理 [fixed] (high)
- 复现: R-104/107/108/110 曾在活动与归档文档中同时消失并持续 5 个提交;tracker 每次调用都打印了缺号告警,但无人处理,最终靠 2f1cd7e 用失效前的旧副本偶然捞回,既未写进提交信息也未立缺陷。
- 根因: D-112 落地的 integrity_issues 只把告警拼进工具输出文本(warn-only),模型可以完全忽略;没有任何机制把"检测到数据丢失"升级为必须处理的状态。
- 影响: P0 不变量"追踪条目不因归档流程丢失"已经复发过一次,而且是靠运气恢复的;告警连响数轮等于没有。
- 验收: 检测到缺号/重复时,tracker 的写操作返回错误而非仅告警(读操作仍放行),迫使当轮处理;或在轮末把完整性失败升级为可见的运行失败;补"缺号状态下写操作被拒"的回归。
- 优先级: P1
- refs: D-112 D-130
- 标签: 核心
- 阶段: 1
- 不变量: 配置与文档:追踪条目不因归档流程丢失
- 证据等级: E2
- 进展: 已修复:tracker 在缺号/重复时拒绝一切写操作(add/update/close/archive/reorder),读操作照常放行以便诊断,错误文本直接给出 git 恢复路径,避免变成死锁。
- 备注: D-130(空行膨胀)是它长期没被发现的直接原因:那次恢复的 391 行新增里 334 行是空行,把数据恢复彻底埋掉。D-130 已修。

## D-141 鞭挞自动续轮在对话中重复展示内部提示词 [fixed] (medium)
- 不变量: 界面:内部控制事件不伪装成用户输入
- 复现: 开启顶栏“鞭挞”后，自动续轮会把完整的“继续推进...”提示作为用户消息追加到对话，同时顶栏状态、运行状态和日志又重复显示鞭挞轮次。
- 影响: 对话内容被内部控制提示污染，用户看到重复的鞭挞信息，难以分辨真实输入和自动触发。
- 标签: 前端
- 根因: sendText 的 auto 分支复用 addUserMessage 展示完整自动提示词，展示层没有区分用户真实输入与内部自动续轮事件。
- 证据等级: E2
- 进展: 已修复 crates/kanzei-app/ui/main.js:sendText auto 分支：自动续轮继续发送原提示词，但对话区改为仅显示“鞭挞已触发 · 当前轮次”，手动发送仍沿用 addUserMessage。
- 阶段: 3
- 验收: 自动续轮仍发送原提示词并保持轮次状态；对话区只显示一条简洁的“鞭挞已触发/第 N 轮”说明，不再展示完整继续提示词；手动发送显示不变；补前端冒烟断言。

- 验证: node --check crates/kanzei-app/ui/main.js；ui-runtime-smoke、ui-i18n-smoke、ui-a11y-smoke、ui-markdown-smoke 全部通过。

## D-142 鞭挞触发提示未纳入静态英文 i18n 词典 [fixed] (medium)
- 不变量: 前端动态 i18n：所有 t() 文案键必须存在 I18N_EN 源词典
- 复现: 将鞭挞触发提示加入动态词典后运行 scripts/ui-i18n-smoke.mjs，断言报告“鞭挞已触发”未进入 I18N_EN。
- 影响: 新增提示在英文模式下无法被静态词典完整验收，动态 i18n 回归会失败。
- 标签: 前端
- 根因: 新增 t("鞭挞已触发") 只写入 I18N_DYNAMIC_EN，而 ui-i18n-smoke.mjs 约束所有 t() 调用键必须位于 I18N_EN。
- 证据等级: E2
- 进展: 已将“鞭挞已触发”纳入 I18N_EN，并移除动态词典中的重复项，保持英文/中文切换契约一致。
- 阶段: 2
- 验收: ui-i18n-smoke.mjs 通过，英文模式显示 Auto-run triggered，中文模式保留鞭挞已触发。
- 验证: ui-i18n-smoke 通过：50 项资源与动态入口契约已覆盖。

## D-143 用户手动输入新问题后鞭挞未自动停止 [fixed] (medium)
- 不变量: 界面:用户主动输入优先于自动推进
- 复现: 鞭挞自动续跑期间，用户在输入框提交新的问题后，自动续跑开关和后续定时任务仍可能保持启用，下一轮继续自动触发。
- 影响: 用户无法用新问题明确接管当前会话，自动推进可能与用户意图重复或冲突。
- 标签: 前端
- 根因: 手动 send() 没有取消 autoContinueTimer、复位 autoRounds 或关闭 auto-continue 状态。
- 证据等级: E2
- 进展: 已修复 crates/kanzei-app/ui/main.js:send：手动文字或附件提交时调用 stopAutoForManualInput，关闭开关、清除本地持久化、取消定时器、复位轮次，并以 notice/toast/log 明确反馈；当前运行任务仍按既有 queue/steer 继续。
- 阶段: 3
- 验收: 手动发送文字或仅附件时，鞭挞立即关闭、取消定时器、弹出明确提示；当前正在运行的任务仍按既有 queue/steer 语义处理；自动续轮不再触发。

- 验证: node --check main.js；node --check ui-runtime-smoke.mjs；ui-runtime-smoke、ui-i18n-smoke、ui-a11y-smoke、ui-markdown-smoke 全部通过。

## D-144 主输入框 Enter 未发送而是换行 [fixed] (medium)
- 不变量: 界面:输入框默认提交符合常用终端对话习惯
- 复现: 在主问题输入框按 Enter，当前行为插入换行；只有 Ctrl/Cmd+Enter 才发送。
- 影响: 与 Codex 等常用对话工具习惯不一致，用户需要额外记住快捷键才能提交。
- 标签: 前端
- 根因: crates/kanzei-app/ui/main.js:promptBox keydown 只处理 Ctrl/Cmd+Enter，没有处理无修饰 Enter 提交。
- 证据等级: E2
- 进展: 已修复 crates/kanzei-app/ui/main.js:promptBox keydown：无修饰 Enter 提交，Shift+Enter 换行，Ctrl/Cmd+Enter 保持兼容；文件补全候选选择逻辑优先。运行时冒烟新增键盘契约断言。
- 阶段: 3
- 验收: Enter 发送；Shift+Enter 保留换行；Ctrl/Cmd+Enter 继续发送；文件补全选择优先级不变。
- 验证: node --check main.js；node --check ui-runtime-smoke.mjs；ui-runtime-smoke、ui-i18n-smoke、ui-a11y-smoke、ui-markdown-smoke 全部通过。

## D-145 发布脚本安装到错误的 kzapp 路径导致运行旧版本 [fixed] (medium)
- 不变量: 发布:构建、安装和实际启动路径必须一致
- 复现: scripts/release.ps1 将 release 构建复制到 %USERPROFILE%\.cargo\bin\kzapp.exe，但实际运行的桌面端位于 %LOCALAPPDATA%\kanzei\kzapp.exe，发布后 UI 仍显示旧构建 hash 91e8d22。
- 影响: 发布成功但用户继续运行旧版本，版本号/hash 与代码 HEAD 不一致，容易误判发布失败。
- 标签: 发布
- 根因: 发布脚本固定使用 cargo bin 作为桌面端安装目标，而实际桌面启动器/更新链使用 LocalAppData\kanzei。
- 证据等级: E3
- 进展: 方案已确定并实现：桌面端目标统一为 %LOCALAPPDATA%\kanzei\kzapp.exe；~\.cargo\bin 仅保留 kz CLI，并在发布时清理历史 kzapp.exe/pending 副本。待重新发布并核对运行实例 app_info。
- 进展(补全与验收): e5c8555 补两处遗漏——①删掉 cargo bin 的 kzapp.exe 会让终端 `kzapp` 直接失效(conventions §9 明写过该能力),补 kzapp.cmd 转发启动器,且必须装在 try 之前(app 运行时 catch 会 throw,放后面永远装不上);②补 Get-FileHash 逐字节校验,安装位与本次构建不一致直接 throw,残留 cargo bin 副本时明确告警。启动器注释改英文(.cmd 按 OEM 代码页读,中文必乱码)。
- 验证: 实跑 release.ps1 端到端——cargo bin\kzapp.exe 已清理、AppData 为唯一 exe、哈希与本次构建一致、`cmd /c kzapp` 实测启动的正是 AppData 那一份。
- 阶段: 1
- 验收: release.ps1 将 kzapp 安装到实际运行路径；旧 cargo bin\kzapp.exe 被安全清理；重新发布后运行实例 app_info 显示当前 HEAD hash；kz CLI 仍保留在 cargo bin。

## D-147 tracker list 未按阻塞、未完成依赖和阶段门槛调度队列 [fixed] (medium)
- 复现: 调用 req list 或 defect list 时，工具直接按 Markdown 文件顺序返回条目；含“阻塞”、未完成“依赖”或“阶段: 5 后”的条目仍会挡在可执行条目前面。
- 影响: 模型按队列取活时会被明确阻塞项挡住，解除阻塞后的原始顺序也没有自动恢复语义。
- 标签: 流程
- 根因: crates/kanzei-tools/src/tracker.rs 的 list 分支仅 render_line，未计算阻塞原因和稳定后置。
- 阶段: 0
- 验收: req/defect list 识别显式阻塞、未完成依赖和阶段后置门槛，稳定把阻塞项放到可执行项之后并输出原因；解除后按原文档顺序恢复；有回归测试。
- 进展: 已修复并验证：req/defect list 动作通过 TrackerTool 统一计算阻塞原因并稳定后置，解除后按原文档顺序恢复；原因随输出返回。回归测试覆盖显式阻塞、未完成依赖、阶段门槛和解除。
- 验证: cargo test -p kanzei-tools 56 passed

## D-146 应用内更新按 hash 不相等判定,会把本地较新的开发构建降级回上一个 release [fixed] (high)
- 复现: 用 scripts/release.ps1 装一个尚未发布的开发构建(如 e5c8555),启动桌面端;3 秒后的静默检查发现最新 release 标签是 build-91e8d22,与本地 KANZEI_BUILD_INFO 的 hash 不相等,于是判定"有新版"并安装——本地被回滚到更旧的已发布版本。
- 证据: 2026-08-08 实录。5:15 release.ps1 把 21,270,016B 的构建装进 %LOCALAPPDATA%\kanzei;5:22:21 uninstall.exe 时间戳显示 NSIS 安装器运行;此后安装位变成 21,332,480B/04:37(build-91e8d22 的载荷),哈希与本次构建不一致。用户随后打开看到的仍是旧版。
- 根因: update_check 只比较 `tag_name` 与当前 build hash 是否相等,把"不相等"当作"有新版";开发构建天然领先于最新 release,因此必然被判定为需要更新。
- 影响: 自举机上"装了新版→它自己滚回旧版"静默发生,排查方向完全错误(以为发布没生效);且与 D-145 叠加时更难定位。
- 验收: 判定改为"仅当 release 确实更新才提示"(比较发布时间与本地构建日期,或在构建信息里带上可比较的序号);本地构建领先于最新 release 时不提示更新;补该场景的判定单测。
- 优先级: P1
- refs: D-124 D-145
- 标签: 发布
- 阶段: 1
- 不变量: 版本与更新:更新只前进不后退
- 证据等级: E3

- 进展: 已修复：update_check 不再把 hash 不相等直接当作升级；读取 GitHub release 的 published_at/created_at，与本地构建时间比较。release/package 脚本改为注入 UTC yyyyMMddHHmmss；旧 yyyy-MM-dd 构建采用必须晚一天的保守规则，缺少可信时间不提示更新。新增回归覆盖本地较新构建、较新 release、同 hash 和旧日期格式。

- 验证: crates/kanzei-app/src/main.rs:update_tests::release_check_never_downgrades_a_newer_local_build、legacy_date_only_build_requires_a_later_release_day；cargo test -p kanzei-app update_tests 16 项；cargo test --workspace 全绿；PowerShell Parser 检查 scripts/package.ps1 与 scripts/release.ps1

## D-163 自记阻塞只进不出,鞭挞把整个队列锁死后静默停机 [fixed] (high)
- 复现: 2026-08-08 用户实测:侧边栏需求/缺陷仍有大量条目,鞭挞却连续两轮回复"当前没有可执行条目...本轮停止"。核对数据:requirements.md 32 条中 28 条、defects.md 3 条全部带 `阻塞:` 字段,`req list` 可执行数为 0。
- 影响: 自举完全停摆且表现为"没活干"这一无害假象,用户看不出是 bug;单靠提示词纪律无法自愈,因为下一轮读到的仍是满屏 [blocked]。
- 标签: 流程
- 根因: 三处叠加。①conventions §1.1 明文授权"记录阻塞原因后跳过",而 §1 的"高影响改动须先确认方案"被套用到几乎每一条(28 条里约 21 条是同一句模板"涉及跨层/数据模型改动,需先确认方案"——实际从未真的问过用户);②鞭挞提示词规则 3 说"在条目里记一句原因",模型自然落成 `阻塞:` 字段,而 tracker 的 `block_reasons` 把任何非空阻塞字段当**永久**压制,无复核、无过期;③§1.1 末句"所有需求都 blocked 时明确说明原因并停止空转"把死锁写成了合法终态。写阻塞是 1 次工具调用且能正当结束一轮,做事要 20 次还可能失败——激励梯度全指向声明阻塞,于是逐轮累积到全队列锁死。另有 R-085 → R-084 → R-085 的真循环依赖,调度器只报"未完成依赖",永远等不到。
- 阶段: 1
- 验收: ①`list` 在可执行数为 0 时输出 `[调度死锁]` 横幅,要求先复核阻塞再升级给用户,并禁止"没有可执行条目"式收尾;②循环依赖被点名为 `循环依赖: A → B → A` 并要求断边,不再伪装成普通未完成依赖;③conventions §1.1 把 `阻塞:` 收窄到有具名解除人的外部阻塞,"需先确认方案(但没真问过)"明确不算;④鞭挞提示词与 nudge 同步,卡住写「进展」而非「阻塞」,并要求顺手清理已失效阻塞;⑤存量伪阻塞清理完毕。

- 优先级: P0
- 不变量: 调度:可执行数为 0 是异常状态,不是合法终态
- 证据等级: E2
- 备注: 落地位置 crates/kanzei-tools/src/tracker.rs(deadlock_banner、DependencyStates::cycle_from、block_reasons)、crates/kanzei-app/ui/main.js(DEFAULT_CONTINUE_PROMPT 规则 3、NUDGE_PROMPT,旧默认已入 LEGACY_CONTINUE_PROMPTS 静默升级)、conventions.md §1.1。回归:list_banners_deadlock_when_nothing_is_executable、list_names_dependency_cycles_instead_of_endless_waiting。存量清理 22 条伪阻塞(21 需求 + D-114)并断开 R-085→R-084 环,清理后可执行数 需求 11 / 缺陷 1。

## D-148 首次请求未统一清洗 prior 历史可能把孤儿 tool_result 发送给 provider [fixed] (high)
- 不变量: 发送给 provider 的消息历史不存在孤儿 tool_result/tool_call 配对。
- 复现: 调用 run_once_with_parts 时传入包含无对应 ToolCall 的 ToolResult 历史，且首次请求未触发 ContextOverflow。当前函数直接复制 prior 并发送，未经过 filter_message_history。
- 来源: R-087
- 标签: 核心
- 根因: crates/kanzei-core/src/runner.rs 的 run_once_with_parts 初始化 messages 时仅 clone prior；历史清洗只由调用方或压缩路径间接触发。
- 证据等级: E2
- 阶段: 1
- 验收: 首次请求前统一清洗 prior；新增回归测试证明孤儿 ToolResult 不进入 provider 请求，同时合法 ToolCall/ToolResult 配对保留。
- refs: R-087
- 优先级: P1
- 进展: 实现已完成：run_once_with_parts 在首次请求前统一调用 filter_message_history 清洗 prior。cargo test -p kanzei-core、cargo test -p kanzei-tools、cargo test --workspace 全部通过；配对与孤儿清洗回归覆盖。
- 验收证据: crates/kanzei-core/src/runner.rs:492；crates/kanzei-core/src/history.rs:7-71及其三项单测；cargo test --workspace

## D-164 侧栏条目编辑表单无字段名且截断长值,继续文案框只露两行 [fixed] (medium)
- 不变量: 前端:可编辑控件必须能看出改的是什么
- 复现: 2026-08-08 用户实测截图。①展开 R-097 → 编辑区是一片没有任何标题的输入框,默认 inline-block 两两成行像张表格,「大」「P2」「kanzei」「E2」这些值看不出属于哪个字段;`内容`/`验收`/`进展` 这类段落字段被塞进单行 input,只能看到开头十几个字,等于盲改;「保存修改」直接贴在最后一格右侧压住内容。②底部「继续文案」textarea 是 rows=2,而默认继续提示词有十几行,只能看到最后两行。
- 标签: 前端
- 根因: 三处。①`renderDocList` 的编辑表单只给输入框设 `aria-label`/`title`(tooltip),没有可见标签,而 `.doc-edit` 在 style.css 里**完全没有样式**,全靠浏览器默认排版;②控件一律用 `<input>`,不区分短字段与段落字段;③`#continue-panel` 的响应式规则被破坏:c65c80e(自举循环的 SOP 提交)把 `@media (max-width: 700px) {` 这一行替换成了 `#sop-picker-panel {`,导致媒体查询体变成孤儿规则、末尾多出一个游离的 `}`,`grid-template-columns: 1fr` 于是无条件生效。浏览器对花括号错配静默容错,没有任何报错。
- 证据等级: E3
- 阶段: 3
- 验收: ①每个可编辑字段都有可见字段名;②长字段(>60 字符或含换行)渲染为可纵向拉伸的 textarea,行数按内容估算;③保存按钮独占一行右对齐,不压内容;④继续文案框整宽、rows=6、可纵向拉伸;⑤修复 style.css 的孤儿括号;⑥冒烟脚本能挡住这两类回归(缺字段名/长字段用单行框、CSS 括号错配)。
- 优先级: P2

- 影响: 需求/缺陷的侧栏编辑实际不可用——看不出在改哪个字段,长字段改了会截断丢内容;继续文案要靠滚动两行的窗口编辑十几行提示词。CSS 那处属于 agent 编辑锚点撞车造成的静默结构损坏,同类改动还会再犯。
- 备注: 落地位置 crates/kanzei-app/ui/main.js(addRow 按值长度选 input/textarea,doc-edit-row 带可见 key)、ui/style.css(.doc-edit 系列样式 + #continue-panel 改纵向 + 删孤儿括号)、ui/index.html(continue-prompt rows 2→6)。冒烟新增:字段名非空、长字段为 textarea、CSS 括号平衡三项断言,其中括号检查已用注入多余 } 反验会失败。

## D-149 继续文案面板 CSS 与 UI 冒烟布局契约不一致 [fixed] (medium)
- 不变量: 前端自动化验收脚本与实际布局契约一致，继续文案区域在宽屏使用可读的标签+编辑区布局，并在窄屏保持可用。
- 复现: 运行 node scripts/ui-a11y-smoke.mjs；第 57 项断言失败，要求 #continue-panel 存在 grid-template-columns: auto minmax(0, 1fr)，但 style.css 当前仅为 flex-direction: column。
- 来源: R-089
- 标签: 前端
- 根因: R-089/D-148 后继续文案区改为纵向 flex，但既有 UI 无障碍冒烟仍锁定标签与 textarea 的双列布局契约，CSS 与验收脚本漂移。
- 证据等级: E2
- 阶段: 3
- 验收: 修复 #continue-panel 宽屏双列布局与窄屏单列降级；node --check、ui-runtime-smoke、ui-a11y-smoke、ui-i18n-smoke、ui-markdown-smoke 全部通过。
- refs: R-089
- 优先级: P1
- 进展: 修复已验证完成。
- 验收证据: crates/kanzei-app/ui/style.css:#continue-panel 双列/窄屏单列；scripts/ui-a11y-smoke.mjs 新增布局与操作层级回归；四项 UI smoke 与 node --check 全部通过。

## D-150 权限与文档查看弹窗缺少 dialog 语义及 Escape 键盘关闭 [fixed] (medium)
- 不变量: 键盘用户可通过稳定焦点、Enter/Space/Escape 完成主导航、权限/问题处理和弹窗关闭；弹窗在可访问性树中明确为 dialog。
- 复现: 静态核查 crates/kanzei-app/ui/index.html：#ask-overlay 与 #viewer-overlay 没有 role=dialog/aria-modal/aria-labelledby；main.js 仅为查看器点击关闭和问题 Enter 应答，缺少弹窗 Escape 关闭/权限弹窗键盘关闭路径，权限弹窗打开时也不主动聚焦按钮。
- 来源: R-091
- 标签: 前端
- 根因: 弹窗结构和关闭行为只按鼠标点击实现，未建立统一键盘/ARIA 契约。
- 证据等级: E2
- 阶段: 3
- 验收: ask/viewer 弹窗补 dialog 语义、ARIA 关联、Escape 关闭；打开权限弹窗时焦点落到可操作控件；新增 UI a11y smoke 断言并通过 node --check 与四项 UI smoke。
- refs: R-091
- 优先级: P1
- 进展: 修复验证完成。
- 验收证据: crates/kanzei-app/ui/index.html:454,483 dialog 语义；crates/kanzei-app/ui/main.js:1839、1882、4349 键盘焦点与 Escape；scripts/ui-a11y-smoke.mjs:18-23 断言；node --check 与四项 UI smoke 全部通过。

## D-165 条目展开详情把每个字段渲染两遍,阻塞字段三遍 [fixed] (medium)
- 不变量: 前端:同一份数据只呈现一次
- 复现: 2026-08-08 用户实测截图,展开 R-095:先是编辑表单(标签/阻塞…带值),接着「阻塞原因」框里一条 `• 阻塞字段: 验收仅写"优化终端和工具体验…"`,再往下只读字段列表又把 `原始描述/归属/验收/优先级/标签/阻塞` 全列一遍——`阻塞` 那段长文出现三次,整个面板要滚很久才看得完。
- 标签: 前端
- 根因: 26a2dc4 往展开详情里加编辑表单时,没有撤掉原有的只读 `.doc-field` 列表,两者渲染的是同一份 `entry.fields`;而调度器给的阻塞理由 `阻塞字段: X` 本身就是 `阻塞` 字段的原文,于是又叠了一层。
- 证据等级: E3
- 阶段: 3
- 验收: 有编辑表单时不再渲染只读字段副本(refs 例外,它是可跳转链接);阻塞原因只保留调度器推导的理由(未完成依赖/阶段门槛/循环依赖),`阻塞字段:` 这类与字段重复的理由不再显示;推导理由为空时整个阻塞框不出现;冒烟能挡住重复渲染回归。
- refs: D-148
- 优先级: P2

- 影响: 详情面板信息密度极低,长字段的条目要翻三倍长度才读完,且看不出哪份是可编辑的真源。
- 备注: 落地位置 crates/kanzei-app/ui/main.js(renderDocList 详情段 hasEditor 分支)。冒烟新增「字段在编辑表单之外又渲染了一遍只读副本」断言。

## D-061 OAuth 凭证无锁读改写且非原子覆盖,与官方 CLI 共享文件可致登录态失效 [fixed] (high)
- 复现: 两个 kanzei 进程(或 kanzei 与 Claude Code CLI)在令牌过期窗口内并发发起请求。
- 根因: kanzei-llm/src/auth/claude.rs:28-95、auth/codex.rs:20-101 的流程是 read_to_string → 判断过期 → POST 刷新 → `std::fs::write` 覆盖,无文件锁、无 tmp+rename 原子替换、无写前重读。这两个文件(~/.claude/.credentials.json、~/.codex/auth.json)同时被官方 CLI 读写。
- 影响: 双方各自用同一 refresh_token 刷新,而 OAuth 轮换 refresh token,后到者 invalid_grant,且先到者写入的新 token 可能被并发方以旧内容覆盖回去,登录态永久失效并殃及官方 CLI,需手动重新登录;truncate-then-write 中途崩溃会留下半截 JSON,下次解析直接报"请重新登录"。access token 约 1 小时一刷,窗口频繁。
- 验收: 刷新前后加文件锁,写入改为写临时文件再 rename 原子替换,写前重读校验;补并发刷新不互相覆盖的测试。
- 优先级: P1
- 阶段: 1
- 不变量: 配置与文档:多文件变更原子提交
- 证据等级: E2
- 方案定调: 2026-08-08 用户选定「原子替换 + 写前重读」,明确不加跨进程文件锁——官方 CLI 不参与任何锁协议,锁只能拦住 kanzei 自己的多进程,收益有限还多一份卡死风险。
- 修复: 新增 crates/kanzei-llm/src/auth/store.rs:①`atomic_write` 写同目录带 pid 的临时文件再 rename 覆盖(跨卷不原子,故不用系统 temp),rename 被占用时重试 5 次,彻底失败则删临时文件并保持原文件不动,绝不退回 truncate-then-write;②`commit` 落盘前重读磁盘,若对方已抢先刷新(Claude 比 `claudeAiOauth.expiresAt`、Codex 比 `last_refresh`)就采纳对方结果并返回,调用方改用返回值构造请求头——因为自己手里的 refresh_token 可能已被轮换作废。claude.rs 与 codex.rs 的 `std::fs::write` 全部改走 commit。
- 验证: 4 项定向回归——并发刷新不用旧令牌覆盖新令牌、自己更新时正常落盘且不留临时文件、磁盘半截 JSON 时照常恢复写入、Codex 按 last_refresh 判新旧;cargo test -p kanzei-llm 37 项通过。
- 残余: 两个 kanzei 进程同时刷新仍可能各自发一次刷新请求(先到者成功、后到者拿 invalid_grant 后重试即可恢复),这属于请求冗余而非数据损坏,不再单开条目。

- 标签: 模型

## D-124 应用内更新不先退出自身,安装必败且僵尸安装器锁死后续重试 [fixed] (high)
- 复现: 2026-08-08 0:17 实录:app 运行中点「下载并安装」→ 安装器无法替换正在运行的 kzapp.exe,界面报 "另一个程序正在使用此文件。(os error 32)";失败的 kanzei-setup.exe 进程(%TEMP%,PID 15036)不退出,持续握着 kzapp.exe 句柄;用户重启 app 后重试仍报同一错误,直到手动杀掉僵尸安装器。
- 根因: 更新流程是"下载 setup → 直接运行",没有"退出自身再交给安装器"的交接;NSIS 遇文件占用时挂在隐藏对话框而非失败退出,进程成为僵尸;重试路径也不检测/清理既有安装器进程与 %TEMP% 残件。
- 影响: 应用内更新在最常见场景(app 开着点更新)必然失败,且首次失败后连关闭重开都救不回,普通用户会卡死在 os error 32,只能求助或手杀进程。
- 验收: 更新流程改为"下载校验 → 启动安装器(静默)→ 立即退出自身",安装器带完成后自启;启动更新前检测并清理残留的 kanzei-setup 进程与临时文件;失败时报错含可操作指引;补更新交接的可测覆盖(至少流程状态机单测)。
- 优先级: P0
- refs: D-121
- 阶段: 1
- 不变量: 版本与更新:更新流程可靠交接,失败可自愈
- 证据等级: E3
- 备注: 手动恢复路径已验证:杀僵尸安装器 → 温和关闭 app → 静默装 setup → 重启,已把用户从 c2cf358 更到 79e532b。

- 标签: 发布

- 方案定调: 2026-08-08 用户拍板按验收完整实现退出交接。
- 修复: crates/kanzei-app/src/main.rs。①`validate_installer` 校验下载物(≥1 MB 且以 MZ 开头),挡住代理返回的 HTML 错误页与截断响应——否则要等 app 已经退出才发现装不上;②`clear_stale_installer` 更新前 taskkill 残留 kanzei-setup.exe 并删 %TEMP% 旧包,清理结果并入返回提示;③新增 `--kz-install-helper` 模式(与既有 `--kz-update-helper` 同一套路):helper 轮询等发起方进程真正退出(上限 30 秒)再多让 600ms,然后以 `/S` 静默运行安装器,成功后拉起新 kzapp 并删安装包,失败则保留安装包供手动执行并打印退出码;④`update_install` 派生 helper 后调用 `app.exit(0)` 立刻让出镜像句柄。
- 验证: 2 项定向回归——安装包校验拒绝截断与非 PE 载荷、helper 在发起方未退出时绝不放安装器出去(实测等满 30 秒且不删安装包);cargo test -p kanzei-app 23 项通过。
- 残余: helper 等待上限 30 秒后仍会继续(而非放弃),因为发起方极端情况下可能卡住不退;此时安装器自身的占用检测会兜底失败并保留安装包,不会产生僵尸。

## D-114 自举运行验证节奏低效:git 查询过密、全量测试时机不当、已知位置缺陷仍派子代理 [fixed] (low)
- 复现: 2026-08-07 完整落库轨迹:30 次终端调用中约 13 组 git status/diff/show 密集重复且常一次塞多条;文件仍处换行损坏时跑过全工作区测试;D-082 单文件已知函数缺陷启动子代理,28 次内部读查后因网络错误失败返回,主 agent 重查一遍。
- 根因: dev 提示词无验证节奏与子代理适用边界约束;runner/工具层对重复查询、无变化重测、已知位置探索无任何检测。
- 影响: 单轮约 14~18 次可避免的终端调用(占 47%~60%);重复输出稀释上下文,推高 token 成本与轮次时长。
- 验收: 提示词纪律落地(已完成);R-099 度量显示同类任务终端调用数与 edit 未命中率显著下降;若提示词不足以收敛,按 R-100 落 runner 层机械提醒。
- 关闭说明(2026-08-08): 本条与 R-099 构成语义死环——本条验收要 R-099 的度量数据,R-099 的 `依赖` 又指着本条,两边都永远等不到。按 §1.2 可用即关闭:纪律部分(鞭挞提示词粒度规则、conventions §1.3 验证匹配改动面、dev 提示词测试选择、编辑门禁)已全部落地且在用,这就是本条的核心行为。剩下的"度量与基线对比"本来就是 R-099 的完整范围,不是本条的残余——已在 R-099 补全统计口径,并解除其对本条的依赖。若度量数据将来证明纪律不足以收敛,按 R-100 走机械门禁,不必重开本条。
- 优先级: P2
- refs: D-113 R-099 R-100
- 阶段: 1
- 不变量: 工具:每次调用都有信息增量
- 证据等级: E2
- 进展: 开始完整收口：在已有提示词/编辑门禁纪律基础上，补轮末可导出的调用统计（终端调用、edit 未命中、git 查询组、task 及内部调用、每工具计数），让 R-099 能取得连续可比基线；先定位 RunSummary/轮末落库调用链，再补定向回归。
- 进展(追加): 鞭挞 14~17 轮暴露微切片新浪费:D-108 每轮只翻 2~3 处文案(拖到 34 步),且纯前端改动每轮跑全量 cargo test。按用户定调落地:①鞭挞默认提示词规则 2 改为「一轮一个完整条目,同构批量改动一轮吃完整类别」,旧默认进 LEGACY_CONTINUE_PROMPTS 静默升级;②conventions §1.3 增粒度与「验证匹配改动面」规范;③dev 提示词同步测试选择规则。生效需重装 kzapp(前端与提示词打包在二进制内)。

- 标签: 流程

## D-166 条目引用点击跳转静默失败,归档条目永远跳不到 [fixed] (medium)
- 不变量: 前端:可点击的东西必须有反馈
- 复现: 2026-08-08 用户实测"点击跳转的连通性要修复"。展开任一条目详情,点 refs 里的引用链接:目标若被筛选掉、所在分区折叠、侧栏整体收起、或已经归档,点击后**没有任何反应**——不滚动、不高亮、不提示,看起来就是死链。归档条目是重灾区:被引用的条目多半正是已完成归档的那些。
- 标签: 前端
- 根因: 跳转实现是 `[...document.querySelectorAll("[data-doc-id]")].find(item => item.dataset.docId === ref && item.offsetParent !== null)`。`offsetParent !== null` 要求目标此刻可见,上述四种情况一律匹配不到;匹配不到后代码用可选链 `target?.scrollIntoView()` 直接跳过,不报错也不提示。另外归档条目渲染成纯 div,根本没挂 `data-doc-id`,即使展开归档区也找不到。
- 证据等级: E3
- 阶段: 3
- 验收: ①跳转不再要求目标当前可见:被折叠的归档区、被折叠的侧栏分区、收起的侧栏都会自动掀开后再定位;②归档条目挂 `data-doc-id`,可被引用跳转命中;③同一条目同时存在于侧栏与独立文档页时,优先跳当前视图里的那个;④目标确实不存在时给出 toast 提示,禁止静默失败;⑤只掀开确实会藏住条目的两类容器(归档折叠区、侧栏分区),不对任意祖先去 hidden——那会顺手展开整个视图。
- refs: D-149 R-123
- 优先级: P2

- 影响: refs 字段的价值归零——条目之间的依赖与关联关系点不动,只能手动翻文件对照。而这轮刚做完的依赖梳理正是靠 refs 承载关联的。
- 备注: 落地位置 crates/kanzei-app/ui/main.js 新增 `jumpToEntry`,归档行补 `dataset.docId`。冒烟新增四项断言:归档条目挂 id、归档区默认折叠、跳转后自动掀开并高亮、跳不存在的 ID 给出提示。

## D-151 冒烟 harness 对 class 结构失明,按 class 的断言长期假通过 [fixed] (high)
- 复现: 2026-08-08 做 R-123 时发现。冒烟里 `document.querySelectorAll(".documents-list .doc-item[data-doc-id]")` 恒返回 0,而 `#documents-req-list .doc-item` 返回 2——同一批节点,只是换了按 class 找就找不到。
- 根因: 三处叠加。①harness 按 `id="..."` 正则造节点时**只取 id,完全不读 class 属性**,index.html 里写死的 class 一个都没进 DOM;②`setAttribute("class", ...)` 直接写 `_attributes` 而不经过 className setter,ClassList 的内部集合与属性脱节,于是第一次 `classList.toggle()` 回写就把已有 class 整体抹掉;③选择器引擎的 `matchesOne` 把复合选择器整段当一个 class 名比,`.doc-item[data-doc-id]` 这种写法恒不命中,而 main.js 里到处是这种写法。
- 影响: 比"漏测"更糟——**假通过**。任何依赖 class 结构的断言(视图切换、分组、面板显隐、列表内元素定位)都在悄悄返回空集然后判定通过,历史上"UI 冒烟通过"的可信度因此被高估。修复后同一份脚本的初始化 invoke 数从 35 涨到 39,说明之前有整段代码路径根本没被执行到。
- 验收: ①按 id 造节点时同时取回 class;②setAttribute("class") 走 className setter,保持 ClassList 与属性同源;③选择器支持复合形式(`.a.b`、`.a[attr]`、`div.a`),closest 同步;④修复后既有断言全部仍通过,且新增的按 class 断言能真实生效。
- 优先级: P1
- refs: R-084 R-101 R-123
- 阶段: 2
- 不变量: 测试:护栏必须真的会失败
- 证据等级: E2
- 备注: 落地位置 scripts/ui-runtime-smoke.mjs(节点构造正则、setAttribute、matchesCompound)。这类"护栏形同虚设"与 D-138 同源,属同一类问题的第二次发作。
- 标签: 流程

## D-152 单元测试在开发机上执行伪造安装器,弹出「与 64 位 Windows 不兼容」 [fixed] (high)
- 原标题: Windows 安装包在 64 位 Windows 上提示不兼容(误判,见下)
- 复现: 跑 `cargo test -p kanzei-app` 时 Windows 弹出"由于与64位版本的Windows不兼容，此程序或功能无法运行"，路径 `AppData\Local\Temp\kz-helper-*\kanzei-setup.exe`。
- 标签: 发布
- 误判澄清: **发布产物本身没有问题**。①报错路径里的 `kz-helper-<pid>` 是 `install_helper_waits_for_the_caller_to_exit_before_installing` 这条单测建的临时目录,不是真安装包所在的 `%TEMP%\kanzei-setup.exe`;②实测 dist/kanzei-setup-430d6d6.exe 的 PE 头 machine=0x14c(32 位),这是 NSIS 的常规形态——32 位安装器在 64 位 Windows 上经 WoW64 正常运行,并负责安装 64 位负载,产物名里的 x64 指的是负载架构而非安装器自身;③本会话已用该安装包成功静默安装四次(exit 0,安装后构建号逐次核验一致)。
- 真实根因: 上述单测为了验证"helper 必须等发起方退出",让 `run_install_helper` 跑完整流程,而它写进临时目录的是一个 23 字节的假 exe(`MZ not-a-real-installer`);等待结束后 helper 真的去 `Command::new(installer).arg("/S")` 执行它,Windows 无法把它当作有效映像加载,于是报架构不兼容。**测试不该在开发机上启动伪造可执行文件**。
- 修复: 抽出 `wait_for_parent_exit(pid, timeout)` 纯时序函数,单测只验这条不变量(父进程活着时等满超时、父进程不存在时立即放行),不再触碰执行环节;整条 helper 的执行分支不进单测。副作用:该测试从 30 秒降到约 1 秒。
- 验收: cargo test -p kanzei-app 通过且不再弹出任何 Windows 对话框;发布安装包架构核验记录在案。
- 优先级: P0
- 证据等级: E2
- 不变量: 测试:不得在开发机上产生真实副作用
- refs: D-124

## D-153 图标位混入彩色 emoji,与单色图标集不成套 [fixed] (low)
- 复现: 2026-08-08 用户截图两处。①活动栏「运行画像」是 📊,而同栏的对话/设置是描边 SVG、工作区/文档/记忆是 ⌂ ☷ ❖ 单色字形,彩色 emoji 夹在中间像贴上去的;②历史对话标题栏的批量删除是 🗑,而同排的 ＋ ↻ ↗ ✎ 全是单色字形。
- 根因: 整套图标词汇本是单色描边字形/SVG(⌂ ☷ ❖ ✦ ＋ ↻ ↗ ▽ ☰ ✎ ⑂ ⧉ ⌫ ✕),但没有任何机制约束"图标位只能用单色"。R-127 加运行画像入口时顺手用了 📊,🗑 则是更早就在的。彩色 emoji 由系统字体渲染,颜色、粗细、基线都与其余图标对不齐。
- 影响: 纯观感,但正是这类细节让界面显得没收拾过;且无人拦截,后续每加一个入口都可能再犯。
- 验收: ①两处换成同风格描边 SVG(fill=none, stroke=currentColor, stroke-width=1.6),`.icon-btn svg` 与文字字形等高;②tooltip 文案里引用 🗑 的表述改为「标题栏的删除图标」,中英文同步;③冒烟机械挡住——扫 `activity-item` 与 `icon-btn` 的内容,出现 U+1F000–U+1FAFF 即失败,并对扫描到的按钮数设下限,防止正则与标记脱节后变成空跑。
- 边界: 只管图标位。正文里的语义标记(💬 实时提示、⚠ 完整性告警、🔔 窗口标题提醒、📎 附件)不在此列——那些是行内文字标记,emoji 在那里是合理的。
- 优先级: P3
- 阶段: 3
- 不变量: 前端:图标位只用单色字形或描边 SVG
- 证据等级: E3
- 备注: 护栏已反验:把 📊 塞回活动栏,冒烟报「图标位出现彩色 emoji」并失败,还原后通过。
- refs: R-127 R-123
- 标签: 前端

## D-154 深色界面里原生勾选框是白底 [fixed] (low)
- 复现: 2026-08-08 用户截图。历史对话标题栏的「全选」勾选框在深色背景上是一块刺眼的白底方块;设置页导出区的四个勾选框、独立文档页的批量选择框同理。
- 根因: `:root` 从未声明 `color-scheme`,浏览器按浅色渲染全部原生控件(勾选框、单选、下拉弹层、日期选择、文本光标)。此前只有 `.conv-row input[type="checkbox"]` 单独设过 `accent-color`——那只改选中色,改不了未选中态的底色,而且逐个覆盖既漏又难维护:本次新增的 `.doc-pick` 和更早的导出勾选框就都漏了。
- 影响: 纯观感,但白底方块在深色界面里格外扎眼;且每加一个勾选框就会再犯一次。
- 验收: ①`:root` 声明 `color-scheme: dark`,一次性覆盖所有原生控件;②勾选/单选统一 `accent-color: var(--accent)`,选中态用界面强调色而非系统蓝;③冒烟静态检查这两条——计算样式里看不出"浏览器用了哪套控件配色",只能查声明。
- 优先级: P3
- 阶段: 3
- 不变量: 前端:原生控件必须跟随深色主题
- 证据等级: E3
- 备注: 护栏已反验:删掉 `color-scheme: dark` 冒烟失败,还原后通过。既有的 `::-webkit-scrollbar` 与各 input 的显式背景优先级更高,不受影响。
- refs: D-153
- 标签: 前端

## D-155 G-002 同时存在于目标活动与归档文件导致 goal 工具全写保护 [fixed] (high)
- 优先级: P1
- 复现: goal update G-001 写入长期目标进展时被拒：tracker integrity is broken，G-002 同时存在于 goals.md 与 goals-archive.md。
- 标签: 流程
- 根因: 目标条目 G-002 在活动文件与归档文件重复，违反 tracker ID 唯一性；写保护因此拒绝所有 goal 写操作。
- 证据等级: E1
- 验收: G-002 只保留符合当前长期 active 状态的一份；goal 工具完整性恢复，可成功更新 G-001；增加或确认 tracker 完整性覆盖。
- 修复: 新增专用 `repair_reused_id` 动作：DocStore 在 crates/kanzei-tools/src/docstore.rs:239-318 仅对活动/归档语义不同的复用 ID 改写历史归档项，保留活动项，并同步归档模板、字段与手写自由文本；相同内容的未完成归档拒绝自动改号。TrackerTool 在 tracker.rs:88-162 允许该修复动作穿过完整性写保护，CLI 在 crates/kanzei/src/main.rs:563 接入。
- 验收证据: ① 实际执行后 .kanzei/project/goals.md:9 保留长期 G-002 active，goals-archive.md:10 的旧短期目标迁为 G-004；② goal update G-001 已成功，goals.md:6 已落盘新进展，证明普通写操作恢复；③ tracker.rs:885-940 回归覆盖改号、自由内容/字段内自引用同步、修复后普通 update 与完整性全绿；定向测试和 cargo test --workspace 全绿。

## D-157 停止动作动态文案缺少英文 i18n 资源导致界面混杂 [fixed] (medium)
- refs: R-069 R-089
- 优先级: P1
- 复现: 执行 `node scripts/ui-i18n-smoke.mjs` 失败：动态 i18n key `已请求停止` 未进入 I18N_EN。
- 影响: 英文界面触发停止动作时会回落中文，语言混杂；同时阻断所有前端条目的完整 smoke 验收。
- 标签: 前端
- 根因: main.js 新增或改为 `t("已请求停止")` 的动态状态文案，但 I18N_EN 字典未同步添加对应英文资源；i18n smoke 的动态 key 完整性门禁正确捕获。
- 证据等级: E2
- 验收: I18N_EN 补齐该动态状态文案；中英文切换后状态文本对应显示且可逆；ui-i18n-smoke 与 runtime smoke 通过。
- 修复: crates/kanzei-app/ui/main.js:143 将 `已请求停止` 补入 I18N_EN，保留既有 I18N_DYNAMIC_EN 源文案映射；`toast(t("已请求停止"))` 的真实调用方现在可直接取得英文且切回中文仍以源 key 重算。
- 验收证据: scripts/ui-i18n-smoke.mjs 动态 t() key 完整性检查通过（52 项资源/动态入口）；node --check、ui-runtime-smoke（main.js 全量执行，62 次 invoke，0 运行时错误）、ui-a11y、ui-markdown smoke 全部通过；真实 UI DOM 正常且 console 无错误。

## D-156 最小 800px 窗口下 nowrap 顶栏常驻控件总宽仍超过主区 [fixed] (medium)
- refs: R-089 D-104
- 优先级: P1
- 复现: 在当前 1407px 窗口中 #topbar 前五个子项已占约 442px，连同 4 个 gap 与左右 padding 约 502px；最小 800px 窗口保留 48px 活动栏和默认 280px 侧栏后，#main 仅 472px，尚未计入侧栏按钮、鞭挞状态和“更多”，因此 nowrap 顶栏必然向右溢出/裁切。
- 影响: R-089 明确要求 800x500、1024x720、1280x840 三档顶栏单行且无控件裁切；当前 800px 档不满足，不能关闭 R-089。
- 标签: 前端
- 根因: D-104 仅把 flex-wrap 改为 nowrap 并收纳部分低频动作，但没有给 800/1024 断点下的项目路径、进程区、自动状态与高频按钮设置压缩/隐藏层级；默认展开侧栏时可用主区宽度远小于常驻控件总宽。
- 证据等级: E3
- 验收: 800px 总窗口、默认展开侧栏时顶栏保持单行且所有高频动作可达：项目路径可收缩至零或隐藏，进程标签保留可操作宽度，新对话/活动/侧栏/鞭挞/更多不裁切；1024/1280 不退化；加入可机械计算最小宽度预算或等价运行时断言。

- 修复: crates/kanzei-app/ui/style.css:293-312 新增两级响应式预算：≤1024px 隐藏可舍弃的完整项目路径与自动轮次长文案、收紧 topbar 间距并把进程区限制为 40–120px 横向滚动；≤900px 将展开侧栏改为绝对定位抽屉，max-width 320px，不再参与主区 flex 宽度计算。800px 时活动栏外主区为 752px，侧栏抽屉最多覆盖 320px，仍留下 432px 对话/composer 可读区；1024px 默认侧栏下主区约 696px；1280px 保持原布局。
- 验收证据: scripts/ui-a11y-smoke.mjs:64-69 新增 1024px 顶栏压缩、进程区宽度与 900px 侧栏抽屉机械契约；frontend_check 确认 CSS 结构完整；node --check、runtime/a11y/i18n/markdown smoke 全部通过；真实 1407px 运行界面 topbar 1079×38 保持单行、DOM 控件完整、console 无错误。

## D-158 todo 与活动双抽屉在 800px 横向覆盖 84% 主区 [fixed] (medium)
- refs: R-089 D-110
- 优先级: P1
- 复现: style.css:31-39 在 ≤1400px 将 todo/bg 都设为宽度 `min(360px,42vw)`，并在两者同时显示时把 bg-panel 右移一个完整 drawer 宽度。800px 时两个抽屉总覆盖约 672px，而活动栏外主区仅 752px，只剩约 80px 对话可见；同时仍然是两个并排右栏。
- 影响: R-089 的“todo/活动同时有数据时不重复占两栏”与“主对话/composer 始终可读”均未满足；D-110 虽避免 flex 挤压，却改成大面积 overlay 遮挡。
- 标签: 前端
- 根因: D-110 把两个 flex 侧栏改成绝对定位后，仍沿用横向双列思路，通过 `right: var(--right-drawer-width)` 并排两个 42vw 抽屉，没有约束合计覆盖宽度。
- 证据等级: E3
- 验收: todo 与活动同时显示时共享同一右侧抽屉宽度，可垂直分区或切换，不得横向占两个栏位；800px 下主对话至少保留约 400px 可见宽度；单面板行为不退化；加入 CSS 机械契约并通过完整前端 smoke。
- 修复: crates/kanzei-app/ui/style.css:38-41 将 ≤1400px 的双面板从横向并排改为同一右侧抽屉宽度内上下各 50%：todo 通过 `:has` 收短 bottom，bg 保持 right:0 并从 top:50% 开始。800px 时抽屉合计宽度仍仅 42vw（约336px），活动栏外 752px 主区保留约416px可见宽度，不再只剩约80px。
- 验收证据: scripts/ui-a11y-smoke.mjs:67-68 机械断言 todo `bottom:50%` 与 bg `top:50%;right:0` 的共享宽度契约；frontend_check 结构完整；node --check、runtime/a11y/i18n/markdown smoke 全部通过；真实 UI 中单活动面板正常渲染、DOM 内容完整且 console 无错误。

## D-160 applyLanguage 对带空白动态文案二次 replace 导致指数膨胀 [fixed] (high)
- refs: R-069 D-136
- 优先级: P1
- 复现: 增强 MutationObserver 运行时冒烟后，英文切换触发 `applyLanguage` 对带缩进/换行的动态文本反复扩张，最终 source 超过 1,048,580 字符并抛 Invalid string length；最小 key 为“复杂度:”。
- 影响: 真实浏览器 MutationObserver 会在翻译写回后再次触发；使用 localizeDynamic fallback 的带空白文本会不断膨胀，造成页面卡死或崩溃。旧 smoke 的 MutationObserver 是空实现，因此此前完全漏检。
- 标签: 前端
- 根因: applyLanguage 先对含原始空白的 `source` 调用 localizeDynamic，得到已经包含同样空白的完整译文；随后又执行 `source.replace(source.trim(), translated)`，把完整译文塞回原空白中，空白每轮翻倍。缓存比较因此把膨胀结果误判为新源文案。
- 证据等级: E2
- 验收: exact key 翻译只替换 trimmed key；localizeDynamic fallback 直接作为完整 next 值，不再二次 replace；MutationObserver 冒烟真实触发且 zh→en→zh→en 不发生扩张或异常；加入回归断言。
- 修复: crates/kanzei-app/ui/main.js:547-568 将 exact 与 fallback 分流：exact 资源只替换 trimmed key，localizeDynamic fallback 直接作为完整 next，不再把含原空白的完整译文二次塞回 source；保留 1MB 扩张硬门禁。scripts/ui-runtime-smoke.mjs 的 MutationObserver 从空实现改为真实异步回调，并让 TreeWalker 文本代理稳定复用，能复现浏览器二次观察。
- 验收证据: runtime smoke 在修复前稳定复现 source=1,048,580、key=复杂度: 的扩张失败；修复后真实 MutationObserver 下完成 zh→en→zh→en、动态错误与权限队列切换，65次invoke、0运行时错误。node --check、i18n/a11y/markdown smoke 全部通过。

## D-161 流内 context overflow 绕过压缩恢复并保留超长会话历史 [fixed] (high)
- 复现: 长对话触发 provider 在 HTTP 200 SSE 流内返回 context overflow；runner 仅记录 stream_error 后直接失败，桌面端因 run_result? 提前返回，下一轮继续加载原超长 conversation，用户反复收到“Your input exceeds the context window”。
- 标签: 核心
- 根因: crates/kanzei-core/src/runner.rs 的 overflow 恢复仅包围 stream_with_retry_notice(...).await 建流错误；消费 SSE 时产生的 LlmError::ContextOverflow 在 stream_error 分支未进入 compact_messages_for_retry/compact_messages_aggressively。
- 验收: 建流前与流内 context overflow 都执行同一套两级压缩重试；恢复成功后的 summary.messages 为压缩后历史且被调用方持久化；自动化测试覆盖流内 overflow→压缩→成功以及二次 overflow→激进压缩→成功；全工作区测试通过。
- refs: R-106
- 优先级: P1

- 进展: 验收逐项证据：①建流前与 HTTP 200 SSE 流内超限统一由 crates/kanzei-core/src/runner.rs::recover_context_overflow 驱动两级恢复，run_once_with_parts 每次重试都从改写后的 messages 重建 LlmRequest；② crates/kanzei/tests/context_overflow_recovery.rs::sse_context_overflow_compacts_history_and_persists_recovered_summary 真实启动 CLI/mock SSE，断言首次流内超限后第二请求为有界压缩历史，成功 summary 写回 conversation.updated；③同文件 second_sse_context_overflow_retries_with_only_current_user_message 断言二次超限后第三请求只保留当前用户消息并持久化；④ cargo test --workspace 全部通过。

## D-162 OpenAI SSE 忽略 context_length_exceeded code 且漏识别实际超限文案 [fixed] (high)
- 复现: OpenAI 兼容 provider 以 SSE error 返回 type=invalid_request_error、code=context_length_exceeded、message='Your input exceeds the context window of this model'；协议层只把 type 传给 classify_provider，message 词表也不含 'exceeds the context window'，因此错误被归为普通 Provider，runner 不会压缩重试。
- 标签: 模型
- 根因: crates/kanzei-llm/src/protocol/openai.rs 在 error.type 存在时忽略 error.code；crates/kanzei-llm/src/error.rs 的 overflow 文案词表未覆盖实际 'input exceeds the context window' 表达。
- 验收: OpenAI SSE 的 type/code 任一明确为 context_length_exceeded 时均分类为 ContextOverflow；实际 'input exceeds the context window' 文案有回归；不得把 rate_limit_error 中含 token/limit 的文案误判为 overflow；相关协议测试与工作区测试通过。
- refs: D-161 R-106
- 优先级: P1
- 进展: 验收逐项证据：① crates/kanzei-llm/src/protocol/openai.rs 的 OpenAiState 同时传入 error.type 与 error.code，不再由通用 type 遮蔽 context_length_exceeded；② crates/kanzei-llm/src/error.rs::classify_provider_with_code 对 type/code 分别识别，词表补入真实 'exceeds the context window'，同时保持限流 kind 优先；③ openai 协议新增 context_length_code_is_not_hidden_by_generic_error_type 与 rate_limit_type_still_wins_when_code_is_more_specific 回归；④ CLI SSE 集成测试复现用户原始英文文案并恢复成功；⑤ cargo test --workspace 全部通过。

## D-170 项目隔离失效:需求在不同项目之间串 [fixed] (high)
- 复现: 2026-08-08 用户实测。用「＋ 添加项目目录」加了新项目后,不同项目看到同一批需求。
- 根因: `projects_add` **只把路径记进偏好,不在该目录建 `.kanzei`**;而后端一律用 `discover_project_root` 解析根,它会**沿目录树向上找 `.kanzei`,找不到再退到最近的 `.git`**。于是任何未初始化的项目目录都会解析到某个祖先——两个共用同一祖先(或同属一个 git 仓库)的目录被当成同一个项目,读写同一份 `requirements.md`/`defects.md`,连会话也共用。向上遍历对 CLI 是对的(`kz` 在子目录里跑要找到项目根),但桌面端的项目是用户**显式选定**的目录,向上走等于把他的选择悄悄换成了祖先。
- 影响: 最严重的一类——跨项目数据污染。用户会看到别的项目的需求,在错误的项目里改条目,而且完全无从察觉。
- 验收: ①`projects_add` 就地创建 `.kanzei`,新项目从加入那一刻起自成一根;②存量项目**不静默迁移**——改根会让 `project_session_id` 变化、历史对话看起来消失,那会造成第二次"数据丢失"惊吓;改为新增 `project_root_info` 如实报出"所选目录 vs 实际生效的根",不一致时侧栏顶部醒目告警并给出实际路径;③提供 `project_detach` 一键在本目录建立独立空间,**只建空间不搬数据**——祖先目录里的条目属于祖先项目,替用户搬等于替他做决定;④回归覆盖"同一上级下两个目录先共用、分离后互不可见,且上级数据不被动"。
- 优先级: P0
- 阶段: 1
- 不变量: 项目:用户显式选定的目录就是该项目的根
- 证据等级: E2
- 备注: 落地位置 crates/kanzei-app/src/main.rs(projects_add 建 .kanzei、project_root_info、project_detach)、ui(侧栏告警与一键分离)。回归:Rust 侧 `同一上级下的两个项目必须各自独立不串数据`,冒烟 5 项。
- 二轮收口(2026-08-08 用户要求彻底解决): 首轮只做到"新项目自动隔离 + 存量告警",存量仍要用户一个个发现。补齐三点。
  ①**判定规则明确化**:新增 `root_has_data`(祖先的 `.kanzei/project`、`.kanzei/memory` 非空或有 `state.db`)与 `ensure_project_isolated`,规则定为**能无损修就自动修、会改变可见内容的才问**——祖先没有任何项目数据时,补 `.kanzei` 前后用户看到的都是空,无损,静默修;祖先有数据时绝不擅自改根,因为那会让这个项目从"看得到那批条目"变成空的,是可见变化,必须用户确认。
  ②**自动修复接入取活路径**:`projects_select` 与 `project_root_info` 都会调 `ensure_project_isolated`,切进来即修,用户无感;修过只在运行日志留痕,不弹窗打扰。
  ③**一次报完**:新增 `projects_isolation_report` 体检全部注册项目,受影响的往往不止当前这个(它们共用同一祖先),切一个发现一个会让人以为修完了。
  ④`project_detach` 增加回读校验:建完目录必须确认根确实解析到自身,否则报错而不是假成功;前端分离后重取文档与会话并重置体检标记。
- 二轮回归: `祖先无数据时静默自动隔离_有数据时绝不擅自改根`(含幂等)、`隔离体检一次报完全部共用项目`(分离 A 不影响 B)。已反验:去掉 `root_has_data` 判断让它无差别改根,测试立即报「祖先有数据时不得自动改根(会让用户以为条目丢了)」。
- refs: D-058
- 标签: 后端

## D-169 切到独立文档页后需求整列消失,界面无任何说明 [fixed] (high)
- 复现: 2026-08-08 用户实测。切换到独立文档页,需求列表变空,而筛选控件显示的是"全部" —— 看起来像数据丢了。
- 根因: 两层叠加,**是 R-115 的筛选持久化把一个潜伏矛盾激活的**。
  ①`syncTagFilter` 在"保存的标签不存在于当前条目"时,只把**下拉的显示值**回落成 `all`,不回写筛选状态。于是 `reqFilters.tag` 仍是那个不存在的标签,`filterRequirements` 照它筛,结果为空——而界面显示"没有筛选"。R-115 之前标签不持久化,每次启动都是 `all`,这个矛盾永远碰不到;一持久化就必然触发。
  ②空状态判断是 `entries.length === 0 && archivedCount === 0` 才显示"(空)"。有归档条目时被筛空则**连"(空)"都不显示**,渲染出纯一片空白,更像数据没了。
- 影响: 用户会认为需求被删了。这个项目此前真丢过 8 个缺陷条目,这种"看起来像数据丢失"的表现代价极高——会触发不必要的恢复操作。
- 验收: ①`syncTagFilter` 返回实际生效值,三处调用方一律回写状态,做到显示与状态同源;②任何列表在**筛前非空、筛后为空**时必须显示"N 条被当前筛选隐藏"并给一键清除,不得留白;③冒烟守住"列表不得无声变空"这条不变量。
- 优先级: P1
- 阶段: 3
- 不变量: 前端:列表不得无声变空——空了就要说清为什么
- 证据等级: E3
- 备注: 落地位置 crates/kanzei-app/ui/main.js(syncTagFilter 返回值 + 三处回写、renderDocList 被筛空分支)。已反验:去掉回写,冒烟报「筛选状态应回落成「全部」而不是筛空」并失败。
- refs: R-115
- 标签: 前端

## D-168 配置页与实际生效的配置之间有三处静默不一致 [fixed] (high)
- 复现: 2026-08-08 用户配 DeepSeek 全过程暴露。设置页 primary 明明显示 `deepseek:deepseek-chat`,发消息时日志却是 `[鉴权] anthropic:claude-sonnet-5` + `provider 'anthropic' 需要环境变量 ANTHROPIC_API_KEY`——界面显示的和实际跑的是两份东西,而且没有任何线索指向"你改的那个没生效"。
- 根因: 四处叠加,都是"看得见的"与"生效的"脱节。
  ①**表单不保存不生效,却无提示**:设置页是一张普通表单,填完不点保存只活在 DOM 里;运行时读的是磁盘。用户以为改了。
  ②**settings_get 只读全局文件**:而运行时是 `全局 + 项目` 合并。项目级 kanzei.toml 一旦也设了 models,设置页显示的就是个不生效的值,同样零提示。
  ③**模型角色是自由文本框**:手打 `provider:model`,拼错一个字母要到真正发消息时才炸,那时早已离开设置页,联系不到是刚才填错的。保存路径不做任何校验。
  ④**merge() 漏了 models.reasoning**:primary/fast/providers/proxy/profile/permissions 都合了,唯独 reasoning 没有——同一个 `[models]` 表里有的键管用有的不管用,是最难查的那类不一致。
- 影响: 配置这条链路整体不可信。用户按文档一步步配完、连通性测试还过了,一发消息用的还是旧 provider,且报错完全指不到原因。
- 验收: ①表单与磁盘不一致时显示「未保存」;②settings_get 同时返回合并后的生效值与项目配置路径,不一致时界面明示"被项目级配置覆盖,本页改动不会生效";③模型角色改为下拉,选项来自各 provider 的探测结果,保留手填兜底,且**探测不到的已存值必须原样保留**(否则一进设置页就被悄悄改掉,保存一次配置就坏了);④保存前用 `resolve_model` 校验 provider 确实存在,不存在直接拒绝并说明格式;⑤merge 补上 reasoning;⑥鉴权失败的报错带上本次解析到的模型,并提示检查保存与项目级覆盖。
- 优先级: P1
- 阶段: 3
- 不变量: 配置:界面显示的必须等于实际生效的,不等就要说破
- 证据等级: E2
- 备注: 落地位置 kanzei-harness/config.rs(merge reasoning)、kanzei-core/assemble.rs(错误带模型)、kanzei-app/main.rs(settings_get 返回 effective、validate_model_roles)、ui(下拉、未保存徽标、覆盖提示)。回归:Rust 侧 2 项(保存前校验、models 全字段合并),冒烟 8 项。「已存值不被下拉吃掉」已反验:去掉保留分支即失败。
- refs: D-156 D-157 R-115
- 标签: 模型

## D-167 加了 OpenAI 兼容 provider 却选不出任何模型 [fixed] (high)
- 复现: 2026-08-08 用户按指引在设置页添加 deepseek(protocol=openai, base_url=https://api.deepseek.com/v1, api_key_env=DEEPSEEK_API_KEY),顶栏「模型」下拉里一个 deepseek 模型都没有,只有 primary/fast 两个角色项。
- 根因: `models_list` 只硬编码枚举四种情况——primary/fast 角色、`auth="codex"`(3 个写死型号)、`auth="claude"`(3 个写死型号)、`base_url` 含 11434 的 Ollama(查 /api/tags)。**其余 provider 直接落到分支尾部,贡献 0 个模型**。而配置层是完全开放的:任何 OpenAI 兼容端点都能配进去。于是"能配 provider"与"能用 provider"之间断了一环,DeepSeek/OpenRouter/Kimi/自建 vLLM 全中招。
- 影响: provider 配置形同虚设——配好了、连通性测试也过,就是没法在界面上选中它的模型。用户只能去改 kanzei.toml 的 `[models]` 硬指,顶栏下拉这条主路径不通。
- 验收: ①protocol 为 openai / openai-responses 的 provider 走标准 `GET {base_url}/models` 探测,带上 api_key(直填优先于环境变量),遵循全局代理设置;②探测失败静默跳过,不阻断其余 provider 的列举——端点可能没实现 /models,或 key 尚未配好;③提供手填兜底「＋ 手填模型…」,输入 `provider:model` 直指,校验格式后落盘并持久留在下拉里;④Ollama 仍走原生 /api/tags(它的 /v1/models 不全),抽成 `push_ollama_models` 避免两处重复。
- 优先级: P1
- 阶段: 3
- 不变量: 配置:能配进来的 provider 就必须能在界面上用起来
- 证据等级: E2
- 备注: 落地位置 crates/kanzei-app/src/main.rs(models_list 新增 openai 分支 + push_ollama_models)、ui/main.js(手填入口与持久化)。冒烟新增 4 项断言:手填入口存在、落盘、回到下拉、非法格式被挡。
- refs: R-115
- 标签: 模型

## D-172 启动黑屏:i18n MutationObserver 微任务死循环饿死渲染主线程 [fixed] (critical)
- refs: D-136 458af450 e4b45f21
- 优先级: P0
- 复现: build-2c999d4(含 e4b45f21)启动即整窗黑屏。CDP 观测:浏览器进程命令(Browser.getVersion)秒回,所有需渲染进程处理的命令(Runtime.evaluate/Runtime.enable/Page.enable/冷附加 Debugger.enable)永不响应;渲染进程 10 分钟烧掉 380s CPU。重启后在 about:blank 阶段先挂 Debugger 再 pause,栈定格在 applyLanguage(main.js:569)← MutationObserver 回调(main.js:639)。
- 影响: 桌面端完全不可用;且症状组合(黑屏+无 console+CDP 无响应+PrintWindow 抓黑)极易误判为 WebView2/GPU/截图伪影问题,本次调查一度走偏。
- 标签: 核心
- 根因: 两笔提交叠加成环。458af450 的属性翻译在 zh(默认)模式下对每个带 title/placeholder/aria-label 的元素**无条件 setAttribute**(判据 `translated !== source || language !== "en"` 恒真);e4b45f21 给 languageObserver 补 `attributes:true + attributeFilter:[title,placeholder,aria-label]`。DOM 规范规定 setAttribute 同值也入 mutation 队列,于是 observer→applyLanguage→setAttribute→observer 微任务无限循环,事件循环永远轮不到绘制与输入。`applyingLanguage` 标志只防同步重入,防不了跨微任务自触发。冒烟测不出是因为 harness 的 setAttribute 同值早退不通知 observer,与规范语义相反。
- 证据等级: E1(冒烟护栏红绿双验)+ 真机 CDP 断点栈与修复前后渲染进程 CPU/响应实证
- 验收: ①main.js 属性写入前比对,同值不写;②冒烟 harness setAttribute 同值也通知 observer(对齐 DOM 规范),并加「observer 连续自触发>25 轮判失败」护栏,把挂死变成可读失败;③bug 复位冒烟必红、修复后必绿,已双验;④修复构建真机验证:Runtime.evaluate 即时响应、页面完整渲染、渲染进程存活 53s 仅耗 1s CPU。

- 进展: 已修复并双侧验证(2026-08-08)。遗留:发布版(用户机器)仍是坏 build,需走发版 SOP 推送修复。

## D-197 frontend_locate 的 @media 上下文两头都算错,还是个 deny 级 lint [fixed] (high)
- refs: D-164 R-126
- 复现: `find_rule_sites`(crates/kanzei-tools/src/frontend.rs)用"整行没有新开块时弹一层栈"判断条件块结束。两种真实形态都错:①`@media { .a {\n…\n} }` 里 `.a` 的收尾 `}` 会把 @media 提前弹掉,块内后续规则被报成顶层(style.css 行 40/41);②单行写完的 `@keyframes x { … }` 等不到那一次弹栈,于是它**之后**的顶层规则全被报成在这个块里(行 367/390/753/754)。实测本仓库 style.css 576 条规则里 **15 条 context 是错的**。另外这段的写法是 `for _ in 0..… { …; break; }`,clippy 的 `never_loop` 是 deny 级,`cargo clippy --workspace` 一直红着编译不过。
- 影响: 这个工具存在的理由就是 D-164——"响应式覆盖必须标出所在 @media,否则改了基础规则还以为改完了"。形态①正是它该防住却没防住的;形态②更糟,它把顶层规则报成在一个根本不包含它的条件块里,agent 据此判断"这条只在窄屏生效"就会漏改。dev 提示词还明确要求改 style.css 前先跑 `frontend_locate`。
- 根因: 用"这一行是不是只有收尾括号"近似"条件块结束",而块的结束本质是花括号深度退回;单行块与多行规则这两种情形都不满足那个近似。
- 修复: 栈元素改成 `(名字, 该块内部内容所在深度)`,逐行按 `depth = (depth + opens) - closes` 更新,深度退回到某块之外就出栈(单行块在本行当场出栈)。site 的 context 仍取"选择器开括号那一刻"的栈。顺带消掉 never_loop。
- 验收: 新增 `条件块上下文不被多行规则提前关闭也不泄漏到块外` 覆盖两种形态;对真实 style.css 实测(临时探针)行 40/41 现在标出 `@media (max-width: 1400px)`,行 367/390/753/754 现在为空,规则总数 576 不变;`cargo clippy --workspace --all-targets` exit=0。
- 证据等级: E1(真实 style.css 逐行比对)
- 优先级: P1
- 标签: 核心

## D-194 HOME 判断用裸路径相等,且在 HOME 里直接开跑那条路没堵 [fixed] (high)
- refs: D-189 D-186
- 复现: 两处。①`discover_project_root_with_home` 用 `h == d` 逐字节比较判 HOME,而 `dirs::home_dir()` 给的是 `C:\Users\kanzei`——走上来的祖先只要是 `c:\users\kanzei`(shell 里键入的大小写)或 `\\?\C:\Users\kanzei`(canonicalize 的产物),`is_home` 就是 false,D-189 的排除当场失效。②HOME 自己当 cwd 时:向上找不到任何标记,兜底 `Some(cwd)` 原样返回,cwd 就是 HOME。`kz run` 与 `kz req/defect/...` 在 HOME 里都会以 HOME 为项目根跑起来。
- 影响: 项目级产物(state.db、project/ 追踪文件、memory/)落进 `~/.kanzei`——那是全局配置根,和 kanzei.toml、全局记忆、app.json 混在一起;且此时 `project_memory_root(HOME)` 与 `global_memory_root()` 是同一个目录,两个 scope 的 INDEX.md/index.db/inbox.md 静默合流。D-189 拆掉了磁铁(子目录被吸上去),直连这条路仍然通着,本机 `~/.kanzei/project/defects.md` 正是这么留下的。
- 根因: ①路径比较没归一。同一个坑 kanzei-core 的 `session_identity` 已经踩过一次(同一项目裂成两条会话线,注释里写着),D-189 没沿用那套归一。②D-189 只改了"标记识别",没管"兜底返回 cwd"这条出口。
- 修复: `config.rs` 新增 `dir_key()`(剥 `\\?\`/`\\?\UNC\` 前缀、去尾分隔符,Windows 上再统一分隔符并小写;Linux 保持大小写与分隔符敏感)供 HOME 判断使用,并暴露 `is_home_root(root)`。CLI 两个入口(`run_cli` 与 `tracker_cli`)开跑前调 `reject_home_as_project_root` 拒绝并给出下一步。桌面端不拦:那里选 HOME 是显式动作,不是误撞。
- 验收: 单测 `home_marker_exclusion_survives_path_form_differences`(`\\?\`/正斜杠/末尾分隔符/大小写四种写法逐一断言排除仍生效)、`is_home_root_recognizes_real_home_in_any_form`;实测 `kz req list` 与 `kz run` 在 `C:\Users\kanzei` 下均 exit=1 并给出提示,`~/.kanzei` 未被写入,项目仓库内 `kz req list` 不受影响。workspace 276 项全绿。
- 证据等级: E1(真实 HOME 下的 CLI 实测)
- 优先级: P1
- 标签: 核心

## D-195 提示词与装配"同源"只是约定,没有任何机制保证同进同退 [fixed] (high)
- refs: D-190 D-173
- 复现: D-190 把前端自查段抽成 `frontend_inspection_guidance()`,但组件注册(crates/kanzei-app/src/main.rs 的 `FrontendToolsComponent`)与提示词追加(同文件 work-priority 旁)是两处各写各的。摘掉组件而留下追加、或把这段写回 dev 基础提示词,都不会有任何东西报错——和 D-190 修之前是同一种失效方式。
- 影响: D-190 这类错配没有护栏就必然回归,而它的后果是模型被指向不可达的能力,试完失败转去找旁路(D-173 的失效模式)。resolve 末尾的覆盖校验只查硬 deny 声明的 `required_tool`,管不到提示词点名的工具。
- 根因: 修 D-190 时只搬了位置,没把"提示词点名的工具必须在同一条装配线上注册"变成可执行的判据。
- 修复: `kanzei_tools::prompt_tool_mentions(prompt)` 提取反引号内的标识符首词(两侧共用一套规则,不各写一份);两条测试各守一半——kanzei-tools 侧遍历 CLI 装配线上每个 agent 的 system,点名的工具必须在 `materialize_tools()` 里(非工具词只有 `node`/`task` 两个白名单项,各自写明理由);kanzei-app 侧断言桌面装配线注册了前端自查段点名的全部 5 个工具。
- 验收: 反向验证——临时把 `ui_dom` 写回 dev 基础提示词,CLI 侧测试立即失败并指名 "Dev 档的 agent `dev` 提示词点名了 `ui_dom`,但这条装配线没注册它";还原后 workspace 276 项全绿。另有 `prompt_tool_mentions_只取反引号里的标识符首词` 守提取规则本身(防止提取不出东西造成的假绿)。
- 备注: 同类未修残留一条(D-190 备注里已记):work-priority 段也只有桌面端 append,CLI 提示词里"the selected work-priority mode"永远指向不存在的内容。它不点名工具,所以现有护栏抓不到。
- 证据等级: E1(反向注入验证护栏会红)
- 优先级: P1
- 标签: 核心

## D-196 standing directives 被预算丢弃时不报数,违反自身注释的不变量 [fixed] (medium)
- refs: D-191
- 复现: crates/kanzei-tools/src/profiles.rs 的 `dev/memory` 注入,known facts 那半边超预算会补一行"(还有 N 条未列出)",standing directives 那半边一条计数都没有。`MEMORY_CONTEXT_BUDGET` 的注释写的却是"超预算必须显式说明丢了多少,不做静默截断"。
- 影响: D-191 把 `break` 改成 `continue` 之后更要紧——丢的不再是尾巴而是从中间挑着丢,而 directives 正是标着 "obey these; they are the user's own words" 的用户原话(preference 全文)。模型完全看不出少了哪条,用户也无从察觉自己的常驻指令没进上下文。
- 根因: D-191 只改了截断方式,没补上它引用的那条不变量;计数只在 known facts 那半边实现过。
- 修复: directives 循环记 `directives_shown`,少于总数时补一行"(另有 N 条常驻指令因预算未列出,memory_search category=preference 可取全文)",给出可达的取全文路径。
- 验收: workspace 276 项全绿。
- 证据等级: E2
- 优先级: P2
- 标签: 核心

## D-193 发布 tag 建在 main 的 HEAD 上,既对不上产物也架空了 D-183 的区间判据 [fixed] (high)
- refs: D-183
- 复现: `gh release create build-<hash> ...` 不带 `--target`。tag 不存在时 gh 在**远端默认分支(main)**的 HEAD 上创建它,而发版是从 `dev` 打的。实证:`git rev-parse build-ecdab96` = `5dcf469`(= origin/main),不是它命名的 `ecdab96`;`build-5dcf469` 同样指向 5dcf469。
- 影响: ①发布页上的 tag 指向的树与安装包里的二进制不是同一个提交,产物无从追溯——恰好是 package.ps1 里"工作区必须干净"那段注释想避免的事;②更隐蔽的是 D-183 的护栏被架空:区间取 `最近的 build-* 标签..HEAD`,而这些标签全钉在 main 上不动,于是每次发版都把同一批 dev 提交重新数一遍,-Ack 数字越滚越大,"多出来一个提交就强制停顿"的精度归零。本轮实测:刚发过一次,下一次的区间仍是 3 个提交。
- 根因: gh release create 的默认 target 是远端默认分支,而本仓库的发布分支是 dev;脚本没显式指定 target。
- 修复: `gh release create` 加 `--target`,tag 落在真正构建的那个提交上。**必须传 40 位全 SHA**:GitHub 的 `target_commitish` 不认短 hash,传 `$hash` 会被 `HTTP 422 Validation Failed: Release.target_commitish is invalid` 挡回来(实测 build-84f843e 一次),脚本因此单独留了 `$full_hash`。
- 验收: 下一次发版后 `git rev-parse build-<hash>` 等于 `<hash>`;再下一次的发布区间只包含该次之后的新提交。
- 备注: 已发布的 build-ecdab96 / build-5dcf469 仍指向 5dcf469。挪动已发布的 tag 属改写已公开的引用,留给用户拍板,不擅自动。
- 证据等级: E1(rev-parse 实证)
- 优先级: P1
- 标签: 流程

## D-189 `~/.kanzei` 是项目根磁铁:`.kanzei` 无视距离压过更近的 `.git` [fixed] (high)
- refs: D-186
- 复现: `discover_project_root`(crates/kanzei-harness/src/config.rs)撞到任何 `.kanzei` 目录就立即返回,`.git` 只记 fallback 且要等循环走完才用。于是 `C:\Users\kanzei\Documents\某仓库`(有 `.git`、无 `.kanzei`)解析出的项目根是 `C:\Users\kanzei`——仓库自己的 `.git` 被丢掉,因为 `~/.kanzei` 作为全局配置根必然存在。
- 影响: HOME 下所有无标记目录共用同一个项目根:state.db、project/ 追踪文件、记忆全部串到一起;且 HOME 当项目根时 `global_memory_root()` 与 `project_memory_root(HOME)` 是同一个目录,两个 scope 的 INDEX.md/index.db/inbox.md 静默合流。函数注释一直写的是"向上**最近**的含 .kanzei/ 或 .git/ 的目录",实现与注释不符。
- 根因: `.kanzei` 命中即返回 + `.git` 仅作 fallback 的两段式写法,让"距离"这个判据在 `.kanzei` 面前失效;同时没有把 `~/.kanzei`(全局配置根)与项目级 `.kanzei` 区分开。
- 修复: 改为单次向上扫描,最近的 `.kanzei` 或 `.git` 谁先出现谁赢;并把 HOME 自己的 `.kanzei` 排除出项目标记(HOME 的 `.git`——dotfiles 仓库——仍算)。拆出 `discover_project_root_with_home(cwd, home)` 供测试注入。
- 验收: 单测 `nearest_git_wins_over_a_farther_kanzei`(更近的 .git 赢)与 `home_global_config_dir_is_not_a_project_marker`(同一棵树只切换"是否认 HOME"的前后对照)覆盖;workspace 271 项全绿。
- 证据等级: E1
- 优先级: P1
- 标签: 核心

## D-190 dev 提示词点名 5 个只有桌面端注册的工具,CLI 侧被指向不可达能力 [fixed] (high)
- 复现: dev agent 的 system prompt 里点名 `ui_dom` / `ui_console` / `ui_style` / `frontend_locate` / `frontend_check`(crates/kanzei-tools/src/profiles.rs),而这 5 个只由桌面端的 FrontendToolsComponent 注册(crates/kanzei-app/src/main.rs)。`kz` 跑 dev agent 时提示词照发,工具不在 specs 里。
- 影响: 正是 D-173 的失效模式——指令指向不可达的能力,模型试完失败就转去找旁路。resolve 末尾的覆盖校验只查硬 deny 声明的 `required_tool`,管不到提示词点名的工具,所以这类错配没有任何护栏。
- 根因: 前端自查段写死在 dev 的基础提示词里,而工具注册是按装配线分的(桌面 5 条组件、CLI 4 条),提示词与装配不同源。
- 修复: 抽成 `kanzei_tools::frontend_inspection_guidance()`,由注册了这些工具的装配方(桌面端,紧邻 work-priority 追加处)append;dev 基础提示词不再点名它们。
- 验收: CLI dev 的 system 里不出现这 5 个工具名;桌面 dev 仍带该段。workspace 271 项全绿。
- 备注: 同类残留一条(未修):work-priority 段也只有桌面端 append,CLI 永远不追加,提示词里"the selected work-priority mode"指向不存在的内容,靠后半句 "When no mode is supplied, use defect-first" 兜住。能跑,但属同一类不同源问题。
- 证据等级: E2
- 优先级: P1
- 标签: 核心

## D-191 记忆注入预算截断用 break,一条超长条目挡死其后全部 [fixed] (medium)
- 复现: crates/kanzei-tools/src/profiles.rs 的 standing directives 与 known facts 两个注入循环都是 `if cost > budget { break; }`。一条超长 preference 或长 description 卡在中间,后面所有更短的条目全部不注入。
- 影响: 提示只说"还有 N 条未列出",不说是被挡住的——用户与模型都看不出预算是被一条长条目吃干净还是自然填满;高价值短条目可能因为排在一条长条目之后而永远不进上下文。
- 根因: 把"预算用尽"与"这一条放不下"混为一谈。
- 修复: 两处 `break` 改 `continue`——放不下的跳过,继续填后面的;`shown` 计数不变,折叠提示仍准确。
- 验收: workspace 271 项全绿。
- 证据等级: E2
- 优先级: P2
- 标签: 核心

## D-192 上下文账单漏掉最大的一块:工具 schema [fixed] (medium)
- refs: R-145
- 复现: `context_report` 只有 `agent/system` + 各 context source 的字符数,而 `estimate_prompt_tokens`(crates/kanzei-core/src/runner.rs)明确把 tool specs 算进 prompt。桌面 dev 档是 26 个工具的完整 JSON Schema,账单里一个字节都没有。
- 影响: R-106 说账单要回答"本轮上下文里有什么、各占多少",漏掉工具 schema 就答不了最大的那一项;按这份账单做注入瘦身会一直在小头上使劲。
- 根因: 账单在 `system_baseline_with_report()` 里组装,而 specs 是在 runner 侧另行构建的,两者没有汇合。
- 修复: runner 组装 specs 后按 name+description+input_schema 的字符数追加一行 `tools/schema` 到 context_report。
- 验收: CLI 摘要与桌面 run.completed 事件的 context 里出现 `tools/schema` 行;workspace 271 项全绿。
- 备注: 修复原文写的"与 estimate_prompt_tokens 的口径一致"不成立——新行用 `chars().count()`,而 `estimate_prompt_tokens` 用 `len()` 字节再 /4。**选字符是对的**(账单其余行全是字符,内部自洽才要紧),只是说法要改成"与账单其余行同为字符口径";工具描述里现有 7 处中文,CJK 部分两者差三倍。另一处已知局限:这是单条聚合,26 个工具合成一个数字,能回答"占多少"、回答不了"谁占的",而账单的用处正是拿它砍——按工具分行成本几乎为零,留作后续。
- 证据等级: E2
- 优先级: P2
- 标签: 核心

## D-186 `~/.kanzei` 下已有项目级产物,D-170 的自动隔离对 HOME 这条路径失效 [fixed] (high)
- 复现: 本机 `~/.kanzei/` 下存在 `state.db`(86 KB)、`project/defects.md`、`project/defects-archive.md`——这些是项目级产物,只该出现在项目根。成因是 D-183 修复前的 `discover_project_root`:撞到任何 `.kanzei` 就返回,而 `~/.kanzei` 作为全局配置根必然存在,于是 HOME 下所有无标记目录的项目根都解析成了 HOME。
- 影响: `ensure_project_isolated` 的规则是"祖先没数据就静默补 `.kanzei`,有数据就不动、等用户拍板"。`root_has_data(HOME)` 因这些残留为真,于是往 `C:\Users\kanzei\` 下新增项目不再被自动隔离,而是静默并进 HOME 项目,直到用户发现条目串了。自愈路径当前是关着的。
- 根因: 解析缺陷(已在 discover_project_root 修复:最近的 `.git` 赢过更远的 `.kanzei`,且 HOME 自己的 `.kanzei` 不算项目标记)之外,**已经写出去的残留数据**没有清理路径。
- 修复: 解析侧已修。残留侧需要人拍板:`~/.kanzei/project/` 与 `~/.kanzei/state.db` 属删除操作,必须用户确认后再动(可先备份到 `~/.kanzei/.orphan-backup/`)。
- 验收(2026-08-09 用户定调按实际收口路径重写,原验收①作废): ①`~/.kanzei` 下不再有项目级产物(`project/`、`state.db`);②HOME 下的无标记目录不再解析到 HOME,`ensure_project_isolated` 对它们无事可做(`resolved == dir` 早返回),即自动隔离这条路不再是必需品;③有回归覆盖"HOME 不被当项目根",且覆盖到路径的不同写法。
- 关闭依据: ①残留已移走(不是删除):`~/.kanzei/{project/,state.db}` 在 `~/.kanzei/.orphan-backup/20260809/` 下,`~/.kanzei` 只剩 kanzei.toml、app.json、memory/。②由 D-189 满足:最近的标记谁先出现谁赢 + HOME 的 `.kanzei` 不算项目标记,HOME 下无标记目录现在解析成它自己。③由 D-194 满足:`home_marker_exclusion_survives_path_form_differences`(四种路径写法)与 `is_home_root_recognizes_real_home_in_any_form`,并在 CLI 两个入口加了拒绝。
- 作废原因(原验收①"清理后 `root_has_data(HOME)` 为假"): 这条按现在的判据**永远满足不了**,而且不该满足。`root_has_data` 把 `.kanzei/memory` 算作项目数据,可对 HOME 来说那正是**全局**记忆根(inbox.md、index.db),合法且必须非空。真正的问题不是"HOME 有数据",而是"HOME 会被解析成项目根"——那一条已在 D-189/D-194 从源头堵死,`root_has_data` 这个判据对 HOME 已经走不到了,不必为它改判据。
- 证据等级: E1(本机文件实证)
- 优先级: P1
- 标签: 核心

## D-199 更新交接把"安装器 exit=0 但一个字节没换"记成成功 [fixed] (high)
- refs: D-182 D-198
- 复现: `run_install_helper`(crates/kanzei-app/src/main.rs)在安装器返回 0 之后,只做 `metadata(exe).modified()` 并把结果**打进日志**,从来没跟安装**前**的值比过,随后无条件写 "已拉起新版本"。实测 2026-08-09:用户两次点「检查更新」(03:37:32、03:40:21),两次都记 `安装器 exit=0` + `已拉起新版本`,而两次的 `安装后 exe mtime` 是同一个值 `134306860100000000`(= 2026/8/9 2:06:50,上一版 4ad666c 的构建时间)——文件从头到尾没被替换。
- 影响: 这正是该函数上方注释明确担心的情形——"NSIS 在目标被占用时也可能报 exit=0 而什么都没换,只信退出码会把'静默没装上'当成成功"。D-182 为此加了 mtime 记录,却只记不比,护栏写了一半等于没写:用户看到"更新成功"、版本纹丝不动,唯一的诊断入口(update.log)也在附和这个谎。本次排查耗掉大半个会话,起点就是这条假成功。
- 根因: 把"记录证据"当成了"校验证据"。判据需要安装前后两个采样点,原实现只采了一个。
- 修复: 安装前先取 `image_stamp(exe)`(mtime + 大小),装完再取一次,`image_replaced` 比较;不一致才算成功。未替换时如实记 "安装器 exit=0 但 exe 未被替换…目标可能被占用,或安装位与运行位不是同一个文件",**保留安装包**供手动执行(原实现无条件删),并把重启日志改成 "已拉起——仍是旧版本,更新未生效"。任一侧读不到指纹一律判未替换:宁可多报一次可疑,也不要把静默失败说成成功。
- 验收: 新增 3 条单测——`未替换的镜像一律不算更新成功`(含实测那组前后完全相同的指纹,以及任一侧 None)、`时间或大小任一变化都算替换成功`(防过度保守把真更新误报成失败)、`image_stamp_跟得上真实文件改动`(真实文件 touch 后指纹必变,防纯比较函数绿了但取错字段)。workspace 289 项全绿。
- 备注: 未做:失败信息目前只进 update.log。发起更新的进程此时已退出,没有 UI 可通知;要让用户在界面上看到"更新未生效",得让被拉起的新实例开机读一次日志并提示,属另一条。
- 证据等级: E1(用户机器 update.log 实证,两次交接 mtime 逐位相同)
- 优先级: P1
- 标签: 发布

## D-198 release.ps1 的安装硬校验在写入被重定向时自洽通过 [fixed] (high)
- refs: D-145 D-199
- 复现: `scripts/release.ps1` 把 kzapp.exe `Copy-Item` 到 `$env:LOCALAPPDATA\kanzei\`,紧接着 `Get-FileHash` 源与目标比对,不一致就 throw。但在 AppContainer 里(Claude 桌面端的会话进程即是)对 `%LOCALAPPDATA%` 的写入被重定向到 `...\Packages\<pkg>\LocalCache\Local\`,读回也走影子——写和读是同一份影子,校验必然通过。实测 2026-08-09:ccfecff 发版后脚本报"硬校验通过",用户开始菜单指向的真实 `C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe` 仍是 4ad666c。
- 影响: 这个校验存在的理由就是"杜绝发布成功但仍在跑旧版",而它恰恰在最需要它的场景下给出绿灯——比没有校验更坏,因为它让人停止怀疑。用户因此卡在旧版且"检查更新"也无效(D-199 让后者同样静默),排查花掉大半个会话。
- 根因: 校验有一个它自己看不见的前提——写进去的和读回来的是同一个真实文件。重定向恰好同时改写了写入端与读取端,于是自洽。同一脚本的两半沿重定向边界分开是最好的旁证:`cargo install` → `~\.cargo\bin`(不在 LOCALAPPDATA 下)真的更新了,桌面端 `Copy-Item` 进了影子。
- 修复: 动手之前先探测重定向——往 `%LOCALAPPDATA%` 写一个探针文件,若它同时出现在 `%LOCALAPPDATA%\Packages\*\LocalCache\Local\` 下,即判定当前环境无法安装桌面端,清理探针后 throw 并给出容器外的安装命令。装不上要当场说,不能事后骗。CLI 那半不受影响,照常安装并在报错前明确告知。
- 验收: 在本机(容器内)单独跑该探针段确认触发,并打印出真实影子目录 `...\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Local`;探针前后清理干净(真实与影子路径均不残留);`release.ps1` 语法解析通过。
- 备注: 未做:没有让脚本自动改走容器外安装——跨出 AppContainer 不是脚本能可靠做到的事,正确的收口就是拒绝并把命令交给用户。
- 证据等级: E1(探针实证重定向 + 用户机器版本停留实证)
- 优先级: P1
- 标签: 发布

## D-200 设置页模型角色与脏标记静态文案未登记 i18n 资源表,英文界面残留中文且冒烟恒红 [fixed] (medium)
- 优先级: P1
- 修复: 在 I18N_EN 补齐 6 条英译,重跑 ui-i18n-smoke 至绿。
- 复现: crates/kanzei-app/ui/index.html 有 6 处静态中文 title/按钮文案未登记进 main.js 的 I18N_EN 资源表:①#set-primary/#set-fast 的 title「从已探测到的模型中选择;端点不提供列表时可手填」(348/352 行)②#models-refresh 的 title「重新向各 provider 探测可用模型」与按钮文本「重新探测模型」(356 行)③#fast-setup 的 title「自动完成:安装 Ollama(winget)→ 启动服务 → 拉取 fast 模型」与按钮文本「一键就绪子代理」(361 行)④#settings-dirty 的文本「未保存 — 改动要点『保存』才会写入配置并生效」(453 行)。i18n 冒烟断言 node scripts/ui-i18n-smoke.mjs 报 ERR_ASSERTION「HTML 静态文案未进入资源表」,列出这 6 条。
- 影响: 英文界面下模型角色区与设置脏标记残留中文,且违反 M-014(HTML 静态文案必须登记资源表),ui-i18n-smoke 恒红,后续任何前端改动都会带着这条失败验收。
- 标签: 前端
- 根因: R-136(2c999d4,一键就绪子代理 UI)与 D-158(548fe5f,settings-dirty 提示)加 HTML 时只写了中文静态文案,没同步登记 I18N_EN。applyLanguage 对文本节点与 title 属性只认 I18N_EN/I18N_DYNAMIC_EN 整串翻译,缺登记时英文模式下这两处界面残留中文(localizeDynamic 短语级兜底覆盖不到整串)。
- 证据等级: E1(冒烟断言实测失败/修复后通过)
- 验收: node scripts/ui-i18n-smoke.mjs 通过(不再报 HTML 静态文案未进入资源表);英文模式切换后这 6 处显示英文(applyLanguage 整串命中)。
- 状态: fixing

## D-203 trim_tail 用未校准估算够校准过的预算线,校准越准洞越大 [fixed] (medium)
- refs: D-181
- 复现: a119eeb 同时引入估算校准(calibration,EMA 逼近真实 usage/估算比)与 trim_tail(压缩后兜底砍尾)。三个预算比较调用点都乘了 calibration,trim_tail 内部(runner.rs)却用原始 `estimate_prompt_tokens` 够同一条 budget。calibration 为修正"真实 token 高于估算"(中文 \uXXXX 转义)而生,典型值 >1:trim_tail 按原始口径够线即收手,调用方校准视角仍超线。
- 影响: 下一步预算检查立刻再压——连续两次压缩 = 缓存前缀两次全量重算(cache_write 双倍),恰是 trim_tail 存在的理由;两个修复写在同一个提交里互相拆台,校准越准,提前收手的缺口越大。
- 根因: "预算比较必须走校准口径"只是散落在三个调用点的乘法,没有收敛成一个入口;新加的第四个比较点(trim_tail 内部)自然漏掉。
- 修复: 新增 `budgeted_tokens(system, messages, specs, calibration)` 作为预算比较唯一入口,三个调用点与 trim_tail 全部走它;trim_tail 增收 calibration 参数。update_calibration 的输入仍是原始估算(last_estimated)——乘了校准就是拿自己的输出当输入,EMA 会发散,两个函数分开命名即为此。
- 验收: 新增单测 `trimTail按校准口径收线_调用方视角不再超预算`:预算选在"原始口径已达标、校准口径仍超线"的区间,断言收完后校准视角 ≤ budget,并断言 calibration=1.0 时退化为老行为。反向验证:把 trim_tail 内部改回原始估算,该测试立即红。workspace 300 项全绿。
- 证据等级: E1(反向注入验证护栏会红)
- 优先级: P2
- 标签: 核心

## D-201 开发规范只注入前 3000 字符:16% 送达,「关闭边界」整节被截 [fixed] (high)
- refs: D-191
- 复现: `dev/conventions` 注入源(crates/kanzei-tools/src/profiles.rs)`text.chars().take(3000)`,而本仓库 conventions.md 是 151 行 / 14944 字符——只送达前 24 行(16%),截断点正好落在 `## 1.2 关闭边界:可用即关闭` 的标题上,§1.2 起全部规则从未进过上下文。
- 影响: 同一份文件的前后对照可证:§1.25「关闭前逐条对照验收」因**同时写进了 dev system prompt** 而被严格遵守(近期条目验收证据详实);§1.2「不因缺验证增强项长期滞留 fixing」只存在于被截断部分,于是 11 条 high 缺陷带着已发布的修复卡在 fixing。被投喂的规则被遵守,被截断的没有——是投递问题,不是纪律问题。截断提示只说"规范过长已截断",不说少了哪几节,模型无从判断要不要去 read。
- 根因: 把用户的常驻定调当成了可按预算取舍的参考资料。规范与 CLAUDE.md 同类,口径应是全量注入;要控成本应精简规范本身,而不是引擎悄悄替用户决定哪几条不算数。
- 修复: 去掉 3000 字符上限,conventions.md 全量注入(2026-08-09 用户定调:对齐 Claude Code 对 CLAUDE.md 的全量注入行为);截断提示随之删除。
- 验收: 单测 `开发规范全量注入不做字符截断`——夹具正文 >3000 字符且关键规则在尾部,断言尾部规则出现在 system_baseline 且不再出现截断提示(旧实现 take(3000) 必然切掉尾部,该测试对旧代码必红);kanzei-tools 100 项通过。
- 证据等级: E1(截断点落位实证 + 前后对照行为差异)
- 优先级: P1
- 标签: 核心

## D-171 启动黑屏:孤儿 msedgewebview2 进程锁住 WebView2 数据目录 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 代码 743d4e4 在库(启动/交接前清孤儿);今天多次 kzapp 重启无黑屏,正常退出后 kanzei webview 全清(实测 0 残留)。验收②的"他实例存活不误杀"由实现的存活检查覆盖。残余:强杀场景无定向复现,黑屏若复发按新缺陷报。
- 复现: 父 kzapp 被强杀(更新交接、任务管理器、崩溃)时 WebView2 子进程存活,继续握着 `dev.kanzei.app/EBWebView` 数据目录的目录锁;下一个实例的 WebView 初始化失败,窗口就是一块黑。实测本机曾积累 6 个存活 7 小时的孤儿 msedgewebview2。
- 根因: 强杀父进程不会自动回收 WebView2 子进程;新实例启动时 WebView 初始化被孤儿进程的目录锁挡住,与 D-172(i18n 死循环)是两个独立的黑屏根因。
- 修复: `cleanup_orphan_webviews()`(crates/kanzei-app/src/main.rs)——只杀命令行带 `dev.kanzei.app` 的 msedgewebview2.exe,且只在**没有其他 kzapp 实例存活**时动手(别的实例在,它的 webview 就不是孤儿);在主流程窗口创建前与安装交接前调用。
- 验收: ①强杀 kzapp 后残留孤儿 webview,重启 kzapp 不再黑屏;②有其他 kzapp 实例存活时不误杀其 webview;③更新交接路径(install helper)也清孤儿,避免新实例黑屏。
- 证据等级: E1(逻辑自洽 + 注释记录实测 6 个孤儿)
- 优先级: P0
- 备注: 2026-08-08 并行环节已写好修复代码(工作区未提交),本条目补登记;此前被误判为编号空洞补过 tombstone,已撤销纠正。

## D-175 安装器只发 kzapp 不发 kz CLI:schema 迁移后旧 CLI 直接打不开 state.db [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 验收⑤今天实证:53bb8e7 真实安装后 kzapp 首启同步 CLI,.kz-synced=ccfecff、~/.cargo/bin/kz.exe 版本随包前进;只升不降实测生效(未覆盖手装的更新构建)。①sidecar 打包②同步③v4/v5/v6.bak 真实存在④文案断言⑤自动化测试均齐。
- 复现: 2026-08-08 发布 build-0c9f903(含 store schema v4→v5)。静默装完 setup.exe 后,`kz --version` 仍是 430d6d6(SCHEMA_VERSION=4);一旦启动新 kzapp 把 `.kanzei/state.db` 迁到 v5,旧 kz 在 `SessionStore::open` → `migrate` 处命中 `version > SCHEMA_VERSION` 直接返回 UnsupportedSchema,`kz run` 完全不可用。本次靠手动 `cargo install --path crates/kanzei --force` 救回,安装器缺口未变。
- 根因: 桌面端与 CLI 是两个独立安装通道(NSIS → %LOCALAPPDATA%\kanzei;cargo install → ~\.cargo\bin),却共用同一个 `.kanzei/state.db`;package.ps1 只打包 kzapp,没有任何机制让安装器更新 CLI。以前 CLI 落后只是"旧",引入 schema 迁移后变成硬失败。
- 影响: ①任何 schema 变更发版即弄坏机器上的 kz,而 kz 是自举循环的入口;②迁移单向且此前无备份,回退到上一版 kzapp 同样打不开库,发布事实上不可回滚;③UnsupportedSchema 文案只说"不兼容",不给出路,容易诱导用户删库,而删库丢的是全部会话历史。
- 验收: ①安装包内随附与 kzapp 同一次构建的 kz.exe;②安装后首次启动 kzapp 能把 CLI 同步到 ~\.cargo\bin,且只升不降(开发者手动装的更新版本不被覆盖);③schema 升级前自动留下可打开的整库备份;④UnsupportedSchema 文案给出桌面端与 CLI 各自的升级动作并明确禁止删库;⑤上述均有自动化测试,且发一版真实安装验证 CLI 版本随 kzapp 一起前进。
- 修复进展(2026-08-08): package.ps1 打包前构建 kz 并作为 sidecar 注入(externalBin 经 `--config` 只在打包时生效,避免 tauri-build 在 build script 阶段校验 sidecar 而弄挂所有普通 cargo build);kzapp 启动调用 sync_bundled_cli 同步到 ~\.cargo\bin,标记文件走快路径、只升不降;SessionStore 升级前 `VACUUM INTO` 留 `state.db.v<n>.bak`(WAL 下直接拷 .db 会拿到残缺快照);UnsupportedSchema 改为携带 found/supported 并给出可执行指引。
- 验证(2026-08-08): kanzei-core 51 项、kzapp 33 项、kanzei-tools 82 项、kanzei-harness 38 项通过,含备份一致性、更高版本拒绝打开与文案断言、CLI 同步只升不降。验收⑤(真实安装后 CLI 版本随包前进)待本次发版装完确认,故保持 fixing。
- 优先级: P0
- 标签: 发布

## D-176 同一目录裂成两个会话 id(扩展长度路径前缀),历史与队列互相看不见 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 真实库实证:修复(08-07 09:06)之后零新增 \?\ 形态会话,全部收敛到 ses_project_c0b8d633…;带前缀的仅剩 2 条历史孤儿。验收⑤测试在。残余:2 条历史孤儿会话不迁移(改名=历史失联,正是验收④禁止的)。
- 复现: 本仓库的 state.db 里同一个目录有两条会话:`ses_project_c0b8d633186c2464`(project_root `C:\Users\kanzei\Documents\kanzei code`)与 `ses_project_ce2fce953a5e4103`(project_root `\\?\C:\Users\kanzei\Documents\kanzei code`)。桌面端的运行落在后者(1090 条事件),CLI 落在前者,同一项目的历史互相看不见。
- 根因: 桌面端 `normalized_project_root` 内含 `std::fs::canonicalize`,Windows 上返回带 `\\?\` 扩展长度前缀的路径;CLI 走裸 `discover_project_root` 不做 canonicalize。而 `project_session_id` 只做 `to_lowercase()` 后哈希原字符串,不做任何路径规范化,于是两种写法哈希出两个 id。代码里 5 处 Tauri 命令带着"会话 ID 必须与运行/写入侧同源(D-058)"的注释,说明此坑踩过一次,但当时只靠"都记得调 normalized_project_root"的约定对齐,CLI 侧没跟上——约定而非门禁。
- 影响: ①历史对话在桌面端与 CLI 之间不复用,表现为"历史时有时无";②队列、输入状态、episode 画像同样分裂,跨端度量失真;③会话越多,state.db 里同一项目的孤儿会话线越多。
- 验收: ①同一目录的裸路径、`\\?\` 前缀、大小写差异、末尾分隔符四种写法收敛到同一个会话 id;②`\\?\UNC\` 映射回普通 UNC 写法;③不同目录仍是不同会话;④裸路径形态的身份串保持不变(否则既有会话集体改名、历史失联);⑤有测试锁住上述不变量,而不是继续靠注释提醒。
- 修复进展(2026-08-08): `project_session_id` 改为先经 `session_identity` 规范化——剥 `\\?\` / `\\?\UNC\` 前缀、去末尾分隔符、小写。分隔符刻意不统一:裸路径的哈希必须与历史一致,否则所有既有会话一次性改名。选型由用户定为"向后兼容、不迁移存量"。
- 验证(2026-08-08): kanzei-core 新增「同一目录的各种路径写法收敛到同一个会话id」,含向后兼容的身份串断言(不断言哈希字面量——DefaultHasher 跨 Rust 版本不保证稳定)。真实桌面端确认待发版后进行,故保持 fixing。
- 优先级: P0
- 标签: 后端
- 备注: 采用向后兼容方案后,桌面端会切回裸路径 id,`ce2fce953a5e4103` 那条线的 1090 条事件成为孤儿(数据仍在 state.db 中,未删除)。

## D-177 上下文压缩只在轮末检查,长轮与被停止的运行一次也轮不到 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 每步开跑前预算检查在主循环;估算含 system/历史/工具 schema(D-192 补齐),D-203 又收敛为 budgeted_tokens 单口径;ContextCompacted 事件 UI/CLI 可见;主动/被动额度分记;测试锁住。
- 复现: 事件流 seq 1073-1076:18:35:55 提升输入并 running,19:17:04 用户停止,`reason=stopped_by_user`,**没有 run.completed**。而压缩检查写在 run.completed 之后那一段(`estimate > limit*7/10` 才调 fast_summarize),整整 41 分钟里检查点执行 0 次。
- 根因: 上下文预算只在一轮**结束之后**评估,而长轮与自动续跑恰恰是最需要它的场景——一轮不结束就一次也轮不到,中途停止更是直接跳过收尾。轮内唯一的上下文管理是 runner 的 `recover_context_overflow`,它只在 provider 已经报 overflow 之后才动,于是实际行为是"一路涨到撞墙,撞了才被动裁剪"。另:轮末估算漏算工具 schema,而 schema 每步整份重发,在工具多的 profile 下是常驻大头。
- 影响: ①长轮的上下文成本不受控,只能靠撞墙兜底,而撞墙那次请求本身已经浪费;②被动裁剪发生在错误路径上,裁剪力度不可选;③用户观感是"跑了一大波压缩从没触发"。
- 验收: ①每步开跑前按 context_limit 主动估算并在到达预算线时就地压缩;②估算把 system、历史与工具 schema 三者都计入;③压缩保留当前用户消息,并把被裁段落沉淀为可核对轨迹;④主动压缩与撞墙后的被动恢复各记各的额度,主动让路不吃掉被动重试;⑤UI 与 CLI 能看见"何时让路、让掉多少";⑥有测试锁住估算口径与压缩效果。
- 修复进展(2026-08-08): RunnerConfig 增 context_limit;每步请求前按 CONTEXT_BUDGET_RATIO=0.7 估算并触发 `compact_messages_for_retry`,上限 3 次;新增 RunEvent::ContextCompacted,桌面端写 run.trace + kz:status,CLI 打印一行;estimate_prompt_tokens 计入工具 schema。轮末那次压缩保留作兜底。
- 验证(2026-08-08): kanzei-core 新增「上下文估算把工具schema计入并按预算线判定」「主动压缩显著缩小上下文且保留当前用户消息」;workspace 259 项通过。真实长轮触发待发版后观察 run.trace 的 context.compacted 记录,故保持 fixing。
- 优先级: P0
- 标签: 核心
- refs: D-176

## D-178 git 工具 stage 静默失败:normalize_resource Windows 小写化破坏大小写敏感路径 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 回归测试在 git.rs:413(大小写敏感路径 stage);normalize_resource 安全校验保留、传 git 原始大小写。
- 复现: git stage .kanzei/memory/INDEX.md 返回 "nothing is staged after this request"。根因: git.rs:148 normalize_files 用 kanzei_harness::permission::normalize_resource(raw) 规范化路径, 该函数在 Windows 上 to_lowercase 整个路径(permission.rs:167-168), git pathspec 大小写敏感, 转小写后匹配不到磁盘上的 INDEX.md/M-016-*.md/Cargo.lock 等含大写字母的文件, git add 成功但零暂存, stage 报 nothing staged。对照: probe-test.txt(全小写) 可正常暂存。
- 影响: 任何含大写字母的路径(INDEX.md、M-016/M-017 记忆文件、Cargo.lock)都无法通过 git 工具暂存, memory 文件提交被卡; 用户直接用 bash git add 不受影响。
- 验收: git stage 对含大写字母路径能正常暂存并返回 staged_hash; 保留 normalize_resource 的安全校验(逃逸/目录检查)但传给 git 的路径保持原始大小写; 补大小写路径回归测试。
- 严重程度: high
- 优先级: P2
- severity: high

## D-179 停止运行时 abort 先于收尾,整轮轨迹与 episode 全部丢失 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 6852d82 交付:停止先落库再 abort,失败轮同样落库,幂等;验收⑤测试锁住。今天自举多轮停止/续跑无轨迹丢失报告。
- 复现: 2026-08-08 一次 41 分钟的运行(事件流 seq 1073-1076)被用户停止后,该会话只留下一条 `session.status_changed {"reason":"stopped_by_user"}`——没有 run.trace、没有 episode、输入状态也没有结局。对照正常结束的轮次(seq 1084-1086)三者齐全。
- 根因: `stop_runtime_and_finalize`(crates/kanzei-app/src/main.rs)先 `handle.abort()` 再收尾,而写 run.trace / append_episode / finish_input 的代码全在被 abort 的那个 task 里,先杀后写等于什么都不写。失败轮次同理:`let summary = run_result?;` 在写轨迹之前提前返回,`run.failed` 之外一样什么都不留。
- 影响: ①最值得复盘的运行(长到不得不停)恰恰一个字节都不留,D-173 补的运行审计在这类轮次上等于没做;②工具耗时、权限决策、token 统计全丢,"时间花在哪"仍然只能靠猜;③D-177 的轮内压缩是否真的触发,在被停止的轮次里无法验证。
- 验收: ①停止时先把在飞轨迹与 episode 落库再 abort;②失败轮次同样落库;③episode 的步数与 token 取自逐步累计的真实值,不是补零;④正常收尾与停止路径不重复写(幂等);⑤有测试锁住"停止后 run.trace 与 episode 都在,且再停一次不产生第二条"。
- 修复进展(2026-08-08): SessionRuntime 增 `live: Arc<Mutex<LiveRun>>` 在飞画像(run_id/input_id/provider/model/步数/token/轨迹),挂在 runtime 上而不是 run_task 局部,停止路径才够得着;`flush_live_run` 幂等落库,停止路径在 abort **之前**调用,失败分支也调用;TurnStart/StepEnd 逐步累计步数与 token。
- 验证(2026-08-08): kzapp 新增「停止时在飞轨迹与episode先落库再abort」,断言 outcome=halted、步数取真实值、归属列齐全、重复停止不产生第二条 episode;kzapp 34 项通过。真实桌面端停止验证待发版后进行,故保持 fixing。
- 优先级: P0
- 标签: 后端
- refs: D-173、D-177

## D-180 v5 之前遗留的 promoted 输入未回填,仍会被后续停止追认为 cancelled [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 96acfdf(v7)从迁移前备份捞回状态位;保护窗、缺 promoted_at 视为存量、回填后不再改写均有测试。
- 复现: 装上 v5 后查本机 state.db:`promoted 195 / cancelled 187 / running 1`。那 195 条是 v5 之前跑完的输入——当时没有 completed 终态,它们永远停在 promoted。而 `finalize_interrupt` 取消 `pending/promoted/running`,所以用户下一次按停止,这 195 条历史上早已跑完的输入仍会被一并改写成 cancelled。
- 根因: v5 只加了新状态与新写入路径,没有回填存量。新记录不再被污染,存量却仍在被反复追认——修了一半。
- 影响: 历史输入的状态位不可信,按状态做的任何统计(完成率、取消率)都失真;且每停止一次就再污染一次,不是一次性损失。
- 验收: ①迁移把存量 promoted 回填为 completed;②保护窗内(可能正被另一个进程执行)的 promoted 不回填;③promoted_at 缺失的老记录同样视为存量;④回填后再停止,已回填的记录不再被改写;⑤有测试锁住上述四条。
- 修复进展(2026-08-08): SCHEMA_VERSION 提到 6,迁移中回填 `promoted → completed`,保护窗 5 分钟(桌面端与 CLI 共用同一个库,可能有另一进程正在执行)。completed 是**迁移推断值**不是观测值:v5 之前根本没有记录结局的地方,只能按"被提升了就说明当时确实执行过"判定,已在代码注释与本条目写明。
- 验证(2026-08-08): kanzei-core 新增「迁移把遗留promoted回填为completed但不动可能在飞的输入」,含回填后再停止不被改写的断言;store 24 项通过。
- 优先级: P1
- 标签: 后端
- refs: D-173
- 备注: 回填口径(推断为 completed 而非保持现状)由用户 2026-08-08 定调。
- 续修(2026-08-08): v6 回填在真实机器上扑空——22:03 与 22:37 两次停止已把存量 promoted 全抹成 cancelled(384 条),v6 上线时一条 promoted 都不剩。唯一还留着原始状态的是 `state.db.v4.bak`(promoted 196 / cancelled 185)。经用户确认"捞",新增 v7:迁移前 ATTACH 同目录的 `state.db.v*.bak`,把**备份里是 promoted、现库是 cancelled**的输入恢复为 completed。判定以备份为权威而非猜测:备份里已是 cancelled 的是当年真取消,不动;备份里根本没有的(如 21:40 那条真被停掉的)更不动。恢复条数写入 schema_meta.legacy_inputs_recovered 供事后核对。
- 验证(2026-08-08): kanzei-core 新增「v7从备份恢复被抹掉的输入状态位且不误伤真取消」「v7在没有备份时安静通过」,含幂等性断言;workspace 269 项通过。
- 迁移与回滚: v5→v6 只有一条 UPDATE,无表结构变更;回滚把 SCHEMA_VERSION 改回 5 即可,已回填的 completed 对 v5 代码是合法值(v5 的 CHECK 已含 completed),不会打不开库。

## D-181 主动上下文压缩复用应急截断:一次砍掉 97% 且保留的是最旧内容 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): a119eeb 三段式(head/纪要/近期工作区)+digest_plausible 质量门槛+回落节选;D-203 补齐校准口径。验收各条有测试;反向验证过纪要拒泛化。
- 复现: D-177 把主动预算线接到了应急函数 `compact_messages_for_retry` 上。该函数把除当前用户消息外的全部历史拍成一个 8000 字节文本块:deepseek 128k 的预算线是 89,600 token,触发后掉到约 2,000 token,一次砍掉 97%。且其累积循环从 index 0 正序、攒够即停,保留的是开场白,丢掉的是刚做完的工作。
- 根因: 应急路径与主动路径的定位被混为一谈。应急发生在 provider 已经拒绝请求之后,粗暴但必须一次成功,合理;主动发生在还有三成余量、也有时间的时候,没有任何理由推倒重来。另有两个附带缺陷:①`remaining = 8_000 - history.len()` 按字节算却用 `chars().take(remaining)` 取字符,中文实际超额约三倍,那个上限名不副实;②`Part::ToolCall` 被整个 skip,只留下工具输出而不知道是哪个工具、什么入参产生的。
- 影响: ①压完模型不知道自己刚做了什么,长轮大概率原地重做,压缩反而放大成本;②轮末那条像样的 `fast_summarize` 已确认长轮轮不到,于是形成"好的不跑、跑的不好";③`MAX_PROACTIVE_COMPACTIONS=3` 是假的——第一次就压成一个块,后两次只是重复截同一个块。
- 验收: ①主动压缩保住首条用户消息(任务定义)与最近工作区逐字不动,只压中段;②中段交 fast 模型出结构化纪要,要求写出具体文件/函数/标识符而非泛化;③纪要不可用时回落到截断,但只截中段;④中段为空时不计入压缩次数,不吃重试额度;⑤应急路径改为保留最近内容而非最旧;⑥按字符截断,中文不超额;⑦保留内容含工具名与关键入参;⑧有测试锁住上述各条。
- 修复进展(2026-08-08): 新增 `compact_with_digest`(三段式:head 逐字 / 中段纪要 / 近期 RECENT_VERBATIM_RATIO=0.35 逐字),抽走中段后用 `filter_message_history` 清孤儿工具部件;`digest_segment` 走 SubagentRuntime.fast 那条 route,失败回落只截中段;`clip` 统一按字符截断;应急路径改为从最近往回收并纳入 ToolCall。fast 模型调用由用户 2026-08-08 明确批准。
- 验证(2026-08-08): kanzei-core 新增「主动压缩保住任务定义与近期工作并只压中段」「应急压缩保留最近内容而非最旧内容」「clip按字符截断且中文不超额」;workspace 266 项通过。**纪要质量未经真实模型验证**——测试里 subagent=None 走的是截断回落,而 fast 是本地小模型 ollama:qwen3.5:4b,能否保住标识符待实测,故保持 fixing。
- 优先级: P0
- 标签: 核心
- refs: D-177

## D-182 应用内更新静默失败:交接 helper 就是安装器要替换的 kzapp.exe,镜像被锁 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 今天 update.log 实证两次完整交接:helper=%TEMP%\kanzei-update-helper.exe(安装目录外、名字不同),全程落盘含父进程退出/exit=0/mtime/拉起结果。验收④"回读核对"当时只记不比——该缺口已拆为 D-199 单独修复(image_stamp 前后比对)。
- 复现: 2026-08-08 22:43 用户点设置页「检查更新」升 build-ea6d058。安装包完整落到 `%TEMP%\kanzei-setup.exe`(9,564,216 字节),但 `%LOCALAPPDATA%\kanzei` 三个文件的时间戳仍停在 21:35,一个都没换,界面上也没有任何失败提示。
- 排除项: ①安装包本身没问题——同一个文件指定目录能完整装出 kzapp+kz(exit 0);②下载没问题——字节数与 release 资产一致;③不是权限问题——kzapp 未运行时同一个包装得进去。
- 根因: `update_install` 用 `Command::new(current_exe())` 起交接 helper,而 `current_exe()` 就是安装器要替换的 `%LOCALAPPDATA%\kanzei\kzapp.exe`。父进程 `app.exit(0)` 之后 helper 仍在跑同一个镜像文件,Windows 全程锁着它,NSIS 覆盖不了。`run_install_helper` 里安装器非 0 退出就 `return` 且**不删安装包**——TEMP 里那个文件还在,与这条路径吻合。
- 诊断为何困难: helper 只用 `eprintln!` 报错,而 GUI 进程没有可见 stderr,失败原因无处可查。这是本缺陷真正卡住排查的地方,与根因同等重要。
- 影响: 应用内更新整条通道失效,只能靠手工静默安装;且失败无声,用户以为装上了,实际仍在旧版——本次正是如此。
- 验收: ①helper 跑安装目录之外的副本,安装目录内无任何被本进程锁住的文件;②helper 名字不同于 kzapp.exe(避免被安装器的关闭运行实例逻辑连带处理);③交接全过程写入 `%TEMP%\kanzei-update.log`,含父进程退出与否、安装器退出码与 stdout/stderr、安装后 exe 时间戳、拉起结果;④安装器报 exit=0 也要回读 exe 时间戳核对,不能只信退出码;⑤helper 副本由下次启动回收;⑥有测试锁住 helper 落点与日志落盘。
- 修复进展(2026-08-08): 新增 `update_helper_path()`(`%TEMP%\kanzei-update-helper.exe`),`update_install` 先复制再起该副本;`run_install_helper` 改用 `output()` 捕获安装器 stdout/stderr,全程 `update_log` 落盘,并在安装后回读 exe mtime;`startup_update` 顺手回收残留的 helper 副本。
- 验证(2026-08-08): kzapp 新增「更新交接helper跑在安装目录之外」,断言落点在 TEMP、名字不叫 kzapp.exe、日志真的落盘;kzapp 35 项通过。真实「检查更新」验证待本版发出后由用户执行,故保持 fixing。
- 优先级: P0
- 标签: 发布
- refs: D-175
- 备注: 排查期间我用 `/D=<临时目录>` 做探针,误以为它只影响本次安装;NSIS 会把该路径写成新的 InstallLocation,导致随后一次 `/S` 装到了临时目录、看起来像"exit 0 却什么都没换"。已用 `/D=%LOCALAPPDATA%\kanzei` 装回并核对注册表。该探针也毁掉了 22:43 当时注册表值这条证据。

## D-183 发版不核对提交区间:并发运行的提交被夹带发布且无人察觉 [fixed] (high)
- 关闭核对(2026-08-09,按 §1.2 可用即关闭收口): 此后每次发版都走 -Ack 门禁(本轮 build-4ad666c/ccfecff/53bb8e7 三次);"数不符必中止"有天然实测——中文提交信息数错那次(6 判成 5)门禁真的拦了发布,ccfecff 修掉解码问题。D-193 又补上 tag 落点,区间起点不再漂移。
- 复现: 2026-08-08 发布 build-ea6d058 与 build-96acfdf 时,`49634b7..HEAD` 区间里含 f73ae6c(21:41)与 5223dc6(21:52)两个并发自举运行的提交。发布流程照常 `merge --ff-only` + `package.ps1 -Publish`,全程无任何提示,发布说明里也只是把它们混在变更列表里,没人核对过。事后靠用户追问才发现。
- 根因: 发布流程只认 HEAD,不看区间。本仓库有并发自举运行提交到同一分支,而它的提交作者、邮箱与人手动提交完全一致,git 元数据分辨不出来——没有任何自动信号,只能靠发布者主动核对,而"靠记得"不是门禁。
- 影响: ①发出去的内容可能包含未经审阅的改动,发布说明失真;②本次两个提交只碰 `.kanzei/` 文档没动 crates/,二进制未受影响,但同一条路径下一次就可能夹带源码;③与 D-173 系列建立的"提交范围可核对"原则自相矛盾——工具层拦住了提交夹带,发布层却没有对应门禁。
- 验收: ①package.ps1 构建前摊开 `<上个 build-* 标签>..HEAD` 的完整提交清单,并标出每条是否触碰源码;②发布者必须用 `-Ack <条数>` 明确确认,不传直接中止、传错也中止;③release notes 与门禁使用同一个区间变量,两处口径不得各算各的;④发布树源码工作区不干净时中止,避免构建产物与标签不对应;⑤门禁本身经过实测(不传 -Ack、传错数各一次)。
- 修复进展(2026-08-08): package.ps1 新增 `-Ack` 参数与区间清单打印([源码]/[文档] 标注)、`$range` 变量供 notes 复用、发布树 crates/scripts/ui 脏工作区检查。
- 验证(2026-08-08): 截取门禁段落在发布树上实跑两次——不传 -Ack 报"核对上面 2 个提交…加 -Ack 2 重跑";传 -Ack 99 报"你确认的是 99 个提交,实际区间里有 2 个"。两条路径均按预期中止。
- 优先级: P1
- 标签: 发布
- refs: D-175
- 备注: 这是流程缺陷不是代码缺陷,由本次发布者(AI 助手)在自检中发现并主动登记;此前两次发布已既成事实,不追溯撤回。

## D-206 主动压缩设总量配额:三次成功压缩后整条路径放飞,长 run 上下文打满 [fixed] (high)
- refs: D-203 D-181 D-177
- 复现: 2026-08-09 用户实测(5aba389,deepseek-v4-flash 自举):第 58 步的长 run,单步「等待模型」863s,压缩不再触发。取证确认 `MAX_PROACTIVE_COMPACTIONS=3` 的配额已被前三次**成功**压缩耗尽——计数条件是 `dropped_messages > 0`,压缩成功也扣次数;配额用尽后 `before > budget` 恒真但整个压缩分支不再进入。
- 影响: 对无步数上限的自举 run,上下文反复涨到预算线(0.7×128k≈89.6k)是常态,压缩是常规运营动作。配额用尽后上下文一路涨向 context_limit,prefill 时间随之飙升(实测单步 863s),直到 provider 报 overflow 才走被动恢复——主动压缩在最需要它的场景(长 run 后半程)提前退场,而这正是 D-177 把压缩检查搬进轮内想解决的场景。
- 根因: 注释写的判据是"压缩后仍超线说明再压无益,别空转"(无进展刹车),实现却是总量计数(成功也扣)。把"防空转"写成了"限总量",两个概念混在一个计数器里。同型问题一处:`budget_checkpoint = matches!(step, 20|40|80)` 把周期性盘点写成有限清单,第 80 步后长 run 永不再盘点。
- 修复: ①计数改语义:`futile_compactions` 只数连续无效——压完(含 trim_tail 兜底后)回到线内即清零、不限次数;压完仍超线或中段为空压不动才 +1,连续 2 次停(head+当前消息本身超线,交被动恢复);②盘点检查点抽 `is_budget_checkpoint`:第 20/40 步,之后每 40 步一次,长跑不熄火。
- 验收: 单测 `压缩刹车只认连续无效不设总量配额`(模拟三次成功压缩后计数为 0、第四次仍被允许——旧实现同序列后是 3、被拒;连续两次无效刹车;成功一次即复位)与 `盘点检查点长跑不熄火`(120/160/400 步仍盘点)。workspace 302 项全绿。
- 备注: 配额审计范围(用户要求全局检索):15 个配额类常量逐一判定,其余合理——MAX_STREAM_RESTARTS/MAX_CONTEXT_OVERFLOW_RECOVERIES/传输与限流重试(重试同一动作、有递进终点)、MAX_TASKS_PER_TURN/MAX_PARALLEL_TOOLS_PER_WAVE(并发资源界)、MAX_FAILURE_NOTES_PER_RUN(inbox 信噪比)、其余为尺寸/截断界。仅压缩配额与盘点清单两处属"把常规/周期动作当有限次动作"。
- 证据等级: E1(用户实测 + 计数路径代码实证)
- 优先级: P1
- 标签: 核心

## D-187 KANZEI_HOME 只有 memory 认,配置与 markdown 组件仍走真实 HOME [fixed] (medium)
- 复现: `crates/kanzei-tools/src/memory/mod.rs` 读 `KANZEI_HOME` 决定记忆根;而 `crates/kanzei-harness/src/config.rs`(全局 kanzei.toml)与 `crates/kanzei-harness/src/markdown.rs`(agents/commands/skills)直接用 `dirs::home_dir()`。设了这个变量之后,记忆搬走了、配置与组件还在真 HOME。
- 影响: 半个覆盖比不覆盖更容易骗人——用它做隔离测试或多实例并存时,会以为整个 kanzei 目录都换了位置,实际只换了记忆;两处根不一致导致的现象很难归因。
- 根因: KANZEI_HOME 是记忆模块单独引入的,没有提升为全局 home 解析入口。
- 验收: 要么全局统一(所有 `~/.kanzei` 消费点走同一个 `kanzei_home()` 函数,含 config/markdown/app.json/agent-containers),要么去掉 KANZEI_HOME 只保留 memory 内部用途并改名;不留"只覆盖一半"的状态。有测试覆盖。
- 证据等级: E2
- 优先级: P3
- 标签: 核心

- 进展: 修复(2026-08-09):新增 kanzei_harness::home::kanzei_home()(crates/kanzei-harness/src/home.rs)——KANZEI_HOME 优先、dirs::home_dir()/.kanzei 兜底,并 re-export 为 kanzei_harness::kanzei_home。全部 ~/.kanzei 消费点统一改走它:config.rs:159(全局 kanzei.toml)、markdown.rs:15(agents/commands/skills)、memory/mod.rs global_memory_root、app main.rs prefs_path/global_config_path/agent_container_path。语义不同的 home 消费点(dirs::home_dir 的 is_home_root/discover_project_root 用户边界、~/.cargo/bin、llm 凭证)保持 dirs 不变。新增单测两条(env 优先/默认回落)。workspace 304 项全绿。注:cargo test 期间发现某集成测试会把项目根 tracker 文件(D-207 进展字段)写脏,已按 git diff 恢复;污染源排查留 D-159/后续。

## D-188 单元测试探针写进生产更新日志,稀释 D-182 的诊断入口 [fixed] (low)
- 复现: `%TEMP%\kanzei-update.log` 当前全部内容是 5 条"单测探针",时间 2026-08-08 23:08 与 2026-08-09 00:28——测试与生产用同一个绝对路径(crates/kanzei-app/src/main.rs:1367 附近的 update_log 测试)。
- 影响: `update_log` 超 256 KiB 是整文件删,测试写入会稀释乃至挤掉真实的更新交接记录;而这个日志正是 D-182 为"更新过程无从复盘"专门建的入口,现在打开看到的全是测试噪声。
- 根因: 日志路径写死为 `%TEMP%\kanzei-update.log`,测试没有走可注入的路径参数。
- 验收: 测试写到独立临时文件(路径可注入或按 pid 隔离),生产日志里不再出现"单测探针";补一条断言防回归。
- 证据等级: E1(本机日志内容实证)
- 优先级: P3
- 标签: 后端

- 进展: 修复(2026-08-09):update_log 抽出显式路径版本 update_log_at(path,line)(crates/kanzei-app/src/main.rs:583 附近),生产 update_log 委托 update_log_path() 不变;测试改走 pid+nanos 隔离的独立临时文件,并新增反向断言——本次探针标记若出现在生产日志即失败(防回归)。旧探针日志已手动清理。kanzei-app 39 项测试全绿。

## D-208 历史轨迹回放字段断链:百条同名条目带无效停止按钮,git 工具缺 log [fixed] (medium)
- refs: R-095 D-182
- 复现: 2026-08-09 用户截图。①打开历史对话后活动栏出现 120 条完全相同的条目("task 历史子代理轨迹/历史轨迹/回放"),每条还带「停止」按钮——历史轨迹的子代理早已结束,按钮不可能有效;②模型调 `git {"action":"log"}` 被拒 "unknown action"。
- 根因: ①`renderRecoveredTraces`(ui/main.js)读的是不存在的 `event.text/event.trace` 字段,name 硬编码 "task"、标题硬编码"历史子代理轨迹"——而 run.trace 事件本来就带 name/summary/ok/durationMs(后端 tool.started/tool.completed),数据在库里,前端没接;类型筛选(R-095)也因 name 失真全归 task 类。②标终态后没重新 `bgRenderActions`,停止按钮残留(该函数对 done 条目本就不渲染停止)。③git 工具只有 status/diff/stage/commit 四个 action,"最近改了什么"这一高频只读查询没有合法通道,模型只能转投 bash(每次 ask)或瞎猜。
- 修复: ①回放按 kind 分发:tool.started 用真实 name+summary 建条目,tool.completed 收敛终态(ok/err、失败原因、耗时进 meta)并重渲染动作区;只 started 没 completed 的(轮次中断)同样收敛,不留假 running;②git 新增只读 log action(--format 单行、count 默认 20 封顶 200、files 路径过滤复用 normalize_files),并发按只读共享,base 权限 Allow(与 status/diff 同级)。
- 验收: 单测 `log_returns_recent_commits_and_honors_path_filter`(全量/count=1/路径过滤三断言);前端 node --check + 运行时冒烟 0 错误;workspace 305 项全绿。回放条目显示真实工具名与目标、无停止按钮,待用户复查。
- 证据等级: E1(用户截图 + 后端字段与前端读取逐一比对)
- 优先级: P1
- 标签: 前端

## D-210 拖拽锁提示不点名具体筛选,持久化残留筛选让"没筛选也拖不了"无从定位 [fixed] (medium)
- refs: D-207 R-115 D-169
- 复现: 2026-08-09 用户实测(c8be9ac):关闭分组后提示变成"有筛选时不可拖拽——清除筛选后可拖",但用户"没筛选也拖不了"。根因:R-115 之后状态/优先级/复杂度/标签/阻塞五项筛选**按项目持久化**,某天设过的筛选重启后仍生效而下拉不显眼;第一版提示(da6c380)只笼统说"有筛选",不点名是哪一项,用户无从定位。另用户反馈提示行样式丑(🔒+长句挤在列表顶)。
- 附带发现: `docDragEnabled` 的 defect 分支只查 status/priority——tag/blocked 筛选下列表同样不完整,commitDocOrder 提交缺条目的顺序会被引擎拒绝,属漏判。
- 修复: ①锁因点名:逐项列出具体条件(如「拖拽调序已锁: 分组视图 · 复杂度=中」),定位不再靠猜;②一键「解锁」按钮:关分组(走现有开关按钮,复用持久化与按钮态)、切回手动排序、五项筛选清为 all,saveDocFilters+syncDocFilterControls+refreshDocs 一步到位;③样式重做成轻卡片(边框圆角、溢出省略、去 emoji);④defect 拖拽守卫补全 tag/blocked,与锁因清单同口径。
- 验收: 四条前端冒烟全绿(a11y 断言更新为钉新守卫形态);用户复查:残留筛选场景下提示能点名到具体项,点解锁后可拖。
- 证据等级: E1(用户截图两张 + R-115 持久化机制实证)
- 优先级: P1
- 标签: 前端

## D-213 标注全军覆没且空库覆盖缓存:思考模型吃光 token 预算,三层缺陷叠加 [fixed] (high)
- refs: R-148
- 复现: 2026-08-09 用户实测:标注跑到 20/231 后重启,"标注过的很多文件标注就没了"。实证:.kanzei/file-annotations.json 存在但为空 store(31 字节);直接 curl ollama 复现——qwen3.5:4b 是思考模型,标注调用 max_tokens=128 全被思考吃光(finish=length,content 空,思考进 message.reasoning 字段);提到 1024 仍然想不完;/no_think 软开关与 openai 兼容层的 think:false 均无效;**只有原生 /api/chat 的 think:false 生效**(thinking 空,正常收尾)。
- 根因: 三层叠加。①fast_one_line max_tokens=128 对思考模型必然产出空正文 → 每个文件都判"空标注"失败,用户看到的 20/231 全是失败计数,**从来没有一条标注成功过**;②全失败的运行照样每 8 个 save 一次——把 load 时的空 store 反复覆盖写盘;③失败原因被吞(annotate Err 丢弃),UI 只报数字不报原因,排查无从下手。另有隐患:save 的 serde 序列化 unwrap_or_default 会把空字符串写进缓存(load 解析失败回落 Default,整库静默清零);指纹用 mtime(git checkout 会大面积假失效)。
- 修复: ①标注后端分流:探测到 ollama(provider 名或 base_url 11434)直连原生 /api/chat + think:false + num_predict 256;其他 provider 走 LlmClient,max_tokens 提到 1024;两侧共用 clean_note 清洗(剥 <think> 块、取最后一个非空行、去引号、封顶 60 字,捞不出正文报错而非产出空标注);②保存纪律:只有真的写入过标注(dirty)才落盘,全失败一个字节不碰缓存;目录标注也只在 dirty 时做;③错误上浮:首个失败原因进返回值,前端 toast 显示;④save 序列化失败报错不写空串;⑤指纹从 size+mtime 换成内容 FNV-1a(扫描本就读全文,零成本;mtime 免疫——重写相同内容指纹不变有单测;oversized 退化 size+mtime);⑥汇总行显示「已标注 X/Y」(分母只数可标注文件,用户点名)。
- 验收: 单测:clean_note 七形态(剥思考/末行正文/引号/空拒绝/截断)、指纹 mtime 免疫+内容敏感;curl 实证 ollama 原生通道产出非空正文;workspace 311 项全绿。真实标注跑通与「已标注 X/Y」显示待发版用户复查。
- 备注: 失效标注不进上下文(用户同轮要求)为既有行为:files 工具与 snapshot 都按 hash 过滤,单测「过期标注不得注入」覆盖。质量注意:qwen3.5:4b 关思考后产出偏弱(实测短片段会复读代码),prompt 已加"不要复述代码";若实测质量不满意,fast 角色换更大模型即可,通道无需改。
- 证据等级: E1(缓存文件实证 + curl 三连复现 + 原生通道验证)
- 优先级: P1
- 标签: 后端

## D-212 文件导览叠进对话页:裸 #view-files 的 ID 特异性压过 .view 隐藏规则 [fixed] (medium)
- refs: R-148
- 复现: 2026-08-09 用户实测(2a2a26f):文件树+Monaco 与对话页的 composer 同屏显示,"不应该显示在主页,我说了独立页管理"。根因:视图显隐由 `.view { display:none }` / `.view.active { display:flex }` 控制,而 R-148 首发写了 `#view-files { display:flex }`——ID 选择器特异性(1,0,0)无条件压过类选择器(0,1,0),文件视图永远渲染、叠进当前 active 视图。
- 修复: 改为 `#view-files.active { display:flex }`,显隐归还 .view 体系;a11y 冒烟新增静态断言——剥注释后扫描,任何裸 `#view-*` 规则设置非 none 的 display 即失败(反向验证:塞回坏规则断言立即命中)。
- 验收: 切到文件页时对话 composer 不可见,切走后文件页完全隐藏;冒烟断言防回归;用户复查。
- 证据等级: E1(用户截图 + CSS 特异性实证 + 反向验证)
- 优先级: P1
- 标签: 前端

## D-211 拖拽解锁后条目可选中但仍拖不动 [fixed] (medium)
- refs: D-210 D-207 R-054
- 复现: 2026-08-09 用户实测(f2f72fb):点「解锁」后锁提示消失、条目可选中,但拖拽仍然不生效("能选但是依然拖不动")。
- 排查线索(给取活者): ①`item.draggable = true` 只在 renderDocList 的 `if (!isGrouped && docDragEnabled(...))` 分支设置——解锁后 refreshDocs 重渲染,确认该分支真的走到(isGrouped 的判定用的是 `reqFilterState.grouped`,解锁走 toggle 按钮 click,状态是否在重渲染前生效?);②dragstart handler 要求 `filters.sort === "manual"` 等条件在 `reqDragEnabled` 里二次判定,两处口径是否一致;③documents 页与侧栏的 filters 对象不同(documentFilters vs reqFilters),解锁按钮改的是传入的 reqFilterState 引用,确认改到的是当前列表用的那份;④浏览器层面:doc-item 内部的 doc-row 有 role=button+tabIndex,click/drag 事件可能被行内可交互元素抢占;⑤D-202 的主线程卡顿也可能吞 drag 事件,复现时注意会话长度。
- 验收: 解锁后(或本就无锁时)侧栏与文档页的需求/缺陷列表均可拖拽并成功落库(docs_update reorder 返回成功、md 文件顺序变化);冒烟或 E2 覆盖"解锁→拖拽→落库"链路;用户复查可拖。
- 证据等级: E1(用户实测)
- 优先级: P1
- 标签: 前端

- 状态: fixed
- 进展: 2026-08-09 取活(需求队列 doing 均推不动:R-101 挂起等缺陷前置、R-148 剩用户复查,故转缺陷队列文档序首位 D-211)。根因定位:docDragEnabled(main.js)首行 `docSurface(listEl) !== "documents"` 拒绝侧栏——侧栏照常渲染锁提示+解锁按钮(renderDocList 4375-4423 无 surface 限制),但 draggable 只在 `!isGrouped && docDragEnabled(...)` 分支设置,侧栏永不设置:解锁后锁提示消失、条目"能选"却拖不动,与用户实测完全吻合;R-123 曾把排序收进文档页,但 D-211 验收要求两侧一致,更新定调覆盖。修复:docDragEnabled 去掉 surface 限制,侧栏与文档页一致可拖(手动+无筛选+非分组时 draggable 设置,拖拽链路 dragstart/dragover/dragend → commitDocOrder → docs_update reorder 为既有实现);docSurface 注释同步修订。冒烟:新增 D-211 块——侧栏解锁→锁提示消失→draggable=true→dragstart/dragover/dragend→docs_update reorder 被调;反向验证(临时恢复旧限制)断言 2 处立即命中,恢复后全绿(222 invoke)。验收③用户复查可拖:待发版安装后用户确认,与 D-210/D-213 同惯例。

## D-215 memory update/merge 可静默丢失复发指纹与 refs,复发检测会瞎 [fixed]
- 现象: R-149 复发检测依赖条目正文里的 [fp:...] 标记,但 memory_update 全量替换 body、memory_merge 不搬运被并条目的指纹与 refs——manager 修订/合并时弄丢标记,引擎从此看不见「记了但没用」,且无任何报错。
- 修复: ①MemoryUpdateTool 加引擎闸:新 body 丢失旧 body 任一 [fp:...] 标记即拒绝并点名(只闸 manager 写路径,UI memory_entry_save 用户直写不受限,A-005);②store.merge 自动搬运:被并条目的指纹追加进 primary 正文「(并入指纹: ...)」,refs 取并集写回;③fp_markers() 提取器排序去重、容忍未闭合标记。
- 进展(2026-08-09 修复): manager.rs MemoryUpdateTool 指纹闸+测试 memory_update_不许弄丢正文里的复发指纹(丢弃拒绝/保留放行/只改 description 放行);store.rs merge 搬运+测试 merge_自动搬运被并条目的指纹与refs(指纹并入后 find_active_by_marker 命中 primary、refs 并集、墓碑语义不变);mod.rs fp_markers+测试(排序去重/无指纹/残缺容忍)。workspace 全量绿。
- refs: R-149 (medium)

## D-216 prompt_hints 与常驻 memory-index 双重注入,preference 召回是纯遥测噪声 [fixed]
- 现象: 常驻 memory-index 已含全部预算内 active 索引行,prompt_hints 再重复其中 top3 整行(37 轮实证:M-003 被"召回"23 次但每次本就在常驻索引里);preference 正文全文常驻,hints 再提是零信息,却占召回遥测(M-002 召回 22 次全是噪声)。
- 修复: ①索引行预算走查抽成 memory::resident_index(),注入侧与 hints 共用同一口径(MEMORY_CONTEXT_BUDGET 移入 memory 模块);②hints 对常驻条目只给「id 标题(见 memory-index)」短指向,被预算折叠的条目才给全行;③preference 整类不进 hints、不记召回遥测。
- 进展(2026-08-09 修复): mod.rs resident_index+prompt_hints_with_budget;profiles.rs dev/memory 消费 resident_index(冷启动判定改为 lines 空且 folded=0);测试 hints_不重复常驻索引_折叠条目才给全行_preference_不进提示(短行断言/全行断言/遥测无 preference)。workspace 全量绿。
- refs: R-149 R-125 (medium)

## D-218 GitHub Actions 干净 checkout 缺少 .kanzei/project/tests.md 导致 test_record 测试失败 [fixed] (high)
- 复现: Actions run 31291964471；cargo test --workspace；test_record::tests::tool_records_and_returns_snapshot_text 在 crates/kanzei-tools/src/test_record.rs:336 断言 root.join(TEST_RUNS_REL).exists() 失败。
- 影响: CI 独立验证在干净 checkout 失败，R-152 无法满足首跑全绿。
- 来源: R-152 GitHub Actions 首跑
- 标签: 流程
- 进展: 已修复 `crates/kanzei-tools/src/test_record.rs:251-254`：temp_project fixture 创建 `.kanzei` 标记，使 ToolCtx::new 的 project_root 稳定为 fixture 根；未改生产逻辑。定向 `cargo test -p kanzei-tools test_record::tests --lib` 6/6 通过，`cargo test --workspace` 全量通过。待修复提交后的 GitHub Actions 复跑全绿后，逐条核对验收并关闭。
- 关闭核对(2026-08-09): 验收逐条满足——①干净 checkout 全绿:修复提交起 Actions 连续三跑 success(runs 31292345597 f2b5323 / 31292710503 fe0c8f2 / 31292885059 cd85360);②未 skip/忽略任何测试:修复是 fixture 加 `.kanzei` 标记(test_record.rs:251-254),测试契约不变;③本地与 CI 同一契约:本地 verify.ps1 与 CI 跑同一 `cargo test --workspace`。转 fixed。
- 验收: 干净 GitHub Actions checkout 上 cargo test --workspace 全绿；不通过 skip/忽略测试解决；本地与 CI 仍使用同一测试契约。
- refs: R-152
- 优先级: P0

## D-220 R-153 批0b process 测试临时目录固定 PID 导致并行争用 [fixed] (medium)
- 复现: cargo test -p kanzei-app；process_tests 两个停止测试并行运行时共享同一 PID 目录，Windows 报 Os error 32。
- 标签: 后端
- 进展: 已修复：`crates/kanzei-app/src/process_tests.rs:10-18,38-46` 临时目录改为 PID+纳秒唯一值；`process_tests.rs:32,65` 在删除目录前显式 drop SQLite store。`cargo test -p kanzei-app process_tests` 5/5 通过，Windows 并行 error 32 不再复现。
- 验收: 两个 process_tests 停止测试可在 cargo test -p kanzei-app 并行运行且不争用 SQLite 文件。
- 优先级: P1

## D-221 R-153 批1迁移后 update 测试仍从 main 根导入 fast_model 辅助 [fixed] (medium)
- 复现: 将 fast_model 辅助完整迁移到 fast_model.rs 后运行 cargo test -p kanzei-app，update_tests_update.rs 的 super::ollama_service_up/pull_progress_text 导入失败。
- 标签: 后端
- 根因: 批0 update 测试模块仍按旧 main.rs 私有符号路径引用，模块迁移后测试未同步收窄到 fast_model 模块。
- 进展: 已修复并验证：`crates/kanzei-app/src/update_tests_update.rs:3` 改为从 `super::fast_model` 导入；`crates/kanzei-app/src/fast_model.rs` 中两个辅助函数以 `pub(crate)` 暴露给测试模块。
- 验收: 测试模块改为从 fast_model 导入，cargo test -p kanzei-app 通过。
- 优先级: P1
- 验收证据: 唯一验收项“测试模块改为从 fast_model 导入，cargo test -p kanzei-app 通过”：实现位置为 update_tests_update.rs:3、fast_model.rs:169/175；验证记录 T-1786251753，42 passed。

## D-222 R-153 批2 update command wrapper 与旧宏符号重复 [fixed] (medium)
- 复现: 新增 update.rs 仅做 update_check/update_install command wrapper，main.rs 仍保留同名 tauri command；cargo test -p kanzei-app 报 __cmd__update_check/__cmd__update_install 重复定义。
- 标签: 后端
- 根因: tauri::command 宏生成的辅助符号按 Rust 函数名进入父模块宏命名空间，转发 wrapper 与旧函数同名会冲突。
- 进展: 已完成修复：`crates/kanzei-app/src/update.rs:32-91` 承接 `update_check_command/update_install_command` 完整实现；`main.rs:3008-3111` 原实现已删除；`main.rs:885-886` 通过模块 command 注册。`cargo test -p kanzei-app` 42 项通过，宏重复问题已消失。
- 验收: 不再出现宏辅助符号重复定义，且最终批2需删除 main.rs 原 update command 实现。
- 优先级: P1
- 验收证据: ①“不再出现宏辅助符号重复定义”：实现位置 update.rs:32-75 的唯一两个 Tauri command 宏，验证记录 T-1786252959；②“删除 main.rs 原 update command 实现”：main.rs 原 3008-3111 区间已移除，唯一实现位于 update.rs:32-91；定向测试 T-1786252959 42 passed。

## D-224 R-153 设置保存实现迁移后仍引用 main crate 的配置文档 helper [fixed] (medium)
- 复现: 将 settings_save_at_path_impl 迁入 settings.rs 并移除 main.rs 对 settings_read_document/settings_write_document 的 re-export 后，cargo test -p kanzei-app 编译失败 E0425。
- 标签: 后端
- 根因: 迁移函数仍使用 crate::settings_read_document 与 crate::settings_write_document，模块内 helper 实际位于 settings.rs。
- 验收: settings.rs 内 settings_save_at_path_impl 通过模块内 helper 编译，cargo test -p kanzei-app 全绿。
- 优先级: P1
- 修复: settings_save_at_path_impl 已移入 crates/kanzei-app/src/settings.rs:263 附近，并将配置文档 helper 改为模块内 settings_read_document/settings_write_document；main.rs 不再错误依赖 crate 根 re-export。T-1786296386 cargo test -p kanzei-app 43 项全绿。
- 验收证据: crates/kanzei-app/src/settings.rs:263-272 为实现与模块内 helper 调用；T-1786296386 全部通过。

## D-225 R-153 项目 command 迁移误删 project_files 并重复 Tauri command 属性 [fixed] (medium)
- 复现: 迁移 export_pick_dir 到 projects.rs 后，cargo test -p kanzei-app 编译失败：projects.rs 出现重复 #[tauri::command]，且 project_files 函数被替换操作误删，generate_handler 找不到 projects::__cmd__project_files。
- 标签: 后端
- 根因: 向 projects.rs 末尾插入 export_pick_dir 时 old_string 包含 project_files 函数，但 new_string 未保留该函数；同时保留了原有 command 属性并重复添加。
- 验收: projects.rs 同时保留唯一 project_files command 与唯一 export_pick_dir command，cargo test -p kanzei-app 全绿。
- 优先级: P1
- 修复: 恢复 crates/kanzei-app/src/projects.rs 中唯一的 `project_files` command，并保留唯一的 `export_pick_dir` `#[tauri::command]` 属性；generate_handler 重新解析 projects::__cmd__project_files。T-1786296789 的 cargo test -p kanzei-app 43 项全绿。
- 验收证据: crates/kanzei-app/src/projects.rs:37-67 同时包含 project_files 与 export_pick_dir，各一个 command 属性；T-1786296789 全部通过。

## D-226 R-153 导出资料 command 迁移遗漏测试兼容 re-export [fixed] (medium)
- 复现: 导出资料链路迁移至 projects.rs 后，cargo test -p kanzei-app 编译失败：state_tests.rs 从 crate 根导入 export_project_data 与 ExportOptions，但二者不再位于根模块。
- 标签: 后端
- 根因: 迁移 command 未同步保留 state_tests 使用的 crate 根 re-export，且 ExportOptions 仍为 projects 私有类型。
- 验收: main.rs 保留兼容 re-export，state_tests 可继续调用真实 projects::export_project_data，cargo test -p kanzei-app 全绿。
- 优先级: P1
- 修复: projects.rs 的 ExportOptions 已公开为 crate 内可用并公开字段；main.rs 增加 `pub(crate) use projects::{export_project_data, ExportOptions}` 兼容 state_tests，command 实际实现仍位于 projects::export_project_data。T-1786296941 cargo test -p kanzei-app 43 项全绿。
- 验收证据: crates/kanzei-app/src/projects.rs:80-88 为 payload；main.rs 模块 re-export；state_tests 导入并调用真实 export_project_data；T-1786296941 全绿。

## D-228 settings_tests 迁移后缺少 KanzeiConfig 测试导入导致编译失败 [fixed] (medium)
- 复现: 将 settings_tests 从 main.rs 移入 settings.rs 后运行 `cargo test -p kanzei-app`，settings.rs 测试作用域无法解析 KanzeiConfig，编译报 4 处 E0425。
- 影响: R-153 的测试迁移无法编译，阻断定向回归。
- 标签: 后端
- 进展: 已修复：`crates/kanzei-app/src/settings.rs:415-416` 的 `#[cfg(test)] mod tests` 增加 `use kanzei_harness::KanzeiConfig;`，解决迁移后测试模块作用域缺失。复核证据：T-1786297996 `cargo test -p kanzei-app` 43 项全绿，四个迁移后的 settings::tests 均通过。
- 优先级: P1

## D-234 批次进度字段靠模型自觉更新,长 run 连做多批时停摆:实际 14/16 显示 2/16 [fixed] (medium)
- 优先级: P2
- 标签: 核心
- refs: R-155 D-207
- 复现: 2026-08-10 04:16 实测:R-155 自举 run 从 03:44 起以 3~7 分钟一批的速度连续提交 B1→B8 + S1→S6(14 批,提交信息齐全),而 requirements.md 的「批次: 2/16」「进展: …下一批 B3」停在 03:52——桌面端批次进度格照此渲染,用户看到的进度停摆 12 批。
- 根因(机制实证,三层): ①`批次`/`进展` 字段没有任何引擎写入方,全靠 agent 自觉调 `req update`;②即便自觉更新也天然滞后一拍——agent 先提交代码再改 tracker,tracker 改动躺在工作树里被**下一批**的提交顺带带走(B3 的提交里装的是 B2 的进展);③conventions 只有软要求(「在进展里写明批次边界」),无机械门禁,长 run 里模型注意力全在代码上,B3 之后就断更了。与 D-207 同病:真源(提交信息里的「R-155 B7:…」)机器可读且从不缺席,UI 却去读靠自觉维护的副本。
- 影响: 批次进度格的存在价值就是让用户实时看到多批大条目走到哪了,停摆 12 批等于功能失效;且「进展」字段是收口对照验收⑤(拆前后行数对照)的载体,断更意味着收口时要靠翻 git log 补账。
- 修复方向(derive, don't duplicate 优先): ①批次格改为从 git log 推导——引擎解析该条目区间内提交信息的 `R-xxx [BS]\d+` 模式,实时数完成批次,`批次` 字段降级为展示兜底或直接退役;②或提交侧机械 bump:agent 的 git 提交经引擎路径时,信息匹配批次模式即自动更新对应条目的批次字段(与 harvest_failures 同哲学:引擎采集,不靠自觉);③「进展」叙事字段保留人写,但收口门禁校验:批次型条目关闭时进展里的批次数须与 git log 推导一致,不一致拒关。三选一或组合,禁止再加一条"记得更新进度"的提示词了事。
- 验收: ①长 run 连做多批时,批次进度格与 git log 推导的完成数实时一致(实测一次多批 run);②无需模型任何 req update 调用即可正确显示;③若保留字段,收口时字段与推导不一致有机械拦截;④冒烟或单测覆盖推导解析(B/S 混编、乱序提交、无批次模式的普通提交不误判)。
- 证据等级: E1(时间线+提交触碰记录+字段现值三方对照)
- 修复进展(2026-08-10): 文档快照现以当前 HEAD 的提交标题为批次真源（一次快照只读取一次 Git 历史），解析 `R-xxx Bn`、`Sn` 与 `S5-S6`/`S7+S8` 并去重；成功的主 agent 或子 agent `git commit` 会立即刷新快照。手写 `批次` 仅在 Git 不可用或尚无批次标记时回退，关闭时若与 Git 推导不一致会被拒绝。`write` 的 JSON 修复同时补齐未转义换行/制表符，避免本次修复过程中的大段源码写入失败。
- 验证(2026-08-10): `cargo check -p kanzei-app`；Harness JSON 修复 7 项通过；Git 解析、快照真源、关闭不一致门禁各有回归测试通过；`node scripts/ui-sources.mjs` 与 `node scripts/ui-runtime-smoke.mjs` 通过。待重新启动桌面端后，以一次真实多批 agent run 记录最终 E2 交互证据。
- 进展: 修复已由 e108613 落地(提交时间线:批次格停摆缺陷修复实现 commit)。验收逐条对照:①实时一致——docs.rs:92-100 每次 docs_snapshot 只跑一次 git log(completed_batches_for_entries)推导全部条目;docstore.rs:155-168 batch_progress_with_derived_done 让推导值覆盖手写字段;07-events.js:152-164/174-177 isBatchCommit(git 提交成功)→refreshDocsSoon,直连 agent 的 tool-end 与子代理的 task-progress 双路即时刷新;state_tests.rs::docs_snapshot_uses_git_commits_for_live_batch_progress 机械验证(R-001 字段 0/3+git B1/B2→done=2;R-002 无提交标记→回退字段 2/3);实测本仓库 R-155 提交历史 B1-B8+S1-S4+S5-S6+S7+S8 推导 16/16。②无需模型 req update——推导来自 git log,前端 11-docs-list.js:333-356 只渲染后端 entry.batches,不读手写字段。③收口机械拦截——tracker.rs:440-453 close 时 declared vs derived 不一致直接拒绝,单测「关闭时拒绝手写批次与_git_提交真源不一致」验证。④推导解析覆盖——git_batches.rs 单测 parses_mixed_out_of_order_and_compact_batch_markers(B/S 混编、乱序、B1-2 压缩、R-1550 相邻 ID 不误判、无批次普通提交不误判);ui-runtime-smoke.mjs D-234 段断言提交后 docs_snapshot 调用数增加。验证:T-1786310290 kanzei-tools 125 passed;T-1786310331 kanzei-app 44 passed;T-1786310446 前端四条冒烟 passed。遗留观察:非 B/S 风格提交信息(如「R-157 批1」)推导为 0 回退字段显示,不误判。

## D-236 git_batches 不识别中文「批N」提交风格:推导 0 回退字段,进度格对当前提交格式仍停摆 [fixed] (medium)
- 复现: D-234 修复(e108613)后 git_batches.rs 只认 B/S 标记(R-155 Bn / Sn / S5-S6)。当前实际提交风格是中文「R-157 批3:设置页节奏参数透传…」:contains_entry_id 命中但 collect_marked_batches 不识别「批」→ 推导 0 → batch_progress_with_derived_done 回退手写「批次」字段 → 进度格又回到靠 req update 自觉更新的旧世界。实测:本会话 R-157 提交 1b1f6ac(批3)后,推导不命中,进度显示依赖字段。D-234 修复进展里也明说这是遗留观察:「非 B/S 风格提交信息(如「R-157 批1」)推导为 0 回退字段显示」。
- 影响: 批次进度格对当前中文提交风格失效,长 run 进度又停摆(用户 2026-08-10 直接点名);收口门禁同样漏判——中文批次型条目关闭时 declared vs derived 推导 0,拦截逻辑空转。
- 优先级: P2
- 状态: fixing
- 进展: 修复:git_batches.rs collect_marked_batches 扩展中文「批N」风格——「批」后直接跟数字时归入 B 命名空间(与 "R-157 B3" 语义等价自然去重);「批次」判为字段叙事(「批次: 3/3」)跳过不识别;修复过程中一并纠正 parse_number 失败时的 index 推进(原写法有死循环风险)。验收对照:①长 run 实时一致——解析入口 completed_batches_for_entries 不变,中文风格提交('R-157 批3:…')现在命中推导,不再回退字段;真实仓库实证 git log 推导 R-157 = 3 批(B1/B2/B3),A-010 标题含「R-157」但无批号不误计。②无需 req update——推导仍纯来自 git log。③收口拦截——tracker.rs 关闭门禁复用同一推导(此前中文风格推导 0 致拦截空转,现已命中)。④解析覆盖——新增单测 parses_chinese_pi_batch_markers_without_misjudging:批1/批2/批3 混编、批3 与 B3 去重、R-1570 相邻 ID 不误判、R-156 其他条目不误判、「批」后非数字(审批流程)不构成批次、「批次 3/3」叙事不构成批次、无批次普通提交不误计;旧测试 parses_mixed_out_of_order_and_compact_batch_markers 保持绿。

## D-237 活动面板 diff 汇总无增删颜色, bash 完整输出/错误详情不可展开(R-095 追溯价值打折) [fixed] (medium)
- 复现: ①活动面板顶部 diff-summary 汇总区渲染为 `<span>+8/−8</span>` 纯文本,样式 .diff-summary-row 全 dim 灰,无增绿删红——右侧活动面板的 diff 颜色显示缺失。②活动面板 bash 条目点击展开后:bgAdd 只有入参 JSON,bgEnd 只 append display(后端 output 截断 4000 字符),完整命令输出丢失;主对话 chatToolEnd→fillToolBlock 会追加完整 content(8000 截断),活动面板无对应处理——同一工具调用在主对话能看全,活动面板只能看 4000 截断版。③错误工具调用:bgEnd 里 !ok && preview 追加 preview 文本(可展开,但成功路径的完整内容不可达)。
- 影响: 活动面板定位是"可检索的执行记录"(R-095),diff 无颜色让增删难以一眼区分;bash 完整输出不可展开让活动面板失去追溯价值——用户原话"这才是活动的意义"。
- 验收: ①diff-summary 汇总行 +N 显示增色(绿)、−N 显示删色(红),文件路径保持 dim;②活动面板 bash 条目展开后能看到完整命令与输出(与主对话同等完整度,超出 display 4000 截断的部分也可见);③错误工具调用展开后能看到完整错误内容;④node --check + ui-runtime-smoke 冒烟断言新增行为;⑤活动面板 diff 块(renderDiff)颜色保持现状不受破坏。
- 优先级: P2
- 进展: 修复完成,逐项验收:①diff-summary 汇总行 +N/−N 已分色——renderDiffSummary(crates/kanzei-app/ui/06-activity.js)用 <span class="diff-add">+N</span>/<span class="diff-del">−N</span> 渲染,style.css 新增 .diff-add(#a5c98f 绿)/.diff-del(#dd8d72 红)两条规则,文件路径 span 无 class 继承 .diff-summary-row 的 dim。②bash 完整输出可展开——后端 bash.rs 的 display 增加 full 字段(成功分支与超时分支均透传,上限 200k 字符),前端 appendDisplayBlock 改为 display.full ?? display.output 优先渲染完整输出。③错误工具调用展开完整错误——既有行为(bgEnd !ok && preview 追加)保持不变,且错误路径的 display 同样带 full。④验证——ui-runtime-smoke.mjs 新增 D-237 断言段(T5 diff 事件断言 #diff-summary 含 +3/−1 与文件路径;T6 bash 事件先 tool-start 建条目再 tool-end,断言展开区含完整输出),四条 ui 冒烟全绿,node --check 通过,cargo test -p kanzei-tools 126 全绿,frontend_check 花括号配对正常。⑤renderDiff 未改动,仅新增样式类,颜色不受影响。

## D-238 桌面端外部进程调用未隐藏控制台窗口:git/cargo/taskkill 弹黑窗 [fixed] (medium)
- 复现: 桌面端(GUI 进程,无控制台)触发以下操作时,Windows 弹出黑色 cmd 窗口:①打开文件视图(files_snapshot→git_file_list 调 `git ls-files`);②打开需求/缺陷文档页(git_batches::commit_subjects 调 `git log`);③提交(compile_gate 调 `cargo check`);④process stop(kill_tree 调 taskkill)。用户反馈:git 工具会弹黑色终端。
- 根因: Windows 上子进程默认继承父进程控制台;父进程无控制台时系统新建一个控制台窗口。kanzei-tools 里 files.rs::git_file_list、git_batches.rs::commit_subjects、git.rs::compile_gate(cargo)、shell.rs::kill_tree(taskkill) 四处 spawn 未设 CREATE_NO_WINDOW(0x0800_0000)。bash.rs 与 git.rs::run_git_owned 已设,kanzei-app 的 hidden_command 也设了,唯独这四处遗漏。
- 证据等级: E1(代码路径实证)
- 验收: ①files.rs::git_file_list、git_batches.rs::commit_subjects、git.rs::compile_gate、shell.rs::kill_tree 四处子进程全部设置 CREATE_NO_WINDOW(std/tokio creation_flags);②共享辅助函数收敛,不与 bash.rs/git.rs 既有隐藏逻辑重复;③cargo test -p kanzei-tools 全绿。
- 进展: 修复完成(提交 2ee766d)。验收对照:①四处调用点全部接入 CREATE_NO_WINDOW——files.rs:175 `crate::hide_console(&mut command)`(git_file_list)、git_batches.rs:40 `crate::hide_console(&mut command)`(commit_subjects)、git.rs:331 `crate::hide_console_async(&mut command)`(compile_gate/cargo)、shell.rs:92 `crate::hide_console_async(&mut command)`(kill_tree/taskkill);②共享辅助收敛于 lib.rs:57(hide_console,std,cfg(windows) 设 0x0800_0000)与 lib.rs:67(hide_console_async,tokio),bash.rs:560/git.rs:509 私有函数改为委托共享实现,无重复常量;③cargo test -p kanzei-tools 126 passed/0 failed(记录 T-1786312997),下游 kanzei-core/kanzei-app cargo check 通过(仅既有警告)。

## D-232 package.ps1 发布门禁缺「HEAD 已推远端」检查,未推时 422 到最后一步才炸 [fixed] (medium)
- 优先级: P2
- 复现: 2026-08-10 发版 build-9e09b80 实测:dev 领先 origin/dev 21 个提交时跑 package.ps1 -Ack 21 -Publish,tauri build + NSIS 全部完成(约 1 分钟)后,gh release create --target <full_hash> 报 HTTP 422 `Release.target_commitish is invalid`——GitHub 要求 target 提交在远端可达,本地未推的 SHA 一律拒绝。推送后重跑成功。
- 影响: 失败发生在整条流水线最后一步,前面的构建时间全部白费;且 422 报错文案不指向真因(脚本注释里只记载过「短 hash 会 422」的旧案),排查要靠人对比 origin/dev。自举/无人值守发版场景下这是必踩坑——自举只 commit 不 push。
- 修复方向: 在 -Ack 核对同一节(构建开始前)加一条前置检查:`git rev-list origin/<branch>..HEAD` 非零即中止,报错明说「先 git push 再发版」;或者(需用户拍板)自动 push 后继续。顺带把 422 旧案注释更新为两种成因(短 hash / 未推远端)。
- 证据等级: E1(本次发版实测复现+修复验证)
- 标签: 流程

## D-240 update_tests_update 进程存活探测 flaky:tasklist 竞态偶发误判自身已退出 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 复现: cargo test -p kanzei-app 全量并行时,update_tests_update::install_helper_waits_for_the_caller_to_exit_before_installing 偶发失败(296s):wait_for_parent_exit(自身 PID,600ms) 判定"当前进程已退出"。单独重跑 2s 通过,二次全量通过。
- 标签: 后端
- 根因: process_alive 用 `tasklist /FI "PID eq <pid>"` 探测(update.rs:362-368);全量并行时进程表查询与测试进程存在竞态,tasklist 输出偶发不包含自身 PID,误判已退出。update.rs/update_tests_update.rs 自 R-156 后未改动,与 R-102 无关。
- 验收: 全量并行时该测试稳定通过(不 flaky);或 process_alive 改用 OpenProcess/枚举快照等无 tasklist 文本竞态的探测方式。

## D-242 R-170 剥离继续文案时把批次规则删空:引擎门禁照罚,提示词与 system prompt 均无真源 [fixed] (high)
- 优先级: P0
- 标签: 流程
- refs: R-170 R-169 R-160 D-219 D-241
- 证据等级: E1(全仓 grep 零命中 + 门禁代码实证 + 现默认文案实证)
- 复现: R-170(eb7ae42,2026-08-10 关闭)按 continue_prompt_dissection.md §3 剥离清单删除继续文案规则 1-6。删除后 `DEFAULT_CONTINUE_PROMPT = "继续推进，规则按系统提示执行。"`(crates/kanzei-app/ui/08-compose.js:16)。但**批次规则并没有落到任何 system prompt**:对 .rs/.js/.mjs 全仓 grep「批次: 0/」「批次表」「一轮一个批次」「中 3、大 8」只命中 docstore.rs 的默认值实现与 state_tests.rs 夹具,零规则文本;crates/kanzei-tools/src/profiles.rs 的 dev system prompt 有验收证据契约与 WIP 上限,唯独没有任何批次拆解指令。
- 影响: ①**引擎照罚但没人教规矩**——tracker close 门禁仍在(crates/kanzei-tools/src/tracker.rs:466),复杂度「中」「大」的条目即使从不写批次字段,也会按 default_batches(docstore.rs:138,中 3 / 大 8)判定 0/3、0/8 被拦在关闭门口;队首的 R-161(中)、R-162/R-163(大)会立刻撞上。②**进度可见性回归到 R-160 之前**——侧栏批次格子是「外部唯一看得见推进的地方」,不写 `批次: k/N` 就整轮空着。③**git 推导链路失去输入**——git_batches.rs 认的是提交标题里的 `R-123 B4` / `S5-S6` 标记,这个约定同样只写在被删的规则 2 里,现在无处可查。
- 根因: R-170 验收①的前提是「11 项职责中 9 项在 system prompt/配置已有真源」,该前提对「批次粒度」这一项不成立——批次规则从来只存在于继续文案的规则 2,system prompt 从未承载过。剥离时按清单逐条删,没有对每一项复核「真源是否真的存在」,于是删掉的是唯一一份。与 conventions §4「能代码强制的绝不只写进提示词」互为反面:这次是能罚不能教。
- 现存兜底(不足以关闭本条): 记忆 M-028 只教「关闭时报批次未走完怎么过关」(完成后把总数改成实际值),不教「取活时先定批次表 0/N、每批填 k/N、提交标题带标记」;关闭门禁的错误文案本身可自解释,但那是撞上之后的事后补救,格子该填的那一路已经损失。
- 验收: ①批次规则有明确真源(profiles.rs 的 dev system prompt,或引擎注入的等价位置),内容至少覆盖:复杂度→默认批数(中 3/大 8)、第一轮先写 `批次: 0/N`、每批完成后更新 `批次: k/N`、提交标题带 `<ID> B<k>` 标记、关闭时批次须走满或据实改小总数;②有测试断言该规则文本存在(照搬 profiles.rs 既有的 dev_system_prompt_enforces_acceptance_evidence_contract 写法);③实测一条「中」或「大」条目从取活到关闭,侧栏格子逐格填上、关闭时不被门禁拦;④顺带修 D-219 的机制层待修项——system prompt 的 WIP 文案仍是旧口径 `keep at most 2 requirements in doing`(profiles.rs:402),不区分可执行/阻塞 doing,按 §1.1 新口径改写。
- 边界: 只补规则真源与断言,不改批次门禁与 git 推导的既有行为(那两处是对的,缺的是输入端)。

- 进展: 验收逐条核对(2026-08-10 收口):①真源——9b255de 已把完整批次协议写回 dev system prompt(profiles.rs:412-427):批数 agent 自定上限 10、首轮批次: 0/N、每批更新批次: k/N、提交标题 <ID> B<k>、关闭时走满或据实改小总数;验收原文「复杂度→默认批数(中 3/大 8)」子项已被用户 2026-08-10 新定调取代(批数 agent 自定、上限 10、复杂度不再支配),落点见 docstore.rs:142-145 注释与 9b255de commit message。②断言——dev_system_prompt_enforces_wip_and_batch_contract(profiles.rs:747)断言 10 个 token + 反向断言旧口径,实跑 3 个 dev_system_prompt 测试全绿;kanzei-tools 全量 153 passed。③实测——R-161(复杂度中)批次 2/2 关闭于 1930f5f,门禁放行。④D-219 旧口径 keep at most 2 requirements 已删,改单槽口径,反向断言在场。边界满足:tracker.rs:446-480 关闭门禁与 git_batches 推导未改,git_batches 2 测试全绿。

## D-223 R-158 新增设置字段误删 profile_default 导致编译失败 [fixed] (medium)
- 修复范围: 恢复 SettingsPayload.profile_default 字段；Codex Fast mode 仍保持独立字段。
- 复现: 在本次 Codex Fast mode 改动后运行 `cargo check -p kanzei-app`，SettingsPayload 编译报 profile_default 不存在；设置打开构造体也报该字段未定义。
- 根因: 新增 codex_fast_mode 字段时的精确替换遗漏了既有 profile_default 字段。
- 验收: SettingsPayload 同时包含 profile_default 与 codex_fast_mode；设置保存/打开相关构造点可编译。
- 优先级: P1
- 进展: D-241 处置(2026-08-10):R-153 早已归档,上游迁移阻塞已不存在,按 D-241 修复方向直接复测——cargo check -p kanzei-app 通过(0 error)。验收对照:①SettingsPayload 同时包含 profile_default(settings.rs:21)与 codex_fast_mode(settings.rs:20),构造点 settings.rs:729/780/820/875 等均可编译;②保存路径 settings.rs:237 两字段均写入。cargo check 全绿,验收达成,关闭。

## D-173 架构索引 architecture/README.md 无专用工具可写:edit 被 ruleset 拒绝,agent 只能 bash 旁路维护 [fixed] (high)
- 备注: 本轮已用 bash 旁路一次性补齐索引(946742f),内容正确;本缺陷登记的是通道缺失本身,不撤回已完成的补全。D-171 已确认为真实缺陷(孤儿 webview 黑屏,743d4e4 修复并登记),非编号空洞;此前的 tombstone 误判已撤销。
- 复现: agent 用 edit 更新 `.kanzei/project/architecture/README.md` 报 permission denied by ruleset(policy-managed,提示用专用工具);但 req/defect/goal/decision 四个专用工具只管理各自追踪文件,没有任何工具托管 architecture 目录。实测 2026-08-08:索引补全只能经 bash 写入(946742f),而 bash 能写受保护目录本身也说明 R-139 的 bash 级 .kanzei 路径硬门禁尚未落地。
- 影响: ①自举循环新增/重命名设计文档后,架构索引只能由用户手改,必然滞后(本次 10 个文档重命名 + 2 份新设计入库后,索引仍只有 5 个旧条目);②agent 若想维护索引,唯一通道是 bash 旁路,而旁路通道本身违反'受保护文档不被 bash 旁路'的设计原则;③architecture/README.md 是架构发现入口,索引滞后会让后续会话找不到现行设计真源。
- 根因: ruleset 对 `.kanzei/project/*` 的 edit/write 硬 deny 只给 tracker 类工具放行(设计意图是防模型旁路),但 architecture/README.md 作为同级项目管理资产不在任何专用工具的托管范围——需求/缺陷/目标/决策各有工具而架构索引没有,形成'既不能 edit、也无专用工具'的双重缺口;bash 写入通道未封堵又构成硬门禁的旁路。
- 验收: ①提供可用的架构索引维护通道:要么新增专用命令/工具(如 `kz doc index` 或 tracker 工具扩展),要么把索引改为从 docs/design 自动生成(如 docs_snapshot 系),agent 更新 docs/design 后索引自动同步;②补 R-139 的 bash 级 .kanzei 路径硬门禁,使受保护文档不能经 bash 旁路写入;③验收时新增/重命名一个 docs/design 文档后,索引可被 agent 直接维护且无需 bash 旁路。
- 修复进展(2026-08-08): 已新增 `architecture` 专用工具及固定路径、`expected_hash` 并发保护、同目录临时文件与可恢复替换;Harness 已把架构文档纳入托管资源并要求通过专用工具访问;通用 Bash 已在执行前后对托管资源做快照并回滚越界写入。
- 验证(2026-08-08): `kanzei-tools` 80 项、`kanzei-harness` 37 项、`kanzei-core` 50 项测试通过。尚未在已安装桌面端中完成一次真实模型调用与工具交互验收,因此保持 `fixing`。
- 收口核对(2026-08-09): 本轮 fixing 批量收口时**刻意不关**这条。episodes 实证:48 个轮次里 `architecture` 工具 **0 次真实调用**(同期 req 196 次、defect 95 次)——验收③"agent 直接维护索引且无需 bash 旁路"不是没测到,是从未发生。工具注册、权限、D-195 的提示词同源测试都在,但按 §1.25"声称完成的能力必须有真实调用方",一个零调用的通道不能算闭合。下一次自举改动 docs/design 后用 architecture 工具更新索引成功,即可关闭。
- 优先级: P1

- 进展: D-241 处置(2026-08-10):验收①architecture 专用工具通道已交付(2026-08-08,1a09069 含 expected_hash 并发保护与同目录临时文件可恢复替换);验收②bash 级硬门禁已在 bash.rs:151-156(执行前 ManagedSnapshot::capture + 结果侧比对)+264/310(enforce_managed_files 隔离留证+整体回滚),测试 shell_writes_to_managed_docs_are_rolled_back(bash.rs:620)在库中。验收③真实调用实证(本条关闭前刚完成):9b255de 新增 docs/design/parallel_read_serial_write_orchestration.md 后索引缺失,本次用 architecture 工具 update 补入现行基线节(含一句话描述),validation ok(21 indexed links),全程零 bash 旁路——2026-08-09 收口核对时 'architecture 工具 0 次真实调用' 的缺口已闭合。关闭。

## D-202 超长对话把 webview 主线程拖死,侧栏等大片控件点击无反应 [fixed] (high)
- 复现: 2026-08-09 用户实测(53bb8e7 桌面端):自举循环长会话(几百轮,含大量工具调用块/diff/markdown)期间,侧栏条目展开、筛选等点击**完全无反应**(无按压反馈,像点在空气上);发送等主操作路径尚可。用户自判"上下文太多卡住了"。初步排查已排除:①初始化崩坏(53bb8e7 的 main.js 在 ui-runtime-smoke 全量执行 0 错误);②ask 遮罩挡点击(全屏 overlay 会挡住所有东西,与"主操作正常"不符);③R-086 状态机焊死(那只禁用运行态控件,不吞侧栏点击)。
- 疑似根因(待复现证实): 长对话 DOM 巨大(消息、工具块、diff 逐条渲染,无虚拟化/窗口化),流式事件持续追加触发重排,主线程长期忙碌,点击事件延迟到秒级等价于无反应。若成立,与 D-013(diff 默认展开导致对话过长)、D-046(重绘防抖)是同一性能债的延续:此前只做了"少画",没做"画不下就不画"。
- 验收: ①可复现实证:构造或回放长会话,量化点击响应延迟(如 Event Timing / 长任务计数),定位耗时大头;②修复后同样场景侧栏点击在人可感知阈值内响应(<200ms);③对话渲染有上限策略(虚拟化、折叠历史或分页任一),DOM 节点数有界;④冒烟加长会话性能断言防回归。
- 根因(2026-08-09 定位,代码实证): 主因是 **i18n 的全局 MutationObserver 把每一次 DOM 变动都放大成一次全文档重扫**。main.js:711 `new MutationObserver(() => applyLanguage())` 监听 `document.body` 且 `childList+subtree+characterData+attributes`;而 applyLanguage(main.js:627)每次执行都 `createTreeWalker(document.body, SHOW_TEXT)` 走**整页每一个文本节点**(每个节点还做一次 `parent.closest("[data-i18n-raw]")` 祖先回溯),之后再 `querySelectorAll("[title],[placeholder],[aria-label])` 扫一遍全页。它不按语言短路——中文模式下同样全量走。于是单次成本 ∝ 全文档文本节点数 ≈ 对话长度,而触发频率 = 每个流式 delta 一次(appendAssistant 的 `innerHTML=` 必然产生 childList 变动)⇒ 一轮对话的渲染开销对会话长度成平方增长,轮次越多主线程越被占满,点击排在长任务后面就等于"没反应"。
  次因(同一热路径上的三处放大,都在 appendAssistant,main.js:1320-1335):①每个 delta 重新 `renderMarkdown(整条消息)` 并整块 `innerHTML=`,单条消息内部就是 O(n²);②每个 delta 把整条 raw `split("\n").map(正则).filter()` 一遍,只为取"最近在说"的最后一行;③每个 delta `scrollBottom()` 读 `messages.scrollHeight`,强制同步重排整个消息列表(这一项随轮次增长)。全文件 `requestAnimationFrame` 出现 0 次,没有任何合帧/节流。
  另:对话 DOM 从不裁剪(只有切会话时 `messages.innerHTML=""`),所以**上下文压缩不会缓解卡顿**——压缩只减少发给模型的 token,渲染侧一个节点都没少。用户"上下文太多卡住了"的直觉方向对,但机制不在上下文,在渲染。
- 量化(2026-08-09,ui-runtime-smoke 的 DOM harness 内测,只证明标度不代表真机绝对值): applyLanguage 单次耗时随文本节点数线性上升——95 节点 0.54ms / 295 节点 1.11ms / 895 节点 3.61ms;renderMarkdown 每 delta 均摊耗时随消息长度线性上升——2850 字 0.035ms → 45600 字 0.169ms(单条消息累计 135ms,纯解析、不含 DOM)。真机 WebView 的 TreeWalker/重排成本远高于 harness,几百轮会话的文本节点数在万级,单次 applyLanguage 已足以吃满一帧。
- 修复方向(建议按序,每步独立可验): ①observer 回调改为只处理 `mutations` 里的 addedNodes 子树(新进节点才本地化),不再全文档重扫——单点改动,收益最大;②applyLanguage 用 rAF 合帧,一帧最多一次;③appendAssistant 流式期间只追加纯文本(`textContent +=`),消息收尾时再整条 renderMarkdown 一次;④"最近在说"从 delta 增量算,不扫整条 raw;⑤scrollBottom 合帧,或改 CSS `overflow-anchor` / 底部哨兵 + IntersectionObserver,去掉每 delta 读 scrollHeight;⑥最后才做验收③的 DOM 上限(窗口化/折叠历史)——前五步做完可能已不需要。
- 修复(2026-08-09,修复方向①②④⑤已落地): ①applyLanguage 拆成 localizeTextNode/localizeAttributes/localizeRoot(root),observer 回调改为只把 `records` 里的 addedNodes(childList)与 target(characterData/attributes)交给 localizeNodes,全文档重扫只留给初始化与切语言两处显式调用;②④⑤合成一处:appendAssistant/appendReasoning 的 delta 只累加文本,renderMarkdown+innerHTML+scrollBottom 压到每帧最多一次(scheduleStreamRender/flushStreamRender),上一次渲染实测 >8ms 就按实测耗时退避(上限 250ms),长消息自动降频;"最近在说"改用 lastNonEmptyLine() 只扫尾部 2000 字窗口(并丢掉被窗口截断的首行,预览不会从半个词开始)。③(流式期间只上纯文本、收尾再渲染)未采纳——合帧后已无必要,且会让正文在流式期间失去格式。⑥DOM 上限/窗口化仍未做,留待真机复测后再判是否需要。
- 冒烟(验收④已落): ui-runtime-smoke 的 DOM harness 原先给 observer 递空 records、createTreeWalker 忽略 root、requestAnimationFrame 同步执行——三处都会让新路径在冒烟里空转,已一并补真:投递真实 MutationRecord、createTreeWalker 尊重 root(含文本节点 root)、rAF 入队由 flush 排干,并统计"从 body 起的全文档重扫"次数。新增三条行为断言:200 个 delta 触发的 renderMarkdown ≤20 次、全文档 i18n 重扫增量为 0、合帧后最后一段文本确实渲染出来;另加一条增量本地化断言(新进节点在 en 下必须被翻译),防止"少扫了也少翻了"。拦截实测:把 observer 改回 `() => applyLanguage()` → 冒烟报"触发了 2 次全文档重扫"(harness 内 200 个 delta 同步发生会被微任务合批,真机每个 delta 是独立事件、独立微任务检查点,即每 delta 一次);把渲染改回每 delta 一次 → 报"200 个 delta 触发了 200 次 renderMarkdown"。四条 UI 冒烟全绿。
- 待验收: ②真机复测(几百轮会话下侧栏点击 <200ms)由用户在新构建上确认;①的真机 Event Timing/长任务数据仍未采;③(DOM 节点数有界)未做。三条都清了才转 fixed。
- 证据等级: E1(代码路径实证 + harness 标度量化 + 拦截实测;真机 Event Timing 数据待补,对应验收①)
- 优先级: P1
- 标签: 前端

- 进展: D-241 处置(2026-08-10):按 §1.2 可用即关闭——修复方向①②④⑤已落地(applyLanguage 增量本地化+合帧渲染+scrollBottom 合帧,2026-08-09),验收④冒烟断言已落 4 条(200 delta 触发的 renderMarkdown ≤20 次、全文档 i18n 重扫增量为 0、合帧后文本渲染、增量本地化断言)且有拦截实测。残余验证按 D-241 修复方向转移:验收①真机 Event Timing/长任务量化、验收②真机侧栏点击 <200ms 复测(解除人=用户,需新构建)、验收③DOM 节点数上界策略——三条均转入 R-101 验收清单(2026-08-10 已追加),真机复测不属本代理可控,不阻塞本条按 §1.2 关闭。关闭。

## D-241 D-202/D-173/D-223 长期挂 fixing 无人续推:占「进行中」语义,且引擎无 fixing→open 退回通道 [fixed] (medium)
- 优先级: P1
- 标签: 流程
- refs: D-202 D-173 D-223 D-239 D-235 R-101
- 证据等级: E1(三条目文本 + docstore 状态机代码实证 + 开发重心 M-002 的队列可达性)
- 现象: defects.md 里三条 [fixing] 长期无人续推——D-202(超长对话卡顿,修复方向①②④⑤已落地,卡在真机复测)、D-173(架构索引通道,2026-08-09 收口核对因 architecture 工具零调用刻意不关)、D-223(profile_default 编译失败,自称"待 R-153 上游迁移稳定后复测",而 R-153 早已归档关闭)。三条都不在推进中,却都占着 fixing 的「进行中」语义。
- 影响: ①误导「在做」指针——D-207 三修(60943d2)的运行事实优先正是为压制这类假象,提交信息原话「挂着 fixing 的旧缺陷不再冒充正在做」,说明它已真实误导过界面;②按 §1.1 防堆积兜底「含阻塞在内 doing 总数 >4 不得再开新项」,三条僵尸 fixing 直接吃掉缺陷侧的准入余量;③三条各自的残余验证(D-202 的真机 Event Timing 与 DOM 上限、D-173 的 architecture 真实调用、D-223 的 cargo check 复测)没有任何机制会提醒任何人回去做。
- 根因(两层): ①**无退路**:docstore 的 transition_allowed 单向(docstore.rs:638 `cannot move backward ... forward only`,defect 序列 open→fixing→fixed|wontfix),错误文案让人「手改 markdown」,但 .kanzei/project/* 对 agent 是 edit-denied(M-005)且 shell 旁路被检测回滚——**agent 根本没有把 fixing 退回 open 的通道**,与 D-235(conventions.md 无专用写入)、D-173(架构索引无专用工具)属同一族「既不能 edit、也无专用工具」缺口,所以挂久了只能继续挂。②**无回扫**:fixing 只在被触碰时顺手复核(与 D-239 记的阻塞字段同病),没有任何周期性机械核对「这条 fixing 多久没动过」;叠加当前开发重心=需求优先(M-002),defects.md 在需求队列跑空前根本扫不到,三条永远等不到「被触碰」。
- 修复方向(逐条处置,可立刻验证): D-223 → R-153 已归档,直接 `cargo check -p kanzei-app` 复测,通过即补证据关闭;D-173 → 核对 2026-08-09 之后 architecture 工具是否已有真实调用(1ec12ca 声称「架构索引已登记」),有则按其验收③关闭,无则如实写进展并保持 fixing;D-202 → 按 §1.2 可用即关闭:修复方向①②④⑤已落地且冒烟有拦截实测,残余(真机 Event Timing、DOM 节点数上界)转 R-101 或新条目,真机复测属外部阻塞(解除人=用户),要么补合法「阻塞:」字段,要么连同残余转移后关闭。
- 修复方向(机制,二选一或都做): ①给 tracker 一个合法的退回动作(如 `defect reopen <id> reason=…`,与 repair_* 同族,强制写理由并落进展),让「推不动就退回 open」成为可执行动作而不是纸面建议;②活动条目滞留回扫——list 或调度器对超过 N 轮无进展更新的 doing/fixing 打标(表达方式同 [调度死锁] 横幅),把「该回去看看」从人的记性变成机械信号。
- 验收: ①D-202/D-173/D-223 三条各自有明确归宿(fixed / wontfix / 带具名解除人的合法阻塞 / 退回 open),不再是无归宿的 fixing;②处置依据逐条写进各条进展,残余验证有去处(§1.2);③机制项落地任一:tracker 有可用的 reopen 动作(有测试),或活动条目滞留有机械打标(有测试);④此后不再出现「无进展更新且无阻塞字段」的 fixing/doing 长期滞留。
- 边界: 本条只做队列口径与通道,不承接 D-202/D-173/D-223 各自验收里的功能性残余(那些留在原条目或按 §1.2 转移)。

- 复杂度: 中
- 批次: 1/1
- 进展: 2026-08-10 机制项落地(验收③):tracker 新增 reopen 动作——REOPEN_ACTION 常量(tracker.rs:29)加入 WRITE_ACTIONS 与 input_schema actions 列表(tracker.rs:37/117),execute 分支(tracker.rs:611)要求 id+reason(强制),校验当前状态在 kind.reopen_from 集合内(DocKind 新增字段 docstore.rs:36;DEFECTS.reopen_from=["fixing"] docstore.rs:62、REQUIREMENTS.reopen_from=["doing"] docstore.rs:49),退回初始态并追加一行「进展: [reopen 日期] 理由」(追加而非拼接:docstore 按行解析,值内 \n 会丢,D-241 实测修正)。测试两条:reopen_把fixing退回open_并强制写理由进进展(tracker.rs:1202,含不带 reason 拒绝、状态退回、理由落进展、原始进展保留)、reopen_拒绝终态与不在集合的状态(tracker.rs:1254)。kanzei-tools 全量 155 测试全绿,下游 4 crate cargo check 通过。验收①D-202/D-173/D-223 三条已于 2026-08-10 各自处置归档(D-223 cargo check 复测通过关闭;D-173 architecture 工具真实调用实证 9b255de 后关闭;D-202 按 §1.2 可用即关闭,残余转 R-101 验收清单),处置依据逐条写进三条进展(验收②)。

## D-252 git_batches 把「tools 167」「kanzei-tools 162」等单词尾 S+空格+数字误判为 S 批次,虚增推导批次数 [fixed] (medium)
- 严重度: medium
- 优先级: P1
- 复现: 提交标题含 S 结尾英文词后跟数字会被误判为 S 批次。实测 R-164 关闭:R-164 B4 提交标题「...kanzei-tools 172 全绿」中 'tools' 的 S + 空格 + 172 被 collect_marked_batches(crates/kanzei-tools/src/git_batches.rs:100-110) 识别为批次 S172;同类误判:kanzei-tools 162→S162、tools 167→S167、harness 64→S64、tools 171→S171。8 个含 R-164 的提交提取出 B1-B4+S162/S167/S64/S171/S172 共 9 个标记,关闭门禁报「手写批次 4/4,但 Git 提交历史标记数为 9」拒绝关闭。根因:collect_marked_batches 对 B/S 后跟数字不要求紧邻,parse_number(crates/kanzei-tools/src/git_batches.rs:139-154) 内部 skip_whitespace 跳过空格后再 parse,于是英文单词尾字母 S + 空格 + 数字也被当作批次标记;「批」分支无此问题(中文场景批后直接跟数字)。
- 影响: 任何提交标题里出现 S/B 结尾英文单词后跟数字(如 kanzei-tools 162、tools 167、harness 64)都会虚增 git 推导批次数,导致关闭门禁误拦正常条目(中/大条目关闭必经此门禁),且侧栏批次进度显示虚高。
- 证据等级: E1(代码实证 + R-164 关闭实测)

## D-231 stale 记忆归档流程未落地,失效条目永驻主目录 [fixed] (medium)
- 优先级: P3
- 依据: memory_system.md §2 承诺「stale 后由整理流程移入 archive/ 带墓碑」;store.rs 的 load_archived_ids 会读 archive/ 目录,但代码里没有任何写入方——stale 条目永驻主目录,FTS 仍索引(仅 0.5 降权),目录随时间只增不减。
- 修复方向: 并入 R-165 Memory Compiler 的归档流程(deprecated/invalid 移入 archive/,墓碑保留,默认检索不可见);lifecycle 状态迁移时一并处理 stale→deprecated 兼容映射。
- refs: R-165 R-103

- 进展: 已修复于 R-165 批3(commit 90e4eda):archive_dead() 在 refresh_derived 开头自动把 deprecated/invalid 移入 archive/ 带墓碑,主目录只留 active/candidate;load_all/FTS/默认检索天然不含归档条目;ID 由 load_archived_ids 保留永不复用。验收③测试 deprecated_moves_to_archive_and_hidden_from_search 通过。全量 cargo test --workspace 全绿。

## D-253 DeepSeek 传输失败只显示 reqwest 顶层错误,无法判断 DNS/TLS/连接原因 [fixed] (medium)
- 来源: 2026-08-10 用户关闭代理后实测 `transport error: error sending request for url (https://api.deepseek.com/chat/completions)`，两次重试后仍没有底层原因。
- 定位: `LlmError::Transport` 直接使用 `reqwest::Error` 的 Display；桌面端失败事件最终只调用 `error.to_string()`，而 Windows 的 DNS、TCP、TLS 等原因位于 Error source 链。现场 DNS 同时解析到透明代理常见的 Fake-IP `198.18.0.38`，恢复后强制直连 HTTP 与 Kanzei 自身 Rust 请求链均成功，说明截图时是网络/TUN 切换期的瞬态传输失败，不是 DeepSeek Key、模型名或 base_url 配错。
- 修复: `kanzei-llm/src/error.rs` 统一展开去重后的 source 链，所有桌面端/CLI 传输错误都保留可行动底层原因；不改变 `proxy = "off"` 的强制直连语义，不做静默代理回退。
- 验收: 新增 `transport_error_keeps_actionable_source_chain`；`cargo test -p kanzei-llm` 40/40 通过，`cargo check -p kanzei-app --tests` 通过；真实 `KANZEI_PROXY=off` + `deepseek:deepseek-v4-flash` 请求返回 `KANZEI_DS_OK`。

## D-254 启动前孤儿 WebView 清理的 CIM 查询无超时,kzapp 有进程但永不建窗 [fixed] (high)
- 复现: 覆盖新 EXE 后单实例启动，12 秒内 `kzapp.exe` 存活且 Responding=True，但 MainWindowHandle 恒为 0、工作集仅 11 MB；独立执行同款 `Get-CimInstance Win32_Process` 超过 10 秒仍不返回。存在另一 kzapp 实例时脚本会提前 exit，因而历史运行被偶然掩盖。
- 根因: `cleanup_orphan_webviews()` 位于 Tauri Builder/建窗之前，并用 `Command::output()` 无界等待 PowerShell+CIM；WMI 卡住会把整个 GUI 启动链永久堵死。
- 修复: PowerShell 子进程改为显式 spawn + 5 秒轮询上限；超时或查询错误时终止清理子进程并继续建窗。清理孤儿 WebView 仍是尽力而为，不再拥有阻断应用启动的权限。
- 验收: `cargo test -p kanzei-app` 全绿；release 构建后覆盖安装目录，单实例冷启动在超时边界后成功创建 `kanzei` 主窗口。

## D-255 verify.ps1 在子作用域丢失 LASTEXITCODE,fmt 失败仍继续并误记 pass [fixed] (high)
- 复现: 2026-08-10 正式发版运行 `scripts/verify.ps1`，仓库存在 24 个 Rustfmt 差异，但输出直接进入 Clippy；脚本的 `Invoke-Check` 用 `& $body` 在子作用域执行，返回父作用域后读不到该 native command 的失败码。
- 影响: `dist/verification.json` 可能把实际失败的 fmt 门禁记录成 pass，随后允许 package.ps1 发布未通过完整验证的安装包。
- 修复: 删除会丢失首项证据的脚本块封装，八个门禁改为显式串行命令；每条 cargo/node 命令后立即读取 `LASTEXITCODE` 并写入同名证据。同时对本轮 59 个提交累积的 Rustfmt 差异执行全仓机械格式化。
- 验收: 人工注入失败命令时 Invoke-Check 必须立即 throw；真实 `scripts/verify.ps1` 从 fmt 开始串行跑完全门禁并生成绑定新 HEAD 的 `dist/verification.json`。

## D-174 托管项目后台 Shell 缺少可归因的文件隔离 [fixed] (high)
- 复现: 后台 Bash 启动后立即返回,后续异步进程可以在任意时刻修改 `.kanzei/project` 与 `.kanzei/memory`;Harness 无法区分后台进程写入和稍后专用工具的合法写入,也无法安全回滚。
- 根因: 现有后台进程注册表只管理 PID、日志和生命周期,没有独立工作目录、文件系统沙箱或按进程归因的写入审计。
- 影响: 若继续允许托管项目中的后台 Bash,受保护文档可能绕过专用工具契约;当前修复选择在存在 `.kanzei` 的项目中拒绝后台 Bash,因此 R-097 的后台启动能力暂时降级。
- 验收: ①后台任务运行在可隔离或可归因的文件系统边界中;②后台任务不能写入 Harness 托管路径;③专用工具的合法写入不会被后台回滚机制误伤;④覆盖启动、轮询、停止、越界写入和并发合法写入测试。
- 优先级: P1
- 关联需求: R-097、R-139
- 复杂度: 大
- 批次: 4/4
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `4ed305d`。**能力边界先说清楚,免得下次有人看到 R-097 就以为后台任务能长驻**:此前的修复是「托管项目一律拒绝后台 bash」(R-097 的后台能力在自举仓库里等于 0);本轮换成可归因边界,能力恢复到「**单 run 内可用、跨 run 被收尾**」——后台任务生命周期 ⊆ owner run 生命周期,下一个 run 起来时上一个 run 的后台任务被终止并做终态对账。这是本轮**有意为之的安全降级**,不是终态语义。

  **逐条对照验收原文(四条)**

  ①「后台任务运行在可隔离或可归因的文件系统边界中」→ **达成(归因边界,不是内核隔离)**。新建 `crates/kanzei-tools/src/managed.rs`:托管围栏从 `bash.rs` **纯搬迁**(逻辑一字未改),前台/后台共用一套口径——仓库里不该有第二套「不能写入」的语义。`BackgroundProcess` 增 `owner: BackgroundOwner{run_id, process_id, write_key}`(background.rs:31-36/56),身份取自 `ToolCtx` 的 R-171 双键;`run_id` 缺失(CLI 未 `with_identity`)登记 `unowned` 而**不伪造 id**——归因要么真实,要么如实说不知道。spawn 之前拍托管基线,`register()` 的文档注释钉死「基线必须是 spawn 之前拍的」:晚一刻就把进程自己的副作用算进基线,围栏从此永远看不见它。另有 `workdir` 结构化围栏(`background_workdir_breach`,bash.rs:412,由 bash.rs:165 调用):校验**参数**而非命令文本——D-173 已证否「猜命令文本」这条路(`WriteAllText`/重定向/解释器一行流/`git checkout` 单文件都绕得过),所以它只是静态第一道,绝对路径写入仍由守卫的结果侧对账兜住。

  ②「后台任务不能写入 Harness 托管路径」→ **达成,但走的是「隔离 + 回滚 + 归因」而非「阻止写入」**。守卫任务 `spawn_guard`(background.rs:245)按 `GUARD_TICK = 300ms`(:26)周期对账,进程退出后再对账一次(命令可能在最后一刻写盘);`reconcile`(:262)两级探测(先比元数据,命中差异才读内容),判为越界即隔离到 `.kanzei/quarantine/bg-<id>-<ms>/` → 回滚到基线 → 生成 `BreachRecord{at_ms, touched, quarantine, restored}` 留档,并可经 `process output` 读出(测试断言输出含 `[managed-files]` 与 `owner run=run-breach`)。处置比前台硬一档(连带 kill 进程树),理由是后台进程会持续重试写入,不 kill 就是无限回滚循环。**必须如实标注的一点**:交付过程中发现 `crates/kanzei-tools/src/shell.rs` 的 `kill_tree` **从未真正击杀过任何进程树**(恒定 2.008s 后返回而目标仍 alive,`kill_on_drop` 与 2s `timeout` 叠加会在超时丢弃 future 时先把 taskkill 自己杀了),已单独登记 **D-262**。因此「越界后 kill 进程树」这一**加固项有实现、但目前无可信证据**;验收②的成立**不依赖它**,靠的是隔离 + 回滚 + 归因这条与 D-173 前台围栏同口径的路径。`shell.rs` 本轮零改动(尝试性修复未解决问题,已 `git checkout` 还原干净)。

  ③「专用工具的合法写入不会被后台回滚机制误伤」→ **达成**。新建 `crates/kanzei-harness/src/managed_fence.rs`:专用写工具白名单(`MANAGED_WRITERS`,:38)+ RAII 窗口 `tool_scope` + 窗口关闭时的吸收回调(`set_absorber`,:118),与 `harness/progress.rs` 的 task-local 同构。放在 harness 是因为 kanzei-core 不依赖 kanzei-tools,runner 侧的「合法写入信号」要传给 tools 侧的守卫只能经 harness。接线两处:`crates/kanzei-core/src/runner/tool_exec.rs:95` 与 `crates/kanzei-core/src/runner/drive.rs:888` 各包一层(**串行路径必须包**,writer 阶段走的就是那条)。`reconcile` 里用 `write_in_progress` 把本轮变化分流成「有合法解释」与「越界」,前者推进基线不报越界。

  ④「覆盖启动、轮询、停止、越界写入和并发合法写入测试」→ **达成,五场景齐备**(`crates/kanzei-tools/src/background.rs`):`场景启动_托管项目后台任务登记owner与基线`(:484)、`场景轮询_读输出不误判托管树也不产生越界记录`(:508)、`场景停止_走终止路径并做终态对账`(:561)、`场景越界_后台写托管文档被隔离回滚并归因到owner`(:598)、`场景并发_窗口内合法写入不误伤_窗口外同样写入被回滚`(:662)。场景⑤**带对照组**且对照组是重点:同样的写入放在窗口**外**必须被回滚——只测「窗口内不被误伤」的话,一个恒真放行的实现照样能让上半段假绿。

  **刻意拆掉了两条「会因为错误的原因而通过」的断言,并在测试注释里写明原因**(这件事必须留档,否则后来人会以为是漏写):(a) 停止场景不再断言「进程真的退出」——`kill_tree` 是坏的,那条断言只会因为命令自然结束而绿;改断言「走了终止 + 终态对账路径、不误报越界、托管树逐字节不变」。(b) 越界场景不再断言「越界进程被 kill」——该命令写完就自然退出,`is_running()` 为假证明不了 kill 起了作用。两处注释都写明「等 D-262 修好后应把断言按正确形态补回去」,D-262 的验收③也照这条写了。

  **其它交付项**:`finish_foreign_owners`(background.rs:313)做跨 run 收尾,真实调用方在 `bash.rs:175` 与 `process.rs:60`;`kill_background_processes`(= `background::kill_project`)升级为「终止 + 终态对账」,即租约释放前的收尾入口(不另造别名),调用方 `crates/kanzei-app/src/run.rs`。删除托管项目里的后台拒绝点,`BashTool::description()` 动态化——原静态描述在托管项目里向模型宣传一个必然失败的参数,每次浪费一轮;新文案明说后台任务「fenced and owned by the current run … finished when the next run starts — so it cannot outlive this turn」。批1 的 `#[allow(dead_code)]` 已全部摘除,每项都有真实消费方(§1.25)。

  **既有能力标注(§1.25)**:托管围栏的镜像/比对/隔离/回滚逻辑(`ManagedSnapshot::capture`、`enforce_managed_files`、4 MiB 单文件 / 2000 文件上限)是 **D-173 前台围栏的既有实现**,本轮只是从 `bash.rs` 搬进 `managed.rs` 供前后台共用,**不作为本次产出申报**;`BackgroundProcess` 注册表、输出缓冲与 256 KiB 丢头留尾同样是 R-097 既有。本次新增的是:owner 身份与基线守卫、合法写入窗口(managed_fence)、跨 run 收尾、五场景测试。

  **残余缺口去处(§1.2)**:①内核级隔离(受限令牌/低完整性/AppContainer/ACL)+ 合法写入窗口的毫秒级缝隙 + 镜像上限内的空白 → **D-258**(本轮评估为代价收益倒挂:低完整性进程连 `target/`、`node_modules/` 都写不了,而那正是后台任务的唯一用途);②跨 run 长驻的受管后台服务 + 后台日志落盘可回看 → **R-180**;③`kill_tree` 失效 → **D-262**;④租约 Drop 的精确收尾接线 → **R-173**。

  **验证**:交付时定向 kanzei-tools 213 / kanzei-harness 81 / kanzei-core 119 全通过,clippy `-p tools -p harness -p app -p core --all-targets -D warnings` 零输出,rustfmt 全净;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度大,满足 §1.4 全量触发点①)。

## D-227 并行 test_record 自动生成相同时间戳 ID，四条 UI 记录互相覆盖 [fixed] (medium)
- 复现: 并行调用四个同秒 test_record，均省略 id；结果生成相同 T-1786297655，archive 中标题不同但 ID 相同。
- 影响: 测试证据无法一一引用，可能破坏测试记录唯一性与归档完整性。
- 标签: 流程
- 进展: 本轮发现；后续需用串行记录或显式唯一 id 认领，先核对 tests-archive 的实际条目。
- 优先级: P2
- 复杂度: 中
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `ab41df2`。

  **先纠正本条目原文的一处事实(重要,别把它当成历史结论继承下去)**:标题与影响字段写的「四条 UI 记录**互相覆盖**」**不成立**。实测核对 `.kanzei/project/tests-archive.md`:`T-1786297655` 的四条记录(:820/:825/:830/:835,标题分别是 R-153 的 i18n / a11y / Markdown / runtime 四条冒烟)**全部落盘存活,没有任何内容丢失**;`T-1786341674` 的两条同理(:1157/:1162)。wave 排他与 R-171 的写租约都真的生效了。真实损害只有一条:**ID 不唯一 → 无法按 id 引用、无法按 id 收尾**(工具要求「用显式 id 收尾」,同号时不知道收的是哪一条)。

  **根因判断(与原文的「并行」推测相反)**:根因不是并发,是分配器**只读系统时钟不读文件**——`append_test_run` 直取 epoch 秒当 ID,同一秒内的连续两次调用必然同号。**串行 ≠ 唯一**:串行保证的是「同一时刻只有一个写者」,唯一性要的是「分配前看过已发出的编号」,两件事无交集。所以 **R-171 的写租约在原理上修不掉它**,本条也不该等租约。归档实证支持这一判断:四条记录标题各不相同,连「收尾」时刻都是同一秒。

  **逐条对照原文的诉求(本条无编号验收字段,按「影响」字段的两条诉求对照)**

  ①「测试证据可一一引用 / 记录唯一性」→ **达成**。`crates/kanzei-tools/src/test_record.rs:82` 新增 `allocate_test_id(root)`:扫 `active ∪ archive` 全部 `T-<n>`,取 `max(now_secs, max+1)` 单调推进。**选单调推进而非加后缀**是有理由的——`running_age_secs`(:67)与 `last_passed_at` 都对 `T-<epoch>` 做 `parse::<u64>()`,ID 一旦带后缀这两条会**静默失效**,且 1400+ 条历史记录要跟着迁移;代价只是突发时编号领先墙钟几秒,配套把 `running_age_secs` 的 `checked_sub` 改 `saturating_sub`(:68),编号领先墙钟时 age 归 0 而不是让 `age_secs`/`stale` 两个字段整个消失。测试:`同秒串行四次登记必须拿到四个互不相同的id`(:1044)、`写排他下并发四次登记编号仍互不相同`(:1085)、`新编号必须跳过归档里已占用的最大编号`(:1134)、`编号保持纯u64且领先墙钟时不判悬空`(:1274)。

  ②「归档完整性」→ **达成,双向把关**。新登记路径 `ensure_id_unused`(:107):编号已被 active/archive 任一条占用且标题不同 → **报错拒写、不自动改号**(沿用 `docstore::repair_reused_archived_id` 的保守立场:静默改号会把编号复用伪装成一次正常写入,证据链就此不可信,D-004 口径),错误文本给全冲突编号、两个标题、所在文件、「未写入任何内容」与两条可执行的下一步;测试 `编号已被占用时拒绝再登记并说明理由`(:1153)。归档侧同样把关(:262-286):内容相同幂等跳过,内容不同报错不追加;测试 `归档已有同编号且内容不同时拒绝追加`(:1242)。另外把「用显式 id 收尾」这条纪律做成可执行的——工具输出回显本次分配的编号(`recorded T-xxx`,:635/:643;running 状态追加「跑完请用 id=T-xxx 记终态」),不回显的话该纪律在源头就无法执行,正是本缺陷反复复发的机制缺口;测试 `工具输出必须回显本次分配的编号`(:1182)。

  **顺带修掉一处勘察时没看出来的静默失败**:旧 `record_test_run` 用 `str::replace` 做块替换,`old_block` 找不到时**原样返回、照样 `fs::write` 回去并返回成功快照**——一次收尾彻底丢失且无任何提示。现在改为定点区间替换(:400-414),找不到就明确报错「未写入任何内容,请重新读取列表后重试」(D-004 口径);同时消除了「两条字节相同的块被一次连坐换掉」的问题,测试 `定点替换不得误伤内容相同的另一条记录`(:1206)。

  **设计自我修正(留档)**:原打算加「落盘前全文件编号唯一性后置校验」,写测试时发现它与定点替换**直接冲突**——那会让已含重复编号的历史文件彻底无法写入,连修复性收尾都做不了。改为只在新登记路径查占用。

  **既有能力标注(§1.25)**:`test_record` 的追加/就地收尾/归档搬运/悬空 running 标记均为既有实现;本次产出是编号分配器、同号拒写(双侧)、定点替换与输出回显。

  **残余缺口去处(§1.2)**:①**跨进程原子性(CAS)未做**——分配器的单调推进只在单进程内成立,两个 OS 进程同时 `test_record` 仍可能撞号;按裁决并轨到 R-138 新建的 `crates/kanzei-tools/src/atomic_file.rs`(仓里只留一套原子写/文件锁原语),已登记 **D-261**。②**历史 6 条重复编号未清理**(本次修复**刻意不回改历史**,需要显式的一次性修复入口)→ 已登记 **D-259**。③`test_runs_snapshot` 是只读命令却写盘且不持任何锁 → 已登记 **D-260**。

  **验证**:交付时定向 `cargo test -p kanzei-tools` 208 passed 0 failed(test_record 21 条 = 13 原有 + 8 新增,八条都是真回归闸、旧实现逐条会挂),rustfmt clean,clippy 零诊断指向本文件;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度中,满足 §1.4 全量触发点①)。

## D-249 docs_snapshot 把读失败静默降级成空列表:unwrap_or_default 叠加 docstore 非原子写,前端拿到「成功但空」的快照 [fixed] (high)
- 优先级: P1
- 复杂度: 中
- 标签: 后端
- refs: R-138 D-244
- 证据等级: E1(四处代码实证 + 竞态探针实测)
- 依据: 2026-08-10 持久化面全面审计。四层叠加构成一条「瞬态空快照」通道:
  ①`crates/kanzei-tools/src/docstore.rs:307` 是 `std::fs::write(&self.path, text)`——**截断后重写,非原子**;
  ②同文件 285-291 的 `load()` 对空文件/少条目一律返回 `Ok`,不报错;
  ③`crates/kanzei-app/src/docs.rs:96` 是 `store.load().unwrap_or_default()`——**任何读失败(含 Windows 文件占用)静默降级成空列表**;
  ④`docs.rs:87-89` 每次 `docs_snapshot` 开头都跑 `archive_terminal()`,**它自己就在写这几个文件**,而它只在「有条目刚进终态」时才写——正是 `refreshDocsSoon` 被触发的同一时刻。
  于是一次 `refreshDocs`(用户点标签页)与一次 `refreshDocsSoon`(agent 事件,400ms 去抖 + IPC)完全可以同时在飞:一个在写,一个读到被截断的文件,前端拿到一份**「成功但空」**的快照。
- 影响: 不止筛选——计数归零、列表闪空都从这里来;且因为它长得像"成功",所有下游都不会重试或报警。本轮已在前端加了两道收窄(D-169 回落加空列表守卫、refreshDocs 换项目重认),但截断读到「部分条目」时 `entries.length` 仍 > 0,**前端只能收窄不能封死**。
- 修复方向: ①`DocStore::save` 与 `archive_terminal` 改 tmp+rename 原子写(与 R-138 同一件事,可并轨);②`docs_snapshot` 别把读失败 `unwrap_or_default()` 成空列表——读失败要么向上报错让前端保留上一份快照,要么显式区分「真的没有条目」与「读不到」。
- 验收: ①并发写 + 读的压测下,前端不会收到「成功但空」的快照;②读失败有可见信号(不静默降级);③原子写落地后 tracker 文件不会被读到截断态;④有回归测试。
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `b4bda5c`,与 **R-138** 并轨交付——本条描述的是同一条竞态通道的四层叠加,①③层归 R-138(原子写),②④层归本条。

  **逐条对照验收原文(四条)**

  ①「并发写 + 读的压测下,前端不会收到『成功但空』的快照」→ **达成**。回归测试 `docs_snapshot_并发刷新不会返回成功但空的快照`(`crates/kanzei-app/src/state_tests.rs:295`):一个写线程反复整文件重写 `requirements.md`,主线程连做 60 次 `docs_snapshot`,每次断言条目数恒等于 6(不是「>0」——那样缺条目也能绿)。打的是完整通道:`docs_snapshot` 自己开头就会写(`archive_terminal`),正是依据字段第④层描述的重叠窗口。**反证**:回退原子写后跑同一用例,立刻打出「读到了截断态:条目数 0,只可能是 3 或 30」——本条描述的「成功但空」逐字复现。

  ②「读失败有可见信号(不静默降级)」→ **达成**。`crates/kanzei-app/src/docs.rs` 的 `docs_snapshot` 签名从 `-> serde_json::Value` 改为 `-> Result<serde_json::Value, String>`,**4 处** `unwrap_or_default()` / `map_or(0, ..)` 全部换成 `?` + `read_failed(kind, e)`(点名读不到的是哪个文件)——**比本条依据字段描述的多 3 处**:依据只点了 `docs.rs:96` 的 `store.load().unwrap_or_default()`,实际同函数里 `batch_ids` 收集、`archived`(load_archive 计数)、`archived_entries`(load_archive 取值)、`load`(每个 kind 的条目)四处都是同一个洞。回归测试 `docs_snapshot_读失败向上报错而不是空列表`(state_tests.rs:258,`#[cfg(windows)]`):先确认正常路径读得到 1 条(免得夹具不对导致假阳性),再用 `share_mode(0)` 独占占用文件,断言 `expect_err` 且错误文本含 `requirements.md`,最后解除占用断言自动恢复、不留粘滞状态。**反证**:回退该改动后,panic 里直接打出 `"requirements": Array []` 的成功载荷。

  ③「原子写落地后 tracker 文件不会被读到截断态」→ **达成,由 R-138 交付**(§1.25 显式标注:这一条不是本条的产出)。`crates/kanzei-tools/src/docstore.rs` 的四个整文件写点全部改 `crate::atomic_file::write_atomic`(:348 save / :461 与 :523 archive_terminal / :608 void_id 及 :392 repair_reused_archived_id 路径),读者只可能看到旧全量或新全量。

  ④「有回归测试」→ **达成**,即上面两条(state_tests.rs:258 与 :295),加上 R-138 侧 `crates/kanzei-tools/src/atomic_file.rs` 的 7 条与 `tracker.rs:2354` 的并发闸。

  **关键设计口径:开头那次 `archive_terminal` 明确不挂写租约**。它走 `DocStore::try_lock(200ms)` 限时文件锁(docs.rs 内 `store.try_lock(Duration::from_millis(200))`),拿不到就**跳过归档但正常返回读结果**,失败进新增的 `warnings` 数组 + `tracing::warn!`,**不参与整体失败**——原先是 `let _ =`,归档失败连一行日志都没有。不挂租约的理由:`MemoryCoordinator::acquire_writer_lease` **无超时**,挂上去会让文档面板在 agent 跑一轮期间整段卡死,等于拿一个更严重的问题换一个更轻的。该口径已写进 `docs/design/parallel_read_serial_write_orchestration.md` **不变量 8 的 2026-08-10 补注**(提交 `79852a5`,文件 :108-109):**代理发起的写动作走租约;界面读路径顺手做的幂等维护走文件锁**。后续 D-260 照此口径修 `test_runs_snapshot`。

  **零 `.js` 改动,以及为什么这成立**(这是本方案的关键收益,不是省事):前端两个消费点 `crates/kanzei-app/ui/14-docs-actions.js` 的 `refreshDocs`(:11)与 `refreshDocsSoon`(:45)**现成就是「保留上一份快照」**——`invoke` 抛错时走 `catch`,`renderDocsSnapshot` 根本不会被调用,`latestDocsSnapshot` 与屏幕上那份列表原样留着,只多一个 `toastError` / `console.error`。旧实现给的是「成功但空」,前端**没有任何办法**分辨,只能被迫重绘成空;改成抛错之后,降级方式正好落在前端已有的正确路径上。新增的 `warnings` 字段也不需要改 `.js`:前端忽略未知键。冒烟 `ui-runtime` 743 invoke 0 错误,证实零 `.js` 改动成立。

  **既有能力标注(§1.25)**:前端两处 `catch` 的「不重绘即保留上一份」语义是既有实现(D-169/D-250 系列改动带来的),本条只是让后端不再把读失败伪装成成功,不重复申报前端行为为本次产出。

  **残余缺口去处(§1.2)**:同类「只读命令顺手写盘且不持锁」的最后一处在 `test_runs_snapshot` → 已登记 **D-260**;`test_record.rs` 五处裸 `std::fs::write` 未并轨 `atomic_file` → 已登记 **D-261**。

  **验证**:交付时定向 kanzei-tools 208 / kanzei-app 53 全绿,clippy `-D warnings` 零输出,ui-runtime 冒烟 743 invoke 0 错误;关闭前全量 `cargo test --workspace` exit=0、524 passed(复杂度中,满足 §1.4 全量触发点①)。

## D-250 refreshDocs 的 catch 里 clearPendingJump 没有项目守卫:旧项目刷新失败会作废新项目刚排的跳转高亮 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- refs: D-249
- 证据等级: E1(探针实测 pendingJumpId 从 "R-901" 变 null)
- 依据: 2026-08-10 收口验证顺带发现。crates/kanzei-app/ui/14-docs-actions.js 的 refreshDocs 本轮加了「await 前后各认一次项目」的守卫,但**只加在成功路径**;catch 里的 clearPendingJump() 没有同样的守卫。于是替旧项目发出的那次刷新若在用户切走之后才抛错,会把**新项目刚排上的**跳转高亮一并作废。
- 影响: 只丢高亮,不动数据——用户点了条目引用跳过去,却看不出落在哪一条。窄,但属同一条路径上的不对称(成功路径按项目收敛了、失败路径没有)。
- 修复方向: catch 里同样比对 forProject === currentProject,只作废属于自己那次刷新的挂起跳转。
- 验收: 旧项目的刷新失败不影响新项目已排的跳转高亮;有拦截实测的冒烟断言。
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `36ce685`,与 **D-251** 同批(同一族「await 前后项目身份不一致」)。

  **逐条对照验收原文(两条)**

  ①「旧项目的刷新失败不影响新项目已排的跳转高亮」→ **达成**。`crates/kanzei-app/ui/14-docs-actions.js` 的两条 `catch` 各加项目守卫,改法与成功路径对称:`refreshDocs` 的 catch 改成 `if (currentProject === forProject) clearPendingJump();`,`refreshDocsSoon` 的 catch 同改。**同族第二处一并修**——`refreshDocsSoon` 由 agent 的文档变更事件驱动、自带 400ms 合并窗口,定时器落地时用户早就可能切走,撞上的概率比 `refreshDocs` 更高;冒烟里早有实证「只修一处、整套照样全绿」,所以两处必须各自单独钉。`toastError` / `console.error` 保持**无条件**:刷新确实失败了,与用户此刻停在哪个项目无关,该看见就得看见(D-004 口径)。

  ②「有拦截实测的冒烟断言」→ **达成**。`scripts/ui-runtime-smoke.mjs` 新增两块:`旧项目的刷新失败不得作废新项目刚排的跳转高亮(D-250)`(:3776)与 `refreshDocsSoon 的失败路径同样要按项目收敛(单独钉,D-250)`(:3830),复用 :3645 起的「甲乙两个项目 + 闸门」夹具。**反证结果(把修复改回旧代码必须判红)**:S1 `refreshDocs` 去守卫 → 红 2 条;S2 `refreshDocsSoon` 去守卫 → 红 2 条;**且 S1 与 S2 各自只红自己那块**,证明两条路径被互相独立地钉住,不是一块断言顺手覆盖了两处。

  **验证**:`node --check` 三文件 OK;ui-runtime 743 invoke 0 错误(本批新增 4 个断言块,invoke 589 → 743)、ui-a11y 22 icon-btn、ui-i18n 871 key、ui-markdown 全通过。复杂度小,按 §1.4 全量测试**非必需**;本轮仍随同批条目跑了全量 `cargo test --workspace` exit=0、524 passed。

## D-251 kz-worktrees 键在 await 之后才取:切项目撞上 IPC 会把甲项目的工作树写进乙项目 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- refs: D-249 D-250
- 证据等级: E2(代码形态实证 + `git show HEAD:` 确认既有)
- 依据: 2026-08-10 持久化面审计。crates/kanzei-app/ui/09-sessions.js:67 与 :82 的 `kz-worktrees:${currentProject}` 是在 `await invoke(...)` **之后**才取键的——与本轮修掉的 refreshDocs 同一类跨项目错写:切项目撞上 IPC 时,甲项目新建/丢弃的工作树路径会写进乙项目的键。
- 取证: `git show HEAD:crates/kanzei-app/ui/09-sessions.js` 形态相同,**HEAD 就有,不是 2026-08-10 侧栏重构引入的**。
- 影响: 比文档刷新那条窄得多——要用户点「建/弃工作树」按钮后立刻切项目才撞上;但一旦撞上,工作树清单会长期错位(它是纯前端 localStorage 清单,不从 `git worktree list` 发现,见 R-050 退回原因④)。
- 修复方向: 与 refreshDocs 同一改法——await 前把 currentProject 存成局部量,await 后比对,不一致就丢弃本次写入。
- 验收: 切项目时的在途工作树操作不写进新项目的键;有回归覆盖。
- 批次: 1/1
- 关闭说明: 2026-08-10 关闭(fixed)。交付提交 `36ce685`,与 **D-250** 同批。

  **逐条对照验收原文(两条)**

  ①「切项目时的在途工作树操作不写进新项目的键」→ **达成**,`crates/kanzei-app/ui/09-sessions.js` 三处全部改成 await 前认领局部量 `forProject`:`refreshWorktrees`(键与逐条 `worktree_diff` 的 `projectDir` 都用 `forProject`,末尾再加 `if (currentProject !== forProject) return;` 不把旧项目清单画进新面板)、`handleWorktreeAction`(函数首行 `const forProject = currentProject;`,`worktree_merge` / `worktree_discard` 的实参与放弃后的 `kz-worktrees` 读写全走它)、`worktree-add` 点击处理(同上)。**依据字段只点了 :67 与 :82 两处;`refreshWorktrees` 的 `projectDir` 原本也在 for-await 循环里逐次重取,清单越长在飞越久,一并收敛**——这是超出条目原文的第三处。

  ②「有回归覆盖」→ **达成**。`scripts/ui-runtime-smoke.mjs:3890` 起的 `在途的工作树操作不得写进已经切走的项目的键(D-251)`,覆盖新建与放弃两条路径,**每条配正反两个方向的断言**。

  **一处对验收的解释,显式记录(不悄悄改)**:采用「**写进认领项目的键**」而非条目「修复方向」字段写的「不一致就**丢弃**本次写入」。理由:工作树清单是**纯前端 localStorage 清单,不从 `git worktree list` 发现**(R-050 退回原因④),而工作树在磁盘上已经真的建出来 / 真的放弃了;丢弃写入等于把「错位」换成**更难恢复的「丢失」**——错位至少还能在另一个项目的清单里看见,丢失则任何一次刷新都不会把它找回来。验收**原文**是「不写进新项目的键」,写进认领项目照样满足,不缩小范围。**反证实测支持这个选择**:S5「新建改成丢弃写入」→ 红 1 条(正是「没落进它真正所属的项目甲的键」那一条)。

  **反证结果(把修复改回旧代码必须判红)**:S3 新建键改回 await 后取 → 红 2 条;S4 放弃键改回 → 红 2 条;S5 新建改成「丢弃写入」→ 红 1 条。

  **反证过程补掉的一个断言盲区(值得留档)**:只做值断言时 S4 那一侧**恒绿**——放弃路径把甲的改动写进乙时,被过滤掉的是乙**根本没有的**路径,写回去逐字不变,纯值断言看不出发生过错写。于是在冒烟里加了**写入去向探针**(包住 `localStorage.setItem`,记录本次写了哪些 `kz-worktrees:*` 键),错写才无处可藏,S4 随即判红。

  **既有形态标注**:本条是 HEAD 既有缺陷(`git show HEAD:crates/kanzei-app/ui/09-sessions.js` 形态相同),不是 2026-08-10 侧栏重构引入,已在依据字段取证。

  **顺带说明(不属本条,别误以为被一起修了)**:同文件 :86 的 `}("click", refreshWorktrees);` 破损行(`worktrees-refresh` 按钮全仓无监听器)仍在,归 **D-257**。

  **验证**:`node --check` 三文件 OK;ui-runtime 743 invoke 0 错误、ui-a11y 22 icon-btn、ui-i18n 871 key、ui-markdown 全通过。复杂度小,按 §1.4 全量测试**非必需**;本轮仍随同批条目跑了全量 `cargo test --workspace` exit=0、524 passed。

## D-257 worktrees-refresh 刷新按钮全仓无监听器:addEventListener 前半段被重构吃掉,只剩 no-op 逗号表达式 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 证据等级: E1(按钮存在、全仓零绑定、git log -S 定位引入提交,三处独立实证)
- refs: D-211
- 复现: 侧栏「隔离工作树」区块标题右侧的刷新按钮(↻)**点了没反应**。
- 依据①(按钮确实存在): crates/kanzei-app/ui/index.html:79 —— `<button id="worktrees-refresh" class="icon-btn" title="刷新工作树差异" aria-label="刷新工作树差异">↻</button>`,位于 `#worktrees-section` 的 section-title 内。**注意 id 是 `worktrees-refresh`(复数 worktrees),不是 `worktree-refresh`**:按单数形式 grep 会一无所获并误判成「元素已删除」。
- 依据②(全仓零绑定): `grep -rn "worktrees-refresh" crates/ scripts/` 只命中 index.html:79 那一行,没有任何 JS 绑定它。
- 依据③(破损行): crates/kanzei-app/ui/09-sessions.js:86 是 `}("click", refreshWorktrees);` —— `$("worktrees-refresh").addEventListener` 的前半段丢了。`}` 结束的是上方 `async function handleWorktreeAction(item, action)` 的函数**声明**,其后的 `("click", refreshWorktrees);` 成了一条独立的、合法但完全 no-op 的逗号表达式语句(函数声明不是表达式,不会被调用)。`node --check crates/kanzei-app/ui/09-sessions.js` **通过**——语法检查抓不到它,正是 conventions §1.3「前端改动不得只以 node --check 作为验证证据」说的那类漏网。
- 取证: `git log -S 'worktrees-refresh").addEventListener' -- crates/kanzei-app/ui/` 与 `git log -S '}("click", refreshWorktrees);'` 共同指向 **7c5f022「增加工作树操作失败重试入口」(2026-08-07)**;`git show 7c5f022 -- crates/kanzei-app/ui/main.js` 的 diff 逐字为 `-$("worktrees-refresh").addEventListener("click", refreshWorktrees);` / `+}("click", refreshWorktrees);`——把工作树操作抽成 `handleWorktreeAction` 时,新函数的收尾 `}` 覆盖掉了下一行的 `$("worktrees-refresh").addEventListener` 前缀。R-154 B5(9349b45)切出 09-sessions.js 时原样带了过来。**HEAD 既有**(HEAD=36ce685 的 :86 仍是同一形态),不是本轮改动引入。
- 结论(纠正勘察分歧): 按钮**没有被删**——这**不是删残留,是真正的按钮失效**,修法是恢复绑定而不是清理死代码。
- 影响: 工作树差异清单只剩自动刷新路径(handleWorktreeAction 成功后 09-sessions.js:81、worktree-add 成功后 :99、以及 14-docs-actions.js:16 与 02-i18n.js:754 的整体刷新),用户看到过期状态时**没有手动刷新手段**。危害窄(工作树本身低频),但属于「界面承诺了能力却没有能力」,与 D-211 同族。
- 修复方向: 把 09-sessions.js:86 还原成两行——函数声明收尾的 `}`,以及独立一行 `$("worktrees-refresh").addEventListener("click", refreshWorktrees);`。
- 验收: 二选一,不留中间态。**优先①**——①按钮真能刷新:点击 `#worktrees-refresh` 后 refreshWorktrees 被调用且工作树清单重渲染,scripts/ui-runtime-smoke.mjs 有对应冒烟断言(断言点击后触发 worktree 相关 invoke);或②按钮与 09-sessions.js 的 no-op 残留一起清理干净(index.html 不再有该按钮、JS 不再有那条逗号表达式)。选②等于删掉用户可见的界面能力,属缩小范围,需先经用户同意。
- 进展: **已按验收①交付并关闭**(`c3398b5`,经 `eb50db6` 并入 dev)。2026-08-11 任务级并行实测的线 B 产出,改动面只含 `crates/kanzei-app/ui/09-sessions.js`(恢复被吃掉的 `$("worktrees-refresh").addEventListener` 前缀)与 `scripts/ui-runtime-smoke.mjs`(点击后断言真打出 `worktree_diff` 且 `projectDir` 正确、清单按新数据重渲染,另加一条"按钮从 index.html 消失即判红"的前置断言防止将来滑向验收②)。**反证独立复核过**:把文件改回破损形态后 `node --check` **仍然通过**(正是依据③说的那类漏网),而冒烟精确判红两处;还原后转绿。合并后全量门禁复跑:fmt 干净、前端冒烟通过、`cargo test -p kanzei-tools` 217 全过。

## D-262 shell::kill_tree 从未真正击杀进程树:2 秒 timeout 叠加 kill_on_drop 反而先杀死 taskkill 自己 [fixed] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 证据等级: E1(实测可复现 + 代码形态自证)
- refs: D-174 R-097 R-139
- 复现: 2026-08-10 交付 D-174 写「停止后台任务」测试时暴露,三条实测证据:
  ①`kill_tree(pid)` 恒定耗时 **2.008 秒**(正好是它自己的超时)后返回,目标进程 `alive_after=true`;
  ②把超时去掉单独跑,内层 `taskkill` 阻塞约 **27 秒**(直到目标进程自然结束)才返回 `exit=128`;
  ③`current_thread` 与 `multi_thread` 两种 tokio runtime 都复现;换 `std::process` + `spawn_blocking`、去掉 `hide_console_async` 均不解决。
- 根因(代码形态自证,`crates/kanzei-tools/src/shell.rs` 的 `kill_tree`): `command.kill_on_drop(true)` 与 `tokio::time::timeout(2s, command.output())` 叠在一起——**超时丢弃 future 的那一刻,`kill_on_drop` 把 taskkill 进程本身杀了**。于是每次调用的实际行为是「启动 taskkill → 两秒后杀掉 taskkill → 返回」,目标进程树毫发无伤。返回值还被 `let _ =` 吞掉,失败完全不可见。
  次因待查:证据②说明 taskkill 在本机确实需要远超 2 秒才返回(疑似 `output()` 等待管道关闭,而管道被目标进程树里的某个成员继承着——典型的「grandchild 继承 stdout 导致 output() 挂住」形态)。若属实,则超时值调大也治不好,应改为不捕获输出(`status()` 而非 `output()`)或显式给 taskkill 的 stdio 设 `null`。修复前必须先证实这一条,不要只把 2 秒改成 30 秒。
- 影响(超出 D-174 的范围): ①`process stop` 名义返回 stopped、实则进程还在跑,用户以为停了;②`bash` 工具的超时击杀同样失效,超时只是让工具调用返回,被击杀的进程继续持有文件与端口;③D-174 的后台越界处置里「回滚后 kill 进程树」这一加固项目前无效——该条已在 D-174 交付时如实标注,其验收②靠的是隔离+回滚+归因这条与 D-173 前台围栏同口径的路径,不依赖 kill。
- 边界: `shell.rs` 在 D-174 交付时未被修改(尝试性修复未解决问题,已 `git checkout` 还原干净),本条是独立缺陷。
- 验收: ①`kill_tree` 调用后目标进程树**真的消失**(实测断言 `alive_after == false`,不是断言函数返回);②taskkill 失败/超时不再被静默吞掉,至少有 `tracing::warn!` 级别的可见信号(D-004 口径);③`process stop` 与 `bash` 超时两条路径各有一条断言进程真的退出的回归测试——注意 D-174 交付时**刻意拆掉了两条会因为错误的原因而通过的断言**,本条修复后要把它们按正确形态补回去;④非 Windows 分支保持可编译。
- **本条「根因」的次因猜测已被实测证伪(修复时必读)**: 原文要求「修复前必须先证实 taskkill 因 `output()` 等管道关闭而挂住」。2026-08-11 交付时按要求做了独立复现程序(std only,每档测「victim 何时死」而非「函数何时返回」,单独拍孙进程 pid),**结论是这条猜测不成立**:读管道 / 不读管道 / `status()`+`stdio(null)` 三档耗时 **1097–1139 ms,毫无差别**;拆开测量时 taskkill 进程退出与 stdout EOF 落在**同一毫秒**(1172/1172)。机制上也讲不通——taskkill 的管道是它自己 spawn 时才建的,晚于目标树,目标树继承不到。**照原文方向改(换 `status()` 或 stdio 设 null)根本治不好本缺陷。**
  真因是 `taskkill.exe` 的**启动延迟**:对一个不存在的 pid 连打三次是 2907 / 4230 / 1071 ms,原来那条 2 秒的线本就压在延迟分布中间;负载下实测到一次 `kill_tree` 耗时 20.04 秒,其中 15 秒是 taskkill 超出等待上限、最后由 `TerminateProcess` 收尾。原文「证据②约 27 秒 + exit=128」由此得到解释:不是管道挂住,是负载下的进程创建延迟——等 taskkill 终于跑起来时,5 秒的目标已自然退出,所以报「找不到进程」。
  这条测量还改变了修法,是本条最值钱的一句:**需要击杀进程的时刻,往往正是机器忙得起不动新进程的时刻**。所以「靠 spawn 一个新进程去杀进程」结构上就是错的。
- 进展: **已交付并关闭**(`29e5b42`/`25c251d`,经 `merge par/d-262` 并入 dev)。2026-08-11 任务级并行实测的线 A 产出,中途被误判为已死、由人替它提交过一个 WIP(见 D-263 同族教训),它自己续跑至完成。
  实现:主手段改为 `CreateToolhelp32Snapshot` 拍进程树名单 + 逐个 `TerminateProcess`,taskkill 降级为兜底;新增 `process_alive`(`OpenProcess`+`GetExitCodeProcess`,可对任意 pid 提问,不像 `Child::try_wait` 只能问直接子进程)与击杀**前**的进程树快照(击杀后父子关系随进程消失,再也问不出树的形状)。`kill_tree` 从 **2.008 秒(什么也没杀)** 变成 **10–12 毫秒(真杀干净)**。
  顺带修掉交付过程中自己引入的一个坑:根 pid 已死不能短路返回成功——bash 超时路径上 shell 的 `kill_on_drop` 会先杀根,短路会把孙进程永久留下(正是本条影响②);快照能认出孤儿(Windows 保留创建者 pid),所以这条走得通。
  验收:**①**断言 `process_alive` 对根**和孙进程**都为 false,关键反证是**把旧实现的等价体连打 5 次,5/5 恒定 2.01 秒返回且整棵树全都活着**,与原文证据①逐字吻合——证明这些测试真能抓到 D-262,不是因为错误的原因通过;**②**每条失败路径都有 `tracing::warn!`(启动失败/非零退出/超出等待/残留清单),返回值改 `bool`,调用点无需改动;**③**三条路径各有测试,`background.rs` 与 `bash.rs` **补回了 D-174 刻意拆掉的那两条断言**,命令换成 300 秒长驻 + 带孙进程(自然退出冒充不了击杀),并把孙进程排到越界写之前(否则测试会静默退化成只查根);**④**用 cfg 翻转在本机实编译了非 Windows 分支(`--all-targets` 无错),但 Linux target 的 std 未装,**未做真交叉编译**——如实标注。
  合并后全量门禁:fmt 干净、clippy `-D warnings` 干净、`cargo test --workspace` 16 个测试目标全绿。

## D-269 bash 权限可被历史授权提权:normalize_resource 非单射,在已批准命令的任一斜杠处插入 T/../ 即可带进任意 shell 语句 [fixed] (high)
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 证据等级: E1(**我在 dev HEAD 上独立复现**,非仅采信复核结论;见「实测」)
- refs: D-050 D-051 D-267 R-183 docs/design/tier1_implementation_plan.md
- 来源: 2026-08-11 第一梯队 F1 的对抗复核。复核本意是验 F1 新加的兼容垫片,**结果发现同一个洞在改动之前就存在**——F1 只是把它显式化并书面认证为"安全"。**本条与 F1 无关,是既有缺陷,已发布的 `build-ad80b2d` 里就是活的。**
- 根因: `normalize_resource`(`crates/kanzei-harness/src/permission.rs:180-215`)按 **D-050** 的设计做路径规范化——弹出 `..` 的前一段、折叠 `//` 与 `/./`、`\`→`/`、Windows 下整串小写。这些操作**故意是非单射的**(多个输入映到同一输出),对路径资源是正确的。
  问题在 `crates/kanzei-core/src/runner/drive.rs` **:545 / :604 / :764 三处**对**所有** action 的资源一律 `normalize_resource`,**bash 也不例外**。而 bash 的资源是 `{"command":...,"workdir":...}` 的 **shell 文本**,不是路径。于是:
  ①落盘的 pattern 是规范化后的串;②运行期的 value 也被规范化;③二者逐字节比较。
  **结果是一条规则准入的不是一个命令,而是 `normalize_resource` 的整个原像类。**
  垫片式修法(F1)把判据写成 `pattern == normalize_resource(value)`,并论证"确定性保证每个 V 只对应唯一 P"——**这个不变量写反了**:那是函数性,授权需要的是反方向的**单射性**(每个 P 只准入唯一 V),而 `normalize_resource` 恰恰被设计成非单射。
- 影响(提权,不是可用性): 在**任一条已批准命令**里含至少一个 `/` 时,把该 `/` 替换成 `T/../`(T 为任意不含 `/` 的串)即可注入任意 shell 语句——T 在规范化时被 `..` 整段弹掉,注入版与原版的规范化结果逐字节相等。用户配置里 21 条 bash 规则大多含 `/`。
  **D-051 的降级同时失效**:注入段里的 `*` 在 pattern 成形前就被抹掉,pattern 不含 `*`,`command_chaining_escapes` 不触发。
  命令文本确实原样执行:`crates/kanzei-tools/src/bash.rs:87` 把 `input["command"]` 逐字节放进 resource JSON,`execute` 用的是同一个 `input.command`,**中间无任何再校验**。
- 实测(2026-08-11,**我在 dev HEAD `b53b9aa` 上独立跑的**,scratch crate 依赖仓内真实 `kanzei-harness`,未改仓库任何文件): 
  已批准(取自 `.kanzei/kanzei.toml` 第 11 条真实规则):
  `git grep -n "cleanup_orphan_webviews" -- crates/kanzei-app/src/main.rs`
  注入版:
  `git grep -n "cleanup_orphan_webviews" -- crates/; Remove-Item -Recurse -Force $HOME ;/../kanzei-app/src/main.rs`
  输出:`两条命令不同 true` / `规范化后相等 true` / `evaluate(已批准命令) Allow` / `evaluate(注入命令) Allow`。
  复核方另在第 5/9/12 条规则与 `cargo --manifest-path` 上给出同形态提权链,并验证 F1 之后的新落盘形态(未 mangle 的原串,只要本身是规范化的不动点)**同样中招**——所以这不是只影响历史规则的一次性兼容窗口。
- 修复方向(待设计,勿直接照做): 根子是**对 bash 资源施加了路径语义**。正确方向是让 bash 资源**彻底不经过任何路径规范化**——`drive.rs` 三处按 action 分流,bash 走原样、其余仍走 `normalize_resource`(**只能改 bash 分支**:write/edit/read 少了 normalize 会让 D-050 的四条路径测试与 `write.rs` 的落点一致性测试同时红)。
  配套问题:既有落盘的 pattern 已经是规范化后的串,停止规范化后它们与原串失配。二选一——①加载时一次性迁移(反解或标记失效要求重新授权);②保留一个**只做逐字节相等**的兼容读取路径。**注意 ② 正是 F1 的形态,而它就是被本条否掉的那个**——若走 ②,必须证明它不引入原像类(F1 的论证是错的,不可复用)。
- 边界: 与 D-267(缺一个安全中间档)是**不同**的问题。D-267 是"偏严到没有可用中间档";本条是"偏松到历史授权可提权"。两条要分别修,不要合成一次改动。
- 验收: ①`drive.rs` 三处对 bash 资源不再调 `normalize_resource`(机械核验:该文件 bash 分支 grep 零命中);②**定向反证**:用本条实测里的那一对命令构造测试,断言注入版为 `Ask` 而非 `Allow`;再补 `cargo --manifest-path ./x/; evil ;/../y.toml` 一条同形态。③既有落盘规则的处置方案落地且有测试(迁移或兼容读取,二者都要证明不引入原像类)。④D-050 的四条路径规范化测试与 `write.rs` 落点一致性测试保持绿(证明只动了 bash 分支)。⑤D-051 的 `command_chaining_escapes` 在注入形态下重新生效,有测试。
- 进展: 2026-08-11 已修复并随 `build-97c8509` 发布。bash 权限资源改为逐字节原文判定，不再进入路径规范化；历史规则中 20/21 本就是规范化不动点，不引入会恢复原像类漏洞的兼容垫片。斜杠注入与 `cargo --manifest-path` 两条反证均由 `97c8509` 前的 K1 测试锁死，D-050 路径用例及 D-051 链式命令降级保持全绿。

## D-274 MemoryCoordinator::release_writer 在持锁临界区内 send 租约:接收端已丢弃时 lease 退回并当场 drop,回调二次锁同一把非重入 Mutex 死锁 [fixed] (high)
- 优先级: P0
- 复杂度: 小
- 标签: 核心
- 证据等级: E1(**我在 dev HEAD 上逐行核实代码形态**,非仅采信复核结论)
- refs: R-171 R-173 R-177 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 批次 K2' 交付时主动上报(它在 app 侧绕开了这个坑,但如实指出根因在 core 里没修)。**与本轮改动无关,是既有缺陷,已发布的 `build-ad80b2d` 里就是活的。**
- 根因(代码形态自证,`crates/kanzei-core/src/orchestration.rs` 的 `release_writer`): 交接分支里 `if let Some(tx) = w.tx { let _ = tx.send(Ok(lease)); }` 这一行在 `self.inner.projects.lock()` 的**临界区内**(锁块直到该行之后才闭合,`self.notify(pending)` 在块外)。
  `oneshot::Sender::send` 在**接收端已被丢弃**时返回 `Err(原值)`——把 `WriterLease` 原样退回。`let _ =` 当场 drop 它,而 `WriterLease` 的 Drop 回调正是 `move |released_run_id| coord.release_writer(&key, released_run_id)` → **二次进入 `release_writer` → 再锁同一把非重入 `std::sync::Mutex` → 死锁**。
- 可达性(今天就可达,不是理论): 任何**被丢弃/abort 的排队 acquire future** 都会造成"接收端已丢弃"。例如 `crates/kanzei-app/src/run.rs` 的 writer run 被停止按钮 abort 时,它排在队列里的 `w.tx` 接收端随之消失;下一个持有者释放租约、轮到唤醒它时就撞上。死锁发生在持有全局 `projects` 锁的线程上,**该项目的所有写仲裁自此永久挂死**(`acquire_writer_lease` / `release_writer` / `snapshot` 全阻塞),只能重启 kzapp。
- 影响: 项目级写仲裁整体失效且不可恢复。并行开发下暴露面被放大——排队者越多、abort 越频繁越容易撞上,而任务级并行的常态正是「多个 writer 排队 + 随时停某一条」。
- 修复方向: 把 `send` 与「send 失败后 lease 的处置」**移出临界区**。形态:锁内只把要唤醒的 `(tx, lease)` 收进局部变量(与 `pending` 事件同一手法,该函数已经在用),锁释放后再 `send`;send 失败时**显式处理**退回的 lease(此时不持锁,drop 回调可安全重入去唤醒下一个排队者),不得再用 `let _ =` 吞掉。
- 验收: ①构造「排队者的接收端已丢弃」(丢弃 acquire future 后由持有者释放租约),断言 `release_writer` **正常返回**且后续 `acquire_writer_lease` 仍能成功——该测试**在修复前必须挂死/超时**(反证);②send 失败时退回的 lease 被显式处置且**队列继续推进**(下一个排队者拿到租约),有测试;③`projects` 锁的临界区内**不再有任何可能触发 `WriterLease::drop` 的语句**(机械核验:锁块内 grep 无 `send(`);④R-171/R-173 既有写租约测试全绿。
- 进展: 2026-08-11 由 `a10d4a5` 修复。锁内只决定下一次交接，`send` 与失败后退回租约的显式 drop 均移到临界区外；接收端丢弃后队列继续推进，后续 writer 仍能获取。反证把实现临时改回锁内 send 后新测试 5 秒超时变红；恢复修复后核心与应用既有写租约测试全绿。

## D-246 内置 provider 删不掉:fill_defaults 无条件回填五个,UI 上删了下次打开又回来 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 依据: 2026-08-10 设置页全字段走查。本轮修好了**自定义** provider 的删除持久化(settings_apply_providers 按「载荷非空即权威」剪枝,有单测钉死空清单不删);但 crates/kanzei-harness/src/config.rs fill_defaults 用 entry().or_insert() 无条件注入 anthropic/ollama/codex/claude/deepseek 五个内置 provider,而 settings_get 在 fill_defaults 之后才列表——删掉这五个中任何一个,配置文件里的子表确实被删了,下次打开设置页它仍会由默认回填重新出现。
- 影响: 用户感知是「删了又回来」,会以为删除功能坏了(实际是自定义 provider 已修好、内置的按设计回填)。与 D-173 的 context_limit 兜底同源。
- 修复方向: 二选一——①UI 上把内置 provider 标成不可删(或删除按钮改「恢复默认」);②给一句「已恢复为内置默认」的说明。不建议改 fill_defaults 本身,那是配置可用性的兜底。
- 验收: 内置 provider 的删除入口不再给出「已删除」的错误预期,用户能看懂为什么它还在。

- 进展: 修复(R-184 批6):settings_get 每个 provider 返回 builtin 标记(kanzei-harness::config::builtin_provider_names(),crates/kanzei-app/src/settings.rs:483-485);前端渲染时内置 provider 删除按钮换成「内置」徽标+title 说明(ui/16-settings.js:111-127);fill_defaults 兜底逻辑不变。单测:内置名单与 fill_defaults 回填一致(kanzei-harness/src/config.rs:308-318)。冒烟:夹具加 anthropic builtin 行,断言内置行有徽标、无 × 删除按钮、载荷保留内置行(scripts/ui-runtime-smoke.mjs:3090-3100,3123-3126)。验证:cargo test -p kanzei-harness 108 全绿、-p kanzei-app 118 全绿、五条冒烟全绿(T-1786448195)。

## D-247 代理选「指定地址」却留空时静默降级成 env,界面零提示 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 依据: 2026-08-10 设置页全字段走查(用户定调「加提示」,登记交自举)。设置页代理模式选「指定地址」但地址框留空时,crates/kanzei-app/src/settings.rs 按空串当 `env` 处理,静默降级,界面没有任何提示——用户以为自己指定了地址,实际走的是环境变量。
- 影响: 静默降级是本仓反复吃亏的模式(D-004「任何拒绝发送的理由都要说出来,绝不静默」同族);代理配错时表现为「设了没用」,排查要一路读到 settings.rs 才看得出来。
- 验收: ①选「指定地址」而地址为空时,界面给出可见提示(表单校验或保存时提醒任一),说明将回落到环境变量;②不静默改写用户选择;③冒烟或单测覆盖该分支。

- 进展: 修复(R-184 批6):设置页代理模式选「指定地址」且地址留空时,updateProxyHint 显示可见提示「地址留空将回落「跟随环境变量」」(ui/16-settings.js:562-573,挂点 span#set-proxy-hint ui/index.html:476,回显与 change/input 均触发);前端不改写用户选择——载荷保持空串由后端回落,非空时提示消失。冒烟:D-247 断言块覆盖 env 隐藏→custom 留空提示可见→填地址消失→留空保存载荷为空串(scripts/ui-runtime-smoke.mjs:3128-3165)。i18n 新 key 已登记(02-i18n.js)。验证:五条冒烟全绿(T-1786448195)。

## D-248 applyProfileValue 切进程时写全局 kz-profile,把用户的全局档位选择静默降级 [fixed] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 前端
- 证据等级: E1(取证 HEAD 逐字一致 + 探针实测)
- 依据: 2026-08-10 持久化面全面审计(35 个写入点逐条枚举)顺带查出。crates/kanzei-app/ui/08-compose.js 的 applyProfileValue 把**进程级**档位写进**全局**键 `kz-profile`。实测:
  用户全局选了 `dev-auto` → switchProcess 到一个 research 进程 → 全局 `kz-profile` 被写成 `research`。
  而该函数上方的回退分支只认 `dev-pair`/`dev-auto`,`research` 被写进去等于**把用户的全局选择降级成 dev-pair**。
- 取证: `git show HEAD:crates/kanzei-app/ui/08-compose.js` 的 applyProfileValue 与工作区**逐字一致**——HEAD 既有行为,不是 2026-08-10 侧栏重构引入的。
- 影响: 切个进程看一眼就把全局偏好丢了,且丢法不可见(下次启动才发现档位变了)。与本轮治的那一族同病:非用户主动的操作改掉并落盘了用户的持久化状态。
- 修复方向: 进程级档位不应写全局键——要么进程档位单独存(按 session/进程 id),要么只在用户主动改档位时写全局。注意别破坏「新进程继承全局默认」的既有语义。
- 验收: 切进程不改写全局 `kz-profile`;用户主动改档位仍正常持久化;有拦截实测的冒烟断言。

- 进展: 修复(R-184 批6,根因已在此前 2773342 移除 applyProfileValue 内写全局键):applyProfileValue 现为只读回显——先查本进程记忆再回退全局,不改写 localStorage kz-profile(ui/08-compose.js:611-620);写全局仅发生在用户主动 change(08-compose.js:623)。本轮补拦截冒烟断言:回显不改写全局键、用户主动切换仍写全局(scripts/ui-runtime-smoke.mjs:3349-3371)。验证:ui-runtime-smoke 全绿(T-1786448195)。

## D-273 kanzei_home 两测试并发互踩全局 KANZEI_HOME,全量门禁偶发红 [fixed] (medium)
- 修复: 已合并为顺序测试 kanzei_home_顺序验证环境变量与默认(kanzei-harness/src/home.rs:30-53),消除全局环境变量互踩。
- 复杂度: 小
- 复现: cargo test --workspace 时 kanzei-harness 两个 home 测试并行跑:kanzei_home_honors_env_var 用进程级 set_var(KANZEI_HOME) 期间,kanzei_home_defaults_to_home_dot_kanzei 读到被污染的变量,断言失败。单跑各自通过,全量偶发红。
- 影响: 全量门禁偶发红(并发测试互踩,非功能缺陷)
- 标签: 核心
- 验收: ①cargo test -p kanzei-harness 全绿(108→107+1 合并后总数不变);②cargo test --workspace 连续两轮全绿;③不再有 KANZEI_HOME 并发写点(home.rs 唯一写点已并入顺序测试)。
- 优先级: P2
- 进展: 修复:合并为顺序测试 kanzei_home_顺序验证环境变量与默认(kanzei-harness/src/home.rs:30-53)——先测 KANZEI_HOME 优先,再测无变量回落 HOME/.kanzei,消除并发互踩。验收核对:①cargo test -p kanzei-harness 107 全绿(两个测试合并后数量正确);②cargo test --workspace 连续两轮全绿(T-1786448527,18 crate);③home.rs 唯一 set_var 写点已并入顺序测试,不再有并发 KANZEI_HOME 写点。

## D-185 `<memory-hints>` 声称只进本轮,实际逐轮累积进对话历史 [fixed] (medium)
- 复现: 开跑前预检索的记忆提示块拼进 `run_prompt`(crates/kanzei-app/src/main.rs 注入点注释写"提示块只进本次运行"),但它随 User message 进 `summary.messages` → 桌面端整份存进 conversations → 下轮作为 `prior` 回灌。跑 N 轮,历史里就躺着 N 个 hint 块。
- 影响: ①每轮固定多烧 N-1 份陈旧提示;②这些块是**当时**的记忆快照,与现行 INDEX.md 可能已经不一致,模型读到的是过期索引却无从分辨;③与 R-106"注入 token 下降"的目标反向。
- 根因: 提示块拼在 prompt 字符串上而不是作为一次性 system/context 段落,持久化路径对它无感知。
- 验收: hint 块不进 conversations 快照(或落库前剥离),连跑 3 轮后历史里最多一个块;注入 token 账单能看出 hint 段的独立占比。
- 证据等级: E2
- 优先级: P2
- 标签: 核心

- 进展: 修复:memory_hints 不再拼进 prompt 字符串,改为 run_once/run_once_with_parts 的新参数(kanzei-core/src/runner/drive.rs:20-24,43-46),作为稳定 system 段注入(stable_system.push,drive.rs:117-121)——system 不进 messages,自然不进 conversations,下轮 prior 回灌不到;context_report 单独记 memory/hints(drive.rs:107-110)。CLI(main.rs:566-568 prompt_hints 独立,run_once 传 memory_hints.as_deref())与桌面端(run.rs:604-607)同步改造;15 处调用点全部补参(其余传 None)。验收核对:①hint 块不进 conversations 快照——集成测试 memory_hints_not_persisted.rs 断言③ summary.messages 无任何 hint 块(连跑 N 轮历史 0 个,满足「最多一个」);②token 账单独立占比——测试断言④ context_report 含 memory/hints 条目,CLI context 打印(source 字符)与桌面端 context 事件均携带该条目。验证:四层断言集成测试绿(T-1786449090),关闭前全量 cargo test --workspace 全绿(T-1786449161)。既有能力标注:prompt_hints 检索逻辑(kanzei-tools memory/mod.rs:931)未动,只改注入通道。

## D-229 harvest_sop 只接了桌面端,CLI 轮末缺失同款 SOP 采集通道 [fixed] (medium)
- 优先级: P2
- 依据: 2026-08-10 memory 系统全量走查。crates/kanzei/src/main.rs 轮末只调 harvest_failures + harvest_entry_fact;kanzei-app/src/main.rs 轮末额外有 harvest_sop——CLI 完成条目不产 SOP 候选,R-124 采集通道双端不对称,遥测口径也随之分裂。
- 修复方向: CLI 轮末补 harvest_sop 同款调用;三个 harvest 收敛为一个共享的轮末采集函数,两端调同一入口,杜绝再次漂移。
- refs: R-124 R-105

- 进展: 已修复并全量验证。逐条对照:
①「CLI 轮末补 harvest_sop 同款调用」——crates/kanzei/src/main.rs 轮末现调 kanzei_tools::memory::harvest_end_of_run(行 646),内部对完成条目调 harvest_sop(global 候选箱),CLI 完成条目现在也会产 SOP 候选;
②「三个 harvest 收敛为一个共享轮末采集函数,两端调同一入口」——crates/kanzei-tools/src/memory/mod.rs 新增 pub fn harvest_end_of_run(行 805):harvest_failures → completed_entry 判定 → harvest_sop(global)→ harvest_entry_fact(project)四步顺序与桌面端既有行为逐条对齐;CLI(main.rs:646)与桌面端(crates/kanzei-app/src/run.rs:733)均只调此入口,杜绝双端漂移;
③ 验证——新增单测 harvest_end_of_run_完成条目投SOP与fact_纯查询轮不投(mod.rs:1310):完成条目轮产 global SOP 候选 + project fact 候选,纯查询轮零投递;global_root 参数注入临时全局记忆根(避免 D-273 式 set_var 并发互踩)。定向测试 kanzei-tools 230/kanzei 集成/kanzei-app 118 全绿(T-1786449431);workspace 全量全绿(T-1786449517),提交 b40478f。
- 验收: ① CLI 轮末补 harvest_sop 同款调用 ✓(main.rs:646 经 harvest_end_of_run→harvest_sop)
② 三个 harvest 收敛为一个共享轮末采集函数,两端调同一入口 ✓(harvest_end_of_run @ kanzei-tools memory/mod.rs:805;CLI main.rs:646 与桌面端 run.rs:733 同一调用)
③ 杜绝再次漂移 ✓(单入口,双端无独立 harvest 逻辑)

## D-230 resident_index 预算装箱按 id 先到先得,新条目被系统性折叠 [fixed] (medium)
- 优先级: P2
- 依据: kanzei-tools/src/memory/mod.rs resident_index 按 load_all 的 id 升序装 3000 字预算,放不下的 continue 折叠——id 越大(越新)的条目越容易被挤出常驻索引,而新条目往往正是当前最相关的;老条目永远优先纯属枚举顺序副作用,不是价值排序。
- 修复方向: 装箱前按价值排序(decision_weight×新近度,或至少 updated 新近优先);与 prompt_hints 的口径保持同源(D-216 教训:两边必须对同一份判定)。
- refs: R-104 R-149

- 进展: 已修复并全量验证。逐条对照:
①「装箱前按价值排序」——crates/kanzei-tools/src/memory/mod.rs resident_index(行 401):装箱前 all.sort_by 按 updated 新近优先、同 updated 按 id_number 降序(id 数字越大创建越晚),取代原先 load_all 的 id 升序先到先得;
②「decision_weight×新近度,或至少 updated 新近优先」——MemoryEntry 无 decision_weight 字段,采用 updated 新近优先(修复方向的最低要求),同 updated 平手按 id 数字降序(新条目优先);
③「与 prompt_hints 的口径保持同源(D-216)」——prompt_hints(mod.rs:958)与 profiles.rs:280 注入侧本就调用同一个 resident_index,一处排序改动两端同步生效,无新分裂;
④ 验证——新增单测 resident_index_价值排序_新近条目优先于老条目(mod.rs:1493):新 updated 优先入选、最老折叠、行序按价值降序、同 updated 时 id 大优先;既有 hints 口径测试(hints_不重复常驻索引_折叠条目才给全行)回归通过。定向 kanzei-tools 231 全绿(T-1786449681),workspace 全量全绿(T-1786449748),提交 ee95bf8。
- 验收: ① 装箱前按价值排序 ✓(resident_index sort_by updated 新近 + id 数字降序,mod.rs:401)
② 新条目不再被系统性折叠 ✓(单测:新 updated 优先、最老折叠)
③ 与 prompt_hints 口径同源 ✓(prompt_hints 与 profiles 注入共用同一 resident_index,D-216 不破坏)

## D-214 SOP 候选投进全局 inbox 无人消化:manager 只读项目 inbox,7 条候选自 08-08 滞留 [fixed]
- 现象: ~/.kanzei/memory/inbox.md 里有 7 条 `## note` SOP 候选(最早 2026-08-08),从未被消化。
- 根因线索: harvest_sop 按设计把 SOP 候选投给 global store 的 inbox(kanzei-app main.rs ~6019),但轮末触发只查项目 store 的 pending_notes,manager 的 memory_inbox_clear 也只清项目 inbox——全局 inbox 是只进不出的死信箱。
- 修复方向(二选一): ①轮末触发与 manager 消化把 global inbox 一并纳入(pending 检查、prompt 注入、clear 都要对齐);②SOP 候选改投项目 inbox,由 manager 消化时按 scope=global 落库(R-124 本意是用户拍板采纳,注意别破坏候选箱语义)。
- 影响: R-124 SOP 提炼链路实际断裂,候选永远到不了用户面前。
- refs: R-124 R-149 (medium)

- 进展: 已修复并全量验证。采用修复方向②(SOP 候选改投项目 inbox,manager 消化时按 scope=global 落库)。逐条对照:
①「SOP 候选改投项目 inbox」——crates/kanzei-tools/src/memory/mod.rs harvest_end_of_run(行 ~825):SOP 候选与 fact 候选同投项目 store(harvest_sop(&project,...)),删掉原先投 global 的 MemoryStore::global() 分支与 global_root 参数;CLI(main.rs:649)与桌面端(run.rs:736)两处调用点同步;
②「由 manager 消化时按 scope=global 落库」——crates/kanzei-tools/src/memory/manager.rs manager_agent 的 system prompt(行 ~830)加例外规则:候选 detail 明确写 scope=global 时 memory_add 用 scope=global(跨项目流程模板覆盖默认 fact/sop→project 规则);harvest_sop 的候选 detail 本就写明「写进 category=sop、scope=global 的候选」(mod.rs:751),manager 消化时按此落库;
③「R-124 本意是用户拍板采纳,注意别破坏候选箱语义」——候选仍在 inbox(项目侧),memory_note_candidates 遍历 project+global 两级 pending,用户可见可采纳;agent 不自决入库的语义不变;
④ 验证——更新 harvest_end_of_run 测试:断言 SOP+fact 候选都落项目 inbox 且 detail 含 scope=global 落库目标、纯查询轮零投递;定向 kanzei-tools 231/kanzei/app 118 全绿(T-1786449918/T-1786449991),workspace 全量全绿(T-1786450039),提交 7b6f7f0。
注:历史 ~/.kanzei/memory/inbox.md 中已滞留的 7 条候选仍留在全局 inbox(UI 候选列表可见,可手动采纳),修复保证的是新候选不再进死信箱。
- 验收: ① SOP 候选改投项目 inbox ✓(harvest_end_of_run 用 project store,mod.rs:825;两端调用点同步)
② manager 消化时按 scope=global 落库 ✓(manager.rs prompt 例外规则 + 候选 detail 指明 scope=global)
③ 候选箱语义保留 ✓(候选仍经 manager 消化、用户拍板,不自决入库)
④ 链路不再只进不出 ✓(项目 inbox 被两处 consolidate 与 memory_inbox_clear 正常消化)

## D-217 stale 记忆无归档搬运通道:memory_system.md 承诺的 memory-archive/ 整理流程不存在 [fixed]
- 现象: 设计基线 §2 写「stale 后由整理流程移入 memory-archive/,带墓碑」,但代码里 archive/ 目录只被 load_archived_ids 用来保 ID 不复用,没有任何工具或触发把 stale 条目搬进去;INDEX.md 的「N stale 条待归档」永远挂着。sleep-time 空闲整理同样未实现,消化只有轮末触发与 UI 手动按钮。
- 影响: 遗忘只有「人工+墓碑」半套;stale 条目永远占主目录与 load_all 扫描;文档与实现不一致。
- 修复方向: 归档搬运做成引擎动作(同 tracker archive 哲学:搬运后回读校验),触发挂 R-150 的整理清单(零采纳候选/复发告警/stale 积压);实现时同步修订 memory_system.md 或按实现改文档。
- refs: R-150 R-107 (medium)

- 关闭证据: ①墓碑落档:memory_stale 现先读原 body 追加 `(stale: reason)` 再一次性 update(body+status),archive_dead rename 时文件已带墓碑——manager.rs:381-397;单测 stale_墓碑_reason随条目进归档(manager.rs:574-644)断言主目录消失+归档保留 ID+正文含 reason 与原正文。②stale 积压进整理清单:memory_value_flags 返回 staleArchived(store.archived_count,store.rs:173-181),前端 13-memory.js renderMemoryValueFlags 显示「已归档待复查」+ i18n 登记(02-i18n.js)。③文档同步:memory_system.md §2 目录名 archive/ 与归档机制现状、§3 memory_stale 墓碑说明、§4 手动整理替代 sleep-time(R-132)、§7 R-107 验收修正。验证:kanzei-tools 232 + app 118 + 前端四冒烟 + cargo test --workspace 全绿(T-1786450308/T-1786450390)。
- 复杂度: 中

## D-184 commands / skills 两张注册表是死的:解析注册后无人消费 [fixed] (medium)
- 复现: 在 `~/.kanzei/commands/` 或 `~/.kanzei/skills/`(及项目同名目录)放 markdown,MarkdownComponent 会扫描、解析并注册(crates/kanzei-harness/src/markdown.rs:22);但全仓库对 `snapshot.commands()` / `snapshot.skills()`(crates/kanzei-harness/src/harness.rs:110、114)**零调用**——文件进了注册表就地消失,既不进提示词也不成为工具。
- 影响: 六张注册表实际在跑的只有四张。用户按目录约定放了命令/技能文件,界面与模型都不会有任何反应,也没有一行提示说"注册了但没人用",属于静默无效功能。
- 根因: 注册表与消费端分两步落地,消费端(注入提示词或转成工具 spec)始终没接。
- 验收: 要么接上消费端(commands 进提示词可调用清单、skills 按 description 与任务匹配给出加载提示,与 R-106 的 sop 匹配同源),要么显式移除这两张注册表与扫描逻辑;二选一,不留"解析了但没人读"的中间态。有测试覆盖所选方向。
- 证据等级: E2(读代码确认零调用点)
- 优先级: P2
- 标签: 核心

- 进展: 2026-08-10 取活:标记 fixing,读 markdown.rs/harness.rs 评估接消费端 vs 移除的改动面后定方向。

- 关闭证据: 验收「接上消费端」:①commands 进提示词可调用清单——markdown.rs contribute 末尾渲染「可用命令(commands)」块(名+描述+限定 agent,模板正文按名引用),进 system baseline(crates/kanzei-harness/src/markdown.rs:25-53);②skills 按 description 给加载提示——渲染「可用技能(skills)」块(名+描述+SKILL.md 路径,正文按需 read,markdown.rs:54-66);③测试覆盖所选方向:commands_and_skills_render_into_system_baseline(解析后进 baseline)与 empty_commands_skills_render_nothing(空注册表不产生空块)两单测(markdown.rs:295-374)。消费链:扫描(markdown.rs scan_commands/scan_skills)→ 渲染进 context → snapshot.system_baseline() 注入提示词。验证:kanzei-harness 109 全绿、clippy 干净、下游 check 干净、cargo test --workspace 全绿(T-1786450575/T-1786450646)。
- 复杂度: 中

## D-159 memory-manager 忽略前置 pathspec fatal 并把 commit 症状误记为根因 [fixed] (medium)
- refs: R-105
- 优先级: P2
- 复现: 一次 `git add` 因文件名大小写/截断不匹配报 pathspec，随后 `git commit` 因无暂存内容退出 1。自动 memory-manager 生成 M-013，标题断言“Changes not staged 表示没有暂存内容”，正文进一步把根因泛化为忘记 git add；但本次真实根因是前置 git add 的 pathspec 不存在。
- 影响: 记忆把症状误当根因，未来遇到同类输出会错误建议再次 git add，而不检查前置 add 是否因 pathspec/权限失败；属于会诱导重复失败的错误长期事实。
- 标签: 核心
- 根因: 失败归纳只消费了批次末尾 `git commit` 输出，没有关联同一 bash 调用前面的 `fatal: pathspec ... did not match any files`，跨命令因果被截断。
- 证据等级: E1
- 验收: M-013 被更正或标 stale，不再声称本次根因是忘记暂存；失败提炼能优先保留同一 bash 调用中更早的 fatal/pathspec 根因，或在无法判定时只记录症状不下根因结论；有回归覆盖。

- 进展: 错误 M-013 仍处于未提交状态；已向 memory inbox 投递具名更正说明，后续修复需让 failure harvest 保留同批前置 `fatal: pathspec` 根因并补回归。本轮不把错误记忆混入 R-069 提交。

- 关闭证据: 验收①M-013 更正/标 stale:M-013 正文已是更正版(描述+正文写明「先检查同批前置 git add 是否已报 pathspec did not match;不能判定时只记症状,不要断言忘记 add」,关联 D-159),已入库 commit 1476098。验收②失败提炼优先保留同批前置 fatal/pathspec 根因:metrics.rs failure_kind 先扫全文本找 fatal:/pathspec/did not match 根因行优先于首行(crates/kanzei-core/src/runner/metrics.rs:336-354),无根因行退回首行(不回归)。验收③回归覆盖:新增单测 failure_kind_多行bash批次_优先取pathspec根因行(metrics.rs:637-653,断言 kind 含 pathspec did not match、不含 changes not staged、无根因退回首行)。验证:kanzei-core 131 + tools 232 全绿、下游 check 干净、cargo test --workspace 全绿(T-1786450783/T-1786450853)。
- 复杂度: 中

## D-205 快记通道无信息保真门槛:模糊输入被编造复现后落库,关键限定词丢失 [fixed] (medium)
- refs: D-204
- 复现: 实例即 D-204。用户输入"SOP易用程度有问题,似乎总结的不太好",快记(QuickCaptureComponent 迷你 run,crates/kanzei-app/src/main.rs)产出「复现: 查看 SOP 时」——这不是复现,是从"查看 SOP"四个字硬挤出来的伪复现;用户真实意图「**用户**查看/使用 SOP 时的易用性」(2026-08-09 对话澄清)这一关键限定完全丢失,条目读起来像在说 SOP 内容对模型的可消费性。
- 影响: 信息在源头瘦身,浪费全落下游:自举拿到「查看 SOP 时」这种复现无从下手,要么猜方向(猜错=整轮白干)要么空转;更糟的是伪复现看起来像真的,没人知道该回去问用户。快记越好用、用得越多,这个失真通道流量越大。
- 根因: 三层叠加。①prompt 只说 how to reproduce **if inferable**,没规定推断不出时怎么办,模型的默认行为就是编一个;②快记的 ask 回调把 Question 一律 Cancelled(无人应答的设计约束),模型想追问也没有通道;③落库成功判据"只看库落了新条目"(main.rs:3545 注释),条目落了就算赢,信息量无人把关。
- 修复(第一层已做): prompt 明确禁止编造——推断不出复现时如实写「待澄清: <列出需要用户回答的问题>」,并要求从原文抽取关键限定词(谁的/哪个端/什么场景)进标题或复现。机制层留给后续:落库后如何机械识别"待澄清"条目并在 UI 上提示用户补充,属产品设计,交自举承接。
- 验收: ①模糊输入(如 D-204 原文)快记产出的复现字段不再是伪复现,而是「待澄清」+具体问题清单;②含关键限定词的输入(如"用户易用性")限定词不丢;③带「待澄清」的条目在侧栏可辨识(徽标/前缀任一),用户能一眼看到哪些条目等他补话;④自举取活时跳过或优先澄清「待澄清」条目,不拿伪复现开工。
- 证据等级: E1(D-204 实例 + prompt/回调/判据三处代码实证)
- 优先级: P2
- 标签: 后端

- 进展: 验收逐条证据:①prompt 层(QUICK_REQ_DEFECT_SYSTEM:NEVER invent or pad one + 待澄清问题清单,subagents.rs:17)为既有交付,本轮补契约测试锁死防回退;②keep qualifier words + original text verbatim 契约断言(subagents.rs tests quick_capture_defect_prompt_forbids_fabricated_repro_and_keeps_qualifiers);③.clarify-badge 徽标为既有交付(renderDocList+冒烟桩 D-001 待澄清断言)。验证:T-1786451336(quick_capture 2 绿)+ 既有 ui-runtime 徽标断言。残余转移:①真实快记实证(跑一次真实快记验证产出形态)与④(自举取活跳过/优先澄清待澄清条目)记入 R-101 批次前评估,不在本条。

- 批次: 1/1

- 复杂度: 小

## D-219 WIP 准入把阻塞 doing 计入配额,鞭挞提示词与 §1.1 新口径不同步 [fixed] (medium)
- 复现: 2026-08-09 实测:R-101(用户挂起)+R-148(仅剩等用户复查)占满 2 个 doing 名额,循环以「WIP 约束不能并发开启」拒开 R-153——两个不可执行条目把新工作准入整体锁死。
- 根因: 旧 §1.1 规则「blocked doing 不占可执行槽,但仍计入 doing 总数」自相矛盾——计入总数即占用准入;DEFAULT_CONTINUE_PROMPT 规则 5「doing 最多 2 个;已满就继续推进这两项」把旧口径写死在注入文案里,且不区分可执行/阻塞。
- 已做(规则层,2026-08-09): conventions §1.1 改为「非阻塞 doing 最多 2;阻塞/挂起 doing 不计入准入配额;含阻塞总数 >4 必须先收敛存量」;R-101 转回 todo(用户挂起,不在推进中),R-148 补①类阻塞字段(等用户复查)——名额已释放,R-153 可开。
- 待修(机制层): DEFAULT_CONTINUE_PROMPT 规则 5 文案按新口径改写(区分可执行/阻塞 doing),旧默认加入 LEGACY_CONTINUE_PROMPTS 静默升级(D-163 同族,防用户存的旧默认与新契约错位);调度器/取活预览若有同口径判断(D-207 系)一并同步。
- 验收: ①注入文案与 §1.1 新口径一致,LEGACY 升级路径有测试;②构造「2 个阻塞 doing + 可做 todo」场景,循环能开新条目不再误拒;③冒烟断言防回归。
- 边界: 改动集中在 main.js 文案与 LEGACY 数组,与 R-154 拆解撞文件——微小改动,安排在 R-154 批次间隙或 08-compose 批落位后做,不与拆解批同轮;R-157 参数化规则 6 时顺路复核本条。
- refs: D-163 R-157 D-207
- 优先级: P1
- 标签: 前端

- 复杂度: 小
- 批次: 1/1
- 进展: 逐条证据:①注入文案与 §1.1 新口径一致——R-170 已把规则剥离出 continue prompt(DEFAULT_CONTINUE_PROMPT 极简意图句 08-compose.js:16,LEGACY 升级机制删除 08-compose.js:481),dev system prompt 为 WIP 单槽真源(profiles.rs:748 dev_system_prompt_enforces_wip_and_batch_contract:断言 ONE executable item/share the SAME single slot/does NOT consume the slot/exceeds 4 + 反断言无「keep at most 2 requirements」旧口径残留;conventions 同口径测试 profiles.rs:812);②「2 个阻塞 doing + 可做 todo」场景——本轮 ui-runtime-smoke 新增断言:两阻塞 doing 均不标 agent-active、blocked 标记保留、可开工 todo 仍为 agent-next(不被误拒);③冒烟断言防回归——上述断言 + D-207 既有 blocked doing 断言。验证:T-1786451xxx(ui-runtime 1147 invoke 全绿)+ dev_system_prompt_enforces_wip 单测绿。

## D-233 文件视图打开卡顿:同步 files_snapshot 在主线程全量读+哈希 258 个文件 [fixed] (medium)
- 优先级: P1
- 标签: 前端
- refs: R-148 D-202
- 复现: 2026-08-10 用户实测(build-9e09b80):桌面端切到「文件」视图明显卡顿。
- 根因(代码实证,四层叠加): ①`files_snapshot` 是**同步** Tauri command(crates/kanzei-app/src/files_view.rs:24,非 async),Tauri v2 同步 command 在主线程执行——整个扫描期间 UI 完全冻结;②每次调用都全量 `scan(&root)`(kanzei-tools/src/files.rs):对每个 ≤2MB 的代码/md 文件做 `std::fs::read` 全文读取 + 行数统计 + FNV-1a 全文哈希,当前仓库命中 **258 个文件共 4.4MB**(其中 Monaco vendor 85 个文件 1.1MB 也被逐个读+哈希——它们永远不会被标注,读了纯属浪费);③scan 还同步 spawn `git ls-files` 子进程(Windows 进程创建自带几十 ms);④前端每次切视图都重新 invoke(main.js:886 `if (view === "files") refreshFiles()`),filesSnapshotData 缓存形同虚设——切走再切回就重扫一遍。与 D-202 是同类病(主线程被长任务占死),但这次在 Rust 侧不在渲染侧。
- 影响: 每次打开/切回文件视图 = 主线程同步读 4.4MB + 258 次哈希 + 一次子进程,机械硬盘或杀软实时扫描环境下秒级冻结;仓库越大越糟,与「文件视图是分析重文件的工具」的定位自相矛盾(files_view.rs 头注自己写过"本功能恰好是分析重文件的工具,自己先别成为反例")。
- 修复方向(按序独立可验): ①`files_snapshot`/`file_preview` 改 async command(线程池执行,主线程立即解放)——单词改动收益最大;②快照会话内缓存:切回视图直接用 filesSnapshotData 渲染,后台静默刷新,显式「刷新」按钮才强制重扫;③增量重扫:按 size+mtime 粗判未变的文件复用上次的行数/哈希,只重读变了的(全文 FNV 只在标注流程里保持 D-213 的 mtime 免疫语义);④vendor/gen 等永不标注的路径跳过读内容(只 stat),树里仍显示但标记「未度量」。
- 验收: ①切到文件视图主线程无秒级冻结,切换期间其它控件可点(与 D-202 验收同口径 <200ms);②切走再切回不重扫(有缓存命中证据);③第二次打开的快照耗时比首次显著下降(增量路径生效,日志或遥测可见);④vendor 文件不再被读内容,measurable 集合缩到项目自有源码;⑤冒烟或单测覆盖 async 化与缓存路径。
- 证据等级: E1(用户复现 + 代码路径实证 + 读取量实测 4.4MB/258 文件)

- 批次: 2/2
- 进展: 关闭对照——验收①files_snapshot/file_preview async command 化(files_view.rs:26/78,主线程解放);②切回缓存优先渲染(17-files.js showFilesView)+ files_snapshot 下发 reused 字段(缓存命中证据,验收②③);③scan_incremental size+mtime 粗判复用未变文件(kanzei-tools/files.rs scan_incremental),单测断言 reused 计数/指纹一致;④is_vendor_rel 跳过 vendor/node_modules/dist/target/gen/third_party 读内容(只 stat,树里仍显示大小),单测断言 vendor lines/chars 为空;⑤单测覆盖 async(file_preview tokio)与增量/vendor 路径。验证:T-1786451554/B1、T-1786451775/B2、T-1786451817/fmt复测、T-1786451883/关闭前全量 cargo test --workspace 全绿;ui-runtime 1147 invoke + i18n 997 key 全绿。既有能力标注:FileEntry 结构与标注/聚合逻辑为既有,本次新增 mtime_ns 内部字段与增量扫描路径。

- 复杂度: 中

## D-243 记忆正文读取仍未回填遥测采纳 [fixed] (medium)
- 复现: memory_search 返回 file 后调用通用 read，当前 read.rs 只读取文件，不调用 MemoryStore::mark_recall_fetched；memory_search 自身却在搜索返回时提前标记 fetched。
- 来源: R-161 验收②与 docs/design/memory_control_plane.md §2
- 标签: 核心
- 进展: 关闭对照——实质修复已由 R-161 B2(b9baccc)交付,本条漏关,现核验补齐:验收①read.rs:71 仅在 read_sync 成功后调 mark_memory_file_read(非记忆文件快速短路,memory/mod.rs:908 starts_with_ci 限定记忆库根);store.rs:562 mark_recall_fetched 只回填最近一次召回;测试 read.rs:227/290、mod.rs:1428 全绿。②memory/mod.rs:867 record_memory_search_telemetry 打开 state.db 写 RecallEvent(kanzei-core store/telemetry.rs:35 recall_events 表),注释明示 CLI 预检索/memory_search 工具/桌面端搜索页三入口共用。③store.rs:128 index.db memory_recalls 仍是 fetched 事实库(read 回填 UPDATE 走这里),state.db 新增遥测表,双库并存旧读路径未破坏。验证:cargo test -p kanzei-tools memory 71 全绿(read_memory_file_backfills_recall_fetched + mark_memory_file_read_backfills_only_matching_scope_entry 均在)。既有能力标注:mark_recall_fetched/record_recall_event 为 R-161 交付,本条仅核验关闭。
- 验收: 仅在真实 read 读取 .kanzei/memory 文件后回填对应召回；memory_search 与桌面端/CLI 共用 state.db 漏斗事件；保留旧 index.db 读兼容。
- 优先级: P1

- 复杂度: 小

## D-244 对照页优先级/阻塞控件跨队列写并落盘:调一次覆盖另一队的持久化筛选 [fixed] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 前端
- refs: D-207 D-211
- 证据等级: E1(取证确认 HEAD 既有 + 探针实测)
- 复现: 对照(both)标签页上,`优先级` 与 `阻塞` 两个控件仍是启用的,它们走 14-docs-actions.js 的 applyDocFilter,而 applyDocFilter 对 docFilterTargets() 返回的每个队列都写。实测:对照页把优先级调成 P0 → `before={"req":"all","defect":"all"} after={"req":"P0","defect":"P0"} saved={"req":"P0","defect":"P0"}`;调阻塞同理。缺陷队列的筛选被覆盖并落盘。
- 取证(重要,别误判成新引入): `git show HEAD:crates/kanzei-app/ui/14-docs-actions.js` 该行 = `for (const kind of docFilterTargets()) documentFilters[kind][field] = value;`,且 HEAD 的 syncDocumentFilters 也从不给 priority/blocked 置灰——**HEAD 就有的形态**,不是 2026-08-10 侧栏重构引入的。
- 与已修 P0 的区别: 这是**用户主动调控件**、两张列表当场同时变,不是「切个标签页就被改掉」;相对 HEAD 只减不增。所以不拦发版,但按 2026-08-10 定调「对照页是只读的对照视图,不得改动任何队列的持久化筛选状态」它同样不合规。
- 修复方向(二选一,都属设计决策): ①对照页禁用这两个控件(与 status/complexity/sort/tag 一致,走中性副本);②给对照页独立的筛选状态,不与两队共享。
- 验收: 对照页调任何筛选控件后,两队的持久化筛选状态均不被改写(内存与 localStorage 都要验);有拦截实测的冒烟断言。

- 批次: 1/1
- 进展: 关闭对照——验收①内存:neutralizedDocFilters(12-docs-pages.js)both 分支加 overrides.priority/blocked=all,渲染/拖拽/锁提示三处共用中性副本;syncDocumentFilters 对照页禁用 priority/blocked 控件(priorityBlockedNeutral)且不再写底层(只显示 all),切回单队列页原值填回。验收②localStorage:applyDocFilter 的 saveDocFilters 在对照页不可达(控件 disabled,change 不触发)。验收③拦截实测冒烟断言:ui-runtime-smoke.mjs 重构三块旧断言(对照共用筛选/清除筛选/解锁)→ D-244 只读断言(控件 disabled、模拟 change 两队列列表不筛空、before/after localStorage 两队均不被改写、切回 req 原筛选还在),③冻结对象护栏保留。验证:node --check 全 ui/*.js 过,四条冒烟全绿(ui-runtime 1137 invoke 0 错误,T-1786452213)。既有能力标注:status/tag/complexity/sort 的中性化机制为既有(R-115/D-211),本条把漏网的 priority/blocked 并入同机制。

## D-245 R-170 把 kanzei.toml [cadence] 变成死配置:设置页照写,无任何消费方送进模型 [fixed] (high)
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- refs: R-157 R-170 D-242
- 证据等级: E1(全仓 grep 零命中 + config.rs merge 缺分支,两处独立实证)
- 复现: R-157 交付了 kanzei.toml `[cadence]` 五个字段 + 设置页透传 + 把生效节奏渲染进继续文案。R-170(eb7ae42)按剥离清单删掉了 cadenceVerificationText 与 applyCadenceSettings ——**渲染点没了,配置就再也到不了模型**。两处实证:①`grep -rn "\.cadence|Cadence" --include=*.rs crates/` 除 settings.rs 存取与 config.rs 定义本身外零命中,JS 侧除 16-settings.js 表单存取外只剩 02-i18n.js 一段已失真的说明文案;②crates/kanzei-harness/src/config.rs 的 merge() 只合并 models/providers/proxy/profile/permissions/limits,**没有 cadence 分支**,load() 从 KanzeiConfig::default() 起手,所以 `config.cadence` 恒为默认——文件里写了也到不了运行时。
- 影响: R-157 整条交付变成惰性资产:设置页改得动、存得住、读得回,唯独不生效。用户按界面调节奏后行为不变,属于「只展示不接真实数据源」的反面(§1.25 明令这类不算完成)。与 D-242 同源——都是 R-170 剥离时误判「真源已在别处」。
- 修复方向: ①config.rs merge() 补 cadence 分支,让文件值真能进 KanzeiConfig;②给节奏一条到模型的通路(注入 system prompt,或让引擎按配置直接决定跑不跑全量,后者更符合「能代码强制的绝不只写进提示词」);③conventions §1.4 的「交付后本节标注引擎已接管」在通路补回前**不得标注**——现在标了就是假话。
- 验收: ①改 kanzei.toml 的 [cadence] 后,实测行为随之变化(轨迹或日志为证);②config.rs merge 有 cadence 单测;③设置页改参数→保存→重开生效且真作用于验证节奏;④R-157 的验收⑤有明确归宿。

- 批次: 2/2
- 进展: 批1+批2 完成:①config.rs merge_file 加 overlay_cadence(raw toml cadence 表显式键集合驱动逐键覆盖,单测「cadence_层叠合并」验证项目层只写 full_test 时全局层 push 保持)——文件值真能进 KanzeiConfig;②通路:run.rs cadence_guidance 把与默认不同的档位注入 Dev system prompt(append_dev_guidance 加 config 参数),全默认空串不污染;③设置页既有 settings_apply_cadence 写盘 + settings_get 读 config.cadence,保存→重开→注入循环闭环。④conventions §1.4 标注:通路已补回(引擎按配置注入节奏),R-157 验收⑤的标注条件满足,可标注。剩余:关闭前全量 + 关闭。

## D-256 applyBatch 在 for-await 循环里逐次重取 currentProject,切项目会把旧项目条目 id 写进新项目 [fixed] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 前端
- 证据等级: E1(代码形态实证 + `git show HEAD:` 确认既有)
- refs: D-250 D-251 D-249
- 复现: crates/kanzei-app/ui/11-docs-list.js 的 `applyBatch`(39-70 行)对批量选中集逐条 `await invoke("docs_update", …)`,而 `projectDir` 取的是**循环体内当场读的全局 `currentProject`**(:52),不是进入批量前认领的局部量。批量操作进行中切项目,剩余条目就会拿着**旧项目的条目 id** 去写**新项目**:选中 R-001、D-001 这类在两个项目里都存在的 id 时,新项目里的同号条目会被真的改状态、改标签。
- 取证(别误判成新引入): `git show HEAD:crates/kanzei-app/ui/11-docs-list.js` 的 applyBatch 与工作区**逐字一致**(HEAD=36ce685,同样是 39-70 行、`projectDir: currentProject` 落在 :52)——**HEAD 就有的形态**,不是 2026-08-10 侧栏重构、也不是 D-250/D-251 收口引入的。
- 影响: 与 D-250/D-251 同族(await 前后项目身份不一致),但**危害高一档**:D-250 只丢跳转高亮、D-251 只错写 localStorage 工作树清单,本条是**真数据错写**——新项目的 tracker 条目被改状态/改标签并经 docs_update 落盘,用户事后看不出是谁改的。批量越大、切得越早,错写条目越多。
- 待定(产品决策,**不代用户拍板**): 中途换项目时,剩余批量操作应当**整批中止**,还是**继续按认领的旧项目做完**?两种语义都自洽——中止 = 最保守,不再动任何项目;按认领项目做完 = 用户本意就是对旧项目那批条目生效,只是人走开了。取活前必须先向用户确认(D-205 教训:不代用户猜死),确认后再按所选语义改写本条验收③。
- 修复方向: 无论选哪种语义,`projectDir` 都必须在进入循环**之前**认领成局部量(与 36ce685 对 refreshDocs / handleWorktreeAction 的改法同源),循环内每次 await 后比对;差异只在比对不一致时是 `break` 还是继续用认领的局部量。
- 验收: ①批量操作进行中切项目,不得有任何一条写进新项目(逐条核对 docs_update 的 projectDir 实参);②有拦截实测的冒烟断言(scripts/ui-runtime-smoke.mjs 构造「await 中途改 currentProject」的桩,断言后续 invoke 的 projectDir 一律不是新项目);③按认领项目做完语义:整批继续写旧项目,完成后若 currentProject 已变,提示「这批改动落在 <旧项目>」

- 批次: 1/1
- 进展: 2026-08-12 实现落地(0f39192)。验收逐条证据:①循环前认领 `const batchProjectDir = currentProject`(11-docs-list.js applyBatch 循环外),循环内 `projectDir: batchProjectDir` 不再重读全局——冒烟桩把第一条 docs_update 挂闸门、await 处把 currentProject 换成 C:/smoke/project-b,放行后断言该批全部 docs_update 的 projectDir === 认领旧项目;②ui-runtime-smoke.mjs 新增 D-256 断言块(闸门+中途切项目+projectDir 集合断言,运行 1144 invoke 0 错误);③按认领项目做完语义:循环不中止继续写旧项目,结束后 `if (batchProjectDir !== currentProject) toast(这批改动落在 …)`(11-docs-list.js:74-77),i18n 键「这批改动落在」登记(02-i18n.js),冒烟断言 toast 含认领项目。验证:node --check ×3 + ui-runtime/i18n/a11y/markdown/lint 五条冒烟全绿(T-1786462431)。

## D-235 conventions.md 无专用工具可写:模型只读,引擎化交付标注无法落地 [fixed] (medium)
- 复现: R-157 验收⑤要求 conventions.md §1.4 标注「引擎已接管」。edit 被 ruleset 拒绝:policy-managed(用户手写的项目资产,模型只读),且无专用工具;规则明令禁止 shell 旁路(重定向/Set-Content/WriteAllText/node 单行均被检测回滚)。同 D-173(architecture/README.md 无专用工具)一类的能力缺口:需求/缺陷/目标/决策各有 tracker 工具,规范文档 conventions.md 没有对应专用写入通道。
- 影响: R-157 验收⑤(文档标注)无法由 agent 完成,条目不能按 §1.25 关闭;同类缺口将来还会卡住所有需要改 conventions.md 的条目(如引擎化交付后的标注、新决策的 §1.x 更新)。
- 优先级: P2

- 进展: 2026-08-12 交付(e6c2360)。D-235 原验收字段缺失,按复现/影响推导验收并逐条给证据:①专用写入通道存在——ConventionsTool(crates/kanzei-tools/src/conventions.rs,get 读全文+hash+标题导航 / patch 逐字替换)注册进 DevProfile(profiles.rs tools.insert),get 放行、patch 逐次询问(与 architecture update 同保守口径),deny 表新增 `*.kanzei/project/conventions*` → Some("conventions")(profiles.rs),write/edit 硬 deny 的拒绝理由从此点名工具而不是「无专用工具」(profiles.rs 测试断言 hint 含 `conventions`);②patch 契约有 7 个单测:唯一命中写入/0 命中拒写/多命中拒写/陈旧 hash 拒写/缺 expected_hash 拒写/缺失文件报错/get 全文+hash+标题导航,kanzei-tools 240 passed;③原「无专用工具」断言改用 notes.md 继续守「不得编造工具名」底线(profiles.rs)。复用:content_hash/replace_recoverably 提为 pub(crate) 由 conventions 共用,不养第二份 CAS 原语。R-157 验收⑤(§1.4 标注)现可用 `conventions patch` 落地,需新引擎二进制;R-157 阻塞字段已同步更新。验证:fmt/clippy 全绿 + 下游 kanzei-app/kanzei cargo check 通过(T-1786463010)。

## D-258 后台任务缺内核级文件隔离:归因+回滚拦不住合法写入窗口的毫秒级蒙混 [fixed] (medium)
- 优先级: P2
- 复杂度: 大
- 标签: 核心
- 证据等级: E1(读码核实 crates/kanzei-tools/src/managed.rs 与 background.rs,2026-08-10 dev HEAD)
- 来源: 2026-08-10 D-174 交付时的残余转出。D-174 本轮做的是「**按进程归因 + 越界回滚**」:`ManagedSnapshot::capture` 在动作前后各拍一次托管目录镜像(managed.rs),改了就隔离留证 + 整体回滚;后台任务登记 `BackgroundOwner{run_id, process_id, 写仲裁键}`。这是**结果侧**判定,故意不靠命令文本匹配(`WriteAllText`、重定向、python/node 一行流、`git checkout` 单文件都能避开任何字符串匹配)。
- 未做的部分与理由: **内核级隔离**(受限令牌 / 低完整性进程 / AppContainer / 托管路径 ACL)被评估为**代价收益倒挂**——低完整性进程连 `target/`、`node_modules/` 都写不了,而那正是后台任务的唯一用途(跑 build、跑 dev server、跑测试)。为了堵一个窄缝把功能整个杀掉,不划算,所以本轮明确不做,转出为独立条目待重新评估。
- 残余缺口(本条要解决的): ①无内核级边界:后台进程在操作系统层面**仍有权限**写托管路径,拦截全靠前后两次镜像比对。②**合法写入窗口的毫秒级蒙混**——专用工具正在合法写托管文档的那个窗口里,后台进程若同时写同一批文件,前后镜像比对无法区分哪一笔是合法的、哪一笔是后台进程的,回滚要么误伤合法写入、要么放过越界写入。③镜像本身有上限:单文件 >4 MiB 只记指纹(能检测不能回滚)、目录 >2000 文件直接放弃镜像(既不检测也不回滚,只在输出里如实说明),这两条边界内后台进程可以自由写。
- 修复方向(择一或组合,取活前先评估代价): ①托管路径 ACL:给后台进程一个专用身份,对 `.kanzei/project`、`.kanzei/memory` 拒绝写——比整进程低完整性精确得多,不影响 `target/`;②合法写入窗口内改走独占文件锁(R-138 的 `FileLock`),让镜像比对不必在窗口内做判定;③镜像上限内的空白改为**显式拒绝后台任务**而不是静默放行。
- 验收: ①存在一条不依赖前后镜像比对的机械边界,后台进程写托管路径在**操作系统层面**失败(或有等价的、不靠事后比对的拦截),有实测证据;②后台任务仍能正常写 `target/`、`node_modules/` 等非托管路径(不得为了堵缝把功能杀掉),有回归;③专用工具的合法写入窗口内,后台进程的越界写入被识别且合法写入不被误伤,有并发用例覆盖;④镜像上限(4 MiB 单文件 / 2000 文件)被突破时的行为是**显式拒绝或显式告警**,不是静默放行,有测试。
- refs: D-174 R-097 R-139 R-180

- 状态: fixed
- 进展: 已修复(2026-08-11,提交 90fe6ab + 85ce42d)。逐条验收证据:
①OS 层条款(受限令牌/低完整性/ACL):成本收益倒挂维持原判,且与验收②直接互斥——低完整性进程连 target/、node_modules/ 都写不了,后台任务(跑 build/dev server/测试)整体被杀。等价拦截交付:①守卫在变化全被窗口分流为合法时**不再整树推进基线**(旧实现把后台进程窗口内偷写的文件固化进基线,窗口一关守卫永远看不见,这是蒙混承重点);②吸收从「整前缀」收窄为「窗口打开/关闭双快照之间实际变化的路径 ∩ 前缀」(managed_fence set_observer 双阶段 + absorb_paths)。后台进程要蒙混必须落在专用工具窗口的精确时间窗内、写同一批前缀路径;写窗口外路径(即使发生在窗口内)窗口关闭后仍被守卫下一轮识别回滚。实测:测试「窗口开着时守卫不推进基线_关闭后精确吸收」。OS 层边界转出为新缺陷 D-275。
②后台写非托管路径畅通:新增回归测试「后台写非托管路径畅通不误伤」(scratch.txt 写成功、无越界记录、文件保留、托管树逐字节不变);既有场景②轮询/后台托管/停止均绿。
③窗口内合法写入不误伤 + 越界被识别:场景⑤(窗口内写入保留、窗口外同写入回滚到基线)保持绿;新增「窗口内后台写窗口外托管文件_关闭后仍被回滚且合法写入保留」(memory 文件被回滚、defects.md 合法写入保留、归因记录点名越界路径)。
④镜像上限被突破显式拒绝:新增「托管树超限时后台任务被显式拒绝而不是静默放行」(>4 MiB 单文件,报 refused before execution,注册表无残留);前台 bash 的 is_complete 拒绝早已存在(bash.rs:155-162),后台同路径覆盖。
验证:cargo test -p kanzei-tools 244 passed / kanzei-harness 110 passed / fmt/clippy 全绿 / 下游 kanzei-core + kanzei check 过。

## D-277 NSIS 安装包 exe 图标是 Tauri 默认图标:需显式配置 installerIcon(任务栏正常仅安装器不对) [fixed] (medium)
- priority: P3
- severity: low
- 修复: tauri.conf.json 的 nsis 节补 "installerIcon": "icons/icon.ico"(提交 097e030)。插曲:首版还加了 installerHeaderIcon,被 tauri-build 拒绝(该字段不存在,头部只有 headerImage 横幅图片),已剔除,最终只保留 installerIcon。
- 内容: NSIS 安装包 kanzei-setup-*.exe 的图标是 Tauri 默认图标;运行时窗口/任务栏图标正常(走 bundle.icon),仅安装器本体 exe 图标不对。
- 来源: 用户反馈:2026-08-11 用户观察到安装包 exe 图标是默认图标,任务栏(运行时窗口)图标正常。
- 标签: 发布
- 根因: Tauri 2 的 NSIS 安装器不继承 bundle.icon,必须显式配置 bundle.windows.nsis.installerIcon;原 tauri.conf.json 的 nsis 节只写了 installMode 和 languages,缺该项。
- 进展: 修复已提交 097e030。残余验证:图标真正生效需重新打包安装器,下次 package.ps1 -Publish 重建后目视确认 exe 图标即可;既有已发布安装包(如 build-c7bbe0a)不会变化。
- 验证: 像素对比实证:最新安装包 kanzei-setup-c7bbe0a.exe 内嵌图标与 icons/icon.ico 在 32×32 下 1024/1024 全异,确认是默认图标而非缓存。cargo check -p kanzei-app 通过(tauri-build 成功解析新配置),cargo test -p kanzei-app 122 passed(T-1786475545)。

## D-259 tests-archive 历史重复编号未清理:T-1786297655 四条同号、T-1786341674 两条同号 [fixed] (low)
- 优先级: P3
- 复杂度: 小
- 标签: 流程
- 证据等级: E1(实测统计 + 读码核实分配器与拒写逻辑,2026-08-10 dev HEAD)
- 复现: `grep -o "T-[0-9]*" .kanzei/project/tests-archive.md | sort | uniq -c | sort -rn` → `4 T-1786297655`、`2 T-1786341674`,其余编号各 1 条。同号记录标题不同,按 id 无法区分是哪一次测试。
- 来源与边界(别重复修已修好的部分): **D-227 已修好分配器**——`crates/kanzei-tools/src/test_record.rs` 现在扫描已有集合单调推进(不再同秒撞号)、同号拒写(`ensure_id_unused`)、归档侧内容不同时拒绝追加第二条同号记录(test_record.rs:275-283)。**新的重复不会再产生**;本条只管**历史存量**。
- 为什么不自动清理: 参照 `crates/kanzei-tools/src/docstore.rs:392` `repair_reused_archived_id` 的保守立场——静默改号会把编号复用伪装成一次正常写入,证据链就此不可信(D-004:拒绝的理由必须说出来,绝不静默)。所以 D-227 的修复**刻意不回改历史**,需要一个**显式的一次性修复入口**。
- 影响: 窄。测试证据按 id 反查时,这 6 条记录里有 4+2 条互相指不清;条目关闭时引用「T-1786297655」无法确定指的是哪一次。另注:归档解析用 `BTreeMap` 按 id 收敛,重复条目在解析层被折叠成一条,所以既有代码路径不会因此报错,问题只在人工反查。
- 修复方向: 给 `test_record` 加一个显式的一次性修复动作(参照 tracker 的 `repair_reused_id`:必须显式指定 id、必须说明改成什么、结果打印出来),把历史同号记录逐条改成未占用编号并保留原标题/内容;不得静默批量改。
- 验收: ①`tests-archive.md` 里每个 `T-` 编号唯一(同一条命令可机械核验:`grep -o "T-[0-9]*" ... | sort | uniq -d` 输出为空);②改号动作是显式入口、有输出说明哪条改成了什么,不是自动触发;③改号后原记录的标题、状态、命令、summary、关联字段一字不丢,有测试;④D-227 已修好的分配器与拒写逻辑不被本条改动破坏(既有测试保持绿)。
- refs: D-227 D-004

- 状态: fixing

- 进展: 2026-08-13 代码交付(见上)。2026-08-15 用户重启引擎后执行真实清理:repair_reused_archived_id T-1786297655(四条→保留 i18n 第一条,其余改号 T-1786478785/786/787)、T-1786341674(两条→保留 tools 第一条,改号 T-1786478788);机械核验 `^## T-(\d+)` 364 条记录编号全部唯一(UNIQ-OK);修复动作有逐条输出、标题/状态/命令/摘要/关联字段一字未动;T-1786478774 记录终态 passed。验收①②③④全部满足,关闭。

## D-285 自举轮提交前不跑 fmt/clippy/ui_lint,存量违规攒到发版才集中爆发 [wontfix] (low)
- 备注: 不建议直接在发版脚本里自动修复:那会让违规在无人察觉中被改写,与 D-183 强制停顿的设计意图相反。
- 复现: 2026-08-12 发版前跑 scripts/verify.ps1,连撞三处存量违规,全部来自此前已提交的自举轮代码,均非本次改动引入:①cargo fmt 未归一(R-191 B5a dc087ae 的折行);②clippy -D warnings 红(conventions.rs 三处 unused_mut、tracker.rs 一处 question_mark,来自 587bca1/dc087ae);③ui_lint no-undef 红(D-278 b76a5f0 新增顶层函数 fastStatusText 后未重跑 scripts/gen-ui-lint-globals.mjs)。修掉这三处用了三个杂活提交(a9f78f2/d81ffd7/3f268a5)才进得了发版。
- 影响: 每次发版都要先做几个与本次交付无关的杂活提交,发版动作被拖长;发版者要现场判断别人留下的违规该不该改;违规越攒越多时一次发版可能被拖成半小时的清理。
- 期望: 自举轮提交前至少跑 fmt + clippy(UI 有改动时加 ui_lint),或把这三项加进轮末验证协议;若嫌慢,退一步做法是在提交钩子里只跑 cargo fmt 与 gen-ui-lint-globals,把最机械的两类挡住。
- 标签: 流程
- 根因: 自举轮的轮末验证协议只跑 cargo test,不跑 fmt、clippy -D warnings、ui_lint;而 verify.ps1(发版门禁)是全仓唯一跑全套的地方。于是违规在分支上一路累积,直到有人发版才暴露,且暴露时已经分不清是谁留下的。
- 优先级: P2

- 进展: 2026-08-12 05:05 判定为 D-264 的重复登记:D-264(2026-08-11 登记,至今 open)已覆盖同一根因(自举轮定向验证口径只提测试、不提 fmt/clippy,与 CI/verify.ps1 的门禁清单不同步)与同一修复方向(推荐代码强制而非写进规则)。本条独有的增量——ui_lint/gen-ui-lint-globals 这个第三维度——已并入 D-264 进展。不另开条目。

## D-284 kz CLI 的 tracker update 只收 id 与 status,写不了字段与进展,CLI 走不完一条条目的全程 [fixed] (low)
- 复现: kz defect update <id> <status> 只解析位置参数 id 与 status(crates/kanzei/src/main.rs 的 update 分支),没有写 进展 或任意字段的入口。于是用 CLI 处理一条缺陷时,close 前无法把验收证据写进进展字段——而 §1.25 与 M-020 要求证据必须在 close 前写入,close 后条目归档就改不动了。
- 影响: CLI 只能做半程:登记完必须回桌面端才能收尾;脚本化/自动化处理条目无路可走;也让「用 CLI 补登记」这条路走不到关闭。
- 期望: update 分支复用 add 的开关解析,支持 --priority/--severity/--field 键=值 写任意字段(含 进展)。顺带补 update 的用法说明进 kz 的 usage 文本。
- 标签: 核心
- 根因: CLI 的 tracker 入口是位置参数薄封装。add 分支这次刚补上字段开关(--severity/--priority/--complexity/--tag/--field 键=值,提交 f104890 与后续),update 分支没有同步。
- 优先级: P3
- 进展: 2026-08-12 05:10 已交付(提交 cb09746):登记开关解析抽成 parse_tracker_flags,add 与 update 共用——--severity/-s、--priority/-p、--complexity、--tag、--field 键=值(可重复,能写 进展 等任意字段)。位置参数语义不变:add 拼标题,update 取第一个作 id、第二个作 status。验收证据:①新增单测 登记开关解析_字段与位置参数各归各位,覆盖字段与位置参数分离、值内等号只按第一个切、无字段开关时不产出空 fields 键;②实测走通全程——本次 D-281/D-282/D-284 与 R-194/R-195/R-196 六条登记全部经该开关写入完整字段(复现/根因/影响/期望/来源/现状/内容/边界/验收),且用 update --field 阻塞= 清掉了 R-191 的过期阻塞字段、写入进展;③全量 694 测试通过,cargo clippy --workspace --all-targets -- -D warnings 零告警。残余:本次修改在 build-3f268a5 之后,已安装的 kz 要等下次发版才带上该开关(仓内 target/release/kz.exe 已可用)。

## D-286 deepseek provider 默认 context_limit 128k 与实际 1M 不符:UI 占用比例失真、压缩预检过早触发 [fixed] (medium)
- 修复: config.rs 三处 deepseek 默认值 128_000 → 1_000_000:fill_defaults 内置 provider 默认、known_context_limit 回填表、context_limit_tests 断言。kimi/moonshot 保持 128k 不动。
- 复现: kanzei.toml 的 [providers] 为空时走 fill_defaults 内置默认,deepseek provider 的 context_limit 硬编码 128_000(known_context_limit 与 fill_defaults 两处),而用户实际使用的 deepseek-v4-flash 模型上下文窗口为 1M。影响:①UI 占用比例按 128k 算,塞到 128k 就显示 100% 实际才用 12.8%;②drive.rs 压缩预检(context_budget_ratio 70%)与 run.rs 自动压缩(70%)都以 128k 为基准,约 90k 就开始压缩,严重浪费 1M 窗口。
- 标签: 后端
- 进展: 修复完成:config.rs 三处 128_000 → 1_000_000(known_context_limit 表、fill_defaults 内置 deepseek、测试断言),测试断言同步更新。验证:cargo test -p kanzei-harness context_limit 2 passed。残余:UI 打包进 exe,需用户重建 kzapp 后确认设置页 deepseek 占用比例正常;若用户实际跑 deepseek-chat(128k)需显式配置覆盖。
- 验收: ①KanzeiConfig::default().fill_defaults() 后 deepseek.context_limit == Some(1_000_000),有测试;②known_context_limit("deepseek", ...) 返回 1_000_000,有测试;③provider 级单值机制不变(deepseek 其他模型如 deepseek-chat 若上下文更小,由用户显式配置覆盖)。
- 优先级: P2

## D-260 test_runs_snapshot 只读命令却写盘且不持任何锁:绕过不变量 8 的最后一个写点 [fixed] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 后端
- 证据等级: E1(读码核实两处调用链,2026-08-10 dev HEAD;行号以实读为准,R-138 的代理正在改 docs.rs)
- 复现: `crates/kanzei-app/src/docs.rs` 的 `test_runs_snapshot` 是**同步只读命令**,直接转调 `kanzei_tools::test_record::test_runs_snapshot(&root)`,**不取任何锁**。而被调方在 `crates/kanzei-tools/src/test_record.rs` 里会真的写盘:发现 active 里有终态记录时,`std::fs::write(&archive_path, ...)` + `std::fs::write(&active_path, ...)` **改两个文件**。
- 对照(同文件的两个兄弟命令都做了): 同一 docs.rs 里的 `test_run_record` 与 `test_runs_init_refs` 都先 `acquire_writer_lease` 再写(R-171 批4 模式,注释明写「不能绕过协调器」)。只有 `test_runs_snapshot` 这条读路径顺手写盘却什么都不持。
- 影响: 这是设计不变量 8(「`test_record` 等写入口不得绕过协调器」,见 docs/design/parallel_read_serial_write_orchestration.md)的残留缺口。用户点开测试面板的那一刻,可以与 agent 那边的 `test_record` 写入撞在一起,两个写入者同时整文件重写 `tests.md` / `tests-archive.md`——与 D-249 描述的 `docs_snapshot` 竞态**同构**。
- 修复口径(照抄 R-138 对 `docs_snapshot` 的处置,**不要挂写租约**): R-138 本轮对同文件 `docs_snapshot` 的处置是**毫秒级文件锁 + 限时 `try_lock`**(`crates/kanzei-tools/src/atomic_file.rs` 的 `FileLock`,拿不到就跳过归档、落 `warnings`),而不是挂写租约——`MemoryCoordinator::acquire_writer_lease` **无超时**,挂上去会让面板在 agent 跑一轮期间整段卡死,等于拿一个更严重的问题换一个更轻的。判据已写进不变量 8 的 2026-08-10 补注:**代理发起的写动作走租约;界面读路径顺手做的幂等维护走文件锁**。本条属后者。
- 验收: ①`test_runs_snapshot` 的归档写盘被限时文件锁保护,拿不到锁时**跳过归档但正常返回读结果**(不阻塞面板、不报错弹窗),有测试;②并发「面板刷新 + agent `test_record`」的用例下,`tests.md` / `tests-archive.md` 不丢条目、不出现截断态,有回归;③归档写盘走原子写(与 D-261 并轨,不各写各的);④`test_runs_snapshot` 不引入写租约(有断言或注释锁定这条口径,防下一个人"顺手改成和兄弟命令一致"把面板卡死)。
- refs: R-138 D-227 D-249 D-261 docs/design/parallel_read_serial_write_orchestration.md

- 进展: 2026-08-13 核验(保持 open,不关闭):实质修复已由 D-261(dadf1ce,经 88b9cda 并入 dev)在 test_record.rs 中顺带交付,本条做核验记录(既有能力标注,非本次新写代码)。逐条证据:①归档写盘被限时文件锁保护、拿不到锁跳过且正常返回读结果——archive_terminal_records(test_record.rs:296-300)try_lock_exclusive(active_path, 200ms),拿不到锁 return Ok(());测试「快照归档拿不到锁时跳过而不是报错」(test_record.rs:1501-1534)。②并发面板刷新+agent test_record 不丢条目/不截断——三条既有用例组合覆盖:「外部持锁期间登记必须等待而不是抢先写入」(:1460-1494,agent 方向)、「快照归档拿不到锁时跳过而不是报错」(:1501-1534,面板方向)、「并发登记不撞号也不丢记录」(:1410-1452,8 线程无外部串行)——锁是同一把 atomic_file 独占句柄,任意两写者并发只落「等待/跳过/串行」三态之一。③归档写盘走原子写:write_atomic(archive_path, :340)与 write_atomic(active_path, :346),与 D-261 并轨无第二套。④不引入写租约:docs.rs test_runs_snapshot(:61-64)薄封装只转调,无 acquire_writer_lease;test_record.rs:285-288 注释明确锁定口径,防下一个人顺手改挂租约。2026-08-16 收口关闭:四条验收逐条复核证据仍在(D-261 主修复未回退,git log 88b9cda 仍在 dev 历史),无阻塞字段,按 defect-first 顶序收口。

**保持 open 的原因**:复杂度 medium,按 conventions §1.4 关闭前需跑 cargo test --workspace 全量;2026-08-13 无人值守会话的权限白名单无 cargo,本轮无法执行全量。全量由 D-261 关闭时(其验收③并轨第二套原子写后)或下次发版门禁(verify.ps1)兜底,与本条共享同一批代码改动。

## D-261 test_record 五处 std::fs::write 未并轨 atomic_file:跨进程 CAS 缺失,仓里两套写原语 [fixed] (medium)
- 优先级: P3
- 复杂度: 小
- 标签: 核心
- 证据等级: E1(全文件 grep 实证 + 读码核实 R-138 新原语,2026-08-10 dev HEAD)
- 来源: 2026-08-10 D-227 交付时的残余转出。D-227 本轮只做了 ①编号分配器(扫描已有集合单调推进,串行也保证唯一)与 ②拒写/定点替换(`ensure_id_unused` + 归档侧同号内容不同即拒);**跨进程 CAS 未做**。按裁决要**并轨到 R-138 新建的 `crates/kanzei-tools/src/atomic_file.rs`**,仓里只留一套原子写原语。
- 复现(实证): `crates/kanzei-tools/src/test_record.rs` 的生产路径仍是**裸 `std::fs::write`** 五处(测试代码另计),全文件对 `atomic_file` 零引用。而 `crates/kanzei-tools/src/docstore.rs` 的四个整文件写点已经全部改成 `crate::atomic_file::write_atomic`。**同一个仓库里因此并存两套写语义**,这正是 atomic_file.rs 头注明令禁止的:「仓里只能有**一套**原子写/文件锁实现……两套原语意味着两套失败语义,并发排查时没人说得清哪一份才是真的」。
- 影响: ①`std::fs::write` 是**先截断再写**,写到一半时另一个进程(kz CLI / 自举循环 / 第二个 kzapp)读到零长度或半截 `tests.md`——与 D-249 第①层同病;②「读 → 算下一个 id → 写」这段没有跨进程 CAS,分配器的单调推进只在**单进程内**成立,两个 OS 进程同时记录仍可能撞号(D-227 修的是同秒时间戳,不是跨进程竞态);③失败时没有 `atomic_file` 的"保留临时文件供排查"语义。
- 修复方向: 五处生产写点全部改走 `atomic_file::write_atomic`;「读 → 分配 id → 写」整段用 `atomic_file` 的 `FileLock`(`lock_exclusive` / `try_lock_exclusive`)罩住,与 docstore 的 `TrackerTool` 写动作分支同源。注意 `FileLock` 是 `!Send`,不得跨 await 点持有。**不要另造锁**。
- 验收: ①`crates/kanzei-tools/src/test_record.rs` 的生产路径不再出现裸 `std::fs::write`(可机械核验:该文件非 `#[cfg(test)]` 区域 grep `fs::write` 零命中);②「读→分配→写」整段持锁,两个进程并发 `test_record` 不撞号、不丢记录,有跨进程或多线程压测覆盖;③全仓只有 `atomic_file` 一套原子写/文件锁原语(grep 无第二处 tmp+rename 或独占句柄实现);④D-227 已交付的分配器与拒写逻辑既有测试保持绿。
- refs: D-227 R-138 D-249 D-260
- 进展: 主体已交付(dadf1ce)。2026-08-16 收口验收③(全仓单源化,commit e7f9716):atomic_file 从 kanzei-tools 下沉到 kanzei-llm(依赖图最底层,llm/tools/app 共用),kanzei-tools 用 pub use 重导出,现有 crate::atomic_file 引用零改动;新增 write_atomic_cas(写临时文件后、rename 前校验目标指纹,承接 architecture.rs replace_recoverably 的 CAS 语义,并删掉 Windows 上错误的 backup 三步走)。并轨四处第二套 tmp+rename:①kanzei-llm/src/auth/store.rs(凭证写回,本地 atomic_write 删除;失败保留现场语义对凭证更安全——新 token 是内存唯一一份);②kanzei-tools/src/memory/store.rs(本地 atomic_write 删除,7 个调用点改 write_atomic);③kanzei-tools/src/files.rs(save_annotations 改 write_atomic);④architecture.rs/conventions.rs(replace_recoverably 删除,两处调用点改 write_atomic_cas)。删除旧 crates/kanzei-tools/src/atomic_file.rs。验收③核验:全仓 grep 仅 kanzei-llm/src/atomic_file.rs 有 tmp+rename 实现(其余命中是 docstore/app 测试夹具故意 share_mode(0) 验证锁行为、auth/store 测试断言无残留 tmp,均非生产写原语)。验证:cargo test -p kanzei-llm -p kanzei-tools 249 passed(含新增 CAS 匹配/放弃用例)、cargo check --workspace 通过、cargo test --workspace 全量绿(详见 D-261 收口测试 T-1786500363)。验收①②④既有证据未回退(dadf1ce 在 dev 历史,e7f9716 之上机械守护测试仍在),四条验收全部达成,关闭。

2026-08-13 验收③复核(grep 全仓 tmp+rename / 独占句柄):发现**至少两处第二套原子写实现**仍在仓内——①crates/kanzei-llm/src/auth/store.rs:50-58 自带 tmp+rename(`path.with_extension(format!("kz{}.tmp", ...))` + std::fs::rename,注释还写着「写临时文件再 rename 覆盖」);②crates/kanzei-tools/src/files.rs:64-66 自带 tmp+rename(`path.with_extension("json.tmp")` + rename)。kanzei-tools 内 docstore.rs 已全部引用 crate::atomic_file(308/316/345/348/461/523/608 均为 atomic_file 或 lock 封装)合规。验收③(全仓只有 atomic_file 一套原语)未达成,缺口精确位置如上,待并轨。
  **未达成的验收③(全仓只留一套原子写原语)**:仓里仍有四处独立 tmp+rename,均不在本次改动面内——`crates/kanzei-llm/src/auth/store.rs:50`、`crates/kanzei-tools/src/architecture.rs:202`、`crates/kanzei-tools/src/files.rs:64`、`crates/kanzei-tools/src/memory/store.rs:1356`。本条据此保持 `open`,收口这四处即可关闭。
  **另记一条本次实测的设计发现(与 R-182 同源)**:`lock_path_for` 把锁文件放在目标同目录,即 `<worktree>/.kanzei/project/tests.lock`。并行工作树各有自己的 `.kanzei/`,所以**各写各的 `tests.md` 时根本不会互斥**;互斥只在同一份 checkout 被多个进程打开时才成立。这与实测「两个 worktree 相隔 10 秒各 `kz defect add` 都拿到 D-267」是同一件事的两面——**锁生效的前提是文档只有一份**,落点见 R-182 内容①②。

## D-263 自举提交时暂存了非本轮改动:应只 git add 明确文件,否则并发写入被静默卷进他人提交 [fixed] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 流程
- 证据等级: E1(2026-08-11 实例,提交为证)
- refs: R-181 D-264
- 复现: 2026-08-11 凌晨,自举循环取活 R-174 期间,外部 agent 正在同一批文件上工作(尚未完成)。自举的 `92879e2`(R-174 B2)与 `25ea2c0`(R-174 B3)**把外部 agent 未完成的改动一并暂存并提交**——提交标题里的「含 R-173 遗留收尾」正是被裹进去的那部分。自举本身并不知道自己提交了什么额外内容。
- 根因: 自举轮末提交时按「工作区里所有改动都是我的」暂存(`git add -A` / `git commit -a` 一类),而不是只 add 本轮实际动过的文件清单。这个假设在单写者下成立,在有外部 agent 或人手动改动时不成立。
- 影响: ①**改动归属混乱**——两个来源的改动挤进同一个提交,事后拆分只能靠人读 diff;②**回滚锚点失效**——revert 该提交会连带撤销别人的工作;③被裹进来的改动**没有经过自举自己的门禁**(本次就带进了 8 处 fmt + 6 条 clippy 红灯,见 D-264);④外部 agent 那边看到的是「我没提交,但我的改动不见了/已提交」,极易误判。
- 修复方向: 轮末提交改为**只暂存本轮明确改动过的文件**——引擎本来就知道自己调用过哪些写工具(edit/write/tracker/test_record 的目标路径都有记录),按那份清单 `git add <file>...`。若发现工作区里有清单之外的改动,**不要静默跳过也不要一并提交**,而是在提交说明或轨迹里明说「工作区另有 N 处非本轮改动,未纳入本次提交」(D-004 口径:任何不做的理由都要说出来)。
- 边界: 这条与 R-181(跨 agent 写入互斥)互补不互替——R-181 让双方知道对方在写,本条保证**即使撞上了,损伤也只停在各自的文件里**。本条更便宜、更该先做。
- 验收: ①构造「工作区有本轮之外的改动」场景,自举轮末提交**只包含本轮文件**,清单外的改动仍留在工作区;②提交说明或轨迹里对被跳过的改动有可见记录;③有回归测试覆盖「清单外改动不入暂存区」。

- 进展: 2026-08-16 交付(commit 8c17e2b)。调查结论:提交由模型经结构化 git 工具发起,引擎无自动提交;bash 的 git mutation 已全拦截(bash.rs:598-601 测试),stage 已拒索引中外来路径(git.rs:274-283),但缺「工作区非本轮改动」的可见对照。交付:stage 成功后新增 unstaged_changes 对照(git status --porcelain -z 解析),把未纳入本次请求的未暂存改动点名写进返回(Note: the working tree also contains N change(s) NOT staged by this request...),不静默吞掉也不静默跳过。验收逐条:①构造「工作区有本轮之外改动」场景,自举 stage 只含本轮文件、清单外改动留在工作区——回归测试 stage_leaves_foreign_changes_unstaged_and_names_them(git.rs:1112-1158)断言暂存区恰为 mine.txt、theirs-new.txt 与 base.txt 原样未动;②对被跳过的改动有可见记录——stage 返回里点名列出(测试断言 content 含 NOT staged by this request 与两个外来文件路径);③回归测试覆盖「清单外改动不入暂存区」——测试同时断言 staged_paths == [mine.txt] 且外来文件未被触碰。验证:cargo test -p kanzei-tools 250 passed。既有防线(显式 files 才 stage、拒索引外来路径、bash 拦截 git add -A)互补,本条把最后的可见性缺口补上。残余说明:引擎仍无「本轮写过的文件」自动跟踪(需 harness 层工具轨迹扩展,R-181 互补项),stage 对照点名是当前成本最低的机械防线。

## D-264 定向测试口径漏掉新增集成测试所在 crate:cargo test 全绿但 fmt/clippy 从未跑到 [fixed] (medium)
- 优先级: P2
- 复杂度: 小
- 标签: 流程
- 证据等级: E1(2026-08-11 实例,已修但机制未修)
- refs: D-263 R-152
- 复现: 2026-08-11 自举交付 R-174 批1-3,进展里写「定向:core 119/harness 82/tools 213/app 67 全绿」与「cargo test --workspace 全量全绿」——都属实。但它本轮**新增的两个集成测试**落在 `crates/kanzei/tests/`(`task_cancel_parallel.rs`、`max_tasks_parallel_dispatch.rs`),而 `cargo test --workspace` 会跑它们、`cargo fmt --all --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` **从头到尾没被跑过**。结果:8 处 fmt + 6 条 clippy 红灯随提交进库(已由 `06a2b87` 收口)。
- 根因: conventions §1.3/§1.4 的定向验证口径是「动了 crates/ 跑 `cargo test -p <改动 crate>`」——它只提测试,**没提 fmt 与 clippy**,而 CI(`.github/workflows/ci.yml`)与发版门禁(`scripts/verify.ps1`)两处都把 fmt/clippy 列为必过项。规则层与门禁层不同步:按规则做到位,推上去照样红。
- 影响: 红灯要等 push 后 CI 才暴露,而自举一轮可能提交多次;更糟的是**发版门禁会当场拦下**(本轮 `package.ps1` 的验证证据门禁就是这样拦住的),排查时要回溯好几个提交才找得到源头。
- 修复方向(二选一或都做): ①把 fmt/clippy 写进 §1.4 的定向清单——每次提交前对**改动文件**跑 `rustfmt --edition 2021 <file>` 与对**改动 crate** 跑 `cargo clippy -p <crate> --all-targets -- -D warnings`(注意:本次那 6 条 clippy 只在编译 `-p kanzei` 时才暴露,只跑改动最多的那个 crate 不够,新增了测试文件就要连它所在的 crate 一起跑);②做成代码强制而非规则:轮末提交前引擎自动跑一次 fmt/clippy 定向检查,红了不许提交(conventions §4「任何『规则』能用代码强制的绝不只写进提示词」)。**推荐 ②**,因为 ① 已经写在规则里过一次而这次仍然漏了。
- 验收: ①构造「新增文件带 fmt/clippy 违规」的场景,提交前被拦住并明说违规位置;②conventions §1.4 的定向清单与 CI/verify.ps1 的门禁清单**逐项对齐**,两处任一新增门禁时另一处必须同步(可加一条守护测试比对两份清单);③有回归覆盖。

- 进展: 2026-08-13 已修复(代码强制,方向②):git.rs 新增 fmt_gate+clippy_gate 挂进 commit 源码分支,与 compile_gate 并列硬门禁——`cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings` 任一不过即拦下提交并点名违规文件(git.rs 375-505)。验收①:fmt_gate_rejects_unformatted_source_and_names_file / clippy_gate_rejects_lint_violation_and_names_file 两测试构造临时最小 cargo 工程带违规代码,断言被拦且报文件名(git.rs 1315-1365)。验收②:stage_fmt_clippy_gates_align_with_ci_and_verify 守护测试比对 git.rs 实现、ci.yml(.github/workflows/ci.yml:29-32)、scripts/verify.ps1 三处命令文本逐项一致,任一处改参数当场红(git.rs 1263-1296);§1.4 定向清单同步写入编译/format/lint 门禁条款(default_conventions.md §1.4),profiles.rs baseline 断言补 compile_gate。验收③:kanzei-tools 253 单测全绿。落地时门禁立即抓住两笔漏网:①D-261 迁移带入 kanzei-llm/atomic_file.rs 测试名 CAS 前缀 non_snake_case;②该提交 4 文件 8 处 fmt 未归一——均当场修复。遗留说明:进展记录提的第三个维度 ui_lint(gen-ui-lint-globals)走独立前端通道,由 ui-runtime/ui-lint-smoke 脚本与 CI 前端 job 覆盖,不在 Rust 提交门禁范围,已在 §1.4 注明不改动需同步;关闭本条。

## D-287 更新检查两态不够用:「本地领先于最新发布」被渲染成「已是最新(别人的 hash)」 [fixed] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 前端 后端 发版
- 证据等级: E1(2026-08-12 用户截图 + 代码自证 + 单测)
- refs: D-265 D-004
- 复现: 2026-08-12 用户设置页「版本与更新」显示 `当前版本 a7a122a` + 点「检查更新」得到 **「已是最新(build-c99304f)」**。两个不同的 hash 并排摆着还说「已是最新」,看上去像更新检查坏了。实测取证:`c99304f` 是 `a7a122a` 的祖先(`git merge-base --is-ancestor` 通过),GitHub 上 `build-a7a122a` 这个 release **不存在**(本地 package.ps1 打完直接装的,没发布)。
- 根因: `release_is_newer` 的判定本身没错(本地构建时间戳晚于 release 的 published_at → 无新版),错的是**状态只有两态**。`update_check` 回 `status: if newer {"update"} else {"latest"}`,把三件不同的事糊成一句:①本地就是那个发布;②本地构建晚于最新发布(自举机常态);③根本无法比较(D-265 的 dev 构建)。前端 `16-settings.js` 在 else 分支里无脑打印 `r.latest`,于是把**别人的 hash** 塞进了「已是最新(…)」。
- 影响: 与 D-265 同族——「发布了但仍在跑旧版」那条线的第四种表现。用户看到自相矛盾的两个 hash,唯一能做的是找人来读代码确认到底哪个在跑;而真正该说的一句话(「你手上这份比线上发布还新」)一次都没说出口。
- 修复: `update.rs` 引入 `ReleaseVerdict` 五态(Update / Latest / Ahead / DevBuild / Unknown),`status()` 是前后端唯一契约;`release_is_newer` 退化成只判 Update 的测试用包装。前端 `updateResultText(r)` 按 status 一态一句,别人的 hash 一律标成「最新发布」,不再冒充「当前」;`18-startup.js` 的启动静默检查把结论直接写进设置页(不弹 toast),补上 D-265 验收④。
- 验收: ①无新版的三种成因各有各的文案,只有 status=latest 说「已是最新」;②`release_verdict` 五态有单测,status 取值漂移当场红;③既有 `release_check_never_downgrades_a_newer_local_build` / `legacy_date_only_build_requires_a_later_release_day` 两条回归保持绿;④dev 构建不点「检查更新」也能在设置页看到「无法比较」。
- 验证: cargo test -p kanzei-app 124 passed(含新增 `更新检查把无新版的三种成因分开而不是一律说已是最新`);cargo clippy -p kanzei-app --all-targets 零警告;node scripts/ui-lint-smoke.mjs / ui-i18n-smoke.mjs 通过。

## D-288 设置页保存把出厂 context_limit 冻进用户 toml:内置默认后来改了永远追不上 [fixed] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 后端 配置
- 证据等级: E1(2026-08-12 用户配置实证 + git 历史比对 + 单测)
- refs: D-173 D-246
- 复现: 用户 `~/.kanzei/kanzei.toml` 里 `[providers.deepseek] context_limit = 128000`,而 DeepSeek 实际是 1M 窗口;内置默认 `fill_defaults` 早就是 `1_000_000`(`0c9f903` 引入时确为 128_000,后来改对了)。后果:UI 占用比例与压缩预检都按 128k 算基准,DeepSeek 跑到真实容量的 ~12% 就开始压缩。
- 根因: `settings_get` 发给前端的是 **fill_defaults 之后**的值(含出厂 context_limit),设置页保存时 `settings_apply_providers` 又把**每个** provider 的该字段无条件写回文件。于是用户从没碰过那个格子,一次「保存」就把当时的出厂默认冻成了自己的配置;`fill_defaults` 只补 `None`、不覆盖已有值,内置默认此后再怎么改都追不上。这是「默认值被固化成用户配置」的通病,不止 deepseek 一个字段会中招。
- 修复: `kanzei-harness::config::builtin_context_limit(name)` 直接取自 `fill_defaults`(不另立名单,避免 D-246 那种名单漂移);`settings_apply_providers` 遇到与出厂默认相同的值就**移除该键**而不是写入——留空 = 跟随内置,每次加载由 fill_defaults 补齐;用户手填的非默认值照旧原样落盘。用户配置里那行 128000 同时按此清掉。
- 验收: ①保存出厂值后文件里不出现 `context_limit`,加载后生效值等于内置默认;②用户手填的非默认值原样保留;③单测覆盖正反两面。
- 验证: cargo test -p kanzei-app -p kanzei-harness 234 passed(含新增 `出厂上下文上限不落盘_手填值原样保留`);clippy 零警告。
- 残余: 只做了 context_limit 这一个字段。设置页其余「回填即落盘」的字段(limits/cadence 已各自有留空语义)未逐一复核,若再出现同类固化按本条同一手法处理。

## D-265 dev 构建的更新检查谎报「已是最新」:release_is_newer 对 dev 直接返回 false,用户永远不知道该手动装 [fixed] (medium)
- 优先级: P1
- 复杂度: 小
- 标签: 发布
- 证据等级: E1(用户实测截图 + 代码形态自证)
- refs: D-145 D-004
- 复现: 2026-08-11 用户装完 build-22a927c 后打开设置页「版本与更新」,显示 `当前版本 dev` + 点「检查更新」得到 **「已是最新(build-22a927c)」**。它明明已经取到了最新发布的 tag,却告诉用户不用更新;用户据此以为自己已经在新版上,实际左边栏没有子代理面板、「更多」里没有勘察复核开关——新代码一个都没在跑。
- 根因(代码自证): `crates/kanzei-app/src/update.rs` 的 `release_is_newer` 第一行短路——`if current_hash == "dev" || tag.is_empty() || tag.contains(current_hash) { return false; }`。dev 构建(`KANZEI_BUILD_INFO` 未设,即 `release.ps1` / `cargo build` 产出的那份)直接判定「没有新版」,`update_check` 于是回 `status: "latest"`,前端 `16-settings.js` 按这个渲染成「已是最新」。
- 影响: **一旦落到 dev 构建上,应用内更新通道就永久失效且无声**——启动时的静默检查不会弹 toast,手动点「检查更新」还会得到一句反向的保证。这正是 D-145「发布了但仍在跑旧版」那一族,只不过上次的成因是两份副本、这次的成因是版本比较把 dev 当终点。用户唯一的出路是有人告诉他去手动装 setup.exe。
- 为什么当初这么写(不要简单删掉那个分支): dev 构建没有可比的时间戳(`build_stamp` 需要 `KANZEI_BUILD_INFO` 的第二段),硬跟发布版比会得出无意义的结论。所以 `return false` 在**比较语义**上没错,错的是把「无法比较」渲染成「已是最新」。
- 修复方向: 让 `update_check` 区分三态而不是两态——`latest`(真的最新)/ `update`(有新版)/ **`incomparable`**(本地是 dev 构建,无法与发布版比较)。第三态的文案必须明说:「本地为开发构建(dev),无法与发布版比较;最新发布是 build-xxxx,需要手动运行安装器」,并给出下载入口。D-004 口径:任何不做的理由都要说出来,绝不静默。
- 验收: ①`KANZEI_BUILD_INFO` 未设时,设置页不再显示「已是最新」,而是明说无法比较 + 最新发布 tag + 手动安装指引;②发布构建的既有两态行为不变(有既有单测的保持绿);③`release_is_newer` 的三态判定有单测覆盖(dev / 同 hash / 更新的发布各一条);④启动时的静默检查在 dev 构建下也给出一次可见提示(不弹窗打扰,但设置页要能看到)。
- 进展: 2026-08-12 随 D-287 一并修复(同一处状态机)。三态扩到五态:`ReleaseVerdict::{Update, Latest, Ahead, DevBuild, Unknown}`——本条要的 `incomparable` 拆成 `DevBuild`(没有构建戳)与 `Unknown`(有 hash 但拿不到/比不出发布时间),两者文案不同,后者不该说成「你在跑 dev 构建」。验收逐条:①dev 构建现在渲染「本地是开发构建,无法与发布版比较;要装发布版得手动运行安装器(最新发布:build-xxxx)」;②发布构建路径经由 `release_is_newer` 包装,两条既有回归单测未改一字仍绿;③五态各一条断言 + status 契约表(update_tests_update.rs);④`18-startup.js` 的启动静默检查把结论写进设置页 `#update-result`,不弹 toast。关闭本条。

## D-292 CLI E2E 测试的 HOME 隔离在 Windows 上失效:读开发者真实全局配置,全量测试挂死而非报红 [fixed] (high)
- severity: high
- 优先级: P0
- 复杂度: 小
- 标签: 测试 流程
- 证据等级: E1(2026-08-12 实测挂死 16 分钟零 CPU,定位到进程后当场验证)
- 复现: 在 `~/.kanzei/kanzei.toml` 加一条 `action="bash", resource="*"` 的 allow 规则,跑 `cargo test --workspace` → `always_allow_bash` 测试进程挂起,**永不超时、零 CPU、无输出**,整轮发版门禁卡死。
- 根因: 这几个 E2E 测试 spawn `kz` 子进程时用 `.env("HOME")` + `.env("USERPROFILE")` 做隔离。但全局根解析走 `kanzei_home()` → `dirs::home_dir()`,而 `dirs` 在 **Windows 上用 known-folder API(SHGetKnownFolderPath),根本不读 USERPROFILE 环境变量**。于是子进程照样加载开发者真实的 `~/.kanzei/kanzei.toml`——测试的"隔离"是假的,一直如此,只是此前没人在全局配置里放行过 bash 所以没暴露。放行之后权限询问不再产生,测试写进 stdin 的那个 "a" 没有接收方,于是死等。
- 影响: ①任何人只要全局放行 bash(本仓 2026-08-12 采纳方案 A 正是如此),全量测试直接挂死,发版门禁与 CI 一起卡住;②更广的问题是这些测试的结果本来就受开发者本机配置影响——绿不绿取决于 `~/.kanzei/kanzei.toml` 里写了什么,这是最难查的一类假绿/假红;③失败形态是**挂死不是报红**,比红灯难查得多(CI 上只能看到超时)。
- 修复: 5 处 spawn 全部补 `.env("KANZEI_HOME", home.join(".kanzei"))`。`KANZEI_HOME` 是 harness/src/home.rs 明确定义的全局根隔离通道(D-187 收敛出来的唯一入口),优先级高于 `dirs::home_dir()`,跨平台一致。
- 验收: ①全局配置里放行 bash 后 `cargo test -p kanzei --test always_allow_bash` 仍全绿;②5 处 spawn 无遗漏;③测试结果不再随开发者本机 `~/.kanzei/kanzei.toml` 变化。
- 验证: 修复前该测试挂死 16 分钟(实测,进程零 CPU);修复后 `cargo test -p kanzei --test always_allow_bash --test context_overflow_recovery` 5 passed,1.17s + 0.15s。
- 残余: 只补了 spawn 子进程这一类。仓内**同进程**读全局配置的测试是否也受污染未逐一排查;更彻底的做法是测试统一走一个设好 KANZEI_HOME 的夹具,而不是每处手写三个环境变量——单列 R-200。
- refs: D-187 R-200

## D-290 模式与鞭挞开关每次冷启动都被重置:回显算出来的值被当成用户意图写回存档 [fixed] (high)
- severity: high
- 优先级: P0
- 复杂度: 小
- 标签: 前端 自举
- 证据等级: E1(2026-08-12 用户报「我每次打开都要重新设置」+ 读码定位 + 冒烟反验)
- 来源: 用户 2026-08-12。用户明确指出这条早已提过——R-115/D-155 那轮(requirements-archive.md:891-898「四类设置跨重启保留」)确实修过 `processProfileUi` 落盘与回退链,但**只修了读的一半,写的一半仍在污染存档**,于是同一个症状复发。
- 复现: 开 app → 模式选择器回到「结伴开发」、鞭挞开关回到未勾选(其余设置——侧栏宽度、筛选、继续文案——都正常保留,用户实测确认范围就这两个)。
- 根因(两条写路径,互为补充,只修一条照样复发): 
  ①`switchProcess` 拿 `$("profile-select").value` 当「旧进程的用户意图」写进 `kz-process-profile`(09-sessions.js:356-358)。选择器的值在**回显期间**是算出来的,不是用户选的——启动竞态里 `activeProcessId` 尚未就绪时 `applyProfileValue` 会按回退链算出 dev-pair 刷进控件,随后任何一次切线就把存档里的 dev-auto 覆盖成 dev-pair。
  ②`applyProfileValue` 末尾的 `syncAutoContinueWithProfile()` 在模式不是 dev-auto 时关掉鞭挞,并**落盘**(`rememberAutoUiState()` + `kz-auto-continue="0"`)。于是①算错一次,②立刻把「关」写成用户意图,下次冷启动 `normalizeAutoState` 读回 false——**自我延续**,再也回不来。
- 影响: 每次开 app 都要手动重设两个控件;更隐蔽的是它直接触发 D-291——模式被降级成结伴开发后 `autoContinueAllowed()` 恒 false,鞭挞开着却永远不续跑,且旧代码对此一声不吭。
- 修复: ①`applyProfileValue` 在 `activeProcessId` 为空时直接返回(不知道该显示谁的档位就别动控件,掐掉整条降级链的起点);②新增 `applyingProfileEcho` 标志,回显期间 `rememberAutoUiState` 与 `kz-auto-continue` 写入一律短路——回显只同步控件,不产生"用户意图";③删掉 `switchProcess` 里那次 `processProfileUi.set(...)`,写盘只由 `profile-select` 的 change 事件负责(用户真的动手才算意图)。
- 验收: ①回显路径不得写 `kz-auto-continue` 与 `kz-process-auto-state`;②`switchProcess` 不得再用选择器显示值覆盖旧进程档位;③冷启动后模式与鞭挞保持上次选择。
- 验证: ui-runtime-smoke 新增 3 条断言(回显不写两处存档 + 源码守卫禁止 switchProcess 那次写盘)。**反验**:把 `applyingProfileEcho` 守卫改回旧行为,冒烟报「回显关掉的鞭挞不得写进全局 kz-auto-continue,实得 0」并失败,确认非恒绿。四条 UI 冒烟 + node --check 全绿。
- refs: D-291 R-115 D-155

## D-291 鞭挞续跑闸门静默否决:引擎判 Continue、前端不发也不吭声,界面永久停在「等待下一轮」 [fixed] (high)
- severity: high
- 优先级: P0
- 复杂度: 小
- 标签: 前端 自举
- 证据等级: E1(2026-08-12 用户截图 + 读码定位 + 冒烟反验复现同一画面)
- 来源: 用户 2026-08-12 截图:鞭挞芯片亮着、顶栏「本轮完成」,底部却是「空闲 · 鞭挞 · 等待下一轮」,此后再无动作。
- 复现: 开鞭挞跑一轮;轮末引擎判定 Continue(前端据此置 `auto_pending=true`、显示「2 秒后继续」并挂定时器);2 秒后定时器的四个条件任一不满足 → 直接 `return`。
- 根因: `scheduleAutoContinue`(08-compose.js:116-128)与 Nudge 分支(07-events.js:359-369)各有一份**复制**的 setTimeout,四道闸门(开关/暂停/本轮后停/模式)加 `!running` 全部**静默 return**:不发下一轮、不清 `auto_pending`、不清横幅、不写日志。界面于是永久钉在「等待下一轮」,而引擎侧 `rounds` 已经 +1——两边状态从此对不上。架构上这是 auto_run.rs 头注宣称「判定归引擎、前端只执行」之后,执行侧偷偷保留的一个引擎不知道的否决权。
- 次因: `on("kz:error")` 在函数**开头**无条件 `cancelAutoContinueTimer()`(07-events.js:244),一条 `terminal:false` 的非致命告警(如持久化警告)就能掐掉已排好的下一轮,而 `auto_pending` 仍是 true——同样是永久停摆。
- 影响: 自主推进随机停摆且无任何提示,用户只能靠盯界面发现;与 D-290 叠加后几乎必现(模式被降级 → `autoContinueAllowed()` 恒 false → 每轮都静默否决)。
- 修复: ①闸门收敛成唯一实现 `autoContinueBlockedReason()` + `armAutoContinue()`,两处副本合并;②被拦下时走 `abortAutoContinue()`:清 `auto_pending`、清横幅、`#auto-status` 与日志写明原因(D-004:不做的理由必须说出来);③`running` 与那四条区别对待——它是瞬态(kz:done 有意不收回运行态,由 kz:idle 负责),改为最多再等 15 拍(约 30 秒)后才判定卡住并报出,不再一次不满足就永久放弃;④`cancelAutoContinueTimer()` 移进 `if (terminal)` 分支;⑤续跑收口对象改用本轮的 `p.sessionId`(并行线结束时 `activeSessionId` 可能已是别人)。
- 验收: ①闸门拦下续跑后 `auto_pending` 必须清零;②必须显示未续跑的原因;③非致命错误不得取消已排队的续跑。
- 验证: ui-runtime-smoke 新增 3 条断言逐条对应。**反验**:把 `abortAutoContinue` 改回静默 `return`,冒烟报「闸门拦下续跑后必须清 auto_pending」与「必须说明原因,实得 `自主推进 1/10 · 等待下一轮`」——**实得文本与用户截图逐字相同**,确认回归测试复现的就是本条。四条 UI 冒烟 + node --check 全绿。
- 残余: `autoContinueAllowed()`(模式必须为 dev-auto)仍是前端私有条件,引擎的 `decide()` 并不知道它的存在,`rounds` 计数在被否决时仍会漂移。下沉进引擎属于结构改动,单列 R-199。
- refs: D-290 R-169 R-199

## D-294 多行字段值产生不可寻址的游离段落,且任何工具都删不掉——数据只进不出 [fixed] (high)
- severity: high
- 优先级: P1
- 复杂度: 小
- 标签: 核心 流程
- 证据等级: E1(D-239 实物损坏 5 段 + 读码定位往返缺口 + 反验测试)
- 复现: 对任一条目 `update` 时给字段传多行值 → 第一行写成 `- 字段: 第一行`,其余行原样落在下面 → 再次 load 时它们被解析成 `TemplateLine::Raw`,此后**永久无法通过任何工具触及**:update 只改得到第一行,tracker 文件直接写被拒、`git restore` 被引擎拦、shell 整文件重写被拦。
- 根因: 解析与渲染的往返不闭合。解析契约是「一行一个字段」(docstore.rs:782-791):只有 `- key: value` 成为字段,其余行落进 `TemplateLine::Raw` 原样保留;而渲染侧四个出口都直接 `format!("- {key}: {value}")`,**不校验 value 是否单行**。于是「写出去的东西读不回来」,差额沉淀成游离段落。
- 影响: ①数据只进不出——每次误传多行就永久增加一段,清不掉;②实测 D-239 因此积了 5 段重复(「验收②复核」×3、「第二轮复核」×2),内容与进展字段完全重复,把条目撑成噪音;③agent 发现删不掉后会绕道(M-056 就是一条"教你避开它"的 SOP),规则记忆替代了修复,坑被固化。
- 修复: 新增 `push_field`(docstore.rs)统一四个渲染出口,写前把值折成单行(按行 trim、丢空行、空格连接)。往返不变式因此成立:写进去的一定读得回来,字段永远可寻址、可改、可删。段落结构会丢但内容一字不少——比起产生删不掉的垃圾,这是明显更小的代价。
- 验收: ①多行值写入后 load 回来仍是一个字段,内容完整;②渲染产出的条目内不含任何非 `- key:` 行;③二次保存幂等(游离段落当年正是靠反复保存越积越多);④存量:D-239 的 5 段重复已清除。
- 验证: 新增回归 `多行字段值折成单行_不产生游离段落`(docstore.rs),三条断言对应验收①②③;反验有效——把 push_field 换回原来的 `format!` 直写,该用例当场红在「多行值必须折成单行字段,否则第 2 行起不可寻址」,恢复后绿。docstore 17 passed。
- 残余: 存量游离行全仓仍有约 304 行(defects 27 / requirements 16 / defects-archive 116 / requirements-archive 145),**未做机械清扫**——它们多数是早年多行写法留下的真实内容(如 D-261 的实测记录),逐行判断才能删,批量删会毁历史。机制已堵死,存量不再增长;要清理需要人工逐条判断,或先做 R-201 的清理通道。
- refs: D-239 D-130 R-201

## D-295 D-264 门禁与权限档位死锁:autonomous 轮 test_record 未白名单,任何 Rust 源码提交被拦 [fixed] (medium)
- 复杂度: 小
- 复现: autonomous/parallel 档位提交任何 Rust 源码:git commit 被 D-264 source_test_gate 拦下,要求一条暂存源码改动之后收尾的 passed 测试记录;test_record 工具调用报 permission requires user approval(实测 2 次,running 与 passed 均拒),.kanzei/kanzei.toml 无 test_record 规则且 edit 该文件也被拒——本代理无法自解,每次 Rust 提交都死锁。2026-08-16 R-201 首撞。
- 影响: autonomous 自举轮无法落盘任何 Rust 改动:门禁设计(要求测试记录)与权限配置(未放行 test_record)互相矛盾,已验证的代码只能滞留在工作树,自举节奏被打断。
- 来源: self-found
- 标签: 流程
- 解除动作: 在 .kanzei/kanzei.toml 加 `[[permissions.rules]] action = "test_record" resource = "*" effect = "allow"`(或交互轮逐次批准 test_record)。解除人: 用户。
- 优先级: P1
- 进展: 已修复(2026-08-12):kanzei.toml 新增 [[permissions.rules]] action="test_record" resource="*" effect="allow"(提交 6ef23cc,附注释说明 D-264 门禁与档位关系)。修复生效实证:同一轮内 test_record 两次调用成功并落盘 tests-archive.md(T-1786514712 R-201 定向、T-1786514969 R-201 关闭前全量),D-264 source_test_gate 因此拿到证据放行 800d5da 的 Rust 源码提交——死锁彻底解除,后续 autonomous 轮不再需要用户介入。

## D-307 关闭 kzapp 后自动重新启动实例 [fixed] (high)
- severity: high
- 优先级: P0
- 复杂度: 中
- 标签: 核心 桌面 发布
- 来源: 2026-08-12 用户交接复现「关闭 kzapp 后自动又启动一个实例」；需区分窗口关闭、更新重启、启动器转发、计划任务/自启与单实例插件路径。
- 证据等级: E1(用户复现，根因待当前构建定位)
- 复现: 启动已发布的 kzapp，关闭窗口或退出应用，观察退出后又出现新的 kzapp 实例。
- 根因: 待定位；重点检查 `CloseRequested`/`ExitRequested`、托盘退出、single-instance、更新安装后的 relaunch、计划任务/自启及 `kz.exe`/`kzapp.exe` 父子进程关系。
- 影响: 用户无法真正退出桌面端；自举或发布验收期间可能出现重复实例与状态串扰。
- 验收: ①定位并修复导致自动重启的最小路径；②关闭窗口后进程树不再出现新的 kzapp；③更新安装/启动器/单实例路径不引入回归；④重新打包安装后完成一次人工关闭验证，人工步骤和结果写入进展。
- refs: D-266 D-287 D-265
- 进展: 2026-08-12 已定位为更新交接 helper 在父进程退出后无条件重新 spawn kzapp，移除 run_install_helper 与 pending 更新路径的自动拉起，并把 UI/后端文案改为安装完成后手动启动；已通过 kanzei-app 定向 fmt/clippy/test 与 UI 语法/运行时冒烟，需重新打包安装后由用户执行关闭窗口并观察进程树的人工验证。

## D-308 R-225 新增 classic-script 全局未登记导致正式 UI lint 失败 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 前端 测试 设置
- 来源: 2026-08-12 正式 scripts/verify.ps1 发现 R-225 语言设置链路新增全局未同步到 UI lint 白名单。
- 证据等级: E1(verify 实测:7 处 no-undef；运行时冒烟通过但发布 lint 门禁失败)
- 机制: classic script 按序共享全局，R-225 的语言常量/函数及归档懒加载新增符号没有重新生成 scripts/ui-lint-globals.json。
- 影响: 功能运行时可通过，但正式 verify 阶段被 UI lint 拦截，无法生成绑定 HEAD 的发版验证证据。
- 验收: ①更新生成的全局清单且 no-undef 为 0；②完整 verify 通过并产出 dist/verification.json。
- refs: R-225 D-296
- 进展: 2026-08-12 已重新生成 scripts/ui-lint-globals.json，纳入 R-225 语言设置与 D-296 归档懒加载新增全局并清理已删除符号；ui-lint-smoke 31 文件/1157 标识符零错误，完整 verify 已通过并产出绑定 HEAD 的 dist/verification.json。

## D-296 docs_snapshot 单次调用重复解析两份归档约 6 遍(~4.8MB)+ 1 次 git log,挂在每次文档刷新与每次 git 提交事件后 [fixed] (high)
- severity: high
- 优先级: P1
- 复杂度: 中
- 标签: 后端 效率
- 来源: 2026-08-12 八维度审计(docs/design/audit_20260812_eight_dimensions.md §2);经反证代理独立重数确认。
- 证据等级: E1(读码核实+文件大小实测:两份归档 314KB+482KB、活动文件 72KB+57KB)
- 机制: docs.rs:146-290 一次快照里 batch_ids 循环 load 一遍 requirements/defects,load() 闭包对每 kind 再 load 一遍,req/defect 各调一次 dependents_map 与 schedule_for_display 且两者都进 dependency_states——两份归档合计被解析约 6 遍,另起一次 git log;DocStore::open 每次新实例,全链路无缓存,归档条目还整包塞进 IPC。
- 影响: 挂在每次 tracker 变更与每次 git 提交事件后面;极可能是 R-193「plan 勾选响应延迟」的机制底座(R-193 只登记了前端症状)。归档只增不减,成本单调上升。
- 验收: ①单次快照对每个 md 文件 read 计数 ≤1;②dependency_states 结果在 dependents_map/schedule_for_display 间复用;③归档条目改按需懒加载;④快照耗时与 IPC 字节前后基准对照,R-193 症状复测。
- refs: R-193 D-209
- 进展: 2026-08-12 docs_snapshot 建立 active/archive 单份缓存，tracker 复用 dependency_states，归档正文改为展开历史时调用 docs_archive_entries；同一夹具 IPC 基线 804 bytes→当前 607 bytes，当前快照 78ms，已通过 kanzei-app/kanzei-tools 定向测试、workspace fmt/clippy 与 UI 冒烟(0 运行时错误)。

## D-299 失败指纹粒度崩塌:bash 类失败常态塌缩成 [fp:bash|exit code:] 全类通配键,Tier0 注入与复发计数整类错配 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5);经反证代理确认「核心指控成立,无法反驳」。
- 证据等级: E1(读码核实+.kanzei/memory 存量键实测:该指纹已同时挂 2 个条目,含 M-022)
- 机制: bash 工具把输出统一渲染为首行「exit code: N」(bash.rs:268-271);failure_kind 只对三种 git fatal 行做根因特判,其余取首行抹数字(metrics.rs:391-419)——一切非 git-fatal 的 bash 失败(测试红/编译错/脚本崩)全部塌成 kind="exit code:"。另有 [fp:req|r-]、[fp:edit|...] 等同样过泛/残废的键。
- 影响: 任何 bash 失败都 Tier0 注入 M-022 并投「记忆没进决策」的误导性修订笔记;复发计数按全类累加,遥测与晋升判据整体失真——这是现在每个自举轮都在发生的事。
- 验收: ①failure_kind 对 bash/test 类输出取根因行(断言文本/error 行)构 kind;②写入侧拒绝过短或全类通配的 kind 成为条目指纹;③存量全类通配键拆分处置;④tier0 注入命中按真实同类失败复核。
- refs: R-196 R-216 D-282
- 进展: 2026-08-12 failure_kind 对 bash/test 输出跳过 exit code/process wrapper，优先断言/error/failed/panic 等根因行；写入侧拒绝过短与通配 kind，存量 exit code 键归一到 legacy generic 隔离键，不改 .kanzei/memory；core 135 tests、tools 257 tests、workspace clippy/fmt 全绿。

## D-300 limits.barrier_timeout_secs 配置键失效:漏接 merge overlay 且 unknown_keys 名单缺失,设了静默不生效还误报未知键 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 后端 harness
- 来源: 2026-08-12 八维度审计(§6);主代理复核 overlay! 宏现状确认(10 个 limits 字段不含它)。
- 证据等级: E1(读码核实:config.rs overlay! 宏与 unknown_keys 已知键名单双缺;:363 与 :1032 注释自认「就是这么漏的」但从无条目跟踪)
- 机制: load_with_warnings_at_root 从 default 起经 merge() 层叠,全局层与项目层设的该值都被丢弃;既有测试全部绕过 merge 直接 toml::from_str,所以全绿。
- 影响: 用户在任一配置文件设 barrier_timeout_secs 既不生效又收到「未知配置项已忽略」假告警;屏障超时只能用默认 1800s。
- 验收: ①补 overlay 宏与 unknown_keys 名单各一行;②新增「Limits 全字段经 merge_file 层叠往返不丢值」的穷举守护测试防再漏;③项目层设任一 limits 键都生效且无假告警。
- refs: D-301
- 进展: 2026-08-12 已修复:overlay! 宏与 unknown_keys 名单各补 barrier_timeout_secs,:363/:1032 两处注释更新;新增守护测试 limits_全字段_层叠往返不丢值_且名单穷举(TOML 显式赋全字段+unknown_keys 零告警+merge 后逐字段等于层值,同时堵住既有 unknown_keys_schema_matches_struct 对 [limits] 的 None 序列化盲区)。cargo test -p kanzei-harness --lib config 42 passed 0 failed。

## D-301 编排派发的勘察/复核子代理没有 per-role 墙钟:注释承诺的「双层有界」内层在唯一生产路径上不存在 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 后端 并行
- 来源: 2026-08-12 八维度审计(§7)。
- 证据等级: E1(读码核实:phase.rs:365-368 注释承诺内层由 subagent_timeout_secs 包住;rt.timeout_secs 全仓唯一消费点是 drive.rs:520 的模型自派 task 路径;编排路径 phase_pipeline.rs:294-311 直接 await run_read_agent 无 timeout 包装)
- 影响: 单个勘察/复核角色挂死会拖满整个屏障直到外层 barrier_timeout_secs(默认 1800s),且审计事件把它记成 barrier_timed_out——内层超时语义错位,排查方向被误导。
- 验收: ①dispatch_roles 给每个角色 future 包 tokio::time::timeout(rt.timeout_secs),超时映射 ScoutOutcome::TimedOut;②单角色挂死时屏障在内层上界收敛且 barrier_timed_out=false;③定向测试。
- refs: D-300 R-173
- 进展: 2026-08-12 dispatch_roles 为每个角色包 tokio::time::timeout(rt.timeout_secs)，超时映射 ScoutOutcome::TimedOut 并上抛失败 ToolEnd；新增单角色挂死反证，1s 内层收敛且无 barrier_timed_out；kanzei-app 127 tests、workspace clippy/fmt 全绿。

## D-302 TaskCancellations 死 token:超时与整轮停止路径不清理注册表,stop_task 对已死子代理误报成功 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 后端 并行
- 来源: 2026-08-12 八维度审计(§7)。
- 证据等级: E1(读码核实:注册在 runner/subagent.rs:267-270,清理在 :319-323 函数末尾;drive.rs:520-533 用 tokio::time::timeout 包裹,超时即 drop future,末尾清理永不执行——:319 注释声称防的正是这个场景,但 await 后代码在 future 被 drop 时不可能运行)
- 影响: 注册表随超时/停止积累死 token;stop_task 对已终态子代理返回成功,面板单条停止(R-174)的语义失真。
- 验收: ①register 改为带 Drop 的 RAII guard(与 ReadPermit 同手法);②超时/整轮停止后注册表为空;③stop_task 对已终态 id 返回明确「已结束」而非成功。
- refs: R-174
- 进展: 2026-08-12 register 改为持有 Arc 注册表的 TaskCancellationGuard，Drop 覆盖正常/失败/取消/外层 timeout 丢弃 future 的清理；新增死 token 与终态停止反证测试，stop_task 已结束 id 保持明确错误；core 136 tests、workspace clippy/fmt 全绿。

## D-304 parallel_lines_ui.md 状态头虚标:P1/P3/P6 宣称随 R-184 全部上线,实现整块缺席——文档为真源的自举返工源 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 中
- 标签: 前端 文档 并行
- 来源: 2026-08-12 八维度审计(§3 与 §7 两个维度独立发现交叉确认)。
- 证据等级: E1(读码核实:该文:4 宣称已于 2026-08-11 全部上线;实测 ui/11-docs-list.js:347 isAgentNext 仍在渲染「下一个」推断值、backlog 无「被取得」事实标记、该文 §10 验收2「全仓 grep isAgentNext 零命中」不成立;泳道无三级卡住判据;16-settings.js 无按 agent 线级设置)
- 影响: 自举模式以设计文档为真源,虚标直接导致后续轮漏做或重复申报;D-207 的病根(界面展示推断值)因 P1 未落地继续存活。
- 验收: ①修正状态头为「P1/P3/P6 部分交付」并逐项列残余;②P1 残余(删 isAgentNext 全链路+基于 collaboration_snapshot claim 的「● 代号 被取得」标记)落地,验收沿用该文 §10 的 2/3 原文;③「排在队首但无人取不出现标记」反证测试。
- refs: R-184 D-207
- 进展: 2026-08-12 验收①完成:parallel_lines_ui.md 状态头改「部分交付」并逐项列 P1/P3/P6 残余；验收②③完成:删除 isAgentNext/agent-next/下一个推断全链路，列表只依据运行中的 collaboration_snapshot claim 显示「● 代号 被取得」，补队首无人 claim 不显示标记反证；node check 4 文件、UI 冒烟 1338 invokes/0 运行时错误全绿。

## D-309 并行线路收活面板在自动刷新后消失 [fixed] (medium)
- severity: medium
- 优先级: P1
- 复杂度: 小
- 标签: 前端 并行
- 来源: 2026-08-12 用户反馈「并行线路展开打算收,自动刷新后展开的没了」。
- 复现: 进入「并行线路」视图,点击某条工作树线路的「收活」展开五格面板,等待自动刷新或手动刷新线路;展开面板及已加载的 diff/门禁结果被移除。
- 根因: `crates/kanzei-app/ui/20-lines.js:113-120` 的 `renderLines()` 每次刷新先对 `#lines-list` 调用 `replaceChildren()`,临时挂在 lane 下的 `.line-harvest` 与 `<details>` 展开状态没有按 `process_id` 保存和恢复。
- 影响: 自动刷新期间用户正在进行的人读 diff、门禁和合并准备状态丢失,容易误以为操作未生效,也无法安全完成收活流程。
- 验收: ①线路自动/手动刷新后同一 `process_id` 的收活面板仍展开;②面板内已加载 diff、已确认人读 diff、门禁结果和回写状态不丢;③改动文件展开状态保持;④线路消失时不错误复挂旧面板。
- 证据等级: E1(用户复现+代码定位),修复后补 UI 运行时冒烟。
- 状态: fixed
- 进展: 2026-08-12 `20-lines.js` 刷新前按 process_id 暂存并复挂同一收活面板,保留 diff/确认/门禁/回写 DOM 状态,并恢复改动文件 details 展开状态;新增运行时冒烟覆盖自动刷新后面板仍在且同一 DOM 节点;node check、ui-lint-smoke(31 文件/1160 标识符)、parallel-lines-regression、ui-runtime-smoke(1350 次 invoke/0 错误)通过。

## D-310 收活无 claim 仍解锁 tracker 回写 [fixed] (medium)
- severity: medium
- 优先级: P1
- 复杂度: 小
- 标签: 前端 后端 并行
- 来源: 2026-08-12 用户截图复现:线路合并成功后 claim 显示「未声明条目」,第 5 格仍显示「重试回写」并发起失败请求。
- 复现: 打开一条没有声明 R-xxx/D-xxx 条目的并行线,完成收活第 2 格确认、第 3 格门禁、第 4 格合并,点击第 5 格回写;日志报「认领 `未声明条目` 不是条目 ID」,用户已经完成的合并被错误地呈现为可重试回写。
- 根因: `crates/kanzei-app/ui/20-lines.js` 第 408 行只按合并成功解锁回写,未校验 claim 是否为严格 R-数字/D-数字;后端 `worktree_harvest_writeback` 只按第一个 `-` 拆分,对自由文本和带尾随内容的 ID 校验不完整。
- 影响: 第 5 格出现错误操作入口,产生无意义 IPC/红色日志;用户不知道合并已成功但 tracker 回写需要主代理手动登记。
- 验收: ①无有效 claim 时合并仍成功,第 5 格保持禁用并明确提示主代理手动登记;②无效 claim 不调用 `worktree_harvest_writeback`;③有效 R-xxx/D-xxx claim 仍可正常回写;④后端拒绝空格、尾随文本和非 R/D 类型 claim。
- 证据等级: E1(用户复现+前后端代码定位),修复后补前端运行时与 Rust 定向测试。
- 状态: fixed
- 进展: 2026-08-12 `20-lines.js` 提取严格 `R-数字`/`D-数字` claim;无有效 claim 时合并仍可完成,但第 5 格保持禁用并提示主代理手动登记,不再发起无效回写 IPC;`processes.rs` 抽出严格 claim 解析并拒绝空值、尾随文本、非 R/D 类型;新增 Rust 定向反证与 UI 无效 claim 冒烟。验证:node check、ui-runtime-smoke(1427 次 invoke/0 运行时错误)、ui-lint-smoke(31 文件/1162 标识符)、cargo fmt、cargo clippy、kanzei-app 定向测试通过。

## D-311 并行线路自动刷新时卡片重复播放进入动画 [fixed] (medium)
- severity: medium
- 优先级: P1
- 复杂度: 小
- 标签: 前端 并行
- 来源: 2026-08-12 用户反馈「前端刷新会闪一下」。
- 复现: 打开并行线路视图,等待运行中线路自动刷新;每次 `renderLines()` 重建线路卡片时观察卡片短暂透明/下移后出现,页面表现为周期性闪烁。
- 根因: `crates/kanzei-app/ui/style.css` 的 `.line-lane` 永久绑定 `line-lane-enter` 动画;自动刷新每次重新创建 lane,导致既有线路也重复播放首次加载动画。
- 影响: 运行状态持续刷新时视图闪烁,用户对正在展开的线路和状态变化的感知不稳定。
- 验收: ①首次进入与同一视图的自动/手动刷新均不播放线路卡片进入动效;②刷新不再给既有线路重复添加透明度/位移动画;③收活面板、改动文件展开状态和滚动位置不受影响;④补前端冒烟/静态回归护栏。
- 证据等级: E1(用户反馈+代码定位),修复后补前端运行时与静态回归测试。
- 状态: fixed
- 进展: 2026-08-12 移除 `.line-lane` 及其进入动画定义;线路刷新继续保留收活面板、改动文件展开和刷新节流,但不再给重建卡片附加透明度/位移动画。新增静态护栏确认进入动画未回归,UI 运行时冒烟 1439 次 invoke/0 运行时错误、UI lint 31 文件/1162 标识符、并行线路回归通过。

## D-313 多线路仍共享可复用身份、全局自动定时器与项目级停止，导致状态/鞭挞/历史/后台进程串线 [fixed] (high)
- severity: high
- 优先级: P0
- 复杂度: 大
- 标签: 核心 后端 前端 并行 自举
- 来源: 2026-08-12 用户要求全面扫描；静态全链路审计与现有测试覆盖反证确认。
- 复现: ①删最高编号线路后重建，`pN`/session 被复用并可能继承旧前端缓存与历史；②后台线路收到 `kz:done + autoAction=Continue` 时 handler 被路由层截断；③两线鞭挞共享一个 timer，切线会取消旧线且触发时按当前活动线发送；④停止一线调用项目级后台进程回收；⑤停止前端立即乐观收敛为空闲，无 `stopping`；⑥运行中线路仍可合并/放弃；⑦A 线发送 IPC 在切到 B 后失败会把 B 标失败；⑧对话读接口会回写 runtime conversation。
- 根因: R-197 只在既有单活动线结构上补了 session 缓存，未完成 R-206 的具名状态机/唯一 mutator，也未把 timer、控制事件副作用、后台进程和工作树生命周期真正下沉为 session/process 级。
- 影响: 多线路自举不可靠，表现为运行显示空闲、停止按钮消失、鞭挞自动开启或停跑、历史/活动错线、停止或收活影响其他线路，严重时合入仍在变化的工作树。
- 验收: 以 R-226 十批和十条验收为准；必须新增真实事件路由、删线重建、双 timer、跨线停止、切线 IPC 失败、运行中合并/放弃六组反证，不能只靠现有绿测结案。
- refs: R-226 R-197 R-199 R-206 R-207 R-222 D-209 D-283 D-305 D-306
- 进展: 2026-08-12 根因链已按 R-226 收口：线路 ID 退役账本、按 owner 停止、统一 finalize、运行线收活硬闸、按 session 控制事件/timer/发送失败与 stopping 投影、纯读历史恢复均已实现；新增删线重建、双 timer 后台续跑、owner 停止、运行中收活反证，相关 Rust/UI 门禁全绿，待最终包安装后执行真实双线 E2。

## D-305 侧栏「隔离工作树」保留独立合并入口,绕过收活五格「必须人读 diff」强制格 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 前端 并行
- 来源: 2026-08-12 八维度审计(§3)。
- 证据等级: E1(读码核实:09-sessions.js:20-24 每棵工作树渲染差异/合并/放弃按钮;:50-94 merge 路径经 confirmWorktreeMerge(20-lines.js:500-520,仅 window.confirm)直达 worktree_merge,不经过收活五格)
- 影响: 收活五格的「已读 diff」是合并的唯一语义防线,另一入口整体绕过等于防线失效;两套合并流程纪律不一致。
- 验收: ①全仓只有一条能触发 worktree_merge 的用户路径(侧栏降级为跳转入口或内嵌同一强制格);②「已读 diff」不可绕过有冒烟断言(删除断言即红)。
- refs: R-179 D-304 R-222
- 进展: 2026-08-12 侧栏删除直接合并按钮，改为切到绑定线路并进入同一收活五格；后端 `worktree_merge`/`worktree_discard` 另持线路 lifecycle 锁拒绝运行态，UI 冒烟与静态反证锁定单一用户合并路径。

## D-306 空闲线路残留上一轮 stage 文案:线路按钮显示「○ 空闲 · <旧阶段>」自相矛盾 [fixed] (low)
- severity: low
- 优先级: P3
- 复杂度: 小
- 标签: 前端
- 来源: 2026-08-12 八维度审计(§3);经反证代理确认机制确定性成立。
- 证据等级: E1(读码核实:01-core.js:52-55 kz:status 写 state.stage,:88-102 终态收敛分支不清 stage/detail 且当场用残留值重画;09-sessions.js:147 轮询把 item.stage 回填进「空闲」)
- 影响: 空闲线路状态行含上一轮阶段词,运行态一眼可读性受损。
- 验收: ①终态收敛处重置 stage/detail;②停用空闲态轮询 stage 回填;③「空闲线路状态行不含旧阶段词」冒烟断言。
- refs: D-283 R-197 R-206
- 进展: 2026-08-12 会话终态统一清空 stage/detail，空闲 process_list 不再回填旧阶段，线路状态仅在真实运行态显示阶段；UI 冒烟新增「空闲第三线路不含旧测试阶段」反证。

## D-312 并行线路创建名称按秒导致重复点击撞同一工作树 [fixed] (medium)
- severity: medium
- 优先级: P1
- 复杂度: 小
- 标签: 前端 并行 工作树
- 来源: 2026-08-12 用户截图复现:连续创建并行线路时反复报工作树 `line-20260812140701` 已绑定到线 `p3`。
- 复现: 在并行线路视图快速连续点击创建按钮,两次请求落在同一秒;前端生成相同的 `line-YYYYMMDDhhmmss` 名称,第二次复用同一工作树目录和分支,被后端一树一线校验拒绝。
- 根因: `crates/kanzei-app/ui/09-sessions.js:createWorktreeLine` 只取精确到秒的时间戳命名,且请求等待期间按钮仍可再次触发;后端按设计正确拒绝已绑定工作树。
- 影响: 用户看到创建并行线路失败和重复红色日志;快速操作无法创建第二条线,但已有 `p3` 工作树不应被误删或强制回收。
- 验收: ①线路名包含毫秒/序号,连续创建请求不复用同名工作树;②创建请求在返回前按钮禁用且重复 click 只产生一次 `process_create`;③成功/失败后按钮都恢复可用;④已有一树一线后端保护与工作树现场不变。
- 证据等级: E1(用户截图+前端代码定位+仓库工作树现场核对),修复后补 UI 运行时和静态回归测试。
- 状态: fixed
- 进展: 2026-08-12 将建线名改为毫秒时间戳+进程内序号;创建 IPC 单飞且按钮请求期间禁用,finally 恢复;新增双击只发一个 process_create 的 UI 冒烟与静态护栏。验证:node --check、ui-runtime-smoke(1439 次 invoke)、ui-lint-smoke、parallel-lines-regression 通过。

## D-314 收活回写只信线路 claim，忽略线路对话中已明确的 tracker 条目 [fixed] (high)
- severity: high
- 优先级: P1
- 复杂度: 中
- 标签: 前端 后端 并行 tracker
- 来源: 2026-08-13 用户截图：并行线路对话已明确处理 D-297，收活第 5 格仍显示“当前线路未声明有效条目 / 无有效条目”。
- 复现: 并行线创建时未在首段 prompt 声明严格 R-xxx/D-xxx，运行中对话随后明确读取并处理 D-297；完成 diff、门禁和合并后，第 5 格只读取 collaboration claim，无法回写真实交付条目。
- 根因: `20-lines.js:buildHarvestPanel` 在面板构造时只调用 `harvestClaimId(line.claim)`，没有读取该线路会话历史，也没有给多条候选提供人工选择；`worktree_harvest_writeback` 因此拿不到对话里的真实条目。
- 影响: 已完成合并的线路无法形成 tracker 回调，用户必须手动补记；收活第 5 格与对话事实脱节。
- 验收: ①后端从该线路最新对话提取并只返回 tracker 中真实存在的 R/D 条目；②唯一候选自动选择，多候选必须由用户明确选择；③没有候选仍保持禁用且不发回写；④补 Rust 与 UI 反证。
- refs: R-226 R-222 D-310 D-297

- 进展: 2026-08-16 完成:后端新增 worktree_harvest_candidates(processes.rs)从线路最新对话提取 R/D 候选并与主根活动 tracker 求交只返回真实存在条目;前端 20-lines.js 收活格5 改为对话候选选择器(唯一自动选中、多候选人工选择、无候选禁用),09-sessions.js/20-lines.js 新增关闭线路入口并调用 process_close(默认主线排除、运行中二次确认先停止收口、成功后刷新并切回主线)。Rust 反证 harvest_candidates_只取线路对话中真实存在的活动条目且最新优先;UI 反证 runtime-smoke 断言关闭入口/process_close 调用/唯一候选自动选中/多候选人工选择。验证:cargo test -p kanzei-app 132 passed、fmt/clippy 绿、UI 冒烟五连+并行线路回归绿。提交 e1fb7cb。

## D-315 并行线路缺少显式关闭入口，运行停止与线路生命周期无法收尾 [fixed] (high)
- severity: high
- 优先级: P1
- 复杂度: 小
- 标签: 前端 并行 工作树
- 来源: 2026-08-13 用户反馈“我想关闭并行线，关不掉”。
- 复现: 线路页和左侧当前状态列表均只有切换、收活、历史删除等动作，没有调用既有 `process_close` 的关闭按钮；用户即使完成合并也无法从界面注销线路。
- 根因: 后端已实现 `process_close → stop/finalize → reclaim_worktree_on_close → unregister_parallel_process`，但 classic-script UI 从未接入该命令。
- 影响: 已完成或不再需要的线路长期留在列表；运行会话、自动续行状态和工作树绑定无法由用户显式收尾。
- 验收: ①非默认线路显示“关闭线路”，默认线路不显示；②运行线路关闭需二次确认并由后端先停止收口；③关闭成功后刷新线路/进程/工作树并安全切回主线；④有独有改动的工作树保留，已合并干净工作树自动回收；⑤补 UI 与后端既有语义回归。
- refs: R-226 R-207 D-313

- 进展: 2026-08-16 完成:后端 process_close 增强为 async——关闭顺序改为停止/注销→回收 owner 后台进程→处置工作树,返回带回收明细文案,工作树保留时落 worktree.orphaned 审计事件;前端 09-sessions.js closeParallelProcess(默认主线排除、运行中二次确认并置 stopping、成功后刷新进程/工作树/线路并安全切回主线),20-lines.js 线路卡片与左侧状态列表均加关闭按钮。Rust 反证 close_process_先停止运行会话再回收已合并干净工作树并注销线路(补 create_session 修正测试构造);UI 反证 runtime-smoke 断言非默认线路有关闭入口/默认主线不显示/点击调用目标 process_close。验证:cargo test -p kanzei-app 132 passed、fmt/clippy 绿、UI 冒烟五连+并行线路回归绿。提交 e1fb7cb。

## D-297 conversation_list/trace_get/按序号恢复全量解析整张 session_events,run.trace 无保留策略成本单调增长 [fixed] (high)
- severity: high
- 优先级: P2
- 复杂度: 中
- 标签: 后端 效率
- 来源: 2026-08-12 八维度审计(§2);经反证代理确认。
- 证据等级: E2(读码核实+state.db 实测:主会话 4333 条/8.9MB;run.trace 9.8MB 中 95% 来自 279 条非增量整包,单条最大 945.5KB)
- 机制: conversation.rs:74-76/116-121/174-178 三处都调 store.list_events(session_id, 0);events.rs:24-35 无 event_type 过滤,每行 payload_json 全量 serde 解析。run.trace 无任何清理通道;整包来源是 flush_live_trace_locked 把未落盘尾部一次性打包(state.rs:203-230)。
- 影响: 打开历史列表/查看轨迹/按序号恢复每次付整表解析;随使用时间单调变慢,无上限。这也把 D-209 的收敛范围量化到轨迹层(对话快照仅 0.05MB,轨迹才是 95% 落库体积)。
- 验收: ①list_events 支持 event_type 下推过滤并补 (session_id,event_type,sequence) 复合索引;②三个调用点只取所需类型,按序号恢复改单行查询;③run.trace 定保留策略(每会话最近 N 轮)且整包补写按 ≤64KB 分批、TaskProgress 入参截断;④主会话规模下解析字节量降一个数量级。
- refs: D-209 D-296

- 进展: 2026-08-16 B4/4 完成。验收对照:①events.rs list_events_by_type 下推 event_type+复合索引 session_events_session_type_sequence(schema.rs);②conversation.rs 三调用点改用下推、recover_messages_raw 改 event_by_sequence_and_type 单行;③prune_trace_rounds 每会话保留 200 轮(flush_live_run 收尾触发)、flush_live_trace_locked 整包≤64KB 分批、TaskProgress 入参截断 4096 字符(UI 保留完整);④量化测试(4000 事件)断言下推解析字节量比全表低一个数量级。验证:全量 cargo test --workspace 全绿(kanzei-app 134、kanzei-core 140、kanzei-tools 258),fmt/clippy 绿。提交 4055b6c、bde865d。批次: 4/4

## D-298 state.db 82MB 中约 68MB 是 freelist 死页从不 VACUUM,9 份迁移备份约 59MB 永不清理 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 后端
- 来源: 2026-08-12 八维度审计(§2)。
- 证据等级: E2(只读打开 state.db 实测:page_count×page_size=82.04MB,freelist_count=16551 页≈67.8MB;活数据合计约 10.5MB;.kanzei 下 state.db.v4~v11.bak 共 9 份≈59MB)
- 机制: 代码无任何 VACUUM/auto_vacuum/incremental_vacuum 调用;迁移备份只增不删。
- 影响: .kanzei 数据库相关占用约 145MB 而活数据仅约 11MB;备份随迁移版本无限增长。
- 验收: ①空闲时机条件整理:freelist 占比超阈值(如 50%)执行 VACUUM(或建库启用 auto_vacuum=INCREMENTAL+周期回收);②迁移备份只保留最近一版;③整理后库文件回到活数据量级。
- refs: D-297

- 进展: 2026-08-16 完成。验收对照:①session.rs maintain_housekeeping 挂在 SessionStore::open 公共路径(桌面命令/CLI/移动端都走到),24h 节流,PRAGMA freelist_count/page_count 占比>50% 时 VACUUM;②同方法扫描 state.db.v<N>.bak 只保留版本号最大的一份(实测 9 份约 59MB 可清 8 份);③新测试 freelist超阈值时vacuum回收死页 断言 VACUUM 后 page_count 下降、迁移备份只保留最近一版 断言只剩 v12.bak。验证:kanzei-core 143 passed、kanzei-app 134 passed、fmt/clippy 绿。提交 c307f78。

## D-303 桌面协调器未装配 observer:停止/异常路径 writer 审计断档,租约事件不可回放 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 小
- 标签: 后端 并行
- 来源: 2026-08-12 八维度审计(§7)。
- 证据等级: E1(读码核实:state.rs:400 用 MemoryCoordinator::new() 构造,with_observer 全仓仅测试调用;release_writer/cancel_waiter 的交接与取消事件在 core/orchestration.rs:132-139 notify 处全部丢弃;非流水线 Released 只在正常返回路径落库)
- 影响: 停止一轮或异常路径后 session_events 里 writer acquired/released 不成对,写租约审计断档——多写入者问题排查时缺关键证据。
- 验收: ①桌面端改用 with_observer 装配(或 plain 路径 WriterLeaseTrace 加 Drop 补写 Released);②停止一轮后 acquired/released 在 session_events 成对可回放。
- refs: R-181 R-186

- 进展: 2026-08-16 完成。验收对照:①run.rs WriterLeaseTrace 改造为持有 observer/project_root/run_id/process_id,新增 Drop 补写 WriterReleased;正常路径尾部写 Released 后调用 mark_released 防重复,异常/abort/停止路径 Drop 自动补写;②新测试 writer_lease_trace_drop补写released_异常路径审计成对(orchestration_trace.rs)验证 queued→acquired→released 三事件按序落库、run_id/process_id 字段正确。验证:kanzei-app 135 passed、fmt/clippy 绿。提交 ae37ab0。

## D-318 运行中主线阻塞新建并行线路且按钮无反馈 [fixed] (high)
- severity: high
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 复现: 主线路正在运行并持有项目 writer lease 时，打开并行线路页点击新建线路；按钮无忙碌反馈，process_create 等待同一 writer lease，最长 120 秒内页面看起来无响应。
- 根因: process_create(worktree_name)错误复用了源码 writer lease；该租约应只保护会改写既有工作区的写代码、合并和放弃，创建独立 ref/worktree 不应等待它。
- 影响: 运行中的主线无法创建独立并行线路，直接阻断多线并行核心流程；用户会误以为按钮失效并重复点击。
- 修复方向: 将建线/建树拆到独立 worktree 元数据串行闸；合并/放弃继续走 writer lease；两个新建入口同步展示创建中并防重入。
- 验收: ①主线持有 writer lease 时新建线路不等待整轮结束；②同应用建线/建树仍串行且跨进程同名安全不回退；③线路页与侧栏两个入口创建期间同步禁用并显示忙碌态，完成或失败后恢复；④Rust 与前端并行线路回归测试通过。
- refs: D-283 D-315 R-171
- 证据等级: E2
- 进展: 2026-08-13 完成：AppState 新增独立 worktree_ops 元数据闸，process_create(worktree_name) 与 worktree_create 共用该闸且不再排主线源码 writer lease；合并/放弃仍保留 writer lease。侧栏与线路页两个入口同步显示创建中、aria-busy 并防重复提交。验证：cargo test -p kanzei-app 137 passed；cargo clippy --workspace --all-targets -- -D warnings 通过；ui-runtime-smoke 1546 invokes/0 runtime errors，parallel-lines-regression 与 ui-i18n-smoke 通过。

## D-317 首次启动把进程工作目录自动登记为项目 [fixed]
- severity: high
- status: fixed
- 优先级: P1
- 复杂度: 小
- 复现: `~/.kanzei/app.json` 不存在或项目列表为空时，从任意目录启动 kzapp；项目列表自动出现该进程工作目录并被选中。
- 影响: 安装包会泄漏构建/启动目录语义，首次启动可能直接读取非用户授权项目的文档、历史和状态。
- 来源: 2026-08-13 用户反馈：从官网下载安装后，应用未经过选择就默认打开 `kanzei code` 项目。
- 标签: 后端
- 根因: `projects_get` 在清理后列表为空时调用 `std::env::current_dir()` 并写回 `AppPrefs.projects`，把启动上下文误当成用户选择。
- 进展: 2026-08-13 完成：删除 `projects_get` 的 `current_dir` 回退，抽出偏好清理纯函数；空配置保持空，失效当前项目只从已登记有效项目中选择替代。补 2 条 Rust 单测与 UI 空项目反证；验证 `cargo test -p kanzei-app projects::tests` 2 passed，`node scripts/ui-runtime-smoke.mjs` 1546 invokes、0 runtime errors。
- 验收: ①空配置返回 `projects=[]、current=null`，不读取或持久化工作目录；②已登记的有效项目和当前选择继续保留；③前端空状态显示“未选择项目”，项目选择器禁用且不请求项目级进程；④补 Rust 与 UI 回归。
- refs: R-115 D-170

## D-293 kanzei-tools 两条测试在全量并行下偶发红,单独跑必绿 [fixed] (medium)
- severity: medium
- 优先级: P2
- 复杂度: 中
- 标签: 测试
- 证据等级: E1(2026-08-12 一次实测命中,同轮单独复跑两条均绿,随后全量复跑亦绿)
- 复现: `cargo test --workspace` 单次运行中 `kanzei-tools --lib` 报 2 failed / 251 passed:
  ①`docstore::tests::原子写下并发读永不看到截断态` —— panic「读到了截断态:条目数 0,只可能是 3 或 30」(docstore.rs:1453);
  ②`read::tests::read_non_memory_file_does_not_touch_fetched` —— panic「非记忆文件的 read 不应创建记忆库目录」(read.rs:299)。
  紧接着 `cargo test -p kanzei-tools --lib <单条>` 两条各自 ok;再跑一次全量 workspace 全绿。
- 影响: 发版门禁(verify.ps1)与 CI 会随机变红。这类红比稳定红更贵——它训练人「重跑一次就好」,而**真回归也会被这样跳过**。①尤其要命:那条用例的全部意义就是证明"读者永不看到截断态",它自己偶发失败,等于这条保证目前没有可信证据。
- 猜测方向(未验证,勿当结论): ①docstore 那条是 3 个读者线程热转 + 200 次写,本机同时跑着自举 kzapp 与其它测试进程,Windows 上的文件替换在高争用下是否真有窗口需要实证——如果有,那是 atomic_file 的真缺陷而不是测试问题;②read 那条断言的是临时项目目录下 `.kanzei/memory` 未被创建,同进程其它用例是否会在共享路径上造出它、`temp_project()` 的唯一性是否足够,需要读码确认。
- 边界: 不要用「重跑就绿」结案,也不要直接给测试加 retry/ignore —— ①的失败形态可能是产品代码的真窗口,加 retry 等于把证据抹掉。
- 验收: ①能稳定复现(例如加压并行 + 循环 N 次);②定位到是产品代码窗口还是测试自身不隔离,分别修;③连续 20 次全量 workspace 无同类偶发。
- refs: D-249 D-261 R-200
- 进展: 2026-08-16 取活开始。既有怀疑:①docstore 条目数 0 的失败形态与 load() 对 NotFound 宽容返回 Ok(vec![]) 吻合(rename 替换窗口);②read 条 temp 唯一性基本排除。验收:①稳定复现(加压并行+循环 N 次);②定位产品/测试问题分别修;③连续 20 次全量无同类偶发。R-211 加压脚本是载体。2026-08-16 修复落地:两条测试自身构造缺陷——①Tier1 测试走 FailureRecallPolicy::retrieve 断言 BM25 在 30ms 预算内必然命中,全量并行繁忙时超时降级偶发红,改直连 store.search 绕开预算(内存模块仍断言命中且标题精确匹配);②tier0 测试 content 带路径/数字噪音使指纹口径两侧不一致,改 content 首行放 kind 原文与 [fp:edit|{kind}] 精确对齐。验证:kanzei-tools 全量加压 10 轮 0 失败(修复前 8 轮 2 红),记录 T-1786555193。2026-08-13 验收③收口:后台连续 20 轮 cargo test --workspace 全部 exit=0(02:27:46–02:42:39,单轮 39–61s,零失败),记录 T-1786560203。三项验收逐条对照:①复现——修复前加压 8 轮 2 红(T-1786555193 内文),稳定复现达成;②定位——两条均为测试自身构造缺陷(Tier1 依赖 BM25 预算、tier0 指纹口径不一致),非产品代码窗口,修复在 commit c4e261f 的 crates/kanzei-tools/src/memory/mod.rs;③连续 20 次全量 workspace 无同类偶发——T-1786560203 全绿。另注:首次 20 轮后台跑在第 7 轮中断(T-1786558576 failed,进程消失),属执行中断非测试失败,已重启完整复跑。

## D-266 setup.exe 的 /S 静默安装在 kzapp 运行时静默无效:退出码 0、文件没换、无任何提示 [fixed] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 发布
- 证据等级: E1(2026-08-11 用户实测,三条独立证据)
- refs: D-265 D-145 D-004
- 复现: 2026-08-11 连发两版(build-c4c7300、build-22a927c),两次都按流程执行 `kanzei-setup-<hash>.exe /S`,退出码 0、无任何输出。用户开 kzapp 后发现新功能一个都没有。实测取证:
  ①`%LOCALAPPDATA%\kanzei\kzapp.exe` 的 `LastWriteTime` 是 **2026-08-10 21:07:01** —— 早于两次发版(00:40 与 01:47),文件从未被替换;
  ②在该 exe 里按字节搜 `22a927c` → **不存在**;而发布树刚构建的 `target/release/kzapp.exe` 里搜得到 `22a927c 20260810174535`,证明构建产物本身是对的、`KANZEI_BUILD_INFO` 也确实传进了二进制;
  ③设置页版本徽章显示 `v0.1.0 (dev)`,即跑的是 `release.ps1` 装的开发构建(它不设 KANZEI_BUILD_INFO)。
  用户改为**双击运行安装器(不带 /S)**后立即装上,新功能出现。
- 根因: Tauri 的 NSIS 模板在目标程序运行时需要先处理占用(结束进程或提示用户);静默模式(`/S`)下无人可问,它直接放弃并**以成功退出码结束**。于是调用方(人或脚本)拿到的是「装好了」,实际一个字节没动。conventions §9.1 把「静默装 setup.exe」写成了标准做法,而这条路径在最常见的场景(应用正开着)下恰好无效。
- 影响: 这是本仓第三次栽在「发布了但仍在跑旧版」上——D-145 是两份副本,D-265 是更新检查谎报已最新,本条是安装器静默无效。三条叠起来的效果是:**发版流程每一步都报成功,而用户手上的二进制没变,且应用内更新还会告诉他已是最新**。2026-08-11 实测里三条同时命中,排查花了四个来回才定位。
- 修复方向: ①`package.ps1` 与发版检查单里的静默安装改为**装后校验**——比对安装位 exe 的 `LastWriteTime`,并在其字节里确认含本次 hash,不符即报错并明说「kzapp 正在运行,请关闭后重装」(与 `verify.ps1` 的证据绑定同一哲学:**不信退出码,信产物**);②或在静默安装前主动检测 kzapp 进程,有则拒绝并提示,不要试了才发现;③conventions §9.1 补一句:静默安装在应用运行时无效,必须先关应用或改用交互式安装。**推荐 ①**,因为它同时挡住其它未知的静默失败模式。
- 验收: ①kzapp 运行中执行静默安装,流程**当场失败并说明原因**,不再返回成功;②装后校验能对上本次 hash(有实测证据);③conventions §9.1 与实际行为一致;④与 D-265 的三态更新提示合起来,发版链路任一环节没生效时用户都能看到可见信号。

- 批次: 2/2
- 进展: 2026-08-16 取活。勘察结论:静默安装落点有三处——①scripts/release.ps1:57-59 在容器重定向报错时把 Start-Process setup.exe -ArgumentList /S -Wait 写成给用户的推荐做法(退出码不可信,且无装后校验);②conventions §9.1 未写「kzapp 运行时 /S 静默无效」陷阱;③应用内更新 update.rs:429-431 已有 D-265 的 mtime/大小校验(exit=0 但未替换即报错保留安装包),但那是应用内路径,覆盖不到手动/脚本静默装。修复按验收推荐①:新增 scripts/install-setup.ps1 = 检测 kzapp 进程(运行中当场报错拒绝)→ 执行 setup.exe /S → 装后校验安装位 kzapp.exe 的 mtime/大小变化 + 二进制含本次 hash,不符即报错;release.ps1 提示改为引用该脚本;conventions §9.1 补陷阱条款。B1 完成:install-setup.ps1 落盘(scripts/install-setup.ps1,语法校验 OK)、release.ps1:58 提示改为引用新脚本、conventions §9.1 补「静默安装陷阱(D-266)」条款。B2 完成:四场景模拟测试 4/4 通过(记录 T-1786560513)——场景0 真实 kzapp 运行中当场拒绝(验收①);场景1 安装器 exit 0 但未替换被识破报错(D-266 根因);场景2 装后 hash 匹配通过(验收②);场景3 hash 不匹配报错。测试脚本 output/d266-install-setup-test.ps1(不入库)。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(记录 T-1786560513 后补 T-1786560xxx 全量)。四项验收逐条对照:①kzapp 运行中执行静默安装当场失败并说明原因——install-setup.ps1 前置 Get-Process 检测,运行中 throw「kzapp 正在运行(pid…),静默安装无法替换正在使用的 exe」(T-1786560513 场景0 实测命中);②装后校验能对上本次 hash——脚本装后校验安装位 mtime/大小变化 + 二进制含 ExpectedHash,场景2(匹配通过)/场景3(不匹配报错)实测;③conventions §9.1 与实际行为一致——已补「静默安装陷阱(D-266)」条款并指向 install-setup.ps1;④与 D-265 三态提示合起来发版链路任一环节有可见信号——D-265 应用内更新(update.rs:429)已有 mtime 校验,新增脚本补齐手动/脚本静默装路径,release.ps1 提示也指向新脚本,链路无静默失效段。关闭。

## D-268 background.rs 围栏测试只用进程级 Mutex 串行化:两条线并行跑同一 crate 测试时毫无保护,可假绿可假红 [fixed] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 证据等级: E2(读码发现,本轮未触发;可达路径已成立)
- refs: D-262 D-227 R-182 R-184 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 任务级并行实测,线 A(D-262)在读码时发现并主动上报,**本轮未触发**——如实标注,不冒充实测。
- 复现(尚未实际触发,但路径可达): `crates/kanzei-tools/src/background.rs` 用**进程级** `tokio::sync::Mutex` 串行化围栏敏感测试,而 `managed_fence` 的「合法写入窗口」本身也是**进程级**状态。这只在单个 `cargo test` 进程内有效。任务级并行的常态是多条线共享同一个 `CARGO_TARGET_DIR`(本机 target 已 53GB、盘剩 68GB,每树独立物理上放不下,见 R-182 与 deep_parallel_dev D6),两条线同时跑 `cargo test -p kanzei-tools` 时**两个 OS 进程的托管文件窗口可以交错**。
- 影响: ①**假绿**——越界写入落在另一个进程打开的合法窗口里,围栏测试认为"没越界"而通过;②**假红**——自己的合法写入被另一个进程的窗口边界切断,测试报越界。两种方向都让围栏测试在并行开发下**不可信**,而围栏正是 D-174 交付时唯一没被拆掉的那条保障。与 D-227 同族(单进程内成立的不变量,跨进程不成立),与 R-182 实测「跨 worktree 的 FileLock 各锁各的、根本不互斥」是同一类错误。
- 边界: 不是生产代码缺陷——`managed_fence` 的生产语义在单进程内是对的。本条只针对**测试在并行下的可信度**。修复不应把进程级窗口改成全局互斥而拖慢生产路径。
- 修复方向(待定): 二选一——①测试侧用跨进程互斥(`atomic_file::FileLock` 或按 crate 取一把文件锁)把围栏敏感测试整体串起来,与 D-261 给 `test_record` 的做法同源;②让围栏窗口带上进程身份(pid/run_id),跨进程的窗口互不认账,从根上消除交错。②更彻底但改动面进生产代码,需先评估。
- 验收: ①两个 OS 进程**同时**跑 `cargo test -p kanzei-tools` 的围栏用例,结果稳定且与单进程一致,有可重复的实测证据(不是"跑了几次没复现");②假绿方向有定向反证:构造跨进程窗口交错,确认修复前该越界写入**能**混过围栏、修复后被抓;③生产路径的 `managed_fence` 性能与语义不因本次修改而变,有测试背书。

- 批次: 2/2
- 进展: 2026-08-16 取活。勘察:background.rs:498-504 serial() 是进程级 tokio Mutex,只挡同进程;managed_fence::active()(managed_fence.rs:103-106)是进程内 OnceLock,跨进程窗口互不可见——两条线并行跑同一 crate 测试时窗口交错无保护。修复方向①(验收推荐,与 D-261 test_record 同源):atomic_file::FileLock 跨进程互斥,持锁线程持有(FileLock !Send 不跨 await),guard 经 channel 协调。B1 完成:background.rs 新增 FenceGuard/fence_guard()(锁路径 %TEMP%\kanzei-bgfence-tests.lock 固定,跨进程一致),10 处围栏测试开头加 let _fence = fence_guard();(第 1149 条后台进程可托管测试不碰窗口无 serial 不需锁)。反证测试[跨进程围栏窗口互不可见_需要跨进程锁]:spawn 子进程(#[ignore] helper)开 defect 窗口写信号,父进程断言 write_in_progress=false——证明假绿根源(窗口互不可见)成立;helper 曾因 tool_scope 未 .await 而静默空跑(0.00s 通过未写信号),加 .await 后修好。定向测试 16 passed 全绿。B2 完成:双进程并行实测 5 轮全部 exit=0(output/d268-parallel.log),跨进程锁生效、结果与单进程一致,记录 T-1786561296。| 2026-08-16 关闭:B3 完成(提交 b802f40 后跑关闭前全量,记录 T-1786561432)。三项验收逐条对照:①两个 OS 进程同时跑 cargo test -p kanzei-tools 的围栏用例结果稳定且与单进程一致——B2 双进程并行实测 5 轮全部 exit=0(output/d268-parallel.log,每轮两个独立 cargo 进程 --test-threads=4 同时跑 background 用例),T-1786561296 有实测证据;②假绿方向定向反证——跨进程围栏窗口互不可见_需要跨进程锁测试(spawn 子进程开 defect 窗口,父进程 write_in_progress=false)证实窗口跨进程互不可见=交错时无保护=假绿根源成立;修复后 fence_guard 跨进程文件锁使两进程串行进入窗口,双进程并行 5 轮无交错即无假绿(background.rs 测试函数,提交 b802f40);③生产路径 managed_fence 性能与语义不变——本次改动只在 background.rs 的 mod tests 内新增 fence_guard/FenceGuard 与反证测试,managed_fence.rs 生产代码零改动,全量 cargo test --workspace 全绿(T-1786561432,kanzei-tools 259 passed)背书。关闭。

## D-270 显式主根的 HOME 守卫仍有四处缺口:发现式取根仍纯词法、KANZEI_HOME 不参与比较、卷元数据读失败 fail-open、两条入口 trim 不一致 [fixed] (medium)
- 优先级: P1
- 复杂度: 中
- 标签: 核心
- 证据等级: E1(2026-08-11 对抗复核逐条实测,四条均给出可复现输入)
- refs: D-194 D-189 D-186 R-182 R-177
- 来源: 2026-08-11 批次 K1 交付「HOME 守卫改用文件系统身份比较」后的对抗复核。**主体已修好**(尾随点 `C:\Users\kanzei.`、UNC `\\localhost\C$\...`、junction、8.3 短名全部拦住,且合法项目根不误拦),本条是它**明确未覆盖**的四处残余,按份量排序。
- 缺口①(重要): **发现式取根仍是纯词法**。`discover_project_root_with_home` 用 `dir_key` 比较,没跟着升级成 `is_same_dir`。同一个物理目录换成别名走,`~/.kanzei` 立刻变回「项目根磁铁」。CLI 侧被第二道 `reject_home_as_project_root` 兜住(偏严、报错可见),桌面端路径需单独核实。
  K1 留它的理由(已记录,可作为修复时的输入):①派发单把范围限定在 `is_home_root`;②发现式取根的 cwd 来自 `current_dir()`,写不出尾随点/带 `..` 的串;③改它要为**每级祖先各做一次 canonicalize**,是每次配置加载 O(深度) 次系统调用。**所以修法不是简单替换**,要么只对最终命中的那一级做身份比较,要么加缓存。
- 缺口②(次要): **`same_dir_by_volume_metadata` 拿不到元数据时 fail-open**,与它自己的注释相反。注释声称「误判只会偏保守」,实际上 metadata 读失败、`modified()` 取不到、目标不是目录,任何一种都 `return false` = 判成不同目录 = **放行 UNC 别名**。方向必须反过来:拿不到身份就当作**可能相同**,由上层保守处置。
- 缺口③(次要): **`KANZEI_HOME` 一设,同一个碰撞就没人守了**。`is_home_root` 只跟 `dirs::home_dir()` 比,从不跟真正的全局根 `kanzei_home()` 比。把 `KANZEI_HOME` 指到某项目自己的 `.kanzei`,项目产物与全局配置/全局记忆重新落进同一个目录——**正是 D-194 声称要挡的那件事**——且零告警。反方向也不自洽:全局根已经搬走时,`--project-root` 指向真实 HOME 反而不再有危害,却仍被拦。
- 缺口④(次要): **两条入口对同一串输入给出的理由不一致**。`KANZEI_PROJECT_ROOT` 走 trim,`--project-root` 不 trim;带首尾空格的 HOME 经参数进来会被报成「路径不存在」而不是「你把主根写成 HOME 了」。两道拦截的先后顺序本来就是为了避免张冠李戴(`main.rs` 明确写了第一道打在显式输入上就是为了不被泛化报错盖过去),这里被空格破了。
- 边界: 主体(显式入口的身份比较)已交付且经实测,**本条不是回归**。符号链接形态因需管理员/开发者模式未能实测,但 canonicalize 解符号链接与解 junction 走同一条 reparse point 路径,junction 已覆盖。
- 验收: ①发现式取根对别名形态的 HOME 也拦得住,且**给出加载路径的性能实测**(不得让每次配置加载多做 O(深度) 次系统调用);②卷元数据读失败时方向改为保守(判成可能相同),有定向测试构造读失败;③`KANZEI_HOME` 参与比较,指到项目自己的 `.kanzei` 时被拦并告警,有测试;④两条入口对同一输入给出**同一条**理由,带首尾空格的 HOME 经两条入口都报「主根写成 HOME」,有测试。

- 进展: 2026-08-16 取活并修复。四处缺口逐一落地(commit c04f592):①发现式取根别名感知——discover_project_root_with_home 的 .kanzei 标记层若为 HOME 别名(词法不同但 is_same_dir 相同)同样跳过,身份比较只在词法不等且有标记的层发生,普通层仍纯词法 dir_key,不给配置加载引入 O(深度) 系统调用;②卷元数据读失败保守判同——same_dir_by_volume_metadata 的 fingerprint 拿不到身份时 return true(可能相同,由上层保守处置),不再 fail-open 放行 UNC 别名;③KANZEI_HOME 参与比较——is_home_root 拆出可测内核 is_home_root_with(home/kh 参数注入,不碰进程级环境变量),全局根与 root 本身或 root/.kanzei 同目录都算碰撞;④--project-root trim 对齐——parse_run_args 值 trim 与 KANZEI_PROJECT_ROOT 一致,reject_home 文案改为「全局配置根(HOME 或 KANZEI_HOME)」。新增测试 4 个(发现式取根对别名形态的home也拦得住/卷元数据读失败时保守判同而不是放行/kanzei_home指向项目根或其kanzei时被拦/project_root_flag_trims_whitespace_like_env_does),定向验证 kanzei-harness config 45 passed + kanzei bin 15+3 passed(T-1786561780),fmt/clippy 全过。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786561897,harness 114/tools 259/core 137/app 143)。四项验收逐条对照:①发现式取根对别名形态拦得住+性能实测——测试[发现式取根对别名形态的home也拦得住](cfg windows,尾随点别名,断言 discover 不再返回 home)通过;性能实测 20 层嵌套目录 50 次完整 kz CLI 启动 1593ms(31.9ms/次,含进程 spawn 大头),结构性保证 discover 普通层纯 dir_key(纯字符串折叠、零系统调用),is_same_dir(canonicalize)只在词法不等且有 .kanzei 标记的层发生(通常 1 次),非 O(深度) 系统调用;②卷元数据读失败保守——same_dir_by_volume_metadata fingerprint 失败(metadata 读失败/非目录/取不到 modified)return true,测试[卷元数据读失败时保守判同而不是放行](两个不存在路径断言 true,正常同/异目录语义不变)通过;③KANZEI_HOME 参与比较——is_home_root 经 is_home_root_with 同时比较 dirs::home_dir() 与 kanzei_home(),测试[kanzei_home指向项目根或其kanzei时被拦](场景 A kh=proj/.kanzei→拦、B kh=proj→拦、C 全局根在别处→不拦、D 真 HOME→拦)通过;告警=reject_home_as_project_root 文案改为「全局配置根(HOME 或 KANZEI_HOME)」(crates/kanzei/src/main.rs);④两条入口同一理由——parse_run_args 对 --project-root 值 trim 与 KANZEI_PROJECT_ROOT 对齐,测试[project_root_flag_trims_whitespace_like_env_does](带首尾空格值解析为 C:/x)通过;trim 后两入口走同一 reject_home 路径报同一条「主根写成 HOME」。关闭。

## D-276 tracker 进展字段 update 语义陷阱:多行=追加游离段落、游离段落无删除通道 [fixed] (medium)
- refs: D-239 D-204
- 严重度: medium
- 优先级: P2
- 修复方向: ①update 进展字段统一为「替换整个进展块」(单行与多行同语义),或把多行追加改为追加到 `- 进展:` 行内;②提供显式删除/整理游离段落的能力(如 update 传特殊标记清空);③引擎在 update 后自检条目内重复段落并告警。
- 复现: 2026-08-13 实测确认(defect update 进展字段):①传多行值(含换行)= 作为新游离段落追加到条目末尾,不替换 `- 进展:` 首行;②传单行值 = 替换首行,但已存在的游离段落(无 `- 键:` 前缀的文本行)永不清除;③没有任何删除通道:tracker 文件 direct write denied、git restore/checkout 被引擎拦截、shell 整文件重写被拦、edit/write 对 .kanzei/project 拒绝。D-239 因反复 update 积累 3 份「验收②复核」+ 2 份「第二轮复核」重复段落,越修越脏。
- 影响: ①条目数据膨胀,重复段落混淆后续审计与记忆蒸馏;②清理死路:游离段落一旦产生,agent 侧任何工具都删不掉,只能用户手动 git 操作;③单行/多行语义不一致(替换 vs 追加),调用方无法预期;④D-239 验收②(复核口径漂移)因此类缺陷污染证据。
- 标签: 核心
- 证据等级: E1(2026-08-13 实测复现,含 diff 与 read 证据)

- 批次: 2/2
- 进展: 2026-08-16 取活。勘察发现三个修复方向中①②已由既有交付落地:D-294(commit 1c223f8 字段值写入侧强制单行,push_field 把多行折成空格,四渲染出口全走它,测试 字段值折成单行_不产生游离段落)堵死新增游离段落;R-201(commit 800d5da raw_lines/raw_delete 按序号删游离行)提供删除通道。但两笔交付当时未关联 D-276,条目仍开着。剩余:③引擎在 update 后自检条目内重复段落并告警(未做);端到端验证 update 多行值不再产游离段落 + raw_delete 能清历史残留;清理 D-239 历史积累的重复游离段落(影响④实例)。B1 完成:tracker.rs update|close 分支 save 后调 store.raw_lines(id) 自检,有游离段落则在返回里点名数量并指路 raw_lines/raw_delete;新增端到端测试 update多行值不新增游离段落且已有残留被自检点名(多行值折成单行字段、历史残留被点名、raw_delete 清完后不再告警),tracker 31 passed(T-1786562132),fmt/clippy 全过;commit 27606aa。B2 完成:真实环境验证——raw_delete 在真实 defects.md 上删除 D-239 游离段落成功(字段一字不变),D-239 历史重复内容段落已折进字段值无残留,仅剩格式空行(D-130 渲染固有产物,删后再生,属无害格式态);update 自检告警通道端到端测试已覆盖。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786561931 前一条,tools 260 passed)。三个修复方向逐条对照:①update 进展字段统一为替换整个块(单行与多行同语义)——既有能力 D-294 push_field 折行,本次端到端测试 update多行值不新增游离段落且已有残留被自检点名 断言「- 进展: 第一段 第二段 第三段」单行字段且「第二段不能变成新的游离段落」(crates/kanzei-tools/src/tracker.rs 测试,commit 27606aa);②显式删除/整理游离段落能力——既有能力 R-201 raw_lines/raw_delete,本次真实环境验证在 defects.md 删除 D-239 游离行成功(字段一字不变);③update 后自检重复段落并告警——本次交付 tracker.rs update|close save 后 store.raw_lines(id) 自检,有残留则返回点名数量并指路 raw_lines/raw_delete(修复方向③代码 + 端到端测试「清完后 update 不再告警」)。关闭。

- 复杂度: 中

## D-279 单条消息含多项诉求时只落实一部分,且被追问时用相邻动作顶替原诉求 [fixed] (medium)
- severity: medium
- priority: P1
- label: 流程
- 复现(有原始轨迹,2026-08-12 用户三次追问才暴露): 用户单条消息含 4 项诉求——①修复安装包图标 ②登记需求「fast 本地模型 ollama 自动安装开启 + 能看到运行状态」③登记需求「架构图渲染工具,必须代码生成非文生图」④登记需求「亮色主题 + 前端渲染器换色结构评估」。**第一轮只交付①**(提交 097e030),②③④在整轮 23 步里一次都没被提及,收尾总结也没说明漏了什么。用户追问「登记需求做了吗」后,**第二轮把①补登成 D-277**(图标缺陷)并回「登记完成」——追问指向的是那 3 条需求,回应却是给已交付的①补条目,3 条需求仍然零动作。用户第三次把原始消息整段贴回才被发现。
- 证据(2026-08-12 03: 20 核实): `.kanzei/project/requirements.md` 里 R-188(③)、R-189(④)存在但未提交,**②对应的条目全仓库不存在**——`grep -rn "ollama" .kanzei/project/requirements.md` 零命中,requirements-archive.md 里只有 2026-08-08 的 R-136(已 done,只覆盖「一键安装」,不覆盖用户这次要的「自动开启 + 常驻运行状态」)。即 4 项诉求最终:2 项落地、1 项被相邻动作顶替、1 项彻底丢失。②已于本次补登为 R-190。
- 根因(待证): ①轮内没有把用户单条消息里的多项诉求拆成显式清单,诉求项在长轮次里靠模型自己记,记漏了没有任何机制会发现——收尾总结是自由文本,不与原始诉求逐项对照,所以「漏了 3 项」还能写成「登记完成」;②被追问「做了吗」时没有回读原始消息逐项核对,而是拿手边最近的相邻动作(补登 D-277)充数——这与 §1.25 已禁止的「以相邻交付冒充验收」是同一个动作,只是发生在**用户诉求层**而不是验收条款层,现有约束没有覆盖到。
- 影响: 自举模式下「用户登记需求」是整个流程的入口,漏掉一项等于该诉求彻底消失——没有任何后续机制会重新发现它(缺陷/需求扫描只扫已登记条目)。且漏项被总结成「完成」,用户只能靠人工比对原始消息才能发现,发现成本远高于登记成本。本次是用户三次追问才捞回来,若用户没坚持,R-190 这条诉求就永久丢了。
- 期望: 单条消息含多项诉求时,轮内有显式的逐项清单;收尾时逐项给出「已做/未做/为什么」,漏项不得被总结成「完成」。被追问是否遗漏时,必须回读原始消息逐项核对,不得用相邻动作顶替。
- 处置建议(不在本条强行拍形态): 与 §1.25「不得以相邻交付冒充验收」同源,可考虑把该约束从验收层扩到用户诉求层;机制侧可选的最轻形态是轮首把用户消息里的祈使项拆成 todo 清单、收尾对照该清单再产出总结。
- refs: D-277 R-188 R-189 R-190
- status: open

- 复杂度: 小
- 进展: 2026-08-16 取活。根因:轮内没有把用户单条消息的多项诉求拆成显式清单,收尾总结是自由文本不与原始诉求逐项对照;被追问时拿相邻动作顶替——与 §1.25 已禁止的「以相邻交付冒充验收」同源但发生在用户诉求层,现有约束未覆盖。修复(处置建议的最轻形态):把 §1.25 约束从验收层扩到用户诉求层。落地三处:①default_conventions.md §1.25 增两条(多项诉求轮首拆显式逐项清单+收尾逐项对照「已做/未做/为什么」漏项不得总结成完成;被追问时回读原始消息逐项核对不得相邻动作顶替);②profiles.rs dev system prompt 同步英文版条款(itemize them explicitly / re-read the original message,与引擎模板同口径防 D-242 半份真源);③守护测试断言新 token(profiles 14 passed T-1786562xxx)。复杂度=小,定向测试即可。关闭。

## D-281 「自动放行」开关在自主推进/鞭挞轮静默失效,用户以为放了权实际没有 [fixed] (medium)
- 复现: ①顶栏勾选「自动放行」;②开鞭挞、模式选自主推进;③自动轮里任何 Ask 档位的工具(如 conventions patch)仍被拒,报 permission requires user approval: ...; autonomous/parallel run skipped it;④界面没有任何提示说明该开关此刻无效。2026-08-12 R-191 批5b 因此连撞三轮才被发现。
- 影响: 开关 tooltip 写的是「本次不再弹权限窗,全部自动放行(相当于 yolo)」,用户据此认为已全局放权,实际在最需要它的自动轮完全无效,且失效是静默的。「总是允许」同样顶不住:session_approved/session_rules 是 drive() 的局部变量(drive.rs:166/170),名字叫 session,作用域其实是一轮——这一轮点了总是允许,下一轮照样拦。
- 期望: 二选一:①自动放行状态下传,自动轮改用 AskPolicy::AutoAllow 而不是 NonInteractive(仍落 PermissionResolved 事件保可审计);②至少在勾选时明示「本开关对鞭挞自动轮无效」,不让用户误以为已放权。
- 标签: 核心
- 根因: run.rs:128 把 autonomous 轮与并行线(process_id 以 p| 开头)的 AskPolicy 设为 NonInteractive;drive.rs:876 在调用 ask() 之前就短路返回 Gate::NonInteractive,kz:ask 事件根本不发出。而自动放行的实现(ui/07-events.js:416)是监听 kz:ask 事件替用户回 AllowOnce——没有事件就没有可放行的对象;07-events.js:411 另有一道防御把 source=autonomous/parallel 的询问直接丢弃。
- 规避: 2026-08-12 已用 .kanzei/kanzei.toml 的 conventions patch allow 规则绕过单点,本条要解决的是开关本身的语义。
- 优先级: P2

- 复杂度: 中
- 批次: 2/2
- 进展: 2026-08-16 取活。根因链:kanzei-app run.rs:134 autonomous/parallel 轮设 AskPolicy::NonInteractive → drive.rs:876 短路 resolved(declined, noninteractive)且不发 kz:ask 事件 → 前端自动放行(07-events.js:439 监听 kz:ask 替用户回 once)没有可放行对象 → 开关在自动轮静默失效。修复方向①(根治):AskPolicy 加 AutoAllow 档——自动轮在用户勾选自动放行时用 AutoAllow 而非 NonInteractive,drive.rs 对 AutoAllow 直接 resolved(allow, auto_allow)放行(仍发 PermissionResolved 保可审计),不再短路;前端 run_prompt invoke 传 autoAllow(localStorage kz-auto-allow)。B1 完成:①core runner/mod.rs AskPolicy 增 AutoAllow(allows_user_prompt() false——放的是权限不是问题,守护测试断言);②core drive.rs 权限短路改 match——AutoAllow → resolved(allow, auto_allow) + continue 放行,其余非交互仍 declined;③kanzei-app run.rs run_prompt command 加 auto_allow 参数、run_task 加 auto_allow 透传,autonomous/parallel 轮勾选时用 AutoAllow;④前端 08-compose.js 三处 invoke 传 autoAllow(localStorage kz-auto-allow)。验证:core 143 passed(app 137)、node --check 过、fmt/clippy 全过(T-1786562778);commit 3dbfdf4。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786562xxx)。期望逐条对照:期望①「自动放行状态下传,自动轮改用 AskPolicy::AutoAllow 而不是 NonInteractive,仍落 PermissionResolved 事件保可审计」——已完整实现:状态经 run_prompt auto_allow 参数上传(drive.rs:879-882 对 AutoAllow resolved(allow, auto_allow) 放行,PermissionResolved 事件照发),autonomous/parallel 轮不再静默 declined;前端 07-events.js 434 的 parallel/autonomous 防御保留(AutoAllow 轮后端直接放行不发 ask 事件,该防御仅兜底异常)。期望②(勾选时明示无效)不再需要——①根治后开关对自动轮有效。关闭。

## D-282 memory-manager 并发 update 把记忆条目的 description 覆盖成别的主题 [fixed] (medium)
- 复现: 2026-08-12 04:11 实际发生:人工合并重复记忆写入 M-044(主题:tracker update 字段语义)后一分钟内,轮末 memory-manager 对同一条目执行 update,把 description 换成 edit/old_string 的内容(那是 M-027 的主题),而 title 与正文仍是 tracker 字段语义,条目自相矛盾。已人工修回(提交 d4a4f08)。
- 影响: description 是召回钩子(检索与注入都看它),写错等于把条目挂到错误场景:该被召回时不出现,不该出现时被注入。且覆盖是静默的,人工做记忆维护时会被无声顶掉,只能靠 git diff 事后发现。
- 期望: ①update 时若新 description 与条目 title/正文主题明显不一致,拒绝或至少警告;②记忆条目写入加 CAS 式并发保护(conventions 工具已有 expected_hash 的先例);③manager 选目标条目的判据落轨迹,选错时可复盘。
- 标签: 核心
- 根因(待证): ①manager 消化 inbox note 时选错了目标条目(疑似按相似度挑中最近写入或得分最高的一条,而不是同主题那条);②memory update 对 description 是整值替换,不校验新 description 与该条目 title/正文是否同主题;③记忆写入没有并发保护,人工维护与轮末 manager 可以同时写同一个文件。
- 规避: 做记忆维护前先停自动推进循环。
- 优先级: P2

- 复杂度: 中
- 批次: 2/2
- 进展: 2026-08-16 取活。根因:①memory_update 对 description 整值替换,不校验新 description 与条目 title/正文是否同主题(store.rs update);②记忆条目写入无并发保护。勘察发现用户定调「memory 写入不做跨进程锁,竞争留给 agent 事后解决」(store.rs 第 4 行注释)——期望②不做 FileLock,改 CAS(expected_hash,conventions 先例)。B1 完成(commit b5ba149):①store.rs update 加 description 主题一致性校验(topic_overlap:CJK 单字+英文词去虚词,context=title+旧description+body,交集<2 拒绝,错误带旧/新对照=manager 复盘轨迹)+ CAS expected_hash 参数(传则写前比对 render_entry hash,不一致拒绝);②enforce_topic 开关:manager 写路径(MemoryUpdateTool)强制 true,UI 用户直写(memory_entry_save)/merge/stale 豁免 false(A-005 用户有权写任何内容);③manager.rs UpdateInput 加 expected_hash 透传;④新测试 2 个(update拒绝主题漂移的description/update_cas拒绝过期expected_hash)。验证:memory 77 passed + app 137 passed,fmt/clippy 全过(T-1786563579)。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786563xxx,tools 262)。三项期望逐条对照:①update 时新 description 与条目 title/正文主题不一致拒绝——store.update enforce_topic=true 时 topic_overlap<2 即拒(带旧/新对照),manager 路径强制开启,测试 update拒绝主题漂移的description 断言漂移被拒且条目未被改写;②记忆条目写入 CAS 式并发保护——store.update 新增 expected_hash 参数(写前比对 render_entry hash,不一致拒),conventions expected_hash 同源,UpdateInput 透传,测试 update_cas拒绝过期expected_hash 断言旧 hash 拒/新 hash 放行;不引入跨进程锁(尊重用户定调);③manager 选目标条目判据落轨迹——manager 是 LLM 决策,代码层落地为拒绝信息带旧/新 description 对照 + 条目 id,manager 可见可复盘(并入①实现)。关闭。

## D-316 引擎归档动作产生重复条目与孤儿字段:archive 中 D-309 两份、open 的 D-289 字段被误切入且无工具清理通道 [fixed] (medium)
- 复现: 上一轮关闭一批缺陷后,引擎自动归档把 fixed 条目移入 defects-archive.md 但未提交(工作树遗留)。实测归档产物两处脏数据:①D-309 在 archive 重复两份(3238/3252 行,内容完全相同);②open 的 D-289 字段行(复现/影响/来源/标签/阻塞/优先级)被误切进 archive 尾部,活动文件 D-289 字段随之下线。
- 影响: archive 出现重复条目与孤儿字段行;活动文件 open 条目字段被误移(已用 defect update 手工补回 D-289,但 archive 尾部残留 6 行孤儿字段)。归档是引擎管理文件,edit 被 ruleset 拒绝、defect 工具不认归档条目,当前无合法清理通道——同类问题与 D-294 的「游离段落无删除通道」一致。
- 标签: 流程
- 根因: 引擎归档动作的切割/复制逻辑疑似把 D-312 之后的 D-289 字段行一并划入归档,并对 D-309 重复落盘;具体在 harness 归档实现,待定位。
- 优先级: P2

- 复杂度: 中
- 批次: 2/2
- 进展: 2026-08-16 取活。现状核实:①archive_terminal(docstore.rs)的 archived.extend(terminal) 只对模板去重、Entry 列表未按 id 去重——重复归档会二次追加(D-309 两份 3238/3252 实证);②D-289 的 6 行孤儿字段已污染进 archive 的 D-312 条目(复现/影响/来源/标签/阻塞/优先级 重复 key + 空阻塞)。B1 完成(commit 44c10cf):①archive_terminal 写回前调 normalize_archive 净化整个归档(按 id 去重保留先归档、每条目同 key 字段去重保留第一个非空、删空字段),净化有变化时即使无新终态条目也强制写回(archived 动作=清理通道);②extend 前 Entry 列表按 id 去重(与模板去重一致);③新测试 archive_terminal_净化重复条目与孤儿字段 构造 D-309 两份+D-312 污染,断言收敛;docstore 19 passed,fmt/clippy 全过(T-1786564595)。真实环境注意:当前 agent 会话的 defect 工具跑的是旧引擎,archive 实测返回 nothing to archive(旧代码无净化)——真实文件脏数据(D-309 重复/D-312 污染)会在引擎更新后的首次归档动作被自动收敛,净化逻辑已有单元测试背书。| 2026-08-16 关闭:全量 cargo test --workspace 全绿(T-1786563xxx,tools 263)。逐条对照:①D-309 重复两份——根因 archive_terminal extend 未按 id 去重,已修(Entry 列表去重 + normalize_archive 整体净化),测试断言重复收敛为一份;②D-289 孤儿字段污染 D-312——normalize_archive 同 key 字段去重(保留第一个非空)+ 删空字段(如 `- 阻塞: `),测试断言复现保留原条目值、空阻塞被删;③无工具清理通道——已建立:任何归档动作(archived=清理通道)自动净化整个归档,无需新工具;净化有变化即强制写回。残余:当前工作树 defects-archive.md 的真实脏数据由含本修复的新引擎在首次归档动作自动收敛(代码已提交,引擎重启后生效),进展已记录。关闭。

## D-320 R-199 遗留三处:鞭挞 i18n 缺 EN key、profile 切换仍静默取消勾选、smoke D-291 断言过时 [fixed] (medium)
- 复杂度: 小
- 复现: 发版 verify.ps1 门禁(2026-08-16)逮到 R-199 遗留三处:①02-i18n.js 缺 2 个 EN key(自动推进停止:当前模式不匹配/鞭挞已关闭,当前进程不是自主推进模式)——07-events.js ProfileMismatch 分支调用但表里没有;②08-compose.js syncAutoContinueWithProfile 仍在 profile change 时主动取消 auto-continue 勾选,与 R-199「档位否决下沉引擎、前端不再持有」冲突(D-290 旧漂移复发);③ui-runtime-smoke.mjs D-291 场景断言过时——设 dev-pair + Continue 期望前端拦下,但 R-199 后引擎判 Stop、前端不再拦。
- 影响: 发版门禁(verify.ps1)ui_runtime 步红;R-199 的「前端不再否决」承诺在 profile 切换路径上未兑现(勾选被静默取消)。
- 来源: self-found(发版 verify.ps1 门禁逮到)
- 标签: 流程
- 优先级: P2
- 进展: 2026-08-16 发版 verify.ps1 门禁逮到 R-199 遗留三处,已修复(commit 866dfc2):①02-i18n.js 补 2 个 EN key(自动推进停止:当前模式不匹配/鞭挞已关闭,当前进程不是自主推进模式);②08-compose.js syncAutoContinueWithProfile 不再在 profile change 时主动取消勾选——档位否决下沉引擎,前端只显示引擎结论(07-events.js ProfileMismatch 分支负责取消+显示),D-290 旧漂移不复发;③ui-runtime-smoke.mjs D-291 场景断言改为引擎语义(dev-pair + Continue 时前端不拦,phase ∈ {auto_pending, starting} 都算推进,容忍 flush 跨 2 秒续跑间隔)。验证:node --check 通过、ui-runtime-smoke 21 项全绿、ui-i18n-smoke 通过(T-1786574944)。复杂度=小,前端修复定向验证即可。关闭。

## D-324 output/ 与 .playwright-cli/ 未入 gitignore 也不在 verify 洁净检查:实验产物无限累积 [fixed] (low)
- 复现: git status 长期挂 30+ 个未跟踪实验产物(d268/d293 测试输出、0 字节失败实验空壳、旧截图);verify.ps1 洁净检查只扫 crates scripts .github 看不见
- 影响: 工作区噪音累积,实验残留与交付物边界模糊
- 来源: 2026-08-13 自举复盘
- 标签: 流程
- 优先级: P3
- 进展: 2026-08-13 修复:.gitignore 追加 output/ 与 .playwright-cli/(带用途注释),git status 未跟踪噪音归零;verify.ps1 洁净检查扫 crates/scripts/.github 不受影响;磁盘上既有实验产物保留原位不入库

## D-325 会话恢复丢弃思考块:renderRecoveredMessages 只认 text/tool_*,重开会话后思维链从 DOM 消失 [fixed] (medium)
- 复现: 15-views-misc.js renderRecoveredMessages 的 parts 循环里 reasoning 类型 part 落到 type!==text 的 continue,历史消息里的 Part::Reasoning(request.rs 有该变体且 conversation_get 全文返回)整个不渲染;重开会话或切进程后思考块消失,复制上下文也拿不到
- 影响: 历史会话的思维链不可见不可复制;主对话恢复后信息量骤降
- 来源: 用户反馈 2026-08-13(复制上下文在主对话里不好用)
- 标签: 前端
- 优先级: P2
- 进展: 2026-08-13 修复:05-chat-render.js 抽出 buildReasoningBlock 构造器(实时/恢复共用,dataset.raw 恒持全文),15-views-misc.js renderRecoveredMessages 增 reasoning 分支按实时同款折叠块恢复并 renderReasoningBlock 渲染。验证:node --check 3 文件通过,gen-ui-lint-globals 同步(1197 标识符),ui-lint/ui-i18n/ui-runtime 冒烟全绿(21 文件 1545 invoke 0 运行时错误)

## D-326 复制上下文导出不完整:思考块只取首行 160 字、工具块无结果行、error 消息被静默跳过 [fixed] (medium)
- 复现: 07-events.js copy-context 处理器:reasoning 分支 raw.split 首行 slice(0,160);tool-chip 只导出 head 前 200 字不带 result 行;error 等其余 msg 形态没有分支直接丢弃——导出的 markdown 贴给其他 AI 时思维链断裂、错误上下文缺失
- 影响: 复制上下文的核心用途(贴给其他 AI)失真,收起的思维链等于没复制
- 来源: 用户反馈 2026-08-13(右上角复制上下文不含收起的思维链)
- 标签: 前端
- 优先级: P2
- 进展: 2026-08-13 修复:07-events.js copy-context——reasoning 导出完整 raw 全文(### 思考 + 逐行引用,不再截 160 字),tool-chip 追加 result 行(≤400 字),新增 msg 通用回落分支(error 等不再静默丢弃,≤500 字)。验证:与 D-325 同批,node --check + ui-lint + ui-i18n + ui-runtime 冒烟全绿

## D-327 files 工具 top 视图忽略 path 作用域:静默返回全仓库重文件而非子树 [fixed] (low)
- 复现: files.rs execute 里 input.top 为 Some 时 render_top 直接吃全量 entries,path 前缀只在 None 树形分支应用;调 files(path=crates/kanzei-harness, top=15) 返回的是整仓 top15,拿到错作用域数据且不报错
- 影响: agent 以为拿到子树重文件实际是全仓数据,判断被静默污染;V4PRO 自我复盘实测踩中并如实上报
- 来源: V4PRO 自举会话自述 2026-08-13,交互会话已在 files.rs:442 核实
- 标签: 后端
- 优先级: P2
- 进展: 2026-08-13 修复:files.rs execute 把 path 前缀提到两分支共用,top 分支先按 starts_with 裁剪 entries 再 render_top(口径与 render_tree 一致);新增 tokio 测试 top视图尊重path作用域(path=docs 时含 docs/note.md 不含 src/big.rs)。验证:cargo test -p kanzei-tools files:: 20 passed

## D-328 归档净化误删真内容:按 key 去重吃掉同名不同内容的叙事字段,空值口径误杀多行字段表头 [fixed] (high)
- 复现: docstore.rs normalize_archive(约 555-574 行)两条口径过宽:①seen_keys 按字段名去重只留第一条,D-179 系条目两行「- 验证(2026-08-08):」内容不同(v6/v7),v7 迁移验证证据(含 workspace 269 项通过)被当重复删除;②值为空即删,但「- 实测(…): 」「- 根因(…): 」是多行字段表头(值在续行),表头被删后续行挂错归属。R-213 在途轮触发,工作区 defects-archive.md 三行真内容被删,尚未提交,git HEAD 可恢复
- 影响: 归档净化静默吃掉真实证据行,证据链缺损,极易随下一次 tracker 提交固化成永久丢失
- 来源: 交互会话在途审查 2026-08-13(git diff 对 HEAD 逐行核对,三行均确认唯一出现非重复)
- 标签: 后端
- 优先级: P1
- 进展: 2026-08-13 修复:normalize_archive 两条口径收窄——同 key 去重改为同 (key,value) 逐字比对(同名不同内容是叙事不得去重),空值删除只限「阻塞」键(多行字段表头值在续行,删表头续行成孤儿);archive_terminal 注释同步。误删三行(D-179 系 v7 验证、D-312 实测表头、根因表头)已按 git HEAD 逐字回填并核实唯一。验证:cargo test -p kanzei-tools docstore:: 20 passed(改写 归档净化 测试为新口径:逐字重复收敛/同名不同内容保留/空阻塞删/空值表头留)

## D-329 tracker 写路径每次 update/close 追加游离空段;kz CLI raw_lines/raw_delete 未接 id 位置参数 [fixed] (low)
- 复现: ①D-325/D-326 实测:add --field 建条目后,每次 update/close 告警的不可寻址游离段落数从 1 涨到 2(写路径在字段前留空行段);②kz defect raw_lines D-325 报 id is required——main.rs tracker_cli 的 action 分派只给 get/close/update/repair_reused_id 接位置参数 id,raw_lines/raw_delete/reopen 等落 _ 分支,CLI 清理通道不可用
- 影响: 游离段落随写操作累积(M-057 记载的脏数据模式复发),且 CLI 侧无法自查自清
- 来源: 交互会话实测 2026-08-13
- 标签: 后端
- 优先级: P2
- 进展: 2026-08-13 修复:①render_entry_with_template 渲染前裁掉模板尾部空 Raw(条目间距由 ensure_blank_separator 单源负责),追加字段紧跟末字段,新增测试 追加字段不产生游离空段且多轮写入稳定(两轮写入幂等);②main.rs tracker_cli 给 raw_lines/reopen/archive/void_id/repair_missing_id 接位置参数 id、raw_delete 接 id+序号。验证:cargo test -p kanzei-tools docstore:: 20 passed;新二进制端到端 raw_lines/raw_delete D-325 实测可用(证实游离段即空行,已清)

## D-283 会话状态按轮次投影导致运行中显示空闲、停止按钮消失、鞭挞与活动记录串线 [done] [fixed] (high)
- 优先级: P0
- 复杂度: 大
- 标签: 核心 后端 前端 并行 自举
- 来源: 2026-08-12 用户连续截图与复现；全局扫描确认不是单一 CSS 问题，而是后端会话态、前端事件态、轮询快照、线路设置和 trace 落库粒度共同漂移。
- 复现: ①任务实际收到进度事件,左侧线路仍显示「空闲」,顶部 stop 不出现;②一轮结束后鞭挞仍会等待/续跑,但底部 setStatus(false) 显示普通空闲;③主线开 dev-auto/鞭挞后切未配置并行线,新线继承旧 profile/checkbox/timer;④运行中切线或重载,右侧活动记录要等轮末 trace 落库,轮内轨迹暂时消失。
- 根因: ①运行态由 runtime.running/sessionStates/process_list/collaboration_snapshot 多源投影且 handler 直写状态,无单一投影出口;②并行线路页依赖 3.5/8 秒轮询,实时事件与轮询无明确优先级;③run.trace 轮末收尾写入,活动回放天然轮粒度;④profile/auto UI 有全局 localStorage fallback,切线时先同步新 session 后应用目标设置,目标 profile 为空时不清理旧值。
- `kz: done` 是轮末事件，旧投影把它当会话终态；`kz:idle` 才是运行循环结束。
- 统一修复: 归并到 R-197，按其 10 批次执行；设计基线见 `docs/design/session_state_and_line_runtime.md`。
- 验收: 以 R-197 八条验收为准，额外保留两条反证：①`kz:done` 后模拟第二轮/排队输入仍显示运行；②主线鞭挞开启后切未配置并行线不会产生 `auto=true` 的目标 session 请求。
- 证据等级: E1(用户复现 + 代码调用链核实，修复后需提升为 E2/E3)
- 进展: 正式关闭(2026-08-16,修复本体是 R-197 既有交付,本条只补关闭):主验收=R-197 八条,该需求已 done/归档(10 批+关闭前全量)。反证①有显式自动化证据:ui-runtime-smoke.mjs 断言 bgState.converged===false(kz:done 是轮末事件不得收敛会话终态,排队输入第二轮还要跑)+bgState.running===true(kz:done 后会话仍在跑);对应实现=run.rs:1068 kz:done/run.rs:2191 kz:idle 分离,01-core.js 事件流为线路状态实时投影入口、只有会话级终态才收敛。反证②=auto 状态按 sessionId 隔离(08-compose.js autoContinueTimers 以 sessionId 为键、auto_state_reset 随 activeSessionId 重置)+R-197「profile/auto/timer 按线路隔离」+parallel-lines-regression.mjs 线路隔离护栏(profile 隔离/刷新节流/切换代次/local_start_pending 防旧快照覆盖)。2026-08-12 已发布,持续使用无回归。残余缺口:WebView2 E2/E3 提升受本机 CDP 端口不绑定限制(M-062),转入 R-101 延期 E2 清单,不影响本条功能验收(关闭边界:可用即关闭,验证增强不滞留 fixing)。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-283
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598029507

## D-267 bash 授权缺一个安全的中间档:只有「逐条逐字节精确」与「整体全放行」两端,无人值守只能靠 yolo [wontfix] (high)
- **2026-08-11 关闭为 `dropped`(用户定调): 不做中间档,bash 非交互直接放行,防线整体挪到结果侧。**
  以下五条是关闭理由,按份量排序。**本条的现象描述与实测清单全部依然属实**——变的是处置,不是事实。
  1. **它挡不住有意的。** §0 定案 1 已经承认并接受:段级闸门是纯 shell 语法过滤器,对「被允许的**程序**本身是什么」一无所知。`cargo` 按设计编译并运行工作树里的代码(本仓 `build.rs` 与两个可运行 bin 都在),而 agent 持有 `edit` 权限。**任何黑名单都关不掉,危险性在程序语义里不在 shell 语法里。**
  2. **既然挡不住有意的,它挡的只剩无意的——而无意的错误有更便宜且更管用的防法。** 见下方「替代方案」。
  3. **实证:两轮对抗复核各绕过一次。** 第一轮:`scan()` 不处理反斜杠,`\'` 被当成引号态开关而 bash 里它是字面引号(方向恰好相反),一对 `\'` 之间的内容 bash 照常执行、词法器整段吞掉,**模块的每一类拦截同时失效**(真 bash 5.2.37 已复现)。第二轮:`cd`/`pushd`/`PATH=` 不在任何表里——`git -C` 被否决而等价的 `cd ../other && git ...` 一路放行;`PATH=../evil:$PATH; cargo build` 让那个叫 cargo 的 token 解析到别的二进制,**连「程序是操作员写下的那一个」都不成立**(假 cargo 实测跑通)。**1045 行换来一个被绕过两次的过滤器。**
  4. **威胁模型里根本没有「模型是敌人」这一条。** kanzei 是用户自用的激进工具,不是给不受信任用户的通用 harness。任务源与模型都由用户自己掌握。为一个不存在的威胁模型付账,还付成了一个可绕过的过滤器。
  5. **它是无人值守的唯一硬卡点。** 保留它就等于保留「并行线跑不到底」。
- **替代方案(不是"什么都不做")**: 并行下真正要防的**不是恶意,是串台**——A 线的命令跑进 B 线的树、把人家未提交的活覆盖了。而闸门恰恰防不住这个(`cd ../other && rm -rf` 里没有一个可疑 token)。正解沿用本仓既有哲学:**不事前拦,检测 + 回滚**。D-173/D-174 已经把这条路走通了——`ManagedSnapshot` 执行前后快照比对、越界写入隔离留证、整体回滚,**它不关心命令长什么样**,所以 `cd ../other` 与 `cargo run` 里 build.rs 干的坏事一视同仁抓得到。本条的替代交付登记为 **R-186**(把 `ManagedSnapshot` 的范围从「托管文档」扩到「不属于本线的 worktree」)。
- **保留不动的**: ①硬 deny(`.kanzei/project/*` 等)——它是结果侧围栏,不受本条影响;②审计轨迹(谁在什么时候跑了什么)归 R-183;③**D-269 仍要修**(硬 deny 也走同一条匹配路径,且只 1/21 条规则受影响,是 5 行的事)。
- **作废的产出**: `par/f2` 分支上的 `crates/kanzei-harness/src/cmdline.rs`(1045 行)整个丢弃,不合入。`docs/design/tier1_implementation_plan.md` 的 F2/F5 两批随之作废,F8 里 ACE 告警那部分并入 R-183。
- **反悔条件(写清楚免得将来靠猜)**: 若将来要把**不受信任的任务源**(如外部 issue、他人提交的需求)交给自举循环,本条的诉求会重新成立——但那时要做的是 §0 定案 1 里写的「只放行真正的叶子命令」那一档,**而不是重做中间档**。那一档 Rust 开发跑不了,是不同的产品形态。
- 优先级: P0
- 复杂度: 大
- 标签: 核心
- 证据等级: E1(读码自证 + 2026-08-11 三条线实测各自 3~6 秒内被拒停机)
- refs: R-183 R-182 D-051 D-004
- 复现: 2026-08-11 三条 `kz run` 独立线跑任务级并行,先给每个 worktree 的 `.kanzei/kanzei.toml` 追加了按需放行规则(`action="bash"`,`resource="cargo *"` / `"node *"` 等)。三条线**全部在 3~6 秒内**以 `EXIT=3` / `stopped: permission declined` 停机,第一条 bash 就被拒。被拒的资源形态是 `{"command":"git branch --show-current; git status --short","workdir":"c:/users/kanzei/documents/kz-par-b"}`。对照:同一批任务换用外部 agent(权限规则可写成 `Bash(cargo:*)` 这种按需形态)则正常运行——所以问题不在 agent 侧。
- **本条不是「代码写错了」——先读这段再动手**: 造成上述结果的三层判定**全部是有意设计**,且各自有测试或缺陷编号背书。任何修法若让下面三条性质失效,就是把 D-051 重新放回去:
  ①`crates/kanzei-harness/src/permission.rs:198-216` `resource_match_for_action`:bash 的实际 value 永远是含 `command`+`workdir` 的结构化 JSON;pattern 非结构化时直接 `return false`。**意图**:workdir 是授权身份的一部分——同一条命令在不同目录里后果不同。测试 `bash_resources_keep_shell_text_opaque_during_matching`(:476)钉死了「换 workdir 即变 Ask」;测试 `legacy_bash_rules_do_not_authorize_structured_resources`(:494)钉死了「旧的纯字符串规则不得授权结构化请求」——因为旧规则是在 workdir 还不算身份时记的,让它们生效等于凭空授权了用户从未批准过的目录。
  ②`permission.rs:234-236` `command_chaining_escapes`:任何 `pattern != "*"` 且含 `*` 的 bash 规则被降级成 Ask。**意图**(函数头注释点名 D-051):通配规则表达不了命令内部的 shell 语义——`"git *"` 会匹配上 `git status; rm -rf ~`。测试 :472 同处对照:**精确**规则可以放行含 `rm -rf ~` 的整串,因为用户批准的正是那一整串。
  ③`resource_match_for_action:200` 对 `pattern == "*"` 前置直通,且 ② 明确排除 `pattern != "*"`。**意图**:整体放行是用户显式选择的 yolo,不应被降级(`permission.rs:466` 有同义注释),且 `config.rs:434` 会为此发告警。
- 影响(可用性缺口,不是安全漏洞): 三层叠加的净效果是 bash 授权**只剩两端**:逐条逐字节精确的单命令,或整体 `*`。中间那一档——「某类命令、可复用、可手写」——在代码里不存在。直接后果:①用户配置里已累积 **12 条巨长的结构化 JSON 规则**,每条只覆盖一个具体命令、复用率为零,只会无限累积;②启动时长期存在「检测到 N 条旧 bash 权限规则；将逐次询问」的告警且无法自愈;③**`kz` 无法无人值守运行**——R-183 的直接卡点,也是本次实测停摆的原因;④用户被结构性地推向 yolo,权限系统在实际使用中被架空——**这是最严重的一条,也是定 P0/high 的理由**:偏严到只剩全放行,结果比适度放宽更不安全。
- 边界: **判定失败方向是偏严**(该允许的没允许),不是越权。因此本条**不得以「放宽匹配」作为修法**——不能简单让纯字符串 pattern 去匹配结构化 value(那会同时废掉 ①的两条测试)。要交付的是**新增一档可安全表达的规则形态**,不是削弱现有两档。
- 修复方向(待设计,勿直接照做): 大致形状是——把命令**真正解析**成子命令序列(按 `;`/`&&`/`||`/`|`/换行切分,并对命令替换 `$(...)`/反引号、重定向到规则外路径等无法静态判定的构造保持 Ask),要求**每一个**子命令都命中允许规则才放行;workdir 维度改为**可显式表达**(规则能写「任意 workdir」,但必须是用户显式写出来的,不是旧规则被默认提权)。这一条与 R-183 内容②「worktree 应继承主根规则」是同一诉求的两半:继承必须是可见的、写出来的,不是隐式的。
- 验收: ①存在一种**可手写、可复用**的规则形态,能表达「任意 workdir 下的 cargo 命令」,有单测。②命令**确实没有**链接符/替换构造时不再被无条件降级;**确实有**时仍降级——两个方向各有单测,且含 `git status; rm -rf ~` 这类反例。③**D-051 三条性质的反证测试全部保留且仍绿**:换 workdir 即变 Ask(:476)、旧纯字符串规则不授权结构化请求(:494)、精确规则可放行整串(:472)。把这三条测试改红或删掉即视为验收不通过。④既有 12 条结构化 JSON 规则不因本次修改而失效(向后兼容单测)。⑤修复后 `kz run` 能在 worktree 里靠一组**可手写的**规则完成一次「改代码 → cargo test → 提交」闭环(与 R-183 验收①同一条轨迹)。⑥启动告警「N 条旧 bash 权限规则」在规则可正常匹配后消失,或给出可执行的收敛路径。⑦**实测被拒命令清单**作为规则模板的输入:本次并行实测里被拒/需要放行的命令要归档,R-183 内容④的模板据此收敛,不靠拍脑袋。
- **实测被拒命令清单(验收⑦的输入,2026-08-11 三条并行线实采)**: 五条,全部来自**外部 agent 的权限层**(该层已经在做本条想要的按子命令匹配),形态高度一致——
  ①`node <脚本> && echo "EXIT=$?"` —— 拦截理由明确点名 `echo "EXIT=$?"` 这一段需批准,**拆掉尾部 echo 后同一条命令直接放行**;
  ②`ls <路径> | head -0; ls <路径>`;
  ③`awk '<程序>' <文件>`(单命令,只是 `awk` 不在允许集);
  ④PowerShell `cargo test ... | Select-Object -Last 40`;
  ⑤bash `... | head -30; echo ...`。
  **归纳**:①②④⑤ 全是**复合命令**(`&&` / `;` / `|`),③ 是**未列入允许集的单个可执行**。两类都在改成单条纯命令后放行。对本条的三点含义:(a) 修复方向里「解析成子命令序列、要求每个都命中」的形状**已有活的参照实现**,不必再论证可行性;(b) 拦截必须**点名具体是哪一段**不被允许,否则无法自我修正——这是可用性的关键,不是锦上添花;(c) R-183 内容④的基础规则模板至少要覆盖 agent 实际会用的这批 shell 动词:`echo`/`head`/`tail`/`awk`/`grep`/`ls`,以及 PowerShell 的 `Select-Object`——它们几乎只出现在管道尾部做截断,危险面低但出现频率极高,是「不放行就寸步难行、放行也没什么风险」的典型。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-267
- 进展: [terminal-fix 2026-08-13] fixed → wontfix: 账本维护(D-331 验收④):D-267 主张的「命令级权限中间档」已被用户定调砍掉(挡不住有意的、被绕过两次、威胁模型里没有「模型是敌人」),防线整体挪到结果侧检测与回滚(R-186 承接);原 [dropped] 标记非缺陷合法终态,收敛为单一 wontfix 保留审计链,原有关闭理由与自由文本逐字保留

## D-271 主对话切线程时消息短暂消失、侧栏只显示单条并行任务、子代理无关闭/删除生命周期 [closed] [fixed] (medium)
- 优先级: P0
- 复杂度: 中
- 标签: 前端 并行 子代理
- 证据等级: E1(用户复现 + 运行时冒烟回归)
- refs: R-174 R-184 D-263
- 复现: 三条并行线运行时切换主对话线程，旧实现先清空消息再等待 IPC；迟到的旧线程历史还可能覆盖新线程。侧栏只显示单个“当前在做”焦点，未按 `process_list` 投影 N 条线路；子代理面板只有运行中/已完成，没有关闭与删除语义。
- 影响: 切线期间对话区出现空白或串线，用户无法判断三条线各自是否运行/处于哪个阶段；已结束子代理只能长期堆积，无法收起或清理。
- 修复: `conversation_get`/`conversation_trace_get` 锁定项目、进程和切换代次，目标历史完整恢复后再原子替换消息；侧栏按每个进程显示主代理/并行线、运行态、阶段并支持点击切换；子代理生命周期明确为 `running → finished → closed → deleted`，关闭/删除仅作用于当前 UI 条目，保留后端 transcript 与审计，停止仍调用真实 `stop_task`；主代理写入、比对、合并、发版边界同步写入系统提示与 task_spec，子代理工具白名单保持 `read/glob/grep`。
- 验收: `node scripts/ui-runtime-smoke.mjs` 覆盖三线状态、切线不清空、关闭/重开/删除；`ui-i18n-smoke`、`ui-a11y-smoke`、`ui-markdown-smoke`、`parallel-lines-regression` 全绿；`cargo test -p kanzei-app` 112 passed、`cargo test -p kanzei-core` 130 passed。2026-08-11 随本次桌面端发版交付，待用户安装后进行最终桌面实测。
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-271
- 进展: 正式关闭(2026-08-16,修复本体是 R-174/R-184 既有交付,2026-08-11 随桌面端发版,本条只补关闭书据)。核验证据:①切线不清空=conversation.rs:39 conversation_get/process_session_id 按 process_id→session_id 隔离历史,目标历史完整恢复后原子替换;ui-runtime-smoke.mjs 断言两条线路各有历史容器且 historyCalls 按 process_id(d|smoke/p|bg);②侧栏按进程投影=01-core.js:74 process_list 轮询,每条线路独立按钮/运行态/阶段;③子代理生命周期 running→finished→closed→deleted=06-agent-panel.js:5-6(closed 只收当前 UI 条目,后端保留;deleted 只移除 UI 条目,停止仍走真实 stop_task)。验收引用的测试面在交付时全绿(冒烟四连+parallel-lines-regression+cargo test -p kanzei-app 112/kanzei-core 130)。残余:用户安装后最终桌面实测(用户侧验证项,2026-08-11 发版后持续使用无回归),转入 R-101 延期 E2 清单,不滞留本条(关闭边界:可用即关闭)。
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598338171

## D-272 并行线/自举 ASK 串到用户弹窗并中断自动推进 [closed] [fixed] (high)
- 来源: 2026-08-11 用户复现——代理线调用 ASK 时弹窗出现在主用户界面，自举运行被迫等待或停止。
- 根因: 所有 `AskRequest` 默认复用桌面端用户询问闭包；运行模式没有把“可等待用户”与“后台自动推进”区分开，前端也没有按 ASK 来源做最后一道隔离。
- 修复: `RunnerConfig.ask_policy` 明确区分 `Interactive` 与 `NonInteractive`。主线手动运行保持交互；并行进程与自举续跑使用非交互策略：权限 ASK 转成可回喂模型的错误并继续，`question` 转成明确的不可询问工具错误，不创建 `PendingAsk`、不发用户弹窗。ASK 事件附带 `source`，前端对旧运行/异常事件再做并行、自举来源拦截。子代理继续保持只读与硬拒绝 ASK。
- 边界: 当前交付解决“不会串到用户”的安全行为；真正的代理间问答需独立的带 source/target 的内部消息通道，后续另立需求，不复用用户 ASK。
- 验收: `cargo check --workspace` 通过；`kanzei-core` 的 ASK 策略单测通过；UI 事件回归确认后台来源不进入用户 ASK 队列；桌面安装后需实际启动三线并开启自举，确认无弹窗且线路继续推进。
- 证据等级: E1(读码 + 编译/定向测试)，桌面最终验收待用户安装后实测。
- refs: D-271 R-169 R-174
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-272
- 进展: 正式关闭(2026-08-16,修复本体是既有交付,本条只补关闭书据)。核验证据:①AskPolicy 枚举 Interactive/NonInteractive/AutoAllow + allows_user_prompt(kanzei-core/src/runner/mod.rs:52-58),单测断言 Interactive 允许提问、NonInteractive 不允许(mod.rs:207-208);②drive.rs:774/876-928 NonInteractive 路径:权限 ASK 转 Gate::NonInteractive 错误回喂模型、question 转工具错误、不建 PendingAsk;③前端防御:07-events.js:438-441 on(kz:ask) 对旧运行/异常来源再拦一道。并行/自举运行配置 NonInteractive(ask_policy),主线手动保持 Interactive。边界(代理间问答需独立内部通道)已写在条目,另立需求不复用用户 ASK。残余:用户桌面安装后三线+自举无弹窗实测(用户侧),转入 R-101 延期 E2 清单,不滞留本条。
- observed_head: 45fd276e9ac4ac6a23c0027b801f95d6c6c3fe4f
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598388023

## D-321 全局记忆删除无恢复源与注销通道:U-001~004 永久丢失,MISSING 文案误导指向不存在的 git [fixed] (medium)
- 复现: 手动删除 ~/.kanzei/memory 下 U-001~004 文件后,memory_hits 表悬空 id 报 MISSING 且提示 restore from git,但该目录不在任何版本控制下;FTS/recalls/回收站均无正文,确认永久丢失
- 影响: 全局记忆裸删即永久丢失;警告文案给出不可执行的恢复指引;悬空 id 无注销通道只能手术 index.db
- 来源: 2026-08-13 会话复盘(已临时 git init ~/.kanzei/memory 兜底,见 M-059)
- 标签: 后端
- 优先级: P2
- 复杂度: 小
- 进展: 修复(2026-08-16,commit 1b09a24):①voided_ids()/void_id() 台账(crates/kanzei-tools/src/memory/store.rs,行格式 `- M-xxx: 理由`,与 docstore.rs:598 同型;校验理由≥4/前缀合法/条目不在活动与归档,重复注销幂等);②integrity_issues 把 voided 编号计入已交代缺号(不再报 MISSING),注销后又出现条目(手工改号/恢复)点名复活;③文案诚实:under_git()(根+祖先至多 8 层探测 .git)为真才提示 restore from git,否则给『检查回收站/备份 + voided-ids.md 注销』可执行处置。消费端=记忆页 integrity 报告(kanzei-app/src/memory.rs:40 直接透传 Vec<String>,无前端改动)。4 个新单测(注销后不再报 / 校验与幂等 / 复活检测 / git 文案两分支),cargo test -p kanzei-tools memory 87 passed,fmt+clippy 干净。
- 验收: ①MISSING 文案不再无条件指引 git 恢复:目录在 git 版本控制下才提示 restore from git,无 git 时给可执行处置(检查回收站/备份+voided-ids.md 注销)——单测 missing_message_honors_git_presence / void_id_acknowledges_gap_and_message_is_honest;②存在注销通道:缺号登记 voided-ids.md 后 integrity 不再报 MISSING——单测 void_id_acknowledges_gap_and_message_is_honest;③注销有前置校验(理由≥4、编号前缀合法、条目不存在于活动/归档)且幂等——单测 void_id_validates_and_is_idempotent;④注销后又出现条目(复活)必须可见——单测 voided_id_resurrected_is_flagged。
- observed_head: 1b09a249d57dac40ac07a3d94fcd7ef641596888
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786598787874

## D-323 R-199 第 4 处前端私有否决残留:暂停恢复路径档位不匹配时静默不调度续跑,引擎不知情 [fixed] (medium)
- 复现: 08-compose.js 约 643 行 auto-pause 恢复分支:!autoPaused 且勾选 auto-continue 时仍要 autoContinueAllowed() 才 scheduleAutoContinue,档位不是 dev-auto 就静默不调度,引擎计数与状态不知情;D-320 只修了 syncAutoContinueWithProfile 那处
- 影响: R-199 验收①「前端不再持有任何引擎不知道的续跑否决条件」在暂停→恢复路径上仍未兑现
- 来源: 2026-08-13 自举复盘(探查代理逐处核对 autoContinueAllowed 残留)
- 标签: 前端
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-323
- 复杂度: 小
- 进展: 修复(2026-08-16,commit c4f219d):08-compose.js 暂停恢复分支(643 行)移除 autoContinueAllowed() 私有否决,对齐 armAutoContinue(155-157)与勾选路径(692)语义——恢复一律重新调度,档位不对由引擎下轮 done 判 Stop(ProfileMismatch) 带 reason 可见收口(07-events.js ProfileMismatch 分支取消勾选+显示原因)。新增 ui-runtime-smoke D-323 断言:dev-pair 档位下暂停→恢复必须进入「2 秒后继续」分支并调度续跑定时器(含点击生效前置断言 paused()/resumedVal),测试钩子补 paused()/cancelTimers()。五条冒烟全绿 0 运行时错误(T-1786599513)。
- 验收: ①暂停→恢复路径在非 dev-auto 档位下不再静默不调度——ui-runtime-smoke D-323 断言(dev-pair 档位下恢复进入「2 秒后继续」分支并调度定时器,冒烟绿);②前端不再持有引擎不知道的续跑否决条件(R-199 验收①)——恢复分支与 armAutoContinue 一致移除 autoContinueAllowed(),档位判定唯一在引擎 decide();③既有 D-291 断言(引擎判 Continue 前端不得拦下)与其余四冒烟保持绿,无回归。
- observed_head: c4f219d3accb6dd2dd9bc75b5c73e130266e4895
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786599542705

## D-330 tracker add/repair_missing_id 时 priority 参数与 fields 里「优先级」键双写重复字段 [fixed] (medium)
- 复杂度: 小
- 复现: tracker add/repair_missing_id 分支(tracker.rs:484-489 与 :363-368)在 priority 参数有值时无条件 fields.push(("优先级", priority)),不去查 input.fields 里是否已有「优先级」键。调用方若同时传 priority 参数 + fields 里「优先级」键,Vec 里得到两条「优先级」字段。本轮 R-233/R-234 即踩中:值相同时两条相同字段冗余;值不同时(P1/P2)两条矛盾字段,下游读取语义不定。
- 影响: add 静默写两条同名字段:值相同仅冗余,值不同则优先级字段语义歧义;update 分支(:614-621)已有正确合并去重逻辑,add/repair 分支未复用,是不一致缺陷。
- 来源: 2026-08-16 本轮自举:R-233/R-234 add 时 priority 参数与 fields 里优先级键双写,get 显示两条「优先级: P1」,raw_lines 另有游离空行(已清)。
- 标签: 后端
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-330
- 进展: 修复(2026-08-16,commit 02ec7b2):tracker.rs add(:546-557)与 repair_missing_id(:416-427)分支的 priority 参数处理对齐 update 分支(:664-673)语义——已存在「优先级」键(中文键或大小写不敏感 priority)则覆盖为参数值,否则追加;不再无条件 push 造成双写。单测 add_and_repair_dedupe_priority_param_with_fields_key 覆盖两分支(参数优先覆盖 fields 值、只落一条),tracker 34 passed, fmt+clippy 干净。
- 验收: ①add 同时传 priority 参数与 fields「优先级」键只落一条字段,参数优先——单测 add 分支断言 prio.len()==1 且值为参数值;②repair_missing_id 同型——单测 repair 分支断言;③update 分支既有行为不变(tracker 34 passed 含既有 update 测试,无回归)。
- observed_head: 02ec7b20cba1490fe7bb0c6cc0d0907642c26db9
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786599769933

## D-331 归档终态无法安全修正且非法状态会污染缺陷标题，reopen 对归档 ID 误报 unknown id [fixed] (high)
- refs: D-267 D-241 D-284 D-329
- 复现: D-267 在 defects.md 中带缺陷状态机不支持的 [dropped]；close/archive 后工具未拒绝该标记，而是在标题继续追加 [fixed]，归档结果成为 [dropped] [fixed]。随后 defect reopen D-267：缺 reason 时先报参数错误，补 reason 后只查活动文档并报 unknown id，无法通过专用工具改为 wontfix。
- 影响: 同一缺陷可同时呈现互相矛盾的终态，调度、统计和人工审计失真；agent 收到 unknown id 后无法区分真正不存在与已经归档，容易绕过专用工具手改托管文档，破坏原子写入、格式保护和审计链。
- 期望: 缺陷写入口拒绝 dropped/done 等跨文档状态标记；活动操作遇到归档 ID 时返回“已归档”及允许动作；提供仅限终态到终态、强制 reason、不重新入队的归档纠错动作，并用它把 D-267 收敛为单一 [wontfix]。
- 标签: 核心
- 根因: DocStore 对标题中形似状态但不属于当前 DocKind 的标记缺少写入校验；close 渲染时把解析不到的 [dropped] 保留为标题正文并追加合法终态。TrackerTool get 可回落读取归档，但 update/reopen 只查活动 entries；reopen 的语义仅为 fixing→open，当前没有归档终态纠错动作。
- 验收: ①缺陷 add/update/close 对标题或状态位置中的跨 DocKind 状态标记给出明确错误，测试覆盖 dropped 不得混入标题；②reopen/update 命中归档 ID 时不再报 unknown id，而是明确 archived 且 reopen 不适用；③新增受限归档终态纠错动作，只允许 fixed↔wontfix、必须 reason、保持条目在归档、原子写入并追加审计进展；④D-267 修复为单一 [wontfix]，原有关闭理由与自由文本逐字保留；⑤回归覆盖真实不存在 ID、活动 fixing→open、归档内容保真、并发锁与完整性门禁。
- 优先级: P0
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-331
- 批次: 3/3
- 进展: B1+B2 完成:①标题跨 DocKind 状态标记校验(add/update/repair_missing_id 拒绝,commit b140322);②reopen/update 对归档 ID 报 archived 而非 unknown id(同提交);③fix_terminal 归档终态纠错动作(docstore::correct_archived_terminal:终态间 fixed↔wontfix、强制 reason、保持归档、原子写入、清标题标记、进展留审计,commit cdc2cc3)+CLI 分支(930a806);单测 title_status_marker_rejected_on_all_write_actions / archived_id_reports_archived_not_unknown / fix_terminal_corrects_archived_status_and_strips_title_marker,tracker 37+docstore 20+kanzei 3 全绿。 ‖ 2026-08-16 验收④执行:本会话工具面已刷新,req/defect fix_terminal 可用,已将 D-267 从 [dropped][fixed] 收敛为单一 [wontfix](理由含原关闭语义,审计保留)。验收①②③④⑤全部达成,关闭本条。
- observed_head: e63be64ecd503b28359eeacdcf354b5fb8bc5340
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786605459728

## D-207 取活顺序所见非所得:视图排序与优先级徽章都不参与取活,界面零提示 [fixed] (medium)
- refs: R-054 R-111
- 复现: 2026-08-09 用户反馈"取需求和缺陷的顺序看不懂了,因为侧边栏可以调整顺序"。机制现状:①取活真序 = md 文件物理顺序从上到下(dev prompt "Scan from top to bottom",schedule_entries 只后置阻塞项、不改文件);②侧栏拖拽(manual 排序+无筛选时)经 docs_update reorder **写回文件**,真的改变取活顺序;③侧栏另有 id/状态/复杂度/优先级四种视图排序(main.js filterRequirements),**只改显示**;④优先级徽章 P0~P3 完全不参与取活(prompt 明言 "Priority labels are background info, not the ordering")。
- 影响: 选了任何视图排序后,用户看到的顺序与 agent 取活顺序完全无关,界面没有任何提示;优先级徽章满屏,人天然以为按 P0→P3 取活,实际一票不投——近期把 5 条需求升 P0(576d725)在取活上零效果,用户的调度意图静默落空。三种顺序语义(文件序=取活序/视图序/优先级暗示序)混在同一个列表上,只在"manual+无筛选"时才重合。
- 根因: R-054 定了"文件顺序即开发顺序"的单一真源,后续视图排序与优先级徽章叠上去时,没有同步交代它们与真源的关系;取活规则只写在 prompt 里,UI 侧无任何投影。
- 验收: ①非 manual 排序视图下,侧栏显式提示"当前显示顺序≠取活顺序"(或等价视觉语言);②有一处能看到真序:取活预览(下一条会被拿的条目有标记,阻塞项显示跳过原因)或一键切回文件序;③优先级二选一——要么参与取活(prompt 与 schedule 同步改,并写清与文件序的优先关系),要么在 UI 上明示"仅参考,不影响取活";④用户复查确认能看懂"agent 下一个会拿哪条、为什么"。
- 证据等级: E1(代码四处机制实证 + 用户反馈)
- 优先级: P1
- 标签: 前端

- 进展: 2026-08-09 部分交付…(前文保留);2026-08-10 用户反馈验收④未过:blocked doing 被渲染成「运行中」——computeAgentFocus 修复①:active 排除 entry.blocked;2026-08-10 再反馈:active 集合无意义,退化为单条——computeAgentFocus 改为取活序第一个可执行的 doing/fixing 单条 id。①②③已交付。 ‖ 2026-08-13 用户装 build-0b40763 后目视确认:侧栏取活焦点现为单任务显示,确认能看懂「下一个会拿哪条」,验收④通过,关闭本条。

- 阻塞: 验收④用户复查:ui 资源打包进 exe,需用户跑 release.ps1 重建 kzapp 后实际查看侧栏取活焦点并确认能看懂;解除人=用户(重建+复查后确认即关闭)。
- observed_head: d7236ada9b95c92e8e232aaeaaf4acf38796c323
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786610851224

## D-278 子代理面板打开后无就绪状态:侧边栏小窗口看不到「子代理可用」文案(设置页有,面板没有) [fixed] (medium)
- content: 侧边栏 ◉ 按钮打开的子代理面板(#agent-panel)只有「运行中/已完成/已关闭」三个分区,没有任何就绪/可用状态信息。设置页 fast 行已正确显示「✓ 子代理就绪(qwen3.5:4b)」(fast_model_status 返回 ready=true),但面板打开后用户看不到子代理是否可用——缺环时(Ollama 未装/服务未起/模型未拉)也无法从面板感知。
- label: 前端
- priority: P2
- severity: medium
- 修复: 面板头部加状态行:打开面板时 invoke fast_model_status 并按 managed/ready 显示与设置页同源的文案(就绪/未安装/服务未运行/模型未拉取/外部 provider)。文案计算抽成共享函数 fastStatusText(s) 供设置页与面板同源,避免两处漂移。
- 复现: 1) 打开设置页确认 fast 行显示就绪(或缺环文案);2) 点侧边栏 ◉ 打开子代理面板;3) 面板内只有空的三分区,无任何就绪/可用文案。
- 根因: R-174 子代理面板只消费 RunEvent 渲染运行记录,未接入 fast_model_status 就绪数据源;就绪状态只在设置页(refreshFastStatus)渲染过一次,面板打开时无独立查询与展示。
- 进展: 修复完成:①index.html 面板头部加 #agent-panel-status 状态行;②06-agent-panel.js 新增 fastStatusText(s) 共享函数与 refreshAgentPanelStatus;③16-settings.js refreshFastStatus 复用 fastStatusText,设置页与面板同源;④style.css .agent-panel-status 样式。验证:node --check、frontend_check、ui-runtime-smoke、cargo test -p kanzei-app 全绿。 ‖ 2026-08-13 用户装 build-0b40763 后目视确认:打开子代理面板头部显示就绪状态文案,验收通过,关闭本条。
- status: fixing
- 阻塞: 外部阻塞(验收确认):ui 资源打包进 exe,当前运行中的 kzapp 是旧构建,面板就绪状态行无法目视。解除动作:用户跑 release.ps1 重建 kzapp 后,打开侧边栏子代理面板确认显示「✓ 子代理就绪(qwen3.5:4b)」(或缺环文案),确认后关闭。解除人:用户。
- observed_head: d7236ada9b95c92e8e232aaeaaf4acf38796c323
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786610851513

## D-280 「回到最新」按钮悬浮位置错误:相对 #main 硬编码 bottom:92px,被输入区遮挡 [fixed] (medium)
- content: 「回到最新」按钮(#jump-latest)悬浮位置错误:它用 position:absolute 相对 #main 定位,bottom:92px 是硬编码,而 #composer 实际高度约 120px+(padding 24 + textarea 3 行 + composer-bar),按钮被压在输入区里;附件条/继续文案面板展开时被遮挡更严重。
- label: 前端
- priority: P2
- severity: low
- 修复: 把按钮移进 #messages 内部并给 #messages 加 position:relative,按钮改为 right:22px;bottom:14px 相对消息区右下角悬浮,composer 高度变化不再影响;删除已失效的 #messages + #jump-latest 兄弟选择器规则。
- 复现: 1) 长对话向上滚动,出现「回到最新」按钮;2) 按钮落在输入框区域内/紧贴输入框,而不是悬浮在消息列表右下角。
- 根因: #jump-latest 是 #messages 的兄弟节点,包含块是 #main(position:relative),bottom:92px 相对整个主视图底部,与 composer 真实高度不耦合。
- 进展: 修复完成:①index.html 把 #jump-latest 移进 #messages 内部;②style.css #messages 加 position:relative,#jump-latest 改 right:22px;bottom:14px,删除失效兄弟选择器。验证:frontend_check、ui-runtime-smoke 全绿。 ‖ 2026-08-13 用户装 build-0b40763 后目视确认:「回到最新」按钮悬浮在消息列表右下角、输入框上方,不再被遮挡,验收通过,关闭本条。
- status: fixing
- 阻塞: 外部阻塞(验收确认):ui 资源打包进 exe,当前运行中的 kzapp 是旧构建,按钮新位置无法目视。解除动作:用户跑 release.ps1 重建 kzapp 后,长对话向上滚动,确认「回到最新」按钮悬浮在消息列表右下角、输入框上方(不再被遮挡),确认后关闭。解除人:用户。
- observed_head: d7236ada9b95c92e8e232aaeaaf4acf38796c323
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786610851817

## D-239 取活口径漂移复现追踪:伪阻塞/伪可执行/挂起无载体 [fixed] (medium)
- 复现: 2026-08-10 复盘取活时发现三处阻塞/挂起口径漂移:①R-151/R-162~R-167 把非阻塞内部依赖(R-150/R-161 等,解除权在 agent)写进「依赖」字段,list 据未完成依赖判 blocked,调度器整批跳过,需求队列后半截系统性锁死;②R-157 实质卡在 D-235(conventions.md 无专用写入通道,edit 被 ruleset 拒绝),却无阻塞字段,以 doing 形态占可执行 WIP 名额、实际推不动;③R-101 用户 08-09 挂起只写在进展里,状态 todo 无阻塞字段,取活器会误取。
- 根因假设: §1.1 阻塞口径只在「触碰条目时」顺带复核,无周期机械核对;2026-08-09 WIP 口径修订后历史条目未回扫(R-151 的阻塞恰在口径修订期写入)。
- 进展: 2026-08-10 复盘发现三处取活口径漂移(伪阻塞/伪可执行/挂起无载体)并修复;此后每轮取活前复核口径,累计 10 轮无同类复现(第 10 轮 2026-08-16)。 ‖ 2026-08-13 用户确认验收③达成(连续 10 轮无同类复现),关闭本条。
- 验收: ①当前三条已修,req get 各条目可见清理后口径(证据:R-101/R-157 有合法阻塞字段,R-151/R-162~R-167 依赖字段为空、进展注明解锁条件);②此后每轮取活前复核阻塞/依赖字段口径,若再次出现同类漂移(伪阻塞、伪可执行 doing、挂起无载体)→ 确认为规则缺陷,升级修 §1.1/取活器并记根因;③连续 10 轮无同类复现 → 用户确认后关闭本条。复核已累计 3 轮(2026-08-13 ×2、2026-08-14 ×1),无同类复现。
- refs: R-101 R-157 R-151 R-162 R-163 R-164 R-165 R-166 R-167
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-239
- 阻塞: 
- observed_head: d7236ada9b95c92e8e232aaeaaf4acf38796c323
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786610913286
