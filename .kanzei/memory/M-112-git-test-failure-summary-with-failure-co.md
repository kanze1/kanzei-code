---
id: M-112
scope: project
category: fact
title: Git tests 跨轮复发：先核对前置条件、环境与批次字段一致性
description: 处理 failures: git::tests 跨轮复发或关闭前出现手写批次与 Git 提交历史标记不一致时必读：先核对测试前置条件、环境一致性及完整批次字段，再更新错误批次并确认失败测试/结束标记后关闭；不要把重试成功或单个 exit code 当作根因。
status: active
created: 2026-08-17
updated: 2026-08-21
source: memory-manager
---

[fp:bash|failures:]
[fp:defect|行动: 何时遇到 failures: git::tests 跨轮复发提示：检查测试前置条件与环境一致性]
复发证据：D-664 的手写批次为 2/2，但 Git 提交历史标记数为 1；关闭前必须先核对并更新批次字段。适用于 git::tests 跨轮复发：检查测试前置条件、环境一致性、完整结束标记和失败测试，并核对手写批次与 Git 历史标记一致后再关闭；不要把重试成功或首个 exit code 当作根因。
