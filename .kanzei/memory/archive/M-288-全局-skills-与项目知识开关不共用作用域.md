---
id: M-288
scope: project
category: fact
title: 全局 Skills 与项目知识开关不共用作用域
description: 项目切换或工作树共享设置时核对 global_home 与项目配置
status: deprecated
created: 2026-10-07
updated: 2026-10-24
source: memory-manager
refs: crates/kanzei-harness/src/skills.rs crates/kanzei-tools/src/project_knowledge.rs crates/kanzei-tools/src/run.rs
subject: contract:kanzei-harness/skills:scope
area: kanzei-harness/skills kanzei-tools/project_knowledge kanzei-app/projects
---

Skills 内容与 disabled 偏好来自全局 KANZEI_HOME/skills 和 skills.json，enabled_catalog 在 Harness 装配使用；项目 .kanzei/skills 不是当前管理真源。project-knowledge.json 则属于主项目，默认关闭，工作树共享；关闭只停止层级扫描/注入，不删除 Markdown 和记忆检索。新项目初始化选项与已有项目 set_enabled 都落同一配置，不能把全局 Skills开关当成每项目开关。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。

(stale: 2026-10-24 按用户要求精简项目知识：组件职责与 Skills 作用域统一维护在 docs/design/system_contracts.md，不再重复维护逐模块摘要；原条目归档保留，不判为实现失败。)
