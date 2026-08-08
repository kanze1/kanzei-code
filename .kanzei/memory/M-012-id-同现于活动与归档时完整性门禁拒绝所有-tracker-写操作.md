---
id: M-012
scope: project
category: fact
title: ID 同现于活动与归档时完整性门禁拒绝所有 tracker 写操作
description: goal/defect/req 写操作报 tracker integrity is broken / present in BOTH active and archive 时必读
status: active
created: 2026-08-08
updated: 2026-08-08
source: inbox:2026-08-08
---

错误原文:
REFUSING to write .kanzei/project/goals.md: tracker integrity is broken. present in BOTH active and archive (incomplete archive?): G-002 Fix it first (reads still work): find the lost entries with `git log -S "## <id>" -- .kanzei/project/go...`

契约:同一 ID 同时存在于活动文档与归档文档时,tracker 拒绝一切写操作(读仍可用),必须先修复完整性问题。修复方式见 repair_reused_id 条目(CLI: `kz goal repair_reused_id <id>`);不要直接编辑托管文档。
