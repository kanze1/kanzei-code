---
id: M-227
scope: project
category: fact
title: bash 测试证据失败不激活
description: bash测试证据不激活时必读：第3次+修复成功才建candidate,否则是单轮噪声
status: deprecated
created: 2026-08-17
updated: 2026-10-07
source: user
---

exit code:1 running tests test docstore::tests::promote_write_evidence_failure_does_not_activate...第3次(跨轮)复发。fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate ... ]是工具契约级复现键,丢失引擎看不见"记了但没用"。晋升规则:2次建candidate+ 修复成功证据时用 episode 证明升 active(R-165)。

(stale: 2026-10-07 源码复核：旧钩子把第3次复发写成 candidate 门槛；当前 lifecycle.rs/manager.rs 为第2次 candidate、第3次且真实恢复证据 active。保留原始失败指纹与墓碑，新契约单独记录。)
