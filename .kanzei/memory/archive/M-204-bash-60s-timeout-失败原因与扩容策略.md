---
id: M-204
scope: project
category: fact
title: Bash 60s timeout 失败原因与扩容策略
description: bash timeout 失败时必读：环境工具契约类知识何时复用；第 3 次+带成功重试证据晋升
status: deprecated
created: 2026-08-17
updated: 2026-08-20
source: user
superseded_by: M-205
---

命令执行超时：timeout: true — command did not finish within 600000 ms and was killed. Retry with a larger timeout_ms if needed. [no output captured before timeout]. 复发检测指纹:[fp:bash|timeout: true — command did not finish within ms and was killed. Retry with a la]。结论：此任务需要调整 bash 超时策略，将 timeout_ms 从默认值上调至更高数值（如 60s→120s+）并重新执行对应命令。
