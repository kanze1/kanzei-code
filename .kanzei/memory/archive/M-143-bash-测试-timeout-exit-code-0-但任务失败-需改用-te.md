---
id: M-143
scope: project
category: fact
title: bash 测试 timeout: exit code:0 但任务失败，需改用 test_record
description: bash 测试失败时必读:exit 0 但不代表成功，用 test_record
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: note 2026-08-17 [fact]
---

环境契约事实：bash 执行测试架构时可能被 timeout 机制捕获导致 exit code:0（预期行为），此时 bash 命令看似成功但任务仍失败。正确做法是使用 test_record 工具，它能在命令退出后继续收集并报告结果。

指纹用于复发检测: [fp:bash|test bash::tests::timeout_kills_command_and_returns_explicit_error ... ok]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-143-bash-测试-timeout-exit-code-0-但任务失败-需改用-te.md)
