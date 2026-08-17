---
id: M-086
scope: project
category: sop
title: Git version release batch number check
description: 处理发布动作失败时必读:核对批次数与 Git 历史标记数一致性
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

Git 发布版本标记数校验：R-175 的手写批次是 Git 提交历史标记数为 X，必须核对并更新批次字段后再关闭。使用 req 工具时报错提示此约束。
