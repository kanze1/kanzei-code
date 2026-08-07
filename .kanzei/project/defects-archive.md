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

## D-147 自记阻塞只进不出,鞭挞把整个队列锁死后静默停机 [fixed] (high)
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

## D-148 侧栏条目编辑表单无字段名且截断长值,继续文案框只露两行 [fixed] (medium)
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

## D-149 条目展开详情把每个字段渲染两遍,阻塞字段三遍 [fixed] (medium)
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

