---
id: M-237
scope: project
category: fact
title: 记忆库工具调用失败模式与可用工具集 — 当内存管理器API找不到对应函数时必读 [fp:memory_get|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, files, symbols, git, question, todowrite, webfetch, websearch, browser, latex, plot, idea, req, defect, work, decision, memory_search; fp:memory_archive|unknown tool ; available: ... memory_no]
description: 内存工具调用失败与可用集更新 — 处理 memory_get/memory_archive不存在时的替代方案 [fp:memory_get|unknown tool ; available:..., fp:memory_archive|unknown tool ; available:...]
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

工具调用失败根因与可用集映射（基于D-480/M-165验证）

**适用场景**: bash调用记忆管理器工具函数时报"unknown tool X; available:"列表型响应：
- memory_get: 查找/读取内存条目时触发
- memory_archive: 归档操作时触发
- 其他不存在于当前环境配置的工具函数

**操作步骤**: 
1. read 记忆库元数据配置（.kanzei/kanzei.toml中的工具模块声明）→ 判断依据：是否启用对应工具模块的load指令
2. grep可访问工具列表与内置工具映射 → 判断依据：bash输出中的available:后跟随的工具名集合
3. 改用现有工具函数替代（如用read替代memory_get，用insert/archive功能组合模拟memory_archive）→ 判断依据：现有工具的功能覆盖度评估
4. 若替换失败仍报unknown → 检查内存库元数据配置是否更新延迟

**边界与例外**: 
- 工具存在但调用失败说明参数/路径错误，需结合其他failure模式综合分析
- bash命令链中memory_no作为fallback选项（见fp:记忆_archive可用集含memory_no标记）
- episode证据累积第3次+可升active状态

**引用失败链证据**: 
[fp:memory_get|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, files, symbols, git, question, todowrite, webfetch, websearch, browser, latex, plot, idea, req, defect, work, decision, memory_search, memory_no]
[fp:memory_archive|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, files, symbols, git, question, todowrite, webfetch, websearch, browser, latex, plot, idea, req, defect, work, decision, memory_search, memory_no]

**引用来源**: 工具可用集信息来自episode_id=775验证
