---
id: M-223
scope: project
category: fact
title: bash compile/test failure pattern detection: keep fp marker
description: 处理 bash 编译/测试失败时的复发检测：保留指纹标记识别已知错误模式
status: deprecated
created: 2026-08-17
updated: 2026-08-17
source: user
refs: R-070
---

[fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate ... ]  — 第3次重复失败表明这是可复用的环境/工具契约问题。错误模式：bash编译阶段遇到解析错误，需要检查输入语法而非重试。复发档位检测机制：第1-2次候选(未验证),第3次+成功修复证据时晋升为active。指纹[Fingerprint]是复发检测键，丢失则引擎无法识别「记了但没用」的历史问题。

(stale: 第 3 次 bash 失败证据已覆盖同一 fp[bash|test memory::store::tests::promote_write_evidence_failure...] 模式，原条目 M-223 已含该复发检测关键标记)
