---
id: M-153
scope: project
category: fact
title: memory_stale 失败模式：未知 ID与 malformed disk error
description: 何时遇到未知 ID 错误：验证 M-[ID] 格式;检查对应记忆是否失效;使用 cargo 验证
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: user:R-216验收3次轮回
---

处理 bash 编辑/编译失败“unknown memory id”:验证 M-[ID] 格式;检查对应记忆是否失效(如 M-160);若存在则使用;若无则跳过;保留 [fp:bash|M-: ERROR unknown memory id] 作为复发检测;涉及 cargo 编译流程
