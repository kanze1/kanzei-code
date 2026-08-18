---
id: M-164
scope: project
category: fact
title: cargo build cannot find function now_ms in module super
description: 处理 bash/cargo 编译找不到函数时必读：判断是否为可复用知识（环境/工具契约类）还是任务内一次性噪声
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

cargo build:cannot find function now_ms in module super (crates\kanzei-memory\src\memory\lifecycle.rs:224)，exit code:1。复发第1次，本次重试成功。可复用性判断要点：①这是工具环境/契约约束导致的错误则建条目；②是 TDD 预期失败或临时手写错误再改对，判 NOOP。

[fp:bash|error[E]: cannot find function in module]
