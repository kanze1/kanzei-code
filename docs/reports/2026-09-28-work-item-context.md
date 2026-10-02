# 条目上下文优化实现报告

- 日期：2026-09-28
- 代码提交：`8ffc5283288acffb1ec38f02f3c276bab4635bf4`
- 分支：`kanzei/item-context-20260928`
- 设计：[work_item_context.md](../design/work_item_context.md)
- 交付状态：已实现、验证、同步主工作目录并发布 build-584a1f42；发布补验见文末。

## 行为

成功认领另一条工作条目后，在下一次模型请求前收纳旧的大段读取/搜索结果。首次认领、同条目续做、查询、认领失败和未完成工具组不会触发。已登记关联的任务保留更大的近期窗口。

总目标、用户修正、决策、交接、错误、bash 日志和后台任务结果保留。原始工具正文保存到已有 artifact 存储，可按引用回读；原始 typed facts 不变。模型发送视图原子保存成功才替换，失败继续使用原上下文。没有新增总结模型调用。

## 验证

| 检查 | 结果 |
| --- | --- |
| Core 全量单元测试 | 343 passed，0 failed |
| Work 模块定向回归 | 43 passed，0 failed |
| CLI 上下文集成（包含原有超限恢复） | 4 passed，0 failed |
| App 单元测试 | 381 passed，0 failed |
| Core / Tools / CLI / App all-target Clippy，`-D warnings` | 通过 |
| 独立实现分支的设计时效和 diff whitespace 检查 | 通过 |
| 主工作目录 App + CLI all-target 编译检查 | 通过 |
| 主工作目录新增 Core 场景 | 7 passed，0 failed |
| 主工作目录新增 CLI 场景 | 2 passed，0 failed |

新增测试共 9 项：7 项 Core 场景与 2 项 CLI 集成。CLI 走真实 runner、WorkTool、存储及恢复代码，模型使用确定性 SSE 桩。

手机 Markdown 阅读器模拟场景：
- 发布基线上的测试请求体：102,747 → 69,076 字节，减少 32.8%。
- 合回主工作目录后的测试请求体：103,905 → 70,234 字节，减少 32.4%。
- 两次模型请求分别用于认领和继续工作，没有额外总结请求。
- 单独验证上轮末尾刚认领时，恢复后的第一次请求就能整理。
- 验证发送内容缩小后，原始 transcript 和后续新事实仍能恢复。

请求体大小是测试场景的序列化字节数，不能等同于真实模型账单、开发耗时或真实阅读器长跑收益。

## 集成说明

主工作目录已有未提交的新 `ModelRoles.web_extract` 字段，但 App 测试合并辅助函数漏填。补齐该测试初始化后，all-target 编译通过；这是一行主工作目录兼容修复，不属于发布基线的运行时代码。

同步前保留了所涉及文件的原内容：
`output/item-context-20260928/before/`。原有其他改动保持原状；主工作目录没有整体暂存或提交。

主工作目录的全量设计索引检查另发现两个既有未登记文件：`decision_console_integration.md`、`product_operating_model.md`。本次 `work_item_context.md` 已登记；没有替旧文档补写未经核实的截至提交或变更其时效身份。该项属于现有文档整理欠项，未计作通过。

开发验证日志位于 `output/item-context-20260928/`。此次没有真实模型长跑或桌面交互验收；发版及本机安装状态见下文补充。

## 测试修正记录

首次完整取活夹具分别触发了 mock provider 的默认 32k 超限保护和无交互 claim 权限拒绝。将该临时测试项目显式设为 128k，并仅允许 `work write:claim` 后，完整链路通过。生产权限规则和预算策略没有因此改动。

## 发布补验（2026-09-28）

- 已发布：[build-584a1f42](https://github.com/kanze1/kanzei-code/releases/tag/build-584a1f42)。
- 发布提交：`584a1f42963780bd5261cdf26391e80e3cfe0bc4`；自上一版本起 2 个提交。
- 15 项完整发版检查全部通过，0 个跳过步骤；Rust 测试 1927 passed、0 failed、2 项原有 ignored。
- 远端标签、Release target 和应用更新的 latest 入口均指向本版；非 draft、非 prerelease。
- 重新下载的安装器与本地一致：40,424,266 字节；SHA-256 `56f40ff0fc630c8ac149d707a7ac563c6ee2047df35a860432cb6dd29dea4f43`。Range 请求返回 HTTP 206，1024 字节。
- 验收时旧版窗口仍运行（PID 19084），因此没有执行本机自动安装；当时安装位 CLI 仍为 build-22d74597。需在应用中“检查更新”安装后再启动自举。
- 发版证据：`output/release-proof-584a1f42/acceptance.json`、`verification.json` 与完整日志。
