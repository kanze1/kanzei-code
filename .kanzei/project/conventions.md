
# Agent 开发规则(项目特有部分)

本文件只放 **kanzei 仓库特有** 规则(架构契约 §4、分支与提交流程 §6、构建与发版 §9)。
**通用**开发规则——取活与阻塞口径、关闭边界、验收证据、标签与依赖字段、批次与
验证节奏、代码修改原则、命名风格、测试与文档纪律、任务级并行——由 kanzei 引擎
内置注入(R-191,单源 kanzei-harness),所有项目默认一致,不要抄到这里:在项目
文件里复制通用规则只会漂移。
- 对话与思考一律使用中文。
## 4. 架构与契约规则(kanzei 版)

- `kanzei-llm` 的 `LlmEvent` / `Part` / `LlmRequest` 变更，必须同步全部三个协议实现（anthropic / openai / openai_responses）和 runner，不允许只改一处。
- `kanzei-harness` 的 `Tool` / `ToolOutput` / 权限契约变更，必须同步 kanzei-tools 所有工具、runner、kz CLI 与桌面端事件转发。
- 桌面端新增/修改 Tauri command：必须注册进 `invoke_handler`，入参返回结构变更必须同步 `ui/main.js` 调用方；新增前端事件必须经 `on()` 订阅（listen 失败要可见）。
- `kanzei.toml` 配置 schema 变更必须向后兼容（serde default），设置页表单必须透传新字段，禁止保存时丢字段。
- 权限规则是硬门禁：任何"规则"能用代码强制的绝不只写进提示词。
- M2 起 SQLite 表结构变更需附迁移与回滚说明。
- **>8k 字符的文本不进命令行参数**(R-238):bash 命令串与 `kz run` 位置 prompt 都受 Windows 命令行 32767 字符上限约束,大文本一律**文件中转**——先用 `write` 工具落文件、命令里引用路径,或以 `kz run --prompt-file <path>` 交付。超长命令由 bash 工具在 spawn 前结构化拒绝(文案同源,见 `kanzei-tools/src/bash.rs` 的 `MAX_COMMAND_CHARS`;不要绕开防护,不要用等价命令换拼法把长文本塞进 shell)。

## 6. 分支与提交流程(kanzei 版)

- 默认且唯一开发分支是 `main`，直接在 `main` 上提交，无 merge/部署环节。
- 开发前 `git status` 检查工作树，提交前确认只包含本次任务的必要改动。
- `.kanzei/project/` 下的需求、缺陷、规范文档是项目资产，随代码一起提交；`.kanzei/summaries/` 对话总结与临时文件不提交。
- commit message 使用中文或英文均可，但一律**不带任何 Co-Authored-By 署名**。
- push 前设置代理：`$env:HTTPS_PROXY = "http://127.0.0.1:12000"`。

## 6.1 外部 agent 协作纪律(R-181 降级交付)

- **kanzei 不做跨进程写租约**——源码并发由 worktree 物理隔离 + git 三方合并/`merge-tree` 预检承担,文档侧互斥由 R-138 `FileLock` 解决(R-182 实测结论)。不要实现 `kz lock acquire|release`,那条路已关闭。
- **外部 agent(Claude Code / Cursor / 手动改)动仓库前,先跑 `kz lock status`**:它是只读可见性入口,报主根、cwd、git 工作树未提交改动与活跃线。看到别人正在写的文件就换 worktree 或先沟通,不要闷头写同一段。
- **提交只暂存自己明确修改的文件**(D-263):`git add <file>` 逐文件,禁止 `git add .` / 目录级扫入——外部 agent 未完成的改动会被静默卷进他人提交,归属混了、CI 红了、两边都不知道对方在写。
- **检测 ≠ 互斥**:`kz lock status` 只提供可见信号,不拦截写入;真正的强隔离是独立 worktree(R-177/R-182)。

## 9. kanzei 本仓库:构建与发版(优先级高于上文通用规则)

- 本仓库是 Rust workspace:`crates/kanzei-{harness,llm,core,tools,app}` + `crates/kanzei`(bin `kz`)。
- **分支流程**:日常开发(含 agent 自举)一律提交到 `dev` 分支;`main` 只接收来自 dev 的合并,保持随时可发布。**main 常驻发布树**(`C:\Users\kanzei\Documents\kanzei-release`),主工作树里 `git checkout main` 会因分支被占而失败——合并发布一律在发布树执行:`git -C <发布树> fetch origin && git -C <发布树> merge origin/dev --ff-only && git -C <发布树> push`,再跑发布树里的 `package.ps1 -Publish`。禁止直接在 main 上做开发提交。
- **提交身份铁律**:commit 的 author/committer 必须且只能是 kanzei 本人(`kanzei <vraniumzwt@gmail.com>`);message 不得包含任何 `Co-Authored-By` 尾注(GitHub 会把共同作者计入贡献者头像墙)。任何工具/AI 不得以自己的身份出现在 git 历史里,发现异常身份立即改写修正并强推。
- 测试:批内定向(`cargo test -p <改动 crate>`),全量 `cargo test --workspace` 的触发点按 §1.4(中/大条目关闭前 + 发版前),全量必须全绿才算条目完成;单 crate 快速检查用 `cargo build -p <crate>`。**「全绿」的定义是 verify.ps1 十步(含六条前端冒烟:ui-runtime/ui-lint/parallel-lines/ui-a11y/ui-i18n/ui-markdown),不是任意子集**(D-371):声称「前端冒烟全过」必须六条全跑——test_record 会对声称「冒烟」且 status=passed 的记录强制比对六条清单,差集非空即拒(机械判据,不靠自觉)。
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
- **发行通道(安装包)**:先确认当前 HEAD 已推送到对应远端分支，再按脚本实际统计的提交数运行 `.\scripts\package.ps1 -Ack <自上个 build 标签以来的实际提交数> -Publish`；脚本先核对发布范围与验证证据，再由 `cargo tauri build` 产出 NSIS 安装器 `dist\kanzei-setup-<hash>.exe`，最后用 `gh release create build-<hash>` 发布到 GitHub Releases。
  - 云端发布不是可选步骤：只跑 `release.ps1` 或省略 `-Publish` 只能完成本机安装/构建，安装版用户不会收到更新；`-Ack` 必须使用脚本实际统计值，不能凭记忆填写。
  - 应用内更新以**最新 release** 为源:启动 3 秒后静默检查(有新版弹 toast),设置页「检查更新」可手动查 + 一键下载安装;发布后需核对 Release target、安装器资产与构建 hash 一致。
  - **静默安装陷阱(D-266)**:`setup.exe /S` 在 kzapp 运行时**静默无效**——目标程序运行中无法替换时必须保留 `.pending` 或明确提示，不能把退出码 0 当作安装成功。

- **发布时机与检查单**:完成一批已验证需求/缺陷后发布;发布前必须 ①`cargo test --workspace` 全绿 ②工作区干净且已 push ③`kz --version` 的 hash 与 HEAD 一致。
- **发布树(worktree)**:发布统一从 `C:\Users\kanzei\Documents\kanzei-release`(`git worktree`,跟踪 main)执行:`git -C <发布树> pull` 后跑其中的 `package.ps1 -Publish`——与 dev 工作树完全隔离,发布时不需要 stash/打断正在进行的开发。**提交了不等于发布了**:安装版用户只认 Releases,合并 main 后记得发布。
- **Release 标签与保留规范**:tag = `build-<short-hash>`,标题 `kanzei <日期> (<hash>)`;公开 Releases 只保留最新稳定版及其安装器,旧 Release 对象与资产发布新版后清理,对应 Git tags 与提交历史保留用于审计和恢复。
- **产物卫生**:`dist/` 只保留最新安装器,`dist/`、`target/`、安装器一律不入库。
### 9.15 波次质量审计(手动触发,2026-08-16)

- 触发方式:**用户手动发起**,不进定时任务、不挂关闭门禁——审计频率由用户按波次节奏掌握,避免拖慢自举吞吐。建议时机:一波密集交付(≥10 提交)后、或发版前。
- 流程、三路只读派发模板与四种失败模式清单(最后一公里接线/证据替身/注释假承诺/沉默降级)见 `docs/design/bootstrap_quality_audit.md`;产物一律 `defect add` 登记,只列真问题不凑数。
- 审计纪律:只读;不跑任何构建(并行线互踩);以已提交 HEAD 为准,工作树未提交改动只标注不定论;关键指控先当场核验(file:line 复查)再登记。

### 9.2 巨石度量与阈值(R-258)

- **度量入口**:`kz metrics [--top N]`——按文件输出 总行数/生产行数/测试行数/函数数/最大函数行数/参数>7 处数,全仓 Top-N 榜单。生产行数 = 总行数 − cfg(test) 块行数(cfg(test) 块按大括号配平识别;外挂声明 `#[cfg(test)] mod x;` 不算测试块;`_tests.rs` 后缀与 `tests/` 目录的外挂测试文件整文件算测试行)。
- **阈值(超了必须登记条目,不自动拒绝提交)**:生产行数 > 1200 视为巨石;单文件参数 > 7 的函数 ≥ 4 处视为参数失控;最大函数行数 > 400 视为函数巨石。超阈值动作 = 在需求/缺陷里登记拆解条目(如「R-xxx 拆解」),不阻塞当前提交(自用工具,威胁模型里没有敌对模型,防线放可见性不放闸门)。
- **基线快照**:每次发布前跑一次 `kz metrics --top 30`,把榜单落进 `docs/design/metrics-baseline.md`(或随拆解条目进展更新),供后续拆解前后对照。

## 10. Research 证据口径(R-221 B3)

research 产出的证据等级使用 V0–V3,与验证体系的 E0–E4 完全分离,不得互换或把 E 等级写成研究结论等级。每条研究结论必须同时写证据域、V 等级和证据锚；文献结论还必须写证据深度。

| 等级 | 代码域 | 文献域 |
| --- | --- | --- |
| V0 | 目录/命名推测 | 无出处断言 |
| V1 | 读码核实(file:line) | 二手转述(博客/新闻/论坛),或一手来源仅摘要级 |
| V2 | 运行时实测 | 一手来源正文级(论文原文/官方文档/仓库源码,经正文核验) |
| V3 | 用户复现 | 至少两份独立一手正文级来源交叉验证,或本地实测复现 |

文献证据深度规则:仅读 title/summary/API 摘要只能标摘要级并封顶 V1；读过正文且论断由正文支撑才可标正文级 V2；摘要不能支撑正文级论断。无证据锚必须显式标 V0,不得凭措辞升级等级。


