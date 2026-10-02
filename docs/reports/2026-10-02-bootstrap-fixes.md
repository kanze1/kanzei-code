# D-772 / D-773 / D-774 自举修复

日期：2026-10-02。范围：9 月 28 日登记的局部校验、运行内批次收口、压缩恢复三个 P1。发布从 `build-2546703e` 建立临时工作树，只带入这三个问题的修复；主目录既有 R-365 和其他未交付改动继续保留。

## 修复与验收对账

| 条目 | 现在的行为 | 可复查实现与测试 |
| --- | --- | --- |
| D-772 | Rust 从包或 workspace 解析 edition；写后只跑单文件格式、语法检查。Cargo 和完整 UI/浏览器回归在报告中明确留到批次验证，VM 命令带 `--experimental-vm-modules`。超时、环境问题、代码/格式失败分别报告 | `crates/kanzei-tools/src/local_validation.rs`；`cargo test -p kanzei-tools local_validation --lib`，14 项通过，包含合法异步 Rust、workspace 继承、真实语法错误、VM 参数、缺失模块、20 秒超时 |
| D-773 | 无固定步数上限的主代理在实际专用文件写入后，每 32 步或 15 分钟进入收口窗口，最多用 8 步验证、按归属提交并回写已有 tracker。暂不能交付则保存真实失败、下一步、范围补丁和写入前后内容，再自动继续 | `crates/kanzei-core/src/runner/drive/batch.rs`；`cargo test -p kanzei-core d773 --lib`；`cargo test -p kanzei --test integration agent_step_budget`，真实工具与 Git 配合本地 HTTP 模型夹具跑 42 步：收口期拒绝继续扩展、保存第 32 步检查点，第 41 步恢复写入，无提问 |
| D-774 | 切点遇到工具调用/结果配对时，先向前寻找完整边界，再向后寻找；保留完整头部。确实没有合法切点时不改变历史，不调用摘要模型，不消耗“摘要无收益”预算，后续仍能重试 | `crates/kanzei-core/src/runner/compaction.rs` 与 `drive/context_budget.rs`；`cargo test -p kanzei-core compaction --lib`，18 项通过，覆盖并行调用、分离结果、重复 call_id 和连续延期后的恢复 |

## 交付与恢复逻辑

收口窗口要求验证、结构化 Git 操作和已有 req/defect 进展记录，随后继续同一任务。它没有新增 Work Unit 或手写完成批数；批数仍从 Git 推导。

只允许暂存本轮专用写工具捕获、首次触及时干净、此后内容仍匹配的文件。已有脏文件和混合修改保存在检查点，不能借收口一起提交。任何后续源码写入都会撤销先前验证/提交/进展旗标。定向回归使用实际临时 Git 仓库验证这些边界。

检查点写入 `.kanzei/artifacts/batch-checkpoints/<hash>/checkpoint.json`，始终记录 `completed: false`；提交、验证和进展回执另存。它是恢复材料，不能充当验收完成。每个小文件最多捕获 4 MiB 的前后内容；Git patch 失败会记录错误，检查点写入失败时保留当前收口状态并重试。

无限主代理仍可长期执行；有限预算和普通 task 的既有退出行为保留。阈值在工具批次之间检查，不能中断正在执行的单个工具。主代理用 shell 或其他专用生成器写入的文件不自动取得暂存归属。

## 验证与发布

正式发布的完整工作区测试：2,137 项通过、0 失败，2 项仓库既有 ignore；改动的 core/tools/CLI 测试目标已通过 all-targets clippy。两个浏览器用例的首次失败为临时目录缺少依赖，按 package-lock.json 执行 npm ci 后完整重跑通过。最终提交 `c5bf3e327c527a72e539f9cb80229c5e0957aebf` 的 `verify.ps1 -Full` 15 项全绿、无跳过项。这里的本地 HTTP 夹具不表示已重跑原先 110 分钟的真实自举，也不表示 R-369 的协作方案完成。

发布门禁发现 drive.rs 相对既有基线增长 120 行，超过 100 行限制。将原有并行权限预检移入既有 drive/parallel_tools.rs，保持原限制；调整后 core 的 366 项单元测试与 CLI 的 36 项 HTTP/真实工具集成回归全部通过。

公开版本为 [build-c5bf3e32](https://github.com/kanze1/kanzei-code/releases/tag/build-c5bf3e32)，范围是 `build-2546703e..c5bf3e32` 的 2 个提交。安装器 41,913,452 字节，重新下载与本地产物 SHA256 同为 `e8aef747327d39708935e51570a819f2788a7e238b0d09c8fa80b773f92dd430`；GitHub digest 一致，1 字节 Range 请求返回 206。

CLI 已安装，`kz --version` 为 `kanzei 0.1.0 (c5bf3e32 20261001233523)`。正在运行的桌面应用保留，新桌面镜像已保存为 `%LOCALAPPDATA%/kanzei/kzapp.exe.pending`，SHA256 为 `5df2568f7c77068183b8f3b7e0b21da284fe391919e444928132366afba78ea8`。待用户关闭应用后通过既有启动更新流程替换；尚未做桌面重启后的验收，也没有启动自举。

主目录定向测试在编译阶段遇到 16 个既有模型接口诊断，涉及 `Route.with_provider_identity`、Hosted 事件/内容/请求字段和 `Usage.web_search_requests`；引用在本轮修改前的快照中已存在。这是主目录与发布源码尚未整合的 R-381 工作，不能把隔离发布全绿当作主目录全绿。错误原文与修复前快照一同归档。

D-772、D-773、D-774 已按发布版验收关闭并归档；R-381 保持在途。网页中的问题按用户确认记录为已登记，后续沿用现有条目。

临时发布工作树、临时本地与远端分支已移除；当前只保留主工作树及原有 `main`、`dev`、`kanzei/release-2026-09-25`。共享 `target` 保留。`dist` 只留当前安装器、验证证据和发布收据，原有两个安装器与旧证据转入 `dist-before` 归档。

按项目规范 §9.1，72 个旧稳定 Release 的元数据和全部资产（1,689,357,519 字节）经下载、大小及哈希核对后归档，再移除旧 Release 对象；公开稳定版仅保留 `build-c5bf3e32`。旧下载链接失效，72 个旧 Git tag 的本地与远端 OID 全部一致，提交历史保留。恢复位置为本次归档的 `previous-releases/`；清单、哈希、删除回执与 tag 核对另存 JSON。

原始事件与根因见 [9 月 28 日审计](2026-09-28-bootstrap-stall-and-subagents.md)。正式证据为仓库 `dist/verification.json` 与 `dist/release-receipt.json`；构建日志、失败日志、修复前快照和恢复材料归档到 `C:/Users/kanzei/Documents/kanzei-archives/2026-10-02-bootstrap-fixes/`，其中 `evidence.zip` 保留本轮证据，`bootstrap-fixes.bundle` 保存两个交付提交（前置基线 `build-2546703e`）。
