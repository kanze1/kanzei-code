---
id: M-222
scope: project
category: habit
title: bash 工具调用失败模式总结
description: 何时遇到工具调用失败：读取可用工具列表确认参数格式;用 memory_search 取代 unknown 工具;保留指纹作为复发检测
status: candidate
created: 2026-08-17
updated: 2026-08-20
source: run:753
---

- [fp:bash|M-: ERROR unknown memory id] → M-[ID] 格式需验证;M-160 已失效/不存在;使用 cargo 编译构建时需先确认记忆 ID 状态;编译完成后验证目标是否真实
- [fp:memory_get|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, ] → 用可用工具替换（见可用列表）
- [fp:memory_archive|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, ] → 用 memory_search 取代
晋升规则：第1次建 candidate，第2次后内存证promote
