---
id: M-216
scope: project
category: fact
title: bash 重复失败模式识别：第 N 次复发是否需记忆化
description: 处理 bash 测试重复失败时必读：确认第 N 次复发确认可复用知识
status: deprecated
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

错误模式：第 3 次 bash 重复失败 + 修改后成功确认可复用知识。原文: exit code: 1 running tests... test docstore::tests::promote_write_evidence_failure_does_not_activate... ok。涉及目标: cargo。复发档位: 第 3 次(跨轮计数)。修复：避免特殊路径/引号问题。\n- [fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate ... ]

(stale: 原条目已包含第3次复发的晋升规则，但新笔记补充了"第2次才建candidate(未验证)"这一关键判据，使判定逻辑不完整。需更新为完整流程并保留原有指纹用于复发检测)
