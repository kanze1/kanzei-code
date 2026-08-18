---
id: M-163
scope: project
category: fact
title: req return unknown id R-216 (recovered after retry)
description: 处理 req 工具返回 unknown id 问题时必读：判断是否为可复用知识（环境/工具契约类）还是任务内一次性噪声
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

复现现象①：调用 req 返回 unknown id `R-216`，提示 existing: R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249,R-264,R-281,R-288，跨轮失败1次；复现现象②：第2次调用重试成功。可复用性判断要点：①这是工具环境/契约约束导致的错误则建条目；②是 TDD 预期失败或临时手写错误再改对，判 NOOP。

[fp:req|unknown id ; existing: R-286, R-283, R-284, R-285, R-287, R-235, R-101, R-242, R-243, R-245, R-248, R-249, R-264, R-281, R-288]
