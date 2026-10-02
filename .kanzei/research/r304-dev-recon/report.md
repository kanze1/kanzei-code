# R-304 dev 勘察工件落点示例

- kind: dev_recon
- topic: r304-dev-recon
- entry_refs: R-304
- status: archived
- created: 2026-08-18

## 结论

dev 侧勘察工件统一落在 `.kanzei/research/<entry-id>-<slug>/`，最终结论固定为 `report.md`。条目的 `refs` 只保留追踪编号；条目的 `进展` 记录一行摘要和本报告相对路径，报告头部用 `entry_refs` 回指条目。

## 证据锚

- V1 / 代码域：`docs/design/research_mode.md:13-16` 记录 dev 勘察此前没有固定工件落点，且 research 与 dev 勘察边界独立。
- V1 / 代码域：`docs/design/research_mode.md:32,43-57` 已有 `.kanzei/research/<topic>/`、kebab-case topic、`report.md`、可选 notes 与 tracker 路径衔接约定。
- V1 / 代码域：`crates/kanzei-memory/src/docstore.rs:370-384,405-437` 对 topic 目录执行小写 kebab-case 校验并隔离 source/finding 文档。
- V1 / 代码域：`crates/kanzei-tools/src/tracker.rs:154-164,257-259,316-329` 强制 source/finding 的 topic 落点；dev 侧本报告复用同一目录，但不伪造 research profile 工具调用。
- V1 / 代码域：`crates/kanzei-tools/src/profiles/dev.rs:102-157` 显示 `.kanzei/project/*` 是托管 tracker 专用通道；`.kanzei/research/**` 不在该项目文档硬拒范围，dev 使用既有 write/edit/insert 即可落盘。

## 清理记录

本示例没有保留不可重建的中间证据；最终报告按生命周期规则标为 `archived` 并保留。后续可删除已被报告吸收且可重建的临时索引/缓存，但必须在对应条目的进展或提交说明中留下记录。

## R-248 复用说明

R-248 恢复时可复用本报告采用的根目录、`<entry-id>-<slug>` 命名、`report.md` 最终文件和 active→archived 生命周期；其 refs API 与 topic 来源决策仍按原阻塞字段等待用户拍板。
