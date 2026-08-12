---
id: M-019
scope: project
category: sop
title: bash 整文件重写(Set-Content)被环境拦截,须用 edit 做定点修改
description: bash 里用 Set-Content / 重定向整文件重写被拦截(报 "whole-file rewrites via shell bypass the edit/write tools' syntax validation and diff display")时必读;也说明 edit 容忍换行符差异、连续两次 miss 后展示文件实际内容
status: deprecated
created: 2026-08-08
updated: 2026-08-12
source: fp:bash 拦截, 2026-08-08
---

环境/工具契约:shell 整文件重写被禁止。错误原文:"`Set-Content` is blocked: whole-file rewrites via shell bypass the edit/write tools' syntax validation and diff display. Use `edit` for targeted changes (it tolerates line-ending differences and, after two misses, shows you the file's actual content)"。
正确做法:用 edit 工具做定点修改,不要用 bash 整文件重写绕过。edit 容忍行尾差异,且连续两次 old_string 未命中后会展示文件实际内容,便于精确匹配。关联 M-010(edit 报 old/new 相同时不要改用 bash 绕过)。
