---
id: M-225
scope: project
category: fact
title: memory_get与memory archive非独立工具名
description: 处理工具名报错时必读：内存中不存在memory_get和memory_archive独立工具，直接使用现有工具组合
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

内存工具未实现: memory_get 和 memory_archive 报错[fp:memory_get|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, ]; [fp:memory_archive|unknown tool ; available: read, write, edit, insert, bash, process, glob, grep, ]。
实际可用工具列表确认: 不存在 memory_get 和 memory_archive 这两个独立工具名;正确调用方式是直接通过 memory_search/memory_note等现有工具完成对应查询/归档功能。

tool contract修正: 用户记忆中可能存在混淆 tool name的记忆(如将"获取记忆数据"误记为memory_get,或"归档功能"理解为memory_archive),实际应使用已有工具组合实现相同任务。
