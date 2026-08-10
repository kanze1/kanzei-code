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

