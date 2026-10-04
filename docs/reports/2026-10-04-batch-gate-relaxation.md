# 批次收尾计时与命令限制修复

2026-10-04，Android Markdown 项目的 R-008 在第 24 步开始验证时被 `BATCH_CLOSING` 拦截。该轮从 12:43 开始，13:00 左右进入收尾；15 分钟计时覆盖了澄清、调查和设计。收尾命令判定又拒绝 PowerShell 的 `&`，且名单没有 Flutter/Dart，因此格式化、静态分析没有执行。随后启动的验证子代理也被收尾规则拒绝。

本次删除 15 分钟触发条件，保留已有的 32 步实现窗口和 8 步收尾窗口。检查点目录的唯一性标记不再参与收尾判定。

收尾不再按命令字符串和技术栈名单拒绝 `bash`。PowerShell 调用运算符、完整程序路径、环境设置和串联检查继续经过原有执行权限与工具 guard。开发提示同步允许使用项目实际验证入口及必要的环境准备。

Shell 调用按真实文件内容观察归属：没有改变捕获文件的命令不会取消归属或已记录的验证；改变捕获文件的命令会使旧验证失效，并取消该文件的自动暂存归属。已有的专用实现工具限制、Git 暂存归属检查、恢复检查点及自动继续逻辑保留。

定向验证已完成：

- `cargo test -p kanzei-core --lib runner::drive::batch::tests`：9 passed，0 failed。覆盖经过 24 小时仍不提前收尾、Flutter/Dart/npm/pytest/Gradle 命令放行、未变文件归属保留及 shell 改动使旧验证失效。
- `cargo test -p kanzei --test integration agent_step_budget`：2 passed，0 failed。真实工具和本地 HTTP 模型驱动 42 步：第 33、34 步执行 PowerShell `& 'git' diff --check; git status --short`，第 35–40 步拒绝继续写入，第 41 步保存检查点并恢复实现；没有用户问题。
- `rustfmt --edition 2021` 对修改的 Rust 文件格式化；`git diff --check` 通过。

集成执行日志保存在 `output/batch-gate-relaxation-2026-10-04/runner-integration.log`。发布验证和资产验收分别由 `dist/verification.json` 与 `dist/release-receipt.json` 记录实际结果。

这次修复只涉及 Kanzei 的运行门禁与开发提示；R-008 的 Android 功能实现仍需继续完成。
