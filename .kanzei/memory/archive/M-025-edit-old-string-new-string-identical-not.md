---
id: M-025
scope: project
category: sop
title: edit old_string/new_string identical: nothing to do
description: 处理 edit 替换失败（old_string new_string identical）必读：工具要求字符串必须不同才能生效
status: stale
created: 2026-08-09
updated: 2026-08-09
source: memory-manager
subject: edit 替换操作契约
superseded_by: M-010
---

Edit command fails when old_string equals new_string exactly. Tool contract violation - operation does nothing, produces no diff.

Error marker (fp detection key): [fp:edit|old_string and new_string are identical — nothing to do]

Action: Verify strings differ before edit; if attempting anyway expect failure/no-op behavior rather than logical change.
