---
id: M-205
scope: project
category: fact
title: bash 命令超时被 kill 后的正确重试策略
description: 何时遇到 bash 命令超时/被 kill — 先查历史 timeout 失败记录再重试
status: active
created: 2026-08-17
updated: 2026-08-20
source: memory-manager
---

bash timeout: true — command did not finish within 600000 ms and was killed. Retry with a larger timeout_ms if needed. [no output captured before timeout] [fp:bash|timeout: true — command did not finish within ms and was killed. Retry with a la]

复发模式：第 3 次失败（前两次已建 candidate）。本例已成功改用 test_record。

重试规则：
1. 确认失败类型是否为 timeout（超时而非错误退出）
2. 增大 timeout_ms（如从 60s 到 120/300+ 秒尝试）
3. 确保命令完整执行，避免无输出截断
4. retry 成功后记录验证证据

本例使用 test_record 修复，说明需要增加超时或改用更稳定的执行方式。
