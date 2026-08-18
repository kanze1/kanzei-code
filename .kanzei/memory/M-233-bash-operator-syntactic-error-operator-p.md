---
id: M-233
scope: project
category: habit
title: bash_operator_syntactic_error: operator pattern mismatch for Rust syntax
description: 处理 bash 语法识别失败时必读:检查操作符格式是否为 !.::?? - 替换为正确的 Rust 语法模式
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

bash operator recognition failure: expected one of `!`, `.`, `::`, `?`, `{`, or an operator, found `store` -> memory::mod.rs:2049:9

Error signature: exit code: 1 error: expected one of `!`, `.`, `::`, `?`, `{`, or an operator, found `store` at match store location

Root cause: Syntax in bash invocation requires proper Rust-like syntax elements (!. :: ? ...) rather than plain identifiers
