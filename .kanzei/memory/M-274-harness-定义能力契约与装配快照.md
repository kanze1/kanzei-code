---
id: M-274
scope: project
category: fact
title: Harness 定义能力契约与装配快照
description: 改工具、权限、上下文、Skills 或区域注册表时核对 Harness 职责
status: active
created: 2026-10-07
updated: 2026-10-07
source: memory-manager
refs: crates/kanzei-harness/src/harness.rs crates/kanzei-harness/src/tool.rs crates/kanzei-harness/src/permission.rs
subject: responsibility:kanzei-harness
area: kanzei-harness
---

HarnessDraft 持有 agents、tools、skills、context、permissions 注册表；Harness::resolve 校验声明的专用工具和 deferred_tools 后形成 HarnessSnapshot。Tool、ToolCtx、ToolOutput、Ruleset 是入口与执行器共用契约。Harness 不持有实际模型协议解码和桌面 UI 状态，也不直接依赖 kanzei-core/kanzei-tools。共享运行装配在 tools/run，不在各入口重复复制。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。
