---
id: M-245
scope: project
category: fact
title: Bash permission denied: whole-file rewrite bypass 限制 — 处理 Guard is blocked Shell Rewrite 时必读
description: 处理 permission denied by guard: whole-file rewrites 时必读 — 识别 Bash shell bypass 限制，改用 edit 工具做逐行修改避免触发 guard 拦截
status: deprecated
created: 2026-08-18
updated: 2026-08-20
source: memory-manager
superseded_by: M-258
---

[fp:bash|permission denied by guard : is blocked: whole-file rewrites via shell bypass th] 第 2 次才建 candidate;本轮第 1 次复发→暂不建，需积累至 3 次+且带修复成功证据再申请记忆

[Pitfall History] 本次 Bash Set-Content/whole-file write 拒绝 → shell bypass 语法导致 guard 拦截
[操作步骤] 改用 edit 工具做逐行修改而非整文件重写 → 避免触发 whole-file rewrite bypass detection
[边界与例外] whole-file rewrites via shell bypass the edit/tools' syntax validation→保留 bash 仅作证据记录，待第3次复发成功修复后升 active
