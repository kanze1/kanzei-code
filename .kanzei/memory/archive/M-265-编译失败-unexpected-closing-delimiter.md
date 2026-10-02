---
id: M-265
scope: project
category: fact
title: 编译失败- unexpected closing delimiter: `}`
description: 处理编译"unexpected closing delimiter"错误必读：补全括号或用rustfmt格式化源文件
status: deprecated
created: 2026-08-19
updated: 2026-08-20
source: memory-manager
refs: R-085 D-204
superseded_by: M-258
---

bash 反复失败后重试成功(1 次)[fp:bash|error: unexpected closing delimiter:] 错误原文: exit code: 1 [stderr] Compiling kanzei-core v0.1.0 (C:\Users\kanzei\Documents\kanzei code\crates\kanzei-core) error: unexpected closing delimiter: `}` --> crates\kanzei-core\src\store\events.rs:967:1 涉及目标: cargo 复发档位: 第 1 次(跨轮计数)
