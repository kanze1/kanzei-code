# CI 与发布证据链

- 身份：validated_design
- 状态：发布证据门禁已实现；CI 当前为手动触发。
- 核对日期：2026-10-20，主工作树源码；本次校正不代表执行了一次发布。
- 关联：R-152、R-146、R-156、R-298；A-009、A-010、A-013。

## 当前合同

公开发布前，`package.ps1` 校验 `dist/verification.json` 或显式传入的验证文件：commit 必须匹配 HEAD，检查必须全量通过且没有跳步。脚本同时检查提交范围、工作树、远端提交及构建版本。检查条目和证据格式以 `scripts/verify-policy.mjs`、`verify.ps1`、`package.ps1` 为准，不再复制一份固定“十步”清单。

```powershell
pwsh -File scripts/verify.ps1 -Full
pwsh -File scripts/package.ps1 -Ack <脚本实际统计的提交数> -Publish
```

需要隔离发布时使用临时干净 worktree；开发中的 dirty 源码先提交到明确的交付版本。发布后删除临时工作树。本机不再要求常驻 `kanzei-release` 目录。

## CI 的实际边界

`.github/workflows/ci.yml` 当前只有 `workflow_dispatch`，没有 push 或 pull_request 触发。因此 push 不代表发生了独立环境复跑，定向本地检查也不能依赖自动 CI 补齐。

workflow 统一调用 `scripts/verify.ps1 -Full -Ci`。本地与 CI 共享 `scripts/verification-checks.json`；`ui_runtime` 的共享参数包含 `--experimental-vm-modules`。CI 的 clippy 通过注册表中的 `ci_args` 检查全部目标，本地提交与 verify 使用轻量 clippy。修改检查项或启动参数时维护注册表，不在 workflow 或文档中另抄一份命令清单。

本地 `verify.ps1 -Full` 是正式发布的验证入口；手动 CI 的实际结果另行记录到具体提交。后台冻结快照与独立 worker 提供执行隔离，仍不等于设计中的完整四角色独立裁决体系。

## 安装与交付

桌面端唯一安装位是 `%LOCALAPPDATA%\kanzei\kzapp.exe`。`release.ps1` 的本机安装、`package.ps1 -Publish` 的公开 Release、安装后真实版本核对分别记录。程序运行中不能把安装器退出码 0 当作替换成功。

本地产物由 `scripts/artifact-lifecycle.mjs` 管理：新版本发布且安装验收后保留当前及上一版安装器；发布引用的验证证据单独保护。公开 Release 的保留规则与本地安装器的恢复保留分开执行。

## 历史与本次校正

2026-08-09 初版规划了自动 push CI，并暂缓 fmt/clippy；之后两条检查已经启用。自动触发承诺不符合当前 workflow，本轮改为手动事实。旧示例脚本与旧发布树流程从正文移除，原文可从 Git 历史和本次备份恢复。没有修改发布脚本或 workflow。
