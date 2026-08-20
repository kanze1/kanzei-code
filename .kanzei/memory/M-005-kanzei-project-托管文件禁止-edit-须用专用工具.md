---
id: M-005
scope: project
category: sop
title: .kanzei/project 托管文件禁止 edit,须用专用工具
description: 处理 .kanzei/memory 或其他 policy-managed 文件写入时必读：不要调用 edit，改用 memory_note/记忆管理工具提交变更；遇 permission denied by ruleset 立即切换合法写入通道。
status: active
created: 2026-08-07
updated: 2026-08-20
source: inbox 2026-08-07
---

复发判据：[fp:edit|permission denied by ruleset: edit on .]
错误原文：permission denied by ruleset: edit on `.kanzei/memory/m-014-html-静态文案必须登记进资源表-否则断言测试失败.md`. This resource is policy-managed (记忆库:写路径属 memory-manager 子代理,主 agent 只投草稿). The ONLY legal write channel is the `memory_note` tool — use it instead.
决策：.kanzei/memory 等 policy-managed 路径由 memory-manager 子代理管理，主 agent 不得 edit；通过 memory_note/记忆管理工具投递草稿。
