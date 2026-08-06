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
