---
id: M-105
scope: project
category: fact
title: bash 测试 failures: 第 N 次跨轮计数失败触发自定义 retry 策略
description: 处理测试 failures 报错 — git::tests::finalize_rejects_fmt_before_tests 重复失败信号,跨轮计数触发
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

处理 bash 重试中 "failures:\n    git::tests::finalize_rejects_fmt_before_tests" 信号时必读：这是 crates/kanzei-tools 测试模块重复失败的固定故障模式。第 3 次跨轮计数失败触发,需按特定方式重试或修复。(refs: )

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-105-bash-测试-failures-第-n-次跨轮计数失败触发自定义-retry.md)
