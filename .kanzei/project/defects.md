# Defects

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

## D-010 桌面端重启后历史对话不可恢复 [open] (high)
- 原因: run_task 只将消息保存在 AppState.conversation 内存中；SQLite 目前仅写入 prompt/run 边界事件，没有保存消息内容，也没有历史会话列表/加载 command。
- 复现: 在 kzapp 中发送对话并关闭应用，再次打开同一项目；消息区域为空，无法查看或继续之前会话。
- 影响: 违反 R-009/R-013 的重启恢复和历史会话加载验收。
- 验收: 重启后可看到项目会话列表，打开任意会话可恢复消息并继续对话；存储读取失败需明确提示。

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
