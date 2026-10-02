---
id: M-043
scope: project
category: sop
title: autonomous会话中bash命令(含git log/Get-ChildItem)失败的处理策略:复发计数与SOP建构阈值
description: 处理 autonomous会话中 bash 命令(无论git log还是Get-ChildItem)报"permission requires user approval":跨轮复发计数,仅第3次+修复成功证据才允许建立SOP记忆——防止将单例TDD失败过拟合为通用规则。本条SOP教会未来agent何时不要构建条目，比单纯记录某一次失败的“决策价值”更高
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
superseded_by: M-041
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-041(内容重复,原文保留供追溯)。
2026-08-13 bash failures in autonomous session 均报 "permission requires user approved":  
(1) Git log --all → task mode success; (2) Get-ChildItem → memory_note成功。复发档位跨轮计数,仅第3次+带修复证据时才允许建SOP记忆:单例/偶发TDD失败勿过拟合为通用规则——这是环境契约类知识累积策略的核心判据
