# Defects

## D-001 docstore 标题末尾括号被误剥为 severity [fixed] (medium)

## D-002 GUI 运行失败时卡在运行中,错误未展示且无法中止 [fixed] (medium)

## D-003 LLM 请求无超时,网络不通时永久挂起且无提示 [fixed] (medium)

## D-004 前端发送被拒时静默无反馈(running 标志卡死后点发送毫无反应) [fixed] (medium)

## D-005 Tauri 缺 capabilities,前端 event.listen 被 ACL 静默拒绝,所有运行事件收不到 [fixed] (medium)

## D-006 release.ps1 在 kzapp 运行时无法覆盖安装包 [fixing] (medium)
- 复现: 保持已安装的 kzapp.exe 运行，执行 .\scripts\release.ps1；cargo build --release -p kanzei-app 成功，但 Copy-Item 覆盖 ~/.cargo/bin/kzapp.exe 失败并中止脚本。
- 影响: 新构建产物位于 target/release/kzapp.exe，但用户目录中的 kzapp.exe 仍是旧版本，安装流程未完成。
- 验收: 脚本在应用运行时给出明确提示并完成可恢复安装，或在构建成功后明确区分构建成功与安装失败。

## D-007 总是允许只对后续运行生效,当前运行内换个命令仍反复弹窗 [fixed] (medium)
