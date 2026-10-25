---
id: M-012
scope: project
category: fact
title: ID 同现于活动与归档时改用 terminal 专用操作
description: 处理 goal/defect/req 报 `id` is required、is archived、ID 同现于活动与归档，或 policy-managed 文件写入触发 tracker integrity/permission 门禁时必读：先判定条目是否为 terminal；terminal 不要调用普通 edit/update，归档终态改用 req fix_terminal 并提供合法 status 与 reason，文件写入改走专用通道。
status: deprecated
created: 2026-08-08
updated: 2026-10-24
source: inbox:2026-08-08
area: kanzei-tools/tracker
---

处理 goal/defect/req 报 is archived、ID 同现于活动与归档，或 policy-managed 文件写入触发 tracker integrity/permission 门禁时：先判定条目是否为 terminal；不要调用普通 edit/update，归档终态改用 req fix_terminal 并提供合法 status 与 reason，文件写入改走专用通道。复发错误原文：`id` is required。新增复发检测键：[fp:req|is required]。既有复发检测键（必须保留）：[fp:defect|is archived — this action does not apply to terminal entries. To correct a wrong] [fp:req|行动: 处理 或其他 policy-managed 文件写入时必读：先不要调用 edit；若出现 tracker integrity broken 或 perm]

(stale: 2026-10-24 按用户要求精简项目知识：通用工具用法、单次故障 SOP 与旧阶段提示退出项目常驻记忆；操作与验证以当前工具、AGENTS.md 和 CONTRIBUTING.md 为准，原文及来源归档保留。)
