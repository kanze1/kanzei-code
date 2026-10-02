---
id: M-235
scope: project
category: fact
title: work写入权限拒绝模式 — .kanzei/memory属memory-manager子代理管理，主agent只投草稿；唯一合法写通道是memory_note工具
description: work写入权限拒绝模式 — .kanzei/memory属memory-manager子代理管理，主agent只投草稿；唯一合法写通道是memory_note工具【新判据】: permission denied by guard : is blocked: whole-file rewrites via shell bypass th 说明shell bypass触发guard拦截
status: deprecated
created: 2026-08-17
updated: 2026-08-19
source: user
---

work/内存写入权限拒绝模式 — .kanzei/memory属memory-manager子代理管理，主agent只投草稿；唯一合法写通道是memory_note工具【复发判据】: permission denied by ruleset: work on .时须按规则转换。本轮发现记忆命中但未能阻止工作流程复发 → description需补充「确认当前请求是否通过memory_note而非直接文件操作」作为前置判据，保留核心指纹：[fp:work|permission denied by ruleset: work on .] 和 [fp:write|permission denied by ruleset: write on .]

(stale: 该 fact 已被 M-XXX 更新覆盖（work 写入权限拒绝判断要点已补全）)
