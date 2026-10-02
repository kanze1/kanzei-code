---
id: M-042
scope: project
category: sop
title: autonomous会话中bash命令触发"permission requires user approval"失败处理（含Git/Get-ChildItem场景）
description: 处理 autonomous会话中bash命令(如git log/Get-ChildItem)报"permission requires user approval":需等待用户交互批准或kanzei.toml加白名单——跨轮复发计数,第3次+修复证据后升active记忆用于快速识别同类误判
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
superseded_by: M-041
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-041(内容重复,原文保留供追溯)。
bash 命令(如 Get-ChildItem、git log --all)在 autonomous会话中触发 "permission requires user approved →原因:autonomous session未执行该 bash 命令,需等待用户交互批准或在 kanzei.toml加白名单。复发档位跨轮计数：第1-2次不晋升;只有第3次+且带修复成功证据时,才用 memory_add 建 entry后用 memory_promote 升active记忆。这是环境/工具契约类的可复用知识:autonomous会话默认拒非白名单 bash命令
