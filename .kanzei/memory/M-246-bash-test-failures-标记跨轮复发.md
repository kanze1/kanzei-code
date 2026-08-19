---
id: M-246
scope: project
category: fact
title: bash test failures 标记跨轮复发
description: 处理 bash test failures 标记跨轮复发 — cargo test框架失败与环境契约知识
status: candidate
created: 2026-08-18
updated: 2026-08-18
source: memory-manager
subject: bash|cargo-test|failures-label
---

[fp:bash|test conversation::tests::latest_segment_recovers_completed_compaction_surface .]

**适用场景**: cargo test 报"failures:"或"failed"标记，M-112 命中但本次仍复发，此为跨环复发的测试环境契约知识。

**操作步骤**:
1. 记录 failures 标识符及失败数量
2. 检查前置依赖、测试框架配置一致性
3. 必要时清理临时文件/缓存后重试

**边界与例外**: flaky test 预期失败、TDD 预设失败场景。
