---
id: M-010
scope: project
category: sop
title: edit identical 是 no-op；缺 HEAD verify 证据不得关闭 defect
description: 处理 edit 报 old_string 与 new_string identical，或 defect 关闭被拒且缺当前 HEAD verify 全绿证据时必读：停止重复 edit，先 read 确认是否已无改动；再运行 .\scripts\verify.ps1，并用 test_record 记录 status=passed、命令含 verify.ps1 且关联当前 defect（本轮为 D-688），未通过先修复门禁欠账。
status: active
created: 2026-08-07
updated: 2026-08-21
source: inbox note 2026-08-07
---

适用场景：\n- edit 报 `old_string` 与 `new_string` identical；\n- defect 关闭被拒，错误原文为：“最近一次提交新增/修改了设计文档或显著改动了单文件，但没有当前 HEAD 绑定的 verify 全绿证据，不能关闭。先运行 .\\scripts\\verify.ps1，再用 test_record 记录 status=passed、命令包含 verify.ps1、关联 D-688；verify 失败时先修复门禁欠账。”\n\n操作步骤：\n1. 遇 identical 时先停止重复 edit，并 read 重读目标，判断目标是否已经是期望内容；若已一致，按 no-op 处理而不是继续重试。\n2. 遇 defect 关闭门禁时运行 `.\\scripts\\verify.ps1`；只有 verify 全绿，才继续记录证据。\n3. 用 `test_record` 记录 `status=passed`，命令字段必须包含 `verify.ps1`，并关联当前 defect（本轮错误对应 D-688）；若 verify 失败，先修复门禁欠账，不得关闭。\n\n判断依据：edit 的 old/new 完全相同表示没有变更；关闭门禁要求当前 HEAD 绑定的 verify 全绿证据，且证据记录需满足命令、状态和关联 ID 三项。\n\n边界与例外：本条不把 verify 失败当作 edit identical 的原因，也不以重复重试替代验证；关联 ID 以当前错误所指 defect 为准，不要沿用过时 ID。\n\n复发判据：[fp:edit|old_string and new_string are identical — nothing to do]\n[fp:defect|D- HEAD 绑定的 verify 全绿证据，不能关闭。先运行 test_record 记录 status=passed、命令包含 verify.ps、关联 ]
