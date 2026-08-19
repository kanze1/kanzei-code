---
id: M-264
scope: project
category: fact
title: work 写入 .kanzei/memory 被拒绝必读:开启 tracker 写通道前校验
description: 处理 work 写入权限拒绝必读：.kanzei/memory属 memory-manager 子代理管理，须显式开启写通道
status: candidate
created: 2026-08-19
updated: 2026-08-19
source: memory-manager
---

permission denied by ruleset: work on `write:claim`.当前分支线未开启 tracker 写入；读取仍可用。请在该线设置中显式开启后再修改唯一主根文档。[fp:work|permission denied by ruleset: work on .]
