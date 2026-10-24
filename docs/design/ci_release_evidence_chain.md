# 本地验证与发布证据链

- 身份：live_design
- 状态：发布证据门禁已有实现；验证和发布检查在本地执行，GitHub Actions CI 禁用。
- 核对日期：2026-10-23，主工作树源码及仓库规则；发布结果以具体提交的验证和打包记录为准。
- 关联：R-152、R-146、R-156、R-298；A-009、A-010、A-013。

## 当前合同

公开发布前，`package.ps1` 校验 `dist/verification.json` 或显式传入的验证文件：commit 必须匹配 HEAD，检查必须全量通过且没有跳步。脚本同时检查提交范围、工作树、远端提交及构建版本。检查条目和证据格式以 `scripts/verify-policy.mjs`、`verify.ps1`、`package.ps1` 为准，不再复制一份固定“十步”清单。

```powershell
pwsh -File scripts/verify.ps1 -Full
pwsh -File scripts/package.ps1 -Ack <脚本实际统计的提交数> -Publish
```

需要隔离发布时使用临时干净 worktree；开发中的 dirty 源码先提交到明确的交付版本。发布后删除临时工作树。本机不再要求常驻 `kanzei-release` 目录。

## 开发验证与执行调度

日常运行 `scripts/verify.ps1`，选择当前未提交改动；工作树干净时默认检查上一提交，显式 `KANZEI_VERIFY_BASE` 可以扩大提交范围。修改检查脚本会选择该套件，脚本引用的本地测试辅助模块也参与选择。注册表的 `paths` 声明功能归属；共享基础模块、全局样式和预览基础设施变化会扩大到全部前端检查，验证策略或注册表变化会选择全部检查。依赖生产者自动加入计划。

定向验证记录 HEAD 和未提交输入的内容摘要，结束时重新核对，写入 `dist/verification-targeted.json`；该文件不覆盖正式发版证据，也不能用于打包。`-Full` 要求干净提交，全部注册检查通过、无跳步，并在结束时核对 HEAD 和构建输入。套件 pass、测试用例 ignored 和未执行场景分别报告。

`verify-runner.mjs` 按依赖调度独立检查，最多四个工作进程、两个浏览器套件；Cargo 和 IPC 签名提取共用单一 Cargo 队列。测量动画帧率的套件声明 `host` 资源，独占主机运行。各套件使用独立临时目录、独立日志和 SHA256，失败只阻止其依赖项，独立项继续执行。结果记录实际墙钟耗时与各项耗时；构建缓存测量在所有 Cargo 生产者结束后执行一次。

## 远端 CI 与历史兼容边界

根 `AGENTS.md` 与 `CONTRIBUTING.md` 规定：不新增、启用或运行 GitHub Actions CI。工作树中的旧 `.github/workflows/ci.yml` 已删除；旧提交保留的 `workflow_dispatch` 不构成当前运行授权。push 和 PR 不代表发生了独立环境复跑。

脚本中保留的 `-Ci` 参数、注册表 `ci_args` 和历史回执字段属于兼容代码，不是当前工作流。检查项及启动参数统一维护在 `scripts/verification-checks.json`，本地入口使用 `scripts/verify.ps1`；需要额外全目标 clippy 时本地显式运行并保留结果。

本地 `verify.ps1 -Full` 是正式发布的验证入口。后台冻结快照与独立 worker 提供执行隔离，仍不等于完整四角色独立裁决体系。

## 安装与交付

桌面端唯一安装位是 `%LOCALAPPDATA%\kanzei\kzapp.exe`。`release.ps1` 的本机安装、`package.ps1 -Publish` 的公开 Release、安装后真实版本核对分别记录。程序运行中不能把安装器退出码 0 当作替换成功。

本地产物由 `scripts/artifact-lifecycle.mjs` 管理：新版本发布且安装验收后保留当前及上一版安装器；发布引用的验证证据单独保护。公开 Release 的保留规则与本地安装器的恢复保留分开执行。

## 历史与本次校正

2026-08-09 初版规划自动 push CI，后续文档曾改为手动 CI；两种描述均由当前本地验证规则替代。fmt/clippy 已在本地检查体系中。文件名保留以维持既有引用；旧合同可从 Git 历史恢复。本次文档维护没有修改发布脚本或 workflow。
