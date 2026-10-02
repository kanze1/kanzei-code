---
id: M-129
scope: project
category: fact
title: [Rust compile] 按错误 token 分类处置：missing field/type no impl/unary op — [fp:bash|error[E]/E0xx...]
description: 处理任意 rust/cargo compile error 反复重试失败 — 需按具体 error token 分类排查 root cause，通用 "类型/字段没搞对" 判据已不足以触发正确操作序列
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: rust 编译 error 处置原则
---

整类编译 error 不可通用修复 — 不同 error token 对应不同 root cause：manual case-insensitive ASCII mismatch(字段拼写/大小写)、missing field on initializer(结构体漏填 required field)、type does not implement bound/trait(类型不满足 Trait/实现缺失)、no field on type(错误用法/类型理解错)、cannot apply unary op(逻辑/类型误解)、unrelated parser/binary error(语法或逻辑)。本轮仍复发说明记忆未触发正确判据，故先 consolidate 当前所有 distinct error token 为独立条目。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-129-rust-compile-按错误-token-分类处置-missing-fiel.md)
