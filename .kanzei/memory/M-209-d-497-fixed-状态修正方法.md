---
id: M-209
scope: project
category: fact
title: D-497(fixed)状态修正方法
description: 处理 D-497 状态管理时必读：defect 工具与 terminal entry 状态冲突时的修正方案
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

defect 工具与 D-497(fixed)状态不兼容导致失败：当 terminal entry 状态为 fixed 时，defect 动作无法应用。需改用 defect fix_terminal id=D-497 status=fixed|wontfix reason=<why> 来修正状态。错误原文：[fp:defect|is archived — this action does not apply to terminal entries. To correct a wrong terminal status (e.g. fixed should be wontfix), use `defect fix_terminal id=D-497 status=<fixed|wontfix> reason=<why>`。
