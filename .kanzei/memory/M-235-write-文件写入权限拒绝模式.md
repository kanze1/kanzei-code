---
id: M-235
scope: project
category: fact
title: write 文件写入权限拒绝模式
description: 处理写入权限拒绝模式 — .kanzei/memory 路径属 memory-manager 子代理管理，主 agent 只投草稿；唯一合法写通道是 memory_note 工具；[fp:write|permission denied by ruleset: write on .]
status: active
created: 2026-08-17
updated: 2026-08-18
source: user
---

处理 .kanzei/memory 下文件写入被 permission denied by ruleset 拒绝模式 — 路径属 memory-manager 子代理管理，主 agent 只投草稿。唯一合法写通道是 memory_note 工具。规则约束策略：禁止 policy-managed 资源用 edit/bash write 等工具直接修改。指纹: [fp:write|permission denied by ruleset: write on .] —— 复发检测键
