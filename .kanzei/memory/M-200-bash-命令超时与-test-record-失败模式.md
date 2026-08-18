---
id: M-200
scope: project
category: fact
title: bash 命令超时与 test_record 失败模式
description: 处理 bash 工具超时失败或 test_record 替代方案:何时遇到 [fp:bash|timeout] 复发即应用此修复
status: active
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: bash_tool_timeout
---

错误触发: timeout: true — command did not finish within ms and was killed. Retry with a larger timeout_ms if needed.\n修复证据:改用 test_record 成功执行→复发档位第3次(跨轮计数),此后可复用方案\n指纹标识:[fp:bash|timeout: true — command did not finish within ms and was killed. Retry with a la]\n适用场景: bash 命令在特定环境下超时(600000ms限制)被 kill,需要降级使用 test_record\n边界例外:仅当确认是工具/环境契约模式(跨轮复发3次+)才建条目;一次性TDD预期失败或自纠错编译错误判NOOP
