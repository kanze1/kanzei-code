---
id: M-112
scope: project
category: fact
title: Git tests 跨轮复发与前端标签关闭前必须有 UI smoke 证据
description: 处理 failures: git::tests 跨轮复发或关闭带“前端”标签的需求/缺陷时必读：先核对真实失败行、完整批次字段和前置条件；关闭前必须附 T-测试记录、file:line 或提交号，并至少跑过本项目 UI smoke（ui-runtime-smoke、ui-lint-smoke、parallel-lines-regression 或 ui-a11y/ui-i18n smoke）；证据不足就显式记录降级或用户执行，禁止沉默跳过或把重试成功当根因。
status: active
created: 2026-08-17
updated: 2026-09-06
source: memory-manager
---

处理 failures: git::tests 跨轮复发或关闭需求/缺陷前，先核对前置条件、环境、完整批次字段和真实失败行；关闭前逐条把验收条款写入进展并附 T-测试记录、file:line 或提交号，不能满足就显式记录降级或用户执行，禁止沉默跳过，也不能把重试成功或单个 exit code 当根因。

带“前端”标签的任务（如 D-743）在关闭前还必须有本项目 UI smoke 的通过记录，候选命令包括：`node scripts/ui-runtime-smoke.mjs`、`node scripts/ui-lint-smoke.mjs`、`node scripts/parallel-lines-regression.mjs`、`node scripts/ui-a11y-smoke.mjs`、`node scripts/ui-i18n-smoke.mjs`。没有任何前端冒烟 passed 记录时不能关闭。

[fp:bash|failures:]
[fp:defect|行动: 何时遇到 failures: git::tests 跨轮复发提示：检查测试前置条件与环境一致性]
[fp:defect|行动: 处理 failures: git::tests 跨轮复发或关闭前出现手写批次与 Git exit code 当作根因。]
[fp:defect|行动: 处理 failures: git::tests T-测试记录、file:line 或提交号，不能满足就显式记录降级或用户执行，禁止沉默跳过，也不能把重试]
