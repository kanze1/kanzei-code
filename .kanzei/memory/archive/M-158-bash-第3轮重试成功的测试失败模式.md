---
id: M-158
scope: project
category: fact
title: bash 第3轮重试成功的测试失败模式
description: 处理测试因证据缺失或工具契约问题反复失败后：需检查是否需追加重试步骤以验证环境契约
status: deprecated
created: 2026-08-17
updated: 2026-08-17
source: run:2026-08-17
refs: R-070
subject: 当前测试运行状态
---

bash 反复尝试（2次）后仍失败，第3轮触发才成功。涉及 cargo test docstore::tests::promote_write_evidence_failure_does_not_activate ... 
复发检测：第1轮失败（跨轮计数）。证据缺失/工具契约验证时需追加重试步骤。[fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate...]
本轮轮次已落库:episode_id=674(state.db episodes 真实存在)。memory_promote 的证据来源必须用它——provenance 硬校验要求 episode_id 真实存在，编造或乱填的 id 会被整体拒绝。

(stale: 已被 consolidation 更新：第 3 次重复失败确认该知识需要记忆化，M-158 是旧的候选条目)
