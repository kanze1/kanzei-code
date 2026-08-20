---
id: M-012
scope: project
category: fact
title: ID 同现于活动与归档时完整性门禁拒绝所有 tracker 写操作
description: 处理 goal/defect/req 写操作因条目同时存在活动与归档或已归档终态而被拒时必读：停止普通更新，确认终态；需要纠正时改用 defect fix_terminal 并填写 fixed/wontfix 与原因。
status: active
created: 2026-08-08
updated: 2026-08-20
source: inbox:2026-08-08
---

ID 同现于活动与归档时，goal/defect/req 写操作会被完整性门禁拒绝。遇到原文 `D-538 is archived — this action does not apply to terminal entries. To correct a wrong terminal status (e.g. fixed should be wontfix), use defect fix_terminal id=D-538 status=<fixed|wontfix> reason=<why>.`：不要继续普通 defect 更新；先确认归档/终态，再仅在确需纠正终态时使用 `defect fix_terminal id=<id> status=<fixed|wontfix> reason=<why>`。 [fp:defect|is archived — this action does not apply to terminal entries. To correct a wrong]
