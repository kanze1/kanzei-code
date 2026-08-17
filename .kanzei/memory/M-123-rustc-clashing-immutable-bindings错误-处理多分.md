---
id: M-123
scope: project
category: fact
title: RustC clashing immutable bindings错误 — 处理多分支变量名重复导致的编译失败：使用不同变量名或let{...}块分离范围
description: Rust编译clashing immutable bindings错误 — 处理error[E0598]: clashing immutable bindings失败必读 - 避免在多分支赋值中使用相同变量名导致作用域冲突
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

exit code: 1 > error[E0598]: clashing immutable bindings -> crates\kanzei-tools\src\docstore.rs:712 [fp:bash|> error[E]: clashing immutable]
