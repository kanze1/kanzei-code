---
id: M-136
scope: project
category: fact
title: bash 整文件重写(Set-Content)被环境拦截,须用 edit 做定点修改
description: 整文件重写的编辑被环境拦截时必读：Set-Content/Move-Item 等整文件操作绕过语法检查与 diff，必须用 edit 做定点改（它容忍换行差异，两次失败后会显示真实内容）
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: 整文件重写被拦截
---

Set-Content/Move-Item 等整文件操作：whole-file rewrites via shell bypass the tools' syntax validation and diff display. Use `edit` for targeted changes (it tolerates line-ending differences and, after two misses, shows you the file's actual content).

错误原文: Set-Content is blocked: whole-file rewrites via shell bypass the edit/write tools' syntax validation an...
[Fp:bash|is blocked: whole-file rewrites via shell bypass the tools' syntax validation an]
