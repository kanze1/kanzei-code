---
id: M-175
scope: project
category: fact
title: req: 无效ID引用的根因(依赖注册表不匹配)
description: 处理req重复失败时必读：未知ID的根本原因是依赖解析流程中外部spec与本地注册表不匹配
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: R-286 R-283 R-284
---

root cause: req references invalid/stale resource ID (e.g., R-216 not in known valid list R-286,R-283...R-288). Pattern: dependency resolution pipeline fetches external spec then validates against registry - mismatch causes repeated rejection at req stage. Prevention: pre-generate validate all spec IDs against current registry before invoking req tool.

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-175-req-无效id引用的根因-依赖注册表不匹配.md)
