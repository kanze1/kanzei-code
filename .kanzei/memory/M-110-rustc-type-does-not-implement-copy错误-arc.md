---
id: M-110
scope: project
category: fact
title: RustC type does not implement Copy错误 — Arc不可Copy编译失败修复:使用非Copy引用或Cloning
description: Type trait impl错误 — 处理 error[E0382]: type does not implement Copy失败必读 - 检查是否使用Copy类型或变量
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

exit code: 1 > error[E0382]: the type `Arc` does not implement `Copy` --> crates\kanzei\tests\background_subagent_dispatch.rs:31 [fp:bash|> error[E]: the type does not implement]
