# 工作树与发布状态同步

核对时间：2026-10-19 17:43:17，Asia/Shanghai。范围：Kanzei 仓库及其历史隔离目录。

## 当前结论

- 正式版为 [build-a8f72e9c](https://github.com/kanze1/kanzei-code/releases/tag/build-a8f72e9c)，发布时间 2026-10-19 08:41:57。发布源码为 `a8f72e9c2da99551a0ff8b380891c4dda3fa955d`。
- 整理前 `HEAD`、`main`、`dev`、`origin/main`、`origin/dev` 与发布标签完全一致。此次同步提交只新增本报告，`main` 与 `dev` 同步至本报告所在提交；与正式版的提交差异仅为状态文档。
- 46 个本地分支均已被正式版覆盖：19 个直接包含于发布历史，26 个补丁等价，1 个源码相同而审查文档段落重排。17 个远端分支引用也全部属于发布历史；当前开放 PR 为 0。
- Git 注册的 Kanzei 工作树只有主目录。19 个已归档目录的名称标记已可恢复地收存，AUTO-OSU 的 `rhythm-convergence` 工作树保留。
- 本机 CLI 与桌面程序均已安装正式版。两个可执行文件的完整字节分别出现在经 GitHub SHA-256 校验的正式安装器载荷中；生命周期登记已同步为已确认安装。
- 主目录原有 11 项在途状态继续原位保留，逐路径存在状态与 SHA-256 前后相同；当前无未收尾的工程验证、构建或原生验收任务。

## 工作树与归档目录

| 对象 | 数量 | 当前状态 |
| --- | ---: | --- |
| Kanzei 注册工作树 | 1 | `C:/Users/kanzei/Documents/kanzei code`，`dev` |
| Kanzei 活动隔离工作树 | 0 | 已归档的源码工作树均已退出 |
| 归档后留下的标记目录 | 19 | 已迁移至本轮证据目录，名称文件 SHA-256 一致 |
| 其他项目的实际工作树 | 1 | `rhythm-convergence/AUTO-OSU`，保留原位 |

本轮收存的目录仅含 `.codex-worktree-name` 和空子目录，均无 Git 元数据及源码：

- `artifact-lifecycle`
- `audit-a0`
- `audit-b0`
- `audit-c0`
- `audit-reliability-release`
- `backlog-maintenance`
- `batch-gate-relaxation`
- `chat-question-update`
- `context-budget-web-evidence`
- `conversation-model`
- `global-skills-release-20261005`
- `native-control-experience`
- `project-desktop-release`
- `project-knowledge-release`
- `reliability-browser-memory`
- `requirements-contract`
- `sidebar-fix-release`
- `subagent-modes-review`
- `update-empty-conversation-fix`

原路径与恢复目标逐项记录在 `output/repository-reconciliation/20261019/marker-archive.json`。恢复标记目录时可将其中的 `destination` 移回对应 `source`。

## 本地分支覆盖清单

“历史包含”通过祖先关系确认；“等价补丁”通过发布提交与分支的对称差集、Git patch-id 判等确认。各分支均无额外合并提交。功能分支记录整理前的提交，主分支随后包含此次状态文档。分支名称和提交引用继续保留，作为历史追溯入口。

| 分支 | 提交 | 发布覆盖 |
| --- | --- | --- |
| `dev` | 本报告所在提交 | 正式版 + 此次状态文档 |
| `kanzei/artifact-lifecycle` | `a8f72e9c` | 历史包含 |
| `kanzei/async-conversation-release-20261008` | `46e2e63a` | 历史包含 |
| `kanzei/audit-a0` | `504f399b` | 等价补丁已进入发布 |
| `kanzei/audit-a1` | `deb0c559` | 等价补丁已进入发布 |
| `kanzei/audit-a2` | `95b7b99d` | 等价补丁已进入发布 |
| `kanzei/audit-a3-ui` | `9db202a2` | 等价补丁已进入发布 |
| `kanzei/audit-a4-state` | `a63d3bf6` | 等价补丁已进入发布 |
| `kanzei/audit-a5-schedules` | `ec96f9b9` | 等价补丁已进入发布 |
| `kanzei/audit-a6-ui-surfaces` | `18214815` | 等价补丁已进入发布 |
| `kanzei/audit-a7-run-control-deltas` | `7f59eac7` | 等价补丁已进入发布 |
| `kanzei/audit-a8-prefs-failure` | `e2162265` | 等价补丁已进入发布 |
| `kanzei/audit-a9-memory-service` | `5d01fb3f` | 历史包含 |
| `kanzei/audit-b0` | `cb8b2a5a` | 等价补丁已进入发布 |
| `kanzei/audit-b1` | `13840c69` | 等价补丁已进入发布 |
| `kanzei/audit-b2` | `441e0c29` | 等价补丁已进入发布 |
| `kanzei/audit-b4-background-stop` | `7a859987` | 等价补丁已进入发布 |
| `kanzei/audit-b5-process-ids` | `fe979bc1` | 等价补丁已进入发布 |
| `kanzei/audit-b6-background-history` | `e8ea4887` | 历史包含 |
| `kanzei/audit-c0` | `be97f1bf` | 等价补丁已进入发布 |
| `kanzei/audit-c1` | `336e68ef` | 等价补丁已进入发布 |
| `kanzei/audit-c2` | `b9f3ae24` | 等价补丁已进入发布 |
| `kanzei/audit-c3` | `3a36b0de` | 等价补丁已进入发布 |
| `kanzei/audit-c4-session-owner` | `07c0d373` | 等价补丁已进入发布 |
| `kanzei/audit-c5-stop-notification` | `df024e4e` | 等价补丁已进入发布 |
| `kanzei/audit-c5-terminal` | `19d95cfa` | 等价补丁已进入发布 |
| `kanzei/audit-c6-startup-outcomes` | `2a9151b2` | 源码相同，审查文档重排 |
| `kanzei/audit-c7-steering-outcomes` | `0e0b24f8` | 历史包含 |
| `kanzei/audit-d1` | `585cde3b` | 等价补丁已进入发布 |
| `kanzei/audit-d2-team-owner` | `cc7e25ec` | 等价补丁已进入发布 |
| `kanzei/audit-d3-permission-context` | `95b731b6` | 等价补丁已进入发布 |
| `kanzei/audit-d6-harness-rest` | `26b2e37a` | 等价补丁已进入发布 |
| `kanzei/audit-m6-docstore` | `989dcd33` | 等价补丁已进入发布 |
| `kanzei/audit-reliability-release-20261016` | `3938210f` | 历史包含 |
| `kanzei/batch-gate-relaxation` | `6638a30a` | 历史包含 |
| `kanzei/chat-question-update-20261005` | `f7f7aa5a` | 历史包含 |
| `kanzei/context-budget-release-20261017` | `24533867` | 历史包含 |
| `kanzei/conversation-model-20261003` | `c6654fae` | 历史包含 |
| `kanzei/global-skills-release-20261005` | `2f9889c8` | 历史包含 |
| `kanzei/harness-reliability-release-20261017` | `b97f8a2e` | 历史包含 |
| `kanzei/native-control-experience` | `d501b4dc` | 历史包含 |
| `kanzei/project-desktop-release` | `0af8d0ae` | 历史包含 |
| `kanzei/project-knowledge-release` | `e5a0d02c` | 历史包含 |
| `kanzei/requirements-contract` | `7b2b8096` | 历史包含 |
| `kanzei/subagent-modes-20261004` | `2e1777b4` | 历史包含 |
| `main` | 本报告所在提交 | 正式版 + 此次状态文档 |

`kanzei/audit-c6-startup-outcomes` 的 `2a9151b2` 与发布历史中的 `0e0b24f8`，排除文档后的完整补丁逐字节相同。`git range-diff` 显示唯一差异是 `docs/reviews/file-audit-2026-10-03/parallel/C6.md` 中“验证与证据”段落的位置，因此该分支没有漏发实现。

## GitHub 发布清单

GitHub 当前存在 7 个正式 Release，全部带安装器且对应源码位于最新正式版历史中。Git 中共有 33 个 `build-*` 标签；另 26 个标签没有当前 Release 对象，属于历史引用，不能只据标签认定安装包仍可下载。完整对应关系保存于 `tag-release-coverage.json`。

| 正式 Release | 发布时间（北京时间） | 安装器字节数 | 当前状态 |
| --- | --- | ---: | --- |
| [build-a8f72e9c](https://github.com/kanze1/kanzei-code/releases/tag/build-a8f72e9c) | 2026-10-19 08:41:57 | 28025115 | Latest |
| [build-0af8d0ae](https://github.com/kanze1/kanzei-code/releases/tag/build-0af8d0ae) | 2026-10-19 08:18:24 | 28021434 | 历史正式版 |
| [build-d501b4dc](https://github.com/kanze1/kanzei-code/releases/tag/build-d501b4dc) | 2026-10-19 06:20:09 | 28021207 | 历史正式版 |
| [build-1964f9f9](https://github.com/kanze1/kanzei-code/releases/tag/build-1964f9f9) | 2026-10-19 02:37:15 | 27919422 | 历史正式版 |
| [build-24533867](https://github.com/kanze1/kanzei-code/releases/tag/build-24533867) | 2026-10-18 05:35:47 | 25763968 | 历史正式版 |
| [build-29b09eaa](https://github.com/kanze1/kanzei-code/releases/tag/build-29b09eaa) | 2026-10-17 20:46:20 | 25766446 | 历史正式版 |
| [build-e6e1ad56](https://github.com/kanze1/kanzei-code/releases/tag/build-e6e1ad56) | 2026-10-17 18:13:55 | 25700775 | 历史正式版 |

最新安装器为 `kanzei-setup-a8f72e9c.exe`，28025115 字节，SHA-256：

`c4bed9e222ba517d225fec238fb57687721e4ebe931ba5e02427b6e433de9c5a`

此次实时查询 `releases/latest`、标签目标和远端 `main/dev`；本地保留安装器的大小与 SHA-256 与 GitHub 资源元数据一致，公开 Range 下载返回 HTTP 206 和 1024 字节。

## 验证与本机安装

发布证据绑定 `a8f72e9c`，完整工程门禁 77/77 通过，跳过步骤为 0；原发布记录的 Rust 测试为 2689 passed、10 ignored、0 failed。本轮重新执行证据校验器，原始检查日志与绑定提交通过校验。此次状态文档同步沿用该发布证据，不生成新二进制。

本机 `kz --version` 输出：`kanzei 0.1.0 (a8f72e9c 20261019001957)`。

| 本机程序 | SHA-256 | 与正式安装器载荷核对 |
| --- | --- | --- |
| `kz.exe` | `0d3b409dced2adcd11382d62d6a339337d602196a72a506b51c82bd92be5a3eb` | 完整字节一致 |
| `kzapp.exe` | `11dd63c965458d66d4cb1ad4ba22fcb66d75a107692c983a9e09a3956e875cef` | 完整字节一致 |

核对以正式安装器中的文件内容为准。当前 `target/release/kzapp.exe` 是后续同提交构建产物，其 SHA-256 与安装版不同；安装版与实际发布载荷完整一致，不能用后续构建目录反推本机安装失败。

旧的“本机尚未安装”记录是发布当时的观测，本次保留原始回执并更新 `output/release-status-current.md` 及生命周期安装登记。此轮未执行安装或重启。

## 主目录保留的在途状态

| 路径 | Git 状态 |
| --- | --- |
| `.kanzei/kanzei.toml` | `M` |
| `.kanzei/memory/INDEX.md` | `M` |
| `.kanzei/memory/M-292-cli-与桌面运行装配共用一个能力入口.md` | `D` |
| `.kanzei/project/defects-archive.md` | `M` |
| `.kanzei/project/defects.md` | `M` |
| `.kanzei/project/requirements-archive.md` | `M` |
| `.kanzei/project/requirements.md` | `M` |
| `.kanzei/project/tests-archive.md` | `M` |
| `docs/reports/2026-10-18-kanzei-improvement-backlog.md` | `M` |
| `.kanzei/research/web-search/prior-art-search.json` | `??` |
| `docs/reports/2026-10-21-code-agent-security-audit.lock` | `??` |

11 项状态均未进入此次同步提交，原始文件副本、Git 二进制补丁和前后 SHA-256 清单保存在本轮证据目录。

## 证据入口

- `output/repository-reconciliation/20261019/snapshot-before.json` 与 `snapshot-after.json`：引用、分支覆盖、工作树、原有文件摘要和工程任务状态。
- `output/repository-reconciliation/20261019/releases.json`、`latest-release.json` 与 `open-pulls.json`：GitHub 原始响应。
- `output/repository-reconciliation/20261019/marker-archive.json`：19 个空目录标记的可恢复迁移回执。
- `output/repository-reconciliation/20261019/c6-integration-range-diff.txt`：C6 与已发布实现的差异核对。
- `output/repository-reconciliation/20261019/installation-payload-verification.json`：本机两个程序与正式安装器载荷的完整字节核对。
- `dist/verification.json` 与对应 `verification-runs`：原发布门禁证据，仍绑定发布源码提交。

原有发布与专项验收回执保持各自的版本归属。此次工作树和发布状态核对不改变历史业务场景的通过、失败及未覆盖记录。
