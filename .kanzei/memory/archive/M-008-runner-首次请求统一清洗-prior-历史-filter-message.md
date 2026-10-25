---
id: M-008
scope: project
category: fact
title: runner 首次请求统一清洗 prior 历史(filter_message_history)
description: 调试 runner 首请求消息构造、prior 历史孤儿 ToolCall/ToolResult、上下文压缩相关问题时必读
status: deprecated
created: 2026-08-07
updated: 2026-10-24
source: memory-manager
area: kanzei-core/runner/drive/assembly
---

根因:run_once_with_parts 原先直接 clone prior 历史,只有在发生上下文压缩或调用方显式清洗时才会去除孤儿 ToolCall/ToolResult,导致首条请求可能携带不完整 tool 配对。

修复:在 crates/kanzei-core/src/runner/drive.rs 首次构造 messages 时统一调用 crate::history::filter_message_history(原 runner.rs 已拆分为 runner/ 模块)。已通过 cargo test --workspace 验证。

(stale: 2026-10-24 按用户要求精简项目知识：通用工具用法、单次故障 SOP 与旧阶段提示退出项目常驻记忆；操作与验证以当前工具、AGENTS.md 和 CONTRIBUTING.md 为准，原文及来源归档保留。)
