---
id: M-171
scope: project
category: fact
title: rust: now_ms 函数未导出到 super 模块导致 E0425
description: 处理 rust 编译 now_ms 函数找不到超模块时必读：检查函数是否 pub 导出到父模块作用域
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

[fp:bash|error[E]: cannot find function in module] 根因：now_ms 函数未导出到 super 模块作用域。代码位置：crates/kanzei-memory/src/memory/lifecycle.rs:224。解决方案：需在 now_ms 前加 pub 修饰符，或通过 re-export 将函数暴露到父模块。环境约束：cargo build 时跨模块查找函数，缺少导出会导致 E0425 错误.

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-171-rust-now-ms-函数未导出到-super-模块导致-e0425.md)
