---
id: M-244
scope: project
category: fact
title: bash/cargo 编译失败 [unexpected closing delimiter]
description: 处理 Cargo/Rust 编译错误时必读：unexpected closing delimiter 检查括号匹配
status: deprecated
created: 2026-08-18
updated: 2026-08-18
source: 2026-08-18 fact note
---

编译环境契约问题。重现场景：cargo build/test Rust 项目，错误包含 `error: unexpected closing delimiter: \`}\``，指向具体文件行号（如 events.rs:967）。根因：代码括号不匹配或字符串内嵌套括号误用。判例：kanzei-core v0.1.0 crates/kanzei-core/src/store/events.rs:967 处 `}``意外闭合。处置：检查该行及前后上下文，确保无缺失的 `{` 前兆；若为字符串字面量内部括号需转义或确认语法结构正确。指纹：[fp:bash|error: unexpected closing delimiter:]

(stale: 被 M-245 取代：新增条目包含更多 FP 模式变体（测试失败、session diff）作为复发检测 key)
