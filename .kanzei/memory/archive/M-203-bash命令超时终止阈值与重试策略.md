---
id: M-203
scope: project
category: fact
title: Bash命令超时终止阈值与重试策略
description: 处理可复用工具超时失败时必读：建立基准超时阈值并调整重试策略，避免命令被终止
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

Bash shell 600000ms (600秒) default 超时阈值导致命令终止。问题根源：任务执行时间超过默认上限被系统强制kill。解决方案：增大timeout_ms参数（建议≥实际预估时长×1.5），或分阶段执行复杂命令。适用范围：所有跨进程/长时间运行的bash命令执行场景。第3次跨轮计数失败后修复成功验证建立此基准阈值。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-203-bash命令超时终止阈值与重试策略.md)
