---
id: M-212
scope: project
category: fact
title: bash cargo test 语法解析错误演进
description: 处理 bash/cargo 重复测试失败（语法解析/环境契约问题）时必读：识别可复用模式还是 TDD 噪声；第3次+修复成功证据才建 active — 融合历史1-2次报错与第3次晋升触发
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

bash/cargo test 语法解析错误演进：第1-2次模式 - error: expected one of ! . :: ? { or an operator, found store (mod.rs:2049)；第3次+修复成功 evidence 触发晋升。阈值规则：重复失败≥2次建 candidate，第3次+修复证据用 episode_id=750 晋升 active。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-212-bash-cargo-test-语法解析错误演进.md)
