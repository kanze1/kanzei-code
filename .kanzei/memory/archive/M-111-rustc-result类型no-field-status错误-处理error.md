---
id: M-111
scope: project
category: fact
title: RustC Result类型no field status错误 — 处理error[E0609]访问Result字段必须先用match或unwrap再访问
description: Rust编译Result类型access field错误 — 处理error[E0609]: no field(status) on Result类型报错必读 - 确认是否尝试对Result直接访问字段，应用unwrap/expect或匹配模式
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

exit code: 1 > error[E0609]: no field `status` on type `Result<Output, std::io::Error>` --> crates\kanzei-tools\src\git.rs:838:16 [fp:bash|> error[E]: no field on type]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-111-rustc-result类型no-field-status错误-处理error.md)
