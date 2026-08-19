---
id: M-255
scope: project
category: fact
title: Rust crate src文件未闭合的}`错误是瞬态语法问题，需定位具体行号补全后再编译
description: 编译出现 unexpected closing delimiter 时必读：检查括号匹配/字符串语法 — [fp:bash|error: unexpected closing delimiter:]
status: deprecated
created: 2026-08-18
updated: 2026-08-18
source: user
---

- 错误指纹：[fp:bash|error: unexpected closing delimiter:]
- 触发条件：cargo build/debug/test出现"unexpected closing delimiter: `}` 伴随stderr指向具体文件行号（如events.rs:967）
- 操作步骤：(1)解析`error:`字段找出文件路径+行号 (2)打开对应文件检查该行是否有未闭合的字符串字面量（常见于正则/日志模板多引号嵌套） (3)添加缺失的`}`补全再重新编译
- 边界与例外：若错误消息不含行号指向，需先通过cargo expand或rust-analyzer定位实际位置；临时禁用字符串转义会暴露问题

决策价值：这是跨项目复用的Rust编译环境契约知识，当前为第1次复发记录。

(stale: 被 M-163(编译错误)覆盖：unexpected closing delimiter 已入 SOP 层无需再记)
