---
id: M-215
scope: project
category: fact
title: memory管理系统工具契约：未知工具和权限拒绝处理
description: 处理 memory_get/memory_archive/write 工具调用失败：辨识可用工具列表以正确引用;第3次+且失败带成功证据则 promote;保留[fp]标记用于复发检测。
status: deprecated
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:memory_get|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, ]

[fp:memory_archive|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, files, symbols, git, question, todowrite, webfetch, websearch, browser, latex, plot, idea, req, defect, work, decision, memory_search, memory_no

[fp:write|permission denied by ruleset: write on .]

工具契约：已知内存管理系统工具列表及调用限制。所有 [unknown tool X] 错误均指向当前环境实际可用工具，而非工具本身缺失。错误重复出现（第3次+且带成功证据）则 promote to active。
根因：memory_manager 代理规则约束，非全局知识；仅针对 M-160/169/178等特定项目内调用。[fp:bash|M-: ERROR unknown memory id] 与 [fp:write|permission denied by ruleset: write on .] 为独立失败模式，需分别记录指纹用于复发检测。

适用场景 + 操作步骤:
- 触发场景：内存操作命令（memory_get/memory_archive/write）返回未知工具或权限拒绝错误。
- 步骤①：对比现有可用工具列表（read, write, edit, insert, bash, process, glob, grep, ...）。若未知工具则检查是否应为 memory_note/memory_search替代。
- 步骤②：尝试用对应已存在工具调用同一语义功能；保留所有 [fp:bash|...|...]指纹于正文以追溯失败。

边界与例外:
- 非全局约束，仅限本项目内存子代理规则。
- write on . 路径为 policy-managed（memory-manager子代理）专用通道，其他 agent 不可直接写。
- M-160::unknown ID错误与 permission denied 错误属独立失败类型，需各自记录指纹。

(stale: 原条目仅涵盖 write permission denied场景，新笔记明确补充了"记忆库:写路径属memory-manager子代理,主agent只投草稿"的底层契约说明。需更新补充该核心约束并保留fp标记用于复发检测)
