---
id: M-135
scope: project
category: fact
title: Rust 错误: manual case-insensitive ASCII comparison
description: Rust 编译：手动大小写不敏感 ASCII 比较报错时，需使用 to_ascii_lowercase() 等正确处理
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:bash|> error: manual case-insensitive ASCII comparison]

错误模式：exit code: 1 > error: manual case-insensitive ASCII comparison
位置：crates/kanzei-tools/src/docstore.rs:712

原因比较时使用了大小写敏感的 == 操作符，需要统一使用 to_ascii_lowercase() 进行不敏感比较。
