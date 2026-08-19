---
id: M-157
scope: project
category: habit
title: bash test failed pattern - memory store moves to archive
description: bash 测试失败重试模式识别：环境/工具契约类知识 vs 一次性噪声
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

bash 反复失败后重试成功(1 次)[fp:bash|test memory::store::tests::deprecated_moves_to_archive_and_hidden_from_search ..]。涉及目标: cargo 。复发档位: 第 1 次(跨轮计数)。判断要点: 这是环境/工具契约类的可复用知识，还是本次任务内的一次性噪声(例如 TDD 里预期的测试失败、自己写错又立刻改对的编译错误)?是前者才建条目，后者判 NOOP。晋升规则: 第 2 次才建 candidate(未验证);第 3 次+ 且带修复成功证据时,用 memory_add 建条目后 memory_promote 带 episode 证据升 active。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-157-bash-test-failed-pattern-memory-store-mo.md)
