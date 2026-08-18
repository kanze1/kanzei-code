---
id: M-195
scope: project
category: fact
title: req工具未知ID异常模式（引用链断裂）
description: req工具未知ID异常根因分析
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
refs: D-480
---

## Req工具异常分析

### 错误形态
- error原文: unknown id R-216; existing valid IDs include R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249
- 工具: req

### 结论
- rule: req fails when referenced ID is outside known valid set (e.g. R-XXX where XXX matches existing collection)
- pattern: repeated same unknown ID indicates broken reference chain, not temporary noise

### Decision standard
- 1st-2nd occurrence: record failure pattern as candidate
- 3rd+ with successful repair evidence: add entry + promote(episode_id=703)
