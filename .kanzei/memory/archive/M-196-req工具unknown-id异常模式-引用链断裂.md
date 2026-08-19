---
id: M-196
scope: project
category: fact
title: req工具unknown ID异常模式（引用链断裂）
description: req工具未知ID异常根因分析
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: D-480
---

Req tool failure pattern: known valid IDs = R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249,R-264,R-281,R-288. Unknown ID R-216 indicates broken reference chain (not temporary noise). Decision standard: 1st-2nd occurrence = record as candidate; 3rd+ with repair evidence = add + promote(episode_id=703).

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-196-req工具unknown-id异常模式-引用链断裂.md)
