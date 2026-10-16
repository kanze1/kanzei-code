---
id: M-276
scope: project
category: fact
title: Core 负责执行循环和持久会话事实
description: 改执行、恢复、压缩、并行工具批或会话数据时核对 Core 边界
status: active
created: 2026-10-07
updated: 2026-10-07
source: memory-manager
refs: crates/kanzei-core/src/lib.rs crates/kanzei-core/src/runner/drive.rs crates/kanzei-core/src/store/mod.rs
subject: responsibility:kanzei-core
area: kanzei-core
---

run_once_with_parts 是模型与工具执行循环；SessionStore 保存 typed session facts、队列、事件与 episode。core 通过 Harness 工具/权限契约和 FailureRecallPolicy 等接口消费能力，不实现具体记忆检索，也不导入 kanzei-tools 或桌面 UI。工具结果分类与模型消息协议同步；SQLite 投影和历史读取必须保持真实 session/turn 边界。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。
