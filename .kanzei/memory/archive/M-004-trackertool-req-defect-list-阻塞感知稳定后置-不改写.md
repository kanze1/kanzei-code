---
id: M-004
scope: project
category: fact
title: TrackerTool req/defect list 阻塞感知稳定后置，不改写 Markdown 顺序
description: 处理 TrackerTool req/defect list 输出、阻塞条目排序或相关回归测试时必读:list 具备阻塞感知稳定后置能力
status: deprecated
created: 2026-08-07
updated: 2026-10-24
source: inbox note 2026-08-07
area: kanzei-tools/tracker
---

实现位置:crates/kanzei-tools/src/tracker.rs。

req/defect list 读取需求与缺陷的活动/归档状态，识别三类阻塞:非空阻塞字段、依赖状态、"阶段: …后"门槛。稳定分区只调整输出顺序(阻塞条目后置),不改写原始 Markdown;解除阻塞后条目按原文件顺序恢复。

回归测试:tracker::tests::list_stably_postpones_blocked_entries_and_restores_order。

(stale: 2026-10-24 按用户要求精简项目知识：通用工具用法、单次故障 SOP 与旧阶段提示退出项目常驻记忆；操作与验证以当前工具、AGENTS.md 和 CONTRIBUTING.md 为准，原文及来源归档保留。)
