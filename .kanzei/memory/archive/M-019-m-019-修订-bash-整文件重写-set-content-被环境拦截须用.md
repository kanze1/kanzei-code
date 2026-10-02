---
id: M-019
scope: project
category: sop
title: M-019 修订：bash 整文件重写(Set-Content)被环境拦截须用 edit
description: M-019《bash 整文件重写(Set-Content)被环境拦截，须用 edit 做定点修改》修订：补全 [fp] 复发标记原文，强化判据召回钩子 — 处理 Set-Content/Out-File/full-file-write guard 被 is blocked 拦截时必读
status: deprecated
created: 2026-08-08
updated: 2026-08-17
source: fp:bash 拦截, 2026-08-08
---

处理[batch]:`Set-Content`, `Out-File`, `[bash]|[fp:bash|`set-content` is blocked: whole-file rewrites via shell bypass the tools' syntax validation and diff display]| [fp:bash|`out-file` is blocked: whole-file rewrites via shell bypass the tools' syntax validation and diff display]| [fp:bash|is blocked: whole-file rewrites via shell bypass the tools' syntax validation and diff display]` 被拦截时必读：改用 edit 定点修改,勿试探 shell 整写；.kanzei 下 policy-managed 文件只能用专用工具。

(stale: 旧判据无法拦截整文件重写 blocked，需更新召回判据为「Set-Content/Move-Item 等整文件操作被环境拦截时」)
