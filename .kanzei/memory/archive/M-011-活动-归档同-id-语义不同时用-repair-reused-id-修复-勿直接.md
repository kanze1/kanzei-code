---
id: M-011
scope: project
category: sop
title: 活动/归档同 ID 语义不同时用 repair_reused_id 修复,勿直接编辑托管文档
description: 处理 tracker 完整性门禁报 present in BOTH active and archive / 活动与归档同 ID 语义不同的修复时必读
status: deprecated
created: 2026-08-08
updated: 2026-08-12
source: inbox:2026-08-08
---

根因场景:长期 G-002 由 R-093 迁入时复用了 goals-archive.md 中旧短期 G-002,完整性门禁因此拒绝所有 goal 写操作。

修复工具:TrackerTool 新增 repair_reused_id。前置条件:活动/归档同 ID 但语义不同,且不存在其他完整性问题。行为:保留活动 ID,将历史归档条目迁到下一未使用 ID,并同步字段、模板和手写自由文本中的引用。

CLI 用法:`kz goal repair_reused_id G-002`

代码位置:crates/kanzei-tools/src/docstore.rs 与 tracker.rs。
