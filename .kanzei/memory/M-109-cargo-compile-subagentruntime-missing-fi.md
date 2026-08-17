---
id: M-109
scope: project
category: fact
title: Cargo compile SubagentRuntime missing field background_notifications错误 — 处理Structurer初始化缺少必要字段时报exit code:1
description: Cargo编译SubagentRuntime缺失background_notifications字段错误 — 处理 error[E0063]: missing field在Initializer失败必读 - 检查 Structur初始化是否包含所有必需字段
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

exit code: 1 > error[E0063]: missing field `background_notifications` in initializer of `SubagentRuntime` --> crates\kanzei\tests\task_cancel_parallel.rs:167:23 [fp:bash|> error[E]: missing field]
