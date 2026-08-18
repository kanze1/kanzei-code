---
id: M-219
scope: project
category: fact
title: bash unknown memory id 错误
description: 处理 bash 运行时工具调用失败时的必读：验证记忆 ID 或内存操作支持前必读
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: run:753
---

bash 命令返回 exit code: 1 伴 M-XXX: ERROR unknown memory id。涉及记忆 ID：M-160, M-160-req-retry..., M-169... 根因可能是已归档或失效的条目未再引用前直接调用。错误原文显示编译完成但运行失败。
[fp:bash|M-: ERROR unknown memory id]
