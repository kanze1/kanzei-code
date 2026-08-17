---
id: M-144
scope: project
category: fact
title: defect archived: 需用 fix_terminal id=X status=... reason=... 专用命令修正状态
description: defect 命令失败时必读:archived 缺陷需用 fix_terminal 专用命令
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: note 2026-08-17 [fact]
---

环境契约事实：defect 工具对 archived 状态的缺陷执行任何 action 都会触发错误提示："is archived — this action does not apply to terminal entries"。必须使用专用命令：defect fix_terminal id=X status=<fixed|wontfix> reason=<why> 才能修正终端状态。

指纹用于复发检测: [fp:defect|is archived — this action does not apply to terminal entries. To correct a wrong]
