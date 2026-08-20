---
id: M-200
scope: project
category: fact
title: bash 命令超时与 test_record 失败模式
description: 运行 bash/test_record 或读取大量工具输出时必读：若出现 600000ms 超时，先缩小读取范围或分段处理并提高 timeout_ms，勿直接重复原命令；确认 partial stdout 后再续作。
status: active
created: 2026-08-17
updated: 2026-08-20
source: memory-manager
subject: bash_tool_timeout
---

复发判据：[fp:bash|timeout: true — command did not finish within ms and was killed. Retry with a la]
错误原文：timeout: true — command did not finish within 600000 ms and was killed. Retry with a larger timeout_ms if needed. [partial stdout before timeout]
决策：bash/test_record 超时后，先基于 partial stdout 缩小或分段读取，并按需增大 timeout_ms，再继续；不要无变化重跑。
