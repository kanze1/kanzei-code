---
id: M-194
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

## 根因分析 - req工具异常

### 错误形态
- **错误原文**: `unknown id 'R-216'`; existing: R-286, R-283, R-284, R-285, R-287, R-235, R-101, R-242, R-243, R-245, R-248, R-249, R-264, R-281, R-288
- **涉及工具**: req（需求提交与资源引用检查）

### 可复用结论
- **环境约束规则**: `req`在引用非连续/断裂资源ID时会抛出`unknown id`异常。**有效资源集合边界**：当目标资源不在已知有效集合（如R-286至R-288等窗口期ID，以及R-235/R-101等历史有效ID）时，需先验证或补充完整引用列表。
- **复发模式**: 同一任务多次出现相同unknown ID即说明引用链本身错误，而非临时噪声。

### 指纹标记建议
- **潜在指纹**: `[fp:req|unknown id R-X]`（当R-X与现有有效集合不匹配时触发）
- **作用**: 用于引擎识别引用链断裂导致的重复失败，丢失将导致复发检测失效。

### 复发判定标准
- **第1-2次出现**: 记录失败模式，标记为待验证candidate。
- **第3次+且带修复成功证据**：建立条目 + memory_promote(episode_id=703)。
