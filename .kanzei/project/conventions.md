
# Agent 开发规则(项目特有部分)

本文件只放 **kanzei 仓库特有** 规则(架构契约 §4、分支与提交流程 §6、构建与发版 §9)。
**通用**开发规则——取活与阻塞口径、关闭边界、验收证据、标签与依赖字段、批次与
验证节奏、代码修改原则、命名风格、测试与文档纪律、任务级并行——由 kanzei 引擎
内置注入(R-191,单源 kanzei-harness),所有项目默认一致,不要抄到这里:在项目
文件里复制通用规则只会漂移。

## 4. 架构与契约规则(kanzei 版)

- `kanzei-llm` 的 `LlmEvent` / `Part` / `LlmRequest` 变更，必须同步全部三个协议实现（anthropic / openai / openai_responses）和 runner，不允许只改一处。
- `kanzei-harness` 的 `Tool` / `ToolOutput` / 权限契约变更，必须同步 kanzei-tools 所有工具、runner、kz CLI 与桌面端事件转发。
- 桌面端新增/修改 Tauri command：必须注册进 `invoke_handler`，入参返回结构变更必须同步 `ui/main.js` 调用方；新增前端事件必须经 `on()` 订阅（listen 失败要可见）。
- `kanzei.toml` 配置 schema 变更必须向后兼容（serde default），设置页表单必须透传新字段，禁止保存时丢字段。
- 权限规则是硬门禁：任何"规则"能用代码强制的绝不只写进提示词。
- M2 起 SQLite 表结构变更需附迁移与回滚说明。

## 6. 分支与提交流程(kanzei 版)

- 默认且唯一开发分支是 `main`，直接在 `main` 上提交，无 merge/部署环节。
- 开发前 `git status` 检查工作树，提交前确认只包含本次任务的必要改动。
- `.kanzei/project/` 下的需求、缺陷、规范文档是项目资产，随代码一起提交；`.kanzei/summaries/` 对话总结与临时文件不提交。
- commit message 使用中文或英文均可，但一律**不带任何 Co-Authored-By 署名**。
- push 前设置代理：`$env:HTTPS_PROXY = "http://127.0.0.1:12000"`。

## 9. kanzei 本仓库:构建与发版(优先级高于上文通用规则)

- 本仓库是 Rust workspace:`crates/kanzei-{harness,llm,core,tools,app}` + `crates/kanzei`(bin `kz`)。
- **分支流程**:日常开发(含 agent 自举)一律提交到 `dev` 分支;`main` 只接收来自 dev 的合并,保持随时可发布。**main 常驻发布树**(`C:\Users\kanzei\Documents\kanzei-release`),主工作树里 `git checkout main` 会因分支被占而失败——合并发布一律在发布树执行:`git -C <发布树> fetch origin && git -C <发布树> merge origin/dev --ff-only && git -C <发布树> push`,再跑发布树里的 `package.ps1 -Publish`。禁止直接在 main 上做开发提交。
- **提交身份铁律**:commit 的 author/committer 必须且只能是 kanzei 本人(`kanzei <vraniumzwt@gmail.com>`);message 不得包含任何 `Co-Authored-By` 尾注(GitHub 会把共同作者计入贡献者头像墙)。任何工具/AI 不得以自己的身份出现在 git 历史里,发现异常身份立即改写修正并强推。
- 测试:批内定向(`cargo test -p <改动 crate>`),全量 `cargo test --workspace` 的触发点按 §1.4(中/大条目关闭前 + 发版前),全量必须全绿才算条目完成;单 crate 快速检查用 `cargo build -p <crate>`。
- **发版安装(用户可见的”构建”)**:`.\scripts\release.ps1`
  - 流程 = 全量测试 → 安装 `kz` 到 `~\.cargo\bin` → release 构建桌面端 kzapp 并复制安装;
  - **桌面端只有一个安装位:`%LOCALAPPDATA%\kanzei\kzapp.exe`**(应用内更新与开始菜单都指向它)。`~\.cargo\bin` 只放 `kz` CLI 与转发启动器 `kzapp.cmd`,**绝不能再放 kzapp.exe**——两份副本各自更新就会出现"发布了但仍在跑旧版"(D-145)。判断当前跑的是哪份:`Get-Process kzapp | Select-Object Path`。
  - kzapp 正在运行时复制会失败,脚本会把新版本存为 `kzapp.exe.pending` 并提示——此时告知用户”关闭 kzapp 后重跑 release.ps1”,**不要强杀用户正在使用的窗口**;
  - `-SkipTests` 仅在用户明确要求时使用。
- 版本验证:`kz --version` 输出 `kanzei 0.1.0 (<git hash> <日期>)`;桌面端右下角有相同版本徽章,以此确认用户装到的是新版。
- 网络:`git push` / 访问 GitHub 需要代理,PowerShell 里先 `$env:HTTPS_PROXY = “http://127.0.0.1:12000”` 再执行。
- 提交规范:commit message 一律**不带任何 Co-Authored-By 署名**;提交前 `git status` 确认只包含本次任务的必要改动。
### 9.1 发布部署(两条通道)

- **开发通道(自举机)**:`.\scripts\release.ps1` = 全量测试 → 装 `kz` → 构建并安装 kzapp;kzapp 运行中会落 `kzapp.exe.pending`,**下次启动自动接力替换**,无需手动处理。
- **发行通道(安装包)**:`.\scripts\package.ps1 -Ack <自上个 build 标签以来的提交数> -Publish` = 先核对发布范围与验证证据，再由 `cargo tauri build` 产出 NSIS 安装器 `dist\kanzei-setup-<hash>.exe` → `gh release create build-<hash>` 发布到 GitHub Releases。
  - 应用内更新以**最新 release** 为源:启动 3 秒后静默检查(有新版弹 toast),设置页「检查更新」可手动查 + 一键下载安装;
  - 所以:想让安装版用户收到更新,**必须带 `-Publish`**;只跑 release.ps1 安装版是感知不到的。
- **发布时机与检查单**:完成一批已验证需求/缺陷后发布;发布前必须 ①`cargo test --workspace` 全绿 ②工作区干净且已 push ③`kz --version` 的 hash 与 HEAD 一致。
- **发布树(worktree)**:发布统一从 `C:\Users\kanzei\Documents\kanzei-release`(`git worktree`,跟踪 main)执行:`git -C <发布树> pull` 后跑其中的 `package.ps1 -Publish`——与 dev 工作树完全隔离,发布时不需要 stash/打断正在进行的开发。**提交了不等于发布了**:安装版用户只认 Releases,合并 main 后记得发布。
- **Release 标签与保留规范**:tag = `build-<short-hash>`,标题 `kanzei <日期> (<hash>)`;公开 Releases 只保留最新稳定版及其安装器,旧 Release 对象与资产发布新版后清理,对应 Git tags 与提交历史保留用于审计和恢复。
- **产物卫生**:`dist/` 只保留最新安装器,`dist/`、`target/`、安装器一律不入库。


