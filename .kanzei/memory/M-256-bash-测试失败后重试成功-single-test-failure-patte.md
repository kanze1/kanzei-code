---
id: M-256
scope: project
category: sop
title: bash 测试失败后重试成功:single test failure pattern 识别
description: 处理测试运行时 single test failed 失败模式必读·环境契约判据校验
status: candidate
created: 2026-08-18
updated: 2026-08-18
source: memory-manager
---

bash 反复失败后重试成功(1 次): [fp:bash|test conversation::tests::latest_segment_recovers_completed_compaction_surface .] 错误原文: exit code: 1 running 1 test FAILED failures: conversation::tests::latest_segment_recovers_completed_compaction_surface;涉及目标: cargo。复发档位: 第 1 次(跨轮计数)。判据:环境/工具契约类可复用知识 vs 一次性噪声(如 TDD 预期测试失败)。晋升规则: 第 2 次才建 candidate(未验证);第 3 次+ 且带修复成功证据时,用 memory_add 建条目后 memory_promote 带 episode 证据升 active。指纹: 原样放入正文 — 复发检测键
