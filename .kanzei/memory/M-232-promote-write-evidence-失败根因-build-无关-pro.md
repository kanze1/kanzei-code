---
id: M-232
scope: project
category: fact
title: promote_write_evidence 失败根因：build 无关 promotion gate 不激活测试产物
description: 处理 bash 轮次重复失败且带 episode_id 证据时必读:判断是环境约束还是单次噪声 - 第3次+修复成功证据才建 candidate → promote(episode 767)
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: D-495(fixed)_promotion_write
---

bash 重复失败根因分析: promote_write_evidence Failure detection mechanism does not activate test artifacts during compilation phase

Error signature: exit code: 1 running N tests with test memory::store::tests::promote_write_evidence_failure_does_not_activate ... 

Root cause: Promotion gate requires R-070 sources at promotion time, not during build. Build failures unrelated to active candidate state.

Action mapping:
1st fail + no success evidence → observe only (no candidate entry)
2nd fail + successful retry evidence → create candidate (scope=fact project|D-495(fixed)_promotion_write)
3rd+ with episode evidence → promote via memory_promote using episode_id 767

Detection key: [fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate ... ] embedded verbatim for recurrence matching.
