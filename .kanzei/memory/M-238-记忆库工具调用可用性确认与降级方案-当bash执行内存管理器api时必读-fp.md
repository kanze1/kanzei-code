---
id: M-238
scope: project
category: fact
title: 记忆库工具调用可用性确认与降级方案 — 当bash执行内存管理器API时必读 [fp:memory_get|unknown tool ; available:...; fp:memory_archive|unknown tool ; available:...]
description: 内存工具调用失败与替代方案 — 处理 memory_get/memory_archive不存在时的处置策略 [fp:memory_get|known tool; fp:memory_archive|known tool]
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

工具失败证据累积后的经验总结（待升级到active状态）

**适用场景**: bash脚本调用记忆API函数时报"unknown tool X; available:"错误，第3次+失败后需修正：
- memory_get/Archive: 批量查找/归档操作的常见错误模式
- 涉及对象ID: m-159/M-160/m-168/M-169等内存条目操作

**操作步骤**: 
1. bash执行时报error → record失败事件与错误原文指纹
2. grep可用工具列表（见fp中available字段）→ 判断依据：错误消息明确列出的可行选项
3. 若memory_get不存在，改用read + insert替代方案；memory_archive不存在用insert+process组合替代 → 判断依据：现有工具的功能覆盖度验证
4. 记录降级调用链并持续收集失败证据 → 判断依据：第2次仍复发则进入第3次修正循环

**边界与例外**: 
- 错误类型与特定内存条目ID绑定（如m-160-req-retry...）需检查条目元数据配置
- bash命令链中memory_no作为fallback选项可用（见fp标注）
- 失败累计到3次+且附带修复证据可申请晋升为active

**引用失败链证据**: 
[fp:memory_get|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, files, symbols, git, question, todowrite, webfetch, websearch, browser, latex, plot, idea, req, defect, work, decision, memory_search, memory_no]
[fp:memory_archive|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, files, symbols, git, question, todowrite, webfetch, websearch, browser, latex, plot, idea, req, defect, work, decision, memory_search, memory_no]

**引用来源**: 工具可用集信息来自episode_id=775验证
